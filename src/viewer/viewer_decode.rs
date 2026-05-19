use super::{EncodingMode, LineFeedMode, PreprocOp, ViewMode};
use ratatui::style::{Color, Modifier, Style};
use std::path::Path;
use unicode_width::UnicodeWidthChar;

const ANSI_SCREEN_COLUMNS: usize = 80;
const ANSI_SCREEN_ROWS: usize = 25;
const DOS_ANSI_TO_PALETTE: [usize; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
const DOS_PALETTE: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00), //  0: Black
    (0x00, 0x00, 0xAA), //  1: Blue
    (0x00, 0xAA, 0x00), //  2: Green
    (0x00, 0xAA, 0xAA), //  3: Cyan
    (0xAA, 0x00, 0x00), //  4: Red
    (0xAA, 0x00, 0xAA), //  5: Magenta
    (0xAA, 0x55, 0x00), //  6: Brown
    (0xAA, 0xAA, 0xAA), //  7: Light gray
    (0x55, 0x55, 0x55), //  8: Dark gray
    (0x55, 0x55, 0xFF), //  9: Light blue
    (0x55, 0xFF, 0x55), // 10: Light green
    (0x55, 0xFF, 0xFF), // 11: Light cyan
    (0xFF, 0x55, 0x55), // 12: Light red
    (0xFF, 0x55, 0xFF), // 13: Light magenta
    (0xFF, 0xFF, 0x55), // 14: Yellow
    (0xFF, 0xFF, 0xFF), // 15: White
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnsiCanvasMode {
    Fixed80x25,
    Unbounded,
}

impl AnsiCanvasMode {
    fn columns(self) -> Option<usize> {
        match self {
            Self::Fixed80x25 => Some(ANSI_SCREEN_COLUMNS),
            Self::Unbounded => None,
        }
    }

    fn rows(self) -> Option<usize> {
        match self {
            Self::Fixed80x25 => Some(ANSI_SCREEN_ROWS),
            Self::Unbounded => None,
        }
    }
}

pub(super) fn detect_ansi_canvas_mode(data: &[u8]) -> AnsiCanvasMode {
    let data = strip_utf8_bom(data);
    if sauce_declares_large_canvas(data) || ansi_cursor_targets_large_canvas(strip_dos_eof(data)) {
        AnsiCanvasMode::Unbounded
    } else {
        AnsiCanvasMode::Fixed80x25
    }
}

fn sauce_declares_large_canvas(data: &[u8]) -> bool {
    let data = data.strip_suffix(&[0x1A]).unwrap_or(data);
    if data.len() < 128 {
        return false;
    }
    let record = &data[data.len() - 128..];
    if &record[..5] != b"SAUCE" {
        return false;
    }
    let data_type = record[94];
    if data_type != 1 {
        return false;
    }
    let width = u16::from_le_bytes([record[96], record[97]]) as usize;
    let height = u16::from_le_bytes([record[98], record[99]]) as usize;
    width > ANSI_SCREEN_COLUMNS || height > ANSI_SCREEN_ROWS
}

fn ansi_cursor_targets_large_canvas(data: &[u8]) -> bool {
    let mut i = 0usize;
    while i + 2 < data.len() {
        if data[i] != 0x1b || data[i + 1] != b'[' {
            i += 1;
            continue;
        }
        let Some((final_byte, params, consumed)) = parse_csi(&data[i + 2..]) else {
            break;
        };
        match final_byte as char {
            'H' | 'f' => {
                let row = params
                    .first()
                    .copied()
                    .filter(|value| *value > 0)
                    .unwrap_or(1);
                let col = params
                    .get(1)
                    .copied()
                    .filter(|value| *value > 0)
                    .unwrap_or(1);
                if row as usize > ANSI_SCREEN_ROWS || col as usize > ANSI_SCREEN_COLUMNS {
                    return true;
                }
            }
            'G' => {
                let col = params
                    .first()
                    .copied()
                    .filter(|value| *value > 0)
                    .unwrap_or(1);
                if col as usize > ANSI_SCREEN_COLUMNS {
                    return true;
                }
            }
            _ => {}
        }
        i += 2 + consumed;
    }
    false
}

pub(super) fn detect_mode(path: &Path, data: &[u8]) -> ViewMode {
    if looks_like_image(path, data) {
        ViewMode::Image
    } else if contains_ansi_escape(data) {
        ViewMode::Ansi
    } else if is_likely_binary(data) {
        ViewMode::Hex
    } else {
        ViewMode::Text
    }
}

fn looks_like_image(path: &Path, data: &[u8]) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "heic" | "heif"
    ) || data.starts_with(b"\x89PNG\r\n\x1a\n")
        || data.starts_with(&[0xFF, 0xD8, 0xFF])
        || data.starts_with(b"GIF87a")
        || data.starts_with(b"GIF89a")
        || data.starts_with(b"BM")
        || (data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP")
        || is_heif(data)
}

fn is_heif(data: &[u8]) -> bool {
    if data.len() < 12 || data.get(4..8) != Some(b"ftyp") {
        return false;
    }

    data[8..].chunks_exact(4).take(16).any(|brand| {
        matches!(
            brand,
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1"
        )
    })
}

fn contains_ansi_escape(data: &[u8]) -> bool {
    let check = &data[..data.len().min(64)];
    check.windows(2).any(|w| w == [0x1b, b'['])
}

fn is_likely_binary(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    let check = &data[..data.len().min(8192)];
    let non_printable = check
        .iter()
        .filter(|&&b| b < 9 || (b > 13 && b < 32) || b == 127)
        .count();
    non_printable * 100 / check.len() > 10
}

pub(super) fn text_lines(
    data: &[u8],
    line_feed: LineFeedMode,
    preproc_ops: &[PreprocOp],
    encoding: EncodingMode,
) -> Vec<String> {
    let processed = preprocess_bytes(data, preproc_ops);
    let lines = split_line_bytes(&processed, line_feed)
        .into_iter()
        .map(|bytes| decode_line_bytes(&bytes, encoding))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AnsiCellStyle {
    pub fg: Color,
    pub bg: Color,
    pub modifier: Modifier,
}

impl Default for AnsiCellStyle {
    fn default() -> Self {
        Self {
            fg: dos_palette_color(7),
            bg: dos_palette_color(0),
            modifier: Modifier::empty(),
        }
    }
}

impl AnsiCellStyle {
    pub fn ratatui(self) -> Style {
        Style::default()
            .fg(self.fg)
            .bg(self.bg)
            .add_modifier(self.modifier)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AnsiCell {
    pub ch: char,
    pub style: AnsiCellStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AnsiLine {
    pub cells: Vec<AnsiCell>,
}

impl AnsiLine {
    pub fn plain_text(&self) -> String {
        self.cells.iter().map(|cell| cell.ch).collect()
    }
}

#[derive(Debug, Clone, Copy)]
struct AnsiState {
    style: AnsiCellStyle,
    palette: [(u8, u8, u8); 16],
    saved_row: usize,
    saved_col: usize,
    insert_mode: bool,
}

impl Default for AnsiState {
    fn default() -> Self {
        Self {
            style: AnsiCellStyle::default(),
            palette: DOS_PALETTE,
            saved_row: 0,
            saved_col: 0,
            insert_mode: false,
        }
    }
}

#[cfg(test)]
pub(super) fn ansi_screen_lines(
    data: &[u8],
    line_feed: LineFeedMode,
    preproc_ops: &[PreprocOp],
    encoding: EncodingMode,
) -> Vec<AnsiLine> {
    ansi_screen_lines_with_canvas(
        data,
        line_feed,
        preproc_ops,
        encoding,
        AnsiCanvasMode::Fixed80x25,
    )
}

pub(super) fn ansi_screen_lines_with_canvas(
    data: &[u8],
    line_feed: LineFeedMode,
    preproc_ops: &[PreprocOp],
    encoding: EncodingMode,
    canvas: AnsiCanvasMode,
) -> Vec<AnsiLine> {
    let processed = preprocess_bytes(data, preproc_ops);
    let data = strip_utf8_bom(&processed);
    let data = strip_dos_eof(data);
    let screen_columns = canvas.columns();
    let screen_rows = canvas.rows();
    let mut lines = vec![AnsiLine { cells: Vec::new() }];
    let mut state = AnsiState::default();
    let mut row = 0usize;
    let mut col = 0usize;
    let mut i = 0usize;

    while i < data.len() {
        let b = data[i];
        if b == 0x1b {
            if i + 1 >= data.len() {
                break;
            }
            match data[i + 1] {
                b'[' => {
                    let Some((final_byte, params, consumed)) = parse_csi(&data[i + 2..]) else {
                        break;
                    };
                    apply_csi(
                        final_byte,
                        &params,
                        &mut lines,
                        &mut row,
                        &mut col,
                        &mut state,
                        screen_columns,
                        screen_rows,
                    );
                    i += 2 + consumed;
                    continue;
                }
                b']' => {
                    let consumed = osc_len(&data[i + 2..]);
                    let payload_len = string_control_payload_len(&data[i + 2..], consumed);
                    apply_osc(&data[i + 2..i + 2 + payload_len], &mut state);
                    i += 2 + consumed;
                    continue;
                }
                b'P' | b'X' | b'^' | b'_' => {
                    i += 2 + string_control_len(&data[i + 2..]);
                    continue;
                }
                b'#' => {
                    i += (i + 2 < data.len()).then_some(3).unwrap_or(2);
                    continue;
                }
                b's' => {
                    state.saved_row = row;
                    state.saved_col = col;
                    i += 2;
                    continue;
                }
                b'u' => {
                    row = clamp_to_screen(state.saved_row, screen_rows);
                    col = clamp_to_screen(state.saved_col, screen_columns);
                    ensure_row(&mut lines, row, screen_rows);
                    i += 2;
                    continue;
                }
                _ => {
                    i += 2;
                    continue;
                }
            }
        }

        match b {
            b'\r' => {
                if matches!(line_feed, LineFeedMode::MacCr) {
                    line_feed_ansi_cursor(&mut lines, &mut row, screen_rows);
                }
                col = 0;
                i += 1;
            }
            b'\n' => {
                if matches!(
                    line_feed,
                    LineFeedMode::DosCrLf | LineFeedMode::UnixLf | LineFeedMode::Mixed
                ) {
                    line_feed_ansi_cursor(&mut lines, &mut row, screen_rows);
                }
                i += 1;
            }
            8 => {
                col = col.saturating_sub(1);
                i += 1;
            }
            b'\t' => {
                let next = ((col / 8) + 1) * 8;
                while col < next {
                    wrap_ansi_cursor(&mut lines, &mut row, &mut col, screen_columns, screen_rows);
                    put_ansi_cell(
                        &mut lines,
                        row,
                        col,
                        ' ',
                        state.style,
                        state.insert_mode,
                        screen_rows,
                    );
                    col += 1;
                }
                i += 1;
            }
            0x00..=0x1f | 0x7f => {
                i += 1;
            }
            _ => {
                let (ch, consumed) = match encoding {
                    EncodingMode::Plain => decode_utf8_char_at(data, i),
                    EncodingMode::Cp437 => (byte_to_display_char(b, encoding), 1),
                };
                if !matches!(ch, '\0') {
                    wrap_ansi_cursor(&mut lines, &mut row, &mut col, screen_columns, screen_rows);
                    put_ansi_cell(
                        &mut lines,
                        row,
                        col,
                        ch,
                        state.style,
                        state.insert_mode,
                        screen_rows,
                    );
                    col += UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
                }
                i += consumed;
            }
        }
    }

    if lines.is_empty() {
        vec![AnsiLine { cells: Vec::new() }]
    } else {
        lines
    }
}

pub(super) fn hex_line(
    offset: usize,
    chunk: &[u8],
    bpr: usize,
    encoding: EncodingMode,
    offset_digits: usize,
) -> String {
    let hex = grouped_hex_bytes(chunk);
    let ascii: String = chunk
        .iter()
        .map(|&b| {
            if b < 0x20 || b == 0x7f {
                '.'
            } else {
                byte_to_display_char(b, encoding)
            }
        })
        .collect();
    let pad = hex_column_width(bpr);
    format!(
        "{offset:0offset_digits$X}  {hex:<width$}  {ascii}",
        offset_digits = offset_digits,
        width = pad
    )
}

/// Compute hex offset width from file size.
///
/// Uses 2 bytes (4 hex digits) up to 16-bit files, 3 bytes (6 digits)
/// up to 24-bit files, etc.
pub(super) fn hex_offset_digits(file_len: usize) -> usize {
    let max_offset = file_len.saturating_sub(1);
    let bytes = if max_offset <= 0xFFFF {
        2
    } else {
        let bits = usize::BITS as usize - max_offset.leading_zeros() as usize;
        bits.div_ceil(8)
    };
    bytes.max(2) * 2
}

pub(super) fn hex_column_width(byte_count: usize) -> usize {
    if byte_count == 0 {
        0
    } else {
        byte_count * 3 - 1 + (byte_count - 1) / 4
    }
}

fn grouped_hex_bytes(chunk: &[u8]) -> String {
    let mut out = String::new();
    for (idx, byte) in chunk.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
            if idx % 4 == 0 {
                out.push(' ');
            }
        }
        out.push_str(&format!("{:02X}", byte));
    }
    out
}

fn wrap_ansi_cursor(
    lines: &mut Vec<AnsiLine>,
    row: &mut usize,
    col: &mut usize,
    screen_columns: Option<usize>,
    screen_rows: Option<usize>,
) {
    let Some(screen_columns) = screen_columns else {
        return;
    };
    if *col < screen_columns {
        return;
    }
    *col = 0;
    line_feed_ansi_cursor(lines, row, screen_rows);
}

fn line_feed_ansi_cursor(lines: &mut Vec<AnsiLine>, row: &mut usize, screen_rows: Option<usize>) {
    let Some(screen_rows) = screen_rows else {
        *row += 1;
        ensure_row(lines, *row, None);
        return;
    };
    if *row + 1 < screen_rows {
        *row += 1;
        ensure_row(lines, *row, Some(screen_rows));
        return;
    }
    scroll_screen_up(lines, screen_rows);
    *row = screen_rows.saturating_sub(1);
}

fn scroll_screen_up(lines: &mut Vec<AnsiLine>, screen_rows: usize) {
    ensure_row(lines, screen_rows.saturating_sub(1), Some(screen_rows));
    if !lines.is_empty() {
        lines.remove(0);
    }
    while lines.len() < screen_rows {
        lines.push(AnsiLine { cells: Vec::new() });
    }
}

fn strip_utf8_bom(data: &[u8]) -> &[u8] {
    data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data)
}

fn strip_dos_eof(data: &[u8]) -> &[u8] {
    data.iter()
        .position(|&b| b == 0x1A)
        .map(|pos| &data[..pos])
        .unwrap_or(data)
}

fn parse_csi(data: &[u8]) -> Option<(u8, Vec<i32>, usize)> {
    let mut end = 0usize;
    while end < data.len() {
        let b = data[end];
        if (0x40..=0x7e).contains(&b) {
            let params = parse_csi_params(&data[..end]);
            return Some((b, params, end + 1));
        }
        end += 1;
    }
    None
}

fn parse_csi_params(data: &[u8]) -> Vec<i32> {
    let s = std::str::from_utf8(data).unwrap_or("");
    let s = s
        .trim_start_matches('?')
        .trim_start_matches('>')
        .trim_start_matches('=');
    if s.is_empty() {
        return vec![0];
    }
    s.split(';')
        .map(|part| part.parse::<i32>().unwrap_or(0))
        .collect()
}

fn osc_len(data: &[u8]) -> usize {
    string_control_len(data)
}

fn string_control_payload_len(data: &[u8], consumed: usize) -> usize {
    if consumed == 0 {
        return 0;
    }
    if consumed <= data.len() && data[consumed - 1] == 0x07 {
        return consumed - 1;
    }
    if consumed >= 2
        && consumed <= data.len()
        && data[consumed - 2] == 0x1b
        && data[consumed - 1] == b'\\'
    {
        return consumed - 2;
    }
    consumed.min(data.len())
}

fn string_control_len(data: &[u8]) -> usize {
    let mut i = 0usize;
    while i < data.len() {
        if data[i] == 0x07 {
            return i + 1;
        }
        if data[i] == 0x1b && i + 1 < data.len() && data[i + 1] == b'\\' {
            return i + 2;
        }
        i += 1;
    }
    data.len()
}

fn apply_osc(data: &[u8], state: &mut AnsiState) {
    let Some(rest) = data.strip_prefix(b"4;") else {
        return;
    };
    apply_osc_palette(rest, state);
}

fn apply_osc_palette(data: &[u8], state: &mut AnsiState) {
    let Ok(data) = std::str::from_utf8(data) else {
        return;
    };
    let parts = data.split(';').collect::<Vec<_>>();
    let mut i = 0usize;
    while i + 1 < parts.len() {
        let Ok(index) = parts[i].parse::<usize>() else {
            i += 2;
            continue;
        };
        if index >= state.palette.len() {
            i += 2;
            continue;
        }

        let Some(rgb) = parse_osc_rgb(parts[i + 1]) else {
            i += 2;
            continue;
        };
        let old_color = palette_color(&state.palette, index);
        state.palette[index] = rgb;
        let new_color = palette_color(&state.palette, index);
        if state.style.fg == old_color {
            state.style.fg = new_color;
        }
        if state.style.bg == old_color {
            state.style.bg = new_color;
        }
        i += 2;
    }
}

fn parse_osc_rgb(value: &str) -> Option<(u8, u8, u8)> {
    let rgb = value
        .strip_prefix("rgb:")
        .or_else(|| value.strip_prefix("RGB:"))?;
    let mut parts = rgb.split('/');
    let r = parse_osc_hex_component(parts.next()?)?;
    let g = parse_osc_hex_component(parts.next()?)?;
    let b = parse_osc_hex_component(parts.next()?)?;
    parts.next().is_none().then_some((r, g, b))
}

fn parse_osc_hex_component(value: &str) -> Option<u8> {
    if value.is_empty() || value.len() > 4 || !value.is_ascii() {
        return None;
    }
    let value = if value.len() == 1 {
        [value, value].concat()
    } else {
        value[..2.min(value.len())].to_string()
    };
    u8::from_str_radix(&value, 16).ok()
}

fn clamp_to_screen(value: usize, max: Option<usize>) -> usize {
    max.map(|max| value.min(max.saturating_sub(1)))
        .unwrap_or(value)
}

fn ensure_row(lines: &mut Vec<AnsiLine>, row: usize, max_rows: Option<usize>) {
    while lines.len() <= row && max_rows.is_none_or(|max_rows| lines.len() < max_rows) {
        lines.push(AnsiLine { cells: Vec::new() });
    }
}

fn put_ansi_cell(
    lines: &mut Vec<AnsiLine>,
    row: usize,
    col: usize,
    ch: char,
    style: AnsiCellStyle,
    insert_mode: bool,
    max_rows: Option<usize>,
) {
    ensure_row(lines, row, max_rows);
    let Some(line) = lines.get_mut(row) else {
        return;
    };
    while line.cells.len() < col {
        line.cells.push(AnsiCell {
            ch: ' ',
            style: AnsiCellStyle::default(),
        });
    }
    if insert_mode && col < line.cells.len() {
        line.cells.insert(col, AnsiCell { ch, style });
    } else if line.cells.len() == col {
        line.cells.push(AnsiCell { ch, style });
    } else if let Some(cell) = line.cells.get_mut(col) {
        *cell = AnsiCell { ch, style };
    }
}

fn apply_csi(
    final_byte: u8,
    params: &[i32],
    lines: &mut Vec<AnsiLine>,
    row: &mut usize,
    col: &mut usize,
    state: &mut AnsiState,
    screen_columns: Option<usize>,
    max_rows: Option<usize>,
) {
    let n = |idx: usize, default: i32| -> usize {
        params
            .get(idx)
            .copied()
            .filter(|value| *value > 0)
            .unwrap_or(default)
            .max(0) as usize
    };

    match final_byte as char {
        'm' => apply_sgr(params, state),
        'H' | 'f' => {
            *row = clamp_to_screen(n(0, 1).saturating_sub(1), max_rows);
            *col = clamp_to_screen(n(1, 1).saturating_sub(1), screen_columns);
            ensure_row(lines, *row, max_rows);
        }
        'A' => *row = row.saturating_sub(n(0, 1)),
        'B' => {
            *row = clamp_to_screen(*row + n(0, 1), max_rows);
            ensure_row(lines, *row, max_rows);
        }
        'C' => *col += n(0, 1),
        'D' => *col = col.saturating_sub(n(0, 1)),
        '@' => {
            ensure_row(lines, *row, max_rows);
            if let Some(line) = lines.get_mut(*row) {
                let count = n(0, 1);
                let at = (*col).min(line.cells.len());
                for _ in 0..count {
                    line.cells.insert(
                        at,
                        AnsiCell {
                            ch: ' ',
                            style: state.style,
                        },
                    );
                }
            }
        }
        'P' => {
            ensure_row(lines, *row, max_rows);
            if let Some(line) = lines.get_mut(*row) {
                let count = n(0, 1);
                if *col < line.cells.len() {
                    let end = (*col + count).min(line.cells.len());
                    line.cells.drain(*col..end);
                }
            }
        }
        'X' => {
            ensure_row(lines, *row, max_rows);
            if let Some(line) = lines.get_mut(*row) {
                let count = n(0, 1);
                let end = (*col + count).min(line.cells.len());
                for cell in line.cells.iter_mut().take(end).skip(*col) {
                    *cell = AnsiCell {
                        ch: ' ',
                        style: state.style,
                    };
                }
            }
        }
        'E' => {
            *row = clamp_to_screen(*row + n(0, 1), max_rows);
            *col = 0;
            ensure_row(lines, *row, max_rows);
        }
        'F' => {
            *row = row.saturating_sub(n(0, 1));
            *col = 0;
        }
        'G' => *col = clamp_to_screen(n(0, 1).saturating_sub(1), screen_columns),
        'J' => match params.first().copied().unwrap_or(0) {
            2 | 3 => {
                lines.clear();
                lines.push(AnsiLine { cells: Vec::new() });
                *row = 0;
                *col = 0;
            }
            0 => {
                ensure_row(lines, *row, max_rows);
                if let Some(line) = lines.get_mut(*row)
                    && *col < line.cells.len()
                {
                    line.cells.truncate(*col);
                }
                lines.truncate(*row + 1);
            }
            1 => {
                for line in lines.iter_mut().take(*row) {
                    line.cells.clear();
                }
                if let Some(line) = lines.get_mut(*row) {
                    for cell in line.cells.iter_mut().take(*col + 1) {
                        *cell = AnsiCell {
                            ch: ' ',
                            style: AnsiCellStyle::default(),
                        };
                    }
                }
            }
            _ => {}
        },
        'K' => {
            ensure_row(lines, *row, max_rows);
            if let Some(line) = lines.get_mut(*row) {
                match params.first().copied().unwrap_or(0) {
                    0 => {
                        if *col < line.cells.len() {
                            line.cells.truncate(*col);
                        }
                    }
                    1 => {
                        for cell in line.cells.iter_mut().take(*col + 1) {
                            *cell = AnsiCell {
                                ch: ' ',
                                style: AnsiCellStyle::default(),
                            };
                        }
                    }
                    2 => line.cells.clear(),
                    _ => {}
                }
            }
        }
        's' => {
            state.saved_row = *row;
            state.saved_col = *col;
        }
        'u' => {
            *row = clamp_to_screen(state.saved_row, max_rows);
            *col = state.saved_col;
            ensure_row(lines, *row, max_rows);
        }
        'h' => {
            if params.contains(&4) {
                state.insert_mode = true;
            }
        }
        'l' => {
            if params.contains(&4) {
                state.insert_mode = false;
            }
        }
        _ => {}
    }
}

fn apply_sgr(params: &[i32], state: &mut AnsiState) {
    let params = if params.is_empty() { &[0][..] } else { params };
    let palette = state.palette;
    let style = &mut state.style;
    let mut i = 0usize;
    while i < params.len() {
        match params[i] {
            0 => *style = default_style_for_palette(&palette),
            1 => {
                style.modifier.insert(Modifier::BOLD);
                style.fg = bright_ansi_color(style.fg, &palette);
            }
            2 => style.modifier.insert(Modifier::DIM),
            3 => style.modifier.insert(Modifier::ITALIC),
            4 => style.modifier.insert(Modifier::UNDERLINED),
            5 | 6 => style.modifier.insert(Modifier::SLOW_BLINK),
            7 => {
                style.modifier.insert(Modifier::REVERSED);
            }
            22 => {
                style.modifier.remove(Modifier::BOLD);
                style.modifier.remove(Modifier::DIM);
            }
            23 => style.modifier.remove(Modifier::ITALIC),
            24 => style.modifier.remove(Modifier::UNDERLINED),
            25 => style.modifier.remove(Modifier::SLOW_BLINK),
            27 => {
                style.modifier.remove(Modifier::REVERSED);
            }
            30..=37 => {
                style.fg = ansi_16_color(
                    params[i] as u8 - 30,
                    style.modifier.contains(Modifier::BOLD),
                    &palette,
                );
            }
            40..=47 => style.bg = ansi_16_color(params[i] as u8 - 40, false, &palette),
            90..=97 => style.fg = ansi_16_color(params[i] as u8 - 90, true, &palette),
            100..=107 => style.bg = ansi_16_color(params[i] as u8 - 100, true, &palette),
            39 => style.fg = palette_color(&palette, 7),
            49 => style.bg = palette_color(&palette, 0),
            38 | 48 => {
                if let Some((color, consumed)) = parse_extended_color(&params[i + 1..], &palette) {
                    if params[i] == 38 {
                        style.fg = color;
                    } else {
                        style.bg = color;
                    }
                    i += consumed;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn parse_extended_color(params: &[i32], palette: &[(u8, u8, u8); 16]) -> Option<(Color, usize)> {
    match params.first().copied()? {
        5 => {
            let idx = *params.get(1)? as u8;
            Some((ansi_256_color(idx, palette), 2))
        }
        2 => {
            let r = (*params.get(1)?).clamp(0, 255) as u8;
            let g = (*params.get(2)?).clamp(0, 255) as u8;
            let b = (*params.get(3)?).clamp(0, 255) as u8;
            Some((Color::Rgb(r, g, b), 4))
        }
        _ => None,
    }
}

fn default_style_for_palette(palette: &[(u8, u8, u8); 16]) -> AnsiCellStyle {
    AnsiCellStyle {
        fg: palette_color(palette, 7),
        bg: palette_color(palette, 0),
        modifier: Modifier::empty(),
    }
}

fn ansi_16_color(idx: u8, bright: bool, palette: &[(u8, u8, u8); 16]) -> Color {
    let palette_index = DOS_ANSI_TO_PALETTE[idx as usize % 8] + if bright { 8 } else { 0 };
    palette_color(palette, palette_index)
}

fn dos_palette_color(idx: usize) -> Color {
    palette_color(&DOS_PALETTE, idx)
}

fn palette_color(palette: &[(u8, u8, u8); 16], idx: usize) -> Color {
    let (r, g, b) = palette[idx % palette.len()];
    Color::Rgb(r, g, b)
}

fn bright_ansi_color(color: Color, palette: &[(u8, u8, u8); 16]) -> Color {
    for sgr_idx in 0..8 {
        if color == ansi_16_color(sgr_idx, false, palette) {
            return ansi_16_color(sgr_idx, true, palette);
        }
    }
    color
}

fn ansi_256_color(idx: u8, palette: &[(u8, u8, u8); 16]) -> Color {
    if idx < 16 {
        return ansi_16_color(idx % 8, idx >= 8, palette);
    }
    if idx < 232 {
        let n = idx - 16;
        let scale = [0, 95, 135, 175, 215, 255];
        return Color::Rgb(
            scale[(n / 36) as usize],
            scale[((n / 6) % 6) as usize],
            scale[(n % 6) as usize],
        );
    }
    let gray = 8 + (idx - 232) * 10;
    Color::Rgb(gray, gray, gray)
}

fn split_line_bytes(input: &[u8], mode: LineFeedMode) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut current = Vec::new();
    let mut i = 0usize;
    while i < input.len() {
        match mode {
            LineFeedMode::DosCrLf => {
                if i + 1 < input.len() && input[i] == b'\r' && input[i + 1] == b'\n' {
                    out.push(current);
                    current = Vec::new();
                    i += 2;
                    continue;
                }
            }
            LineFeedMode::MacCr => {
                if input[i] == b'\r' {
                    out.push(current);
                    current = Vec::new();
                    i += 1;
                    continue;
                }
            }
            LineFeedMode::UnixLf => {
                if input[i] == b'\n' {
                    out.push(current);
                    current = Vec::new();
                    i += 1;
                    continue;
                }
            }
            LineFeedMode::Mixed => {
                if i + 1 < input.len() && input[i] == b'\r' && input[i + 1] == b'\n' {
                    out.push(current);
                    current = Vec::new();
                    i += 2;
                    continue;
                }
                if input[i] == b'\r' || input[i] == b'\n' {
                    out.push(current);
                    current = Vec::new();
                    i += 1;
                    continue;
                }
            }
        }
        current.push(input[i]);
        i += 1;
    }
    out.push(current);
    out
}

pub(super) fn preprocess_bytes(data: &[u8], preproc_ops: &[PreprocOp]) -> Vec<u8> {
    let mut out = data.to_vec();
    for op in preproc_ops {
        match *op {
            PreprocOp::Xor(v) => {
                for b in &mut out {
                    *b ^= v;
                }
            }
            PreprocOp::And(v) => {
                for b in &mut out {
                    *b &= v;
                }
            }
            PreprocOp::Or(v) => {
                for b in &mut out {
                    *b |= v;
                }
            }
            PreprocOp::Neg => {
                for b in &mut out {
                    *b = (0u8).wrapping_sub(*b);
                }
            }
            PreprocOp::Ror(v) => {
                let r = v % 8;
                for b in &mut out {
                    *b = b.rotate_right(r as u32);
                }
            }
            PreprocOp::Add(v) => {
                for b in &mut out {
                    *b = b.wrapping_add(v);
                }
            }
            PreprocOp::Latin => {}
            PreprocOp::Elite => {
                for b in &mut out {
                    let c = (*b as char).to_ascii_uppercase();
                    *b = match c {
                        'A' | 'E' | 'I' | 'O' | 'U' | 'Y' => c.to_ascii_lowercase() as u8,
                        _ => c as u8,
                    };
                }
            }
        }
    }
    out
}

pub(super) fn byte_to_display_char(b: u8, encoding: EncodingMode) -> char {
    if b == b'\n' {
        return '\n';
    }
    if b == b'\r' {
        return '\r';
    }
    if b == b'\t' {
        return '\t';
    }
    if b < 0x20 || b == 0x7f {
        return ' ';
    }
    match encoding {
        EncodingMode::Plain => {
            if b.is_ascii() {
                b as char
            } else {
                '.'
            }
        }
        EncodingMode::Cp437 => CP437[b as usize],
    }
}

fn decode_line_bytes(bytes: &[u8], encoding: EncodingMode) -> String {
    match encoding {
        EncodingMode::Plain => {
            let mut out = String::new();
            for ch in String::from_utf8_lossy(bytes).chars() {
                match ch {
                    '\t' => out.push_str("    "),
                    ch if ch.is_control() => out.push(' '),
                    _ => out.push(ch),
                }
            }
            out
        }
        EncodingMode::Cp437 => bytes
            .iter()
            .map(|&b| byte_to_display_char(b, encoding))
            .collect::<String>()
            .replace('\t', "    "),
    }
}

fn decode_utf8_char_at(data: &[u8], start: usize) -> (char, usize) {
    let first = data[start];
    if first.is_ascii() {
        let ch = first as char;
        return if ch.is_control() { (' ', 1) } else { (ch, 1) };
    }

    let width = if (first & 0b1110_0000) == 0b1100_0000 {
        2
    } else if (first & 0b1111_0000) == 0b1110_0000 {
        3
    } else if (first & 0b1111_1000) == 0b1111_0000 {
        4
    } else {
        1
    };

    if width == 1 || start + width > data.len() {
        return ('\u{FFFD}', 1);
    }

    if data[start + 1..start + width]
        .iter()
        .any(|b| (b & 0b1100_0000) != 0b1000_0000)
    {
        return ('\u{FFFD}', 1);
    }

    match std::str::from_utf8(&data[start..start + width]) {
        Ok(s) => {
            let ch = s.chars().next().unwrap_or('\u{FFFD}');
            if ch.is_control() {
                (' ', width)
            } else {
                (ch, width)
            }
        }
        Err(_) => ('\u{FFFD}', 1),
    }
}

pub(super) fn preproc_op_label(op: PreprocOp) -> String {
    match op {
        PreprocOp::Xor(v) => format!("XOR {:02X}", v),
        PreprocOp::And(v) => format!("AND {:02X}", v),
        PreprocOp::Or(v) => format!("OR {:02X}", v),
        PreprocOp::Neg => "NEG".into(),
        PreprocOp::Ror(v) => format!("ROR {}", v % 8),
        PreprocOp::Add(v) => format!("ADD {:02X}", v),
        PreprocOp::Latin => "Latin".into(),
        PreprocOp::Elite => "Elite".into(),
    }
}

const CP437: [char; 256] = [
    '\0', '☺', '☻', '♥', '♦', '♣', '♠', '•', '◘', '○', '◙', '♂', '♀', '♪', '♫', '☼', '►', '◄', '↕',
    '‼', '¶', '§', '▬', '↨', '↑', '↓', '→', '←', '∟', '↔', '▲', '▼', ' ', '!', '"', '#', '$', '%',
    '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', '0', '1', '2', '3', '4', '5', '6', '7', '8',
    '9', ':', ';', '<', '=', '>', '?', '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K',
    'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '[', '\\', ']', '^',
    '_', '`', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q',
    'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '{', '|', '}', '~', '⌂', 'Ç', 'ü', 'é', 'â', 'ä',
    'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù',
    'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬',
    '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜',
    '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨',
    '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀', 'α', 'ß', 'Γ', 'π',
    'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±', '≥', '≤', '⌠', '⌡', '÷',
    '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', ' ',
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_lines_plain_keeps_utf8_chars() {
        let input = "caf\u{00e9}\n\u{4f60}\u{597d}".as_bytes();
        let lines = text_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        assert_eq!(
            lines,
            vec!["caf\u{00e9}".to_string(), "\u{4f60}\u{597d}".to_string()]
        );
    }

    #[test]
    fn ansi_screen_lines_plain_keeps_utf8_chars() {
        let input = "\u{00e9}cho".as_bytes();
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        let plain = lines.iter().map(AnsiLine::plain_text).collect::<Vec<_>>();
        assert_eq!(plain, vec!["\u{00e9}cho".to_string()]);
    }

    #[test]
    fn detect_mode_prefers_esc_bracket_over_binary_heuristic() {
        let input = b"\x1b[2J\x1b[31mANSI\x1b[0m\x00\x01\x02\x03";
        assert_eq!(detect_mode(Path::new("art.dat"), input), ViewMode::Ansi);
    }

    #[test]
    fn detect_mode_treats_heic_as_image_before_ansi_scan() {
        let input = b"\x00\x00\x00\x28ftypheic\x00\x00\x00\x00mif1MiHE\x1b[31m";
        assert_eq!(detect_mode(Path::new("photo.heic"), input), ViewMode::Image);
    }

    #[test]
    fn detect_ansi_canvas_mode_keeps_classic_canvas_by_default() {
        assert_eq!(
            detect_ansi_canvas_mode(b"\x1b[25;80HOK"),
            AnsiCanvasMode::Fixed80x25
        );
    }

    #[test]
    fn detect_ansi_canvas_mode_uses_unbounded_for_large_cursor_targets() {
        assert_eq!(
            detect_ansi_canvas_mode(b"\x1b[26;1HLOW"),
            AnsiCanvasMode::Unbounded
        );
        assert_eq!(
            detect_ansi_canvas_mode(b"\x1b[1;81HWIDE"),
            AnsiCanvasMode::Unbounded
        );
    }

    #[test]
    fn detect_ansi_canvas_mode_ignores_many_physical_rows_without_large_targets() {
        let input = ["X"; ANSI_SCREEN_ROWS + 1].join("\n");
        assert_eq!(
            detect_ansi_canvas_mode(input.as_bytes()),
            AnsiCanvasMode::Fixed80x25
        );
    }

    #[test]
    fn detect_ansi_canvas_mode_uses_unbounded_for_large_sauce_dimensions() {
        let mut input = b"ART".to_vec();
        input.push(0x1A);
        let mut sauce = [0u8; 128];
        sauce[..7].copy_from_slice(b"SAUCE00");
        sauce[94] = 1;
        sauce[95] = 1;
        sauce[96..98].copy_from_slice(&(ANSI_SCREEN_COLUMNS as u16 + 1).to_le_bytes());
        sauce[98..100].copy_from_slice(&(ANSI_SCREEN_ROWS as u16).to_le_bytes());
        input.extend_from_slice(&sauce);
        assert_eq!(detect_ansi_canvas_mode(&input), AnsiCanvasMode::Unbounded);
    }

    #[test]
    fn ansi_screen_lines_applies_cursor_positioning() {
        let input = b"A\x1b[2;4HB";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        let plain = lines.iter().map(AnsiLine::plain_text).collect::<Vec<_>>();
        assert_eq!(plain, vec!["A".to_string(), "   B".to_string()]);
    }

    #[test]
    fn ansi_screen_lines_keeps_sgr_style() {
        let input = b"\x1b[31mR\x1b[0mW";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        assert_eq!(lines[0].cells[0].ch, 'R');
        assert_eq!(lines[0].cells[0].style.fg, Color::Rgb(0xAA, 0, 0));
        assert_eq!(lines[0].cells[1].ch, 'W');
        assert_eq!(lines[0].cells[1].style.fg, Color::Rgb(0xAA, 0xAA, 0xAA));
    }

    #[test]
    fn ansi_screen_lines_maps_bold_to_bright_foreground() {
        let input = b"\x1b[1;31mR";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        assert_eq!(lines[0].cells[0].style.fg, Color::Rgb(0xFF, 0x55, 0x55));
    }

    #[test]
    fn ansi_screen_lines_uses_dos_vga_palette_order() {
        let input = b"\x1b[31mR\x1b[34mB\x1b[33mY\x1b[1;33m!";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        assert_eq!(lines[0].cells[0].style.fg, Color::Rgb(0xAA, 0, 0));
        assert_eq!(lines[0].cells[1].style.fg, Color::Rgb(0, 0, 0xAA));
        assert_eq!(lines[0].cells[2].style.fg, Color::Rgb(0xAA, 0x55, 0));
        assert_eq!(lines[0].cells[3].style.fg, Color::Rgb(0xFF, 0xFF, 0x55));
    }

    #[test]
    fn ansi_screen_lines_uses_dos_vga_background_colors() {
        let input = b"\x1b[44mB\x1b[103mY";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        assert_eq!(lines[0].cells[0].style.bg, Color::Rgb(0, 0, 0xAA));
        assert_eq!(lines[0].cells[1].style.bg, Color::Rgb(0xFF, 0xFF, 0x55));
    }

    #[test]
    fn ansi_screen_lines_applies_osc_4_palette_colors() {
        let input = b"\x1b]4;4;rgb:10/38/28;1;rgb:86/92/C7\x07\x1b[31mR\x1b[34mB";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        assert_eq!(lines[0].cells[0].style.fg, Color::Rgb(0x10, 0x38, 0x28));
        assert_eq!(lines[0].cells[1].style.fg, Color::Rgb(0x86, 0x92, 0xC7));
    }

    #[test]
    fn ansi_screen_lines_applies_osc_4_with_st_terminator() {
        let input = b"\x1b]4;7;rgb:82/92/7D\x1b\\\x1b[0mW";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        assert_eq!(lines[0].cells[0].style.fg, Color::Rgb(0x82, 0x92, 0x7D));
    }

    #[test]
    fn ansi_screen_lines_decodes_cp437_box_drawing() {
        let input = b"\xC9\xCD\xBB\xCC\xD0\xDD\xDE";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Cp437);
        let plain = lines.iter().map(AnsiLine::plain_text).collect::<Vec<_>>();
        assert_eq!(
            plain,
            vec!["\u{2554}\u{2550}\u{2557}\u{2560}\u{2568}\u{258c}\u{2590}".to_string()]
        );
    }

    #[test]
    fn ansi_screen_lines_ignores_nonprinting_control_bytes() {
        let input = b"A\x00\x14\x16B";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Cp437);
        let plain = lines.iter().map(AnsiLine::plain_text).collect::<Vec<_>>();
        assert_eq!(plain, vec!["AB".to_string()]);
    }

    #[test]
    fn ansi_screen_lines_skips_dcs_payload() {
        let input = b"A\x1bPq~sixeldummy\x1b\\B";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        let plain = lines.iter().map(AnsiLine::plain_text).collect::<Vec<_>>();
        assert_eq!(plain, vec!["AB".to_string()]);
    }

    #[test]
    fn ansi_screen_lines_supports_insert_and_delete_char() {
        let input = b"AC\x1b[2G\x1b[4hB\x1b[4l\x1b[2G\x1b[P";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        let plain = lines.iter().map(AnsiLine::plain_text).collect::<Vec<_>>();
        assert_eq!(plain, vec!["AC".to_string()]);
    }

    #[test]
    fn hex_line_groups_hex_bytes_by_four() {
        let line = hex_line(0, &[0, 1, 2, 3, 4, 5, 6, 7], 8, EncodingMode::Plain, 4);
        assert!(line.starts_with("0000  00 01 02 03  04 05 06 07  "));
    }

    #[test]
    fn ansi_screen_lines_wraps_at_eighty_columns() {
        let input = b"\x1b[1;80HAB";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        let plain = lines.iter().map(AnsiLine::plain_text).collect::<Vec<_>>();
        assert_eq!(plain, vec![format!("{}A", " ".repeat(79)), "B".to_string()]);
    }

    #[test]
    fn ansi_screen_lines_scrolls_when_wrapping_from_bottom_row() {
        let input = b"\x1b[1;1HTOP\x1b[25;80HAB";
        let lines = ansi_screen_lines(input, LineFeedMode::UnixLf, &[], EncodingMode::Plain);
        let plain = lines.iter().map(AnsiLine::plain_text).collect::<Vec<_>>();
        assert_eq!(plain.len(), ANSI_SCREEN_ROWS);
        assert_eq!(plain[0], "");
        assert_eq!(plain[23], format!("{}A", " ".repeat(79)));
        assert_eq!(plain[24], "B");
    }

    #[test]
    fn ansi_screen_lines_unbounded_does_not_wrap_at_eighty_columns() {
        let input = b"\x1b[1;80HAB";
        let lines = ansi_screen_lines_with_canvas(
            input,
            LineFeedMode::UnixLf,
            &[],
            EncodingMode::Plain,
            AnsiCanvasMode::Unbounded,
        );
        let plain = lines.iter().map(AnsiLine::plain_text).collect::<Vec<_>>();
        assert_eq!(plain, vec![format!("{}AB", " ".repeat(79))]);
    }

    #[test]
    fn ansi_screen_lines_unbounded_does_not_scroll_at_twenty_five_rows() {
        let input = b"\x1b[1;1HTOP\x1b[25;80HAB";
        let lines = ansi_screen_lines_with_canvas(
            input,
            LineFeedMode::UnixLf,
            &[],
            EncodingMode::Plain,
            AnsiCanvasMode::Unbounded,
        );
        let plain = lines.iter().map(AnsiLine::plain_text).collect::<Vec<_>>();
        assert_eq!(plain.len(), ANSI_SCREEN_ROWS);
        assert_eq!(plain[0], "TOP");
        assert_eq!(plain[24], format!("{}AB", " ".repeat(79)));
    }
}
