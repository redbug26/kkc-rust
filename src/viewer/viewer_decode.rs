use super::{EncodingMode, LineFeedMode, PreprocOp, ViewMode};
use std::path::Path;

pub(super) fn detect_mode(path: &Path, data: &[u8]) -> ViewMode {
    if looks_like_image(path, data) {
        ViewMode::Image
    } else if is_likely_binary(data) {
        ViewMode::Hex
    } else if contains_ansi_escape(data) {
        ViewMode::Ansi
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
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp"
    ) || data.starts_with(b"\x89PNG\r\n\x1a\n")
        || data.starts_with(&[0xFF, 0xD8, 0xFF])
        || data.starts_with(b"GIF87a")
        || data.starts_with(b"GIF89a")
        || data.starts_with(b"BM")
        || (data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP")
}

fn contains_ansi_escape(data: &[u8]) -> bool {
    let check = &data[..data.len().min(65536)];
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
        .map(|bytes| {
            bytes
                .into_iter()
                .map(|b| byte_to_display_char(b, encoding))
                .collect::<String>()
                .replace('\t', "    ")
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

pub(super) fn ansi_lines(
    data: &[u8],
    line_feed: LineFeedMode,
    preproc_ops: &[PreprocOp],
    encoding: EncodingMode,
) -> Vec<String> {
    let processed = preprocess_bytes(data, preproc_ops);
    let text = ansi_to_text(&processed, line_feed, encoding);
    if text.is_empty() {
        vec![String::new()]
    } else {
        text
    }
}

pub(super) fn hex_line(offset: usize, chunk: &[u8], bpr: usize, encoding: EncodingMode) -> String {
    let hex = chunk
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ");
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
    let pad = bpr.saturating_mul(3).saturating_sub(1).max(1);
    format!("{:08X}  {:<width$}  {}", offset, hex, ascii, width = pad)
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

fn byte_to_display_char(b: u8, encoding: EncodingMode) -> char {
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

fn ansi_to_text(data: &[u8], line_feed: LineFeedMode, encoding: EncodingMode) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut row = 0usize;
    let mut col = 0usize;
    let mut i = 0usize;
    while i < data.len() {
        let b = data[i];
        if b == 0x1b && i + 1 < data.len() && data[i + 1] == b'[' {
            i += 2;
            let start = i;
            while i < data.len() && !data[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i >= data.len() {
                break;
            }
            let cmd = data[i] as char;
            let args = std::str::from_utf8(&data[start..i]).unwrap_or("");
            let params = parse_ansi_params(args);
            match cmd {
                'J' => {
                    if params.first().copied().unwrap_or(0) == 2 {
                        lines.clear();
                        lines.push(String::new());
                        row = 0;
                        col = 0;
                    }
                }
                'K' => {
                    if let Some(line) = lines.get_mut(row) {
                        truncate_at_char_boundary(line, col);
                    }
                }
                'H' | 'f' => {
                    row = params.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                    col = params.get(1).copied().unwrap_or(1).saturating_sub(1) as usize;
                    while lines.len() <= row {
                        lines.push(String::new());
                    }
                }
                'A' => row = row.saturating_sub(params.first().copied().unwrap_or(1) as usize),
                'B' => {
                    row += params.first().copied().unwrap_or(1) as usize;
                    while lines.len() <= row {
                        lines.push(String::new());
                    }
                }
                'C' => col += params.first().copied().unwrap_or(1) as usize,
                'D' => col = col.saturating_sub(params.first().copied().unwrap_or(1) as usize),
                _ => {}
            }
            i += 1;
            continue;
        }

        match b {
            b'\r' => {
                if matches!(line_feed, LineFeedMode::MacCr | LineFeedMode::Mixed) {
                    row += 1;
                    while lines.len() <= row {
                        lines.push(String::new());
                    }
                }
                col = 0;
            }
            b'\n' => {
                if matches!(
                    line_feed,
                    LineFeedMode::DosCrLf | LineFeedMode::UnixLf | LineFeedMode::Mixed
                ) {
                    row += 1;
                    while lines.len() <= row {
                        lines.push(String::new());
                    }
                }
            }
            8 => col = col.saturating_sub(1),
            b'\t' => {
                let next = ((col / 8) + 1) * 8;
                while col < next {
                    put_char(&mut lines, row, col, ' ');
                    col += 1;
                }
            }
            _ => {
                let ch = byte_to_display_char(b, encoding);
                put_char(&mut lines, row, col, ch);
                col += 1;
            }
        }
        i += 1;
    }
    lines.into_iter().map(|l| l.replace('\t', "    ")).collect()
}

fn put_char(lines: &mut Vec<String>, row: usize, col: usize, ch: char) {
    while lines.len() <= row {
        lines.push(String::new());
    }
    let line = &mut lines[row];
    let len = line.chars().count();
    if len < col {
        line.push_str(&" ".repeat(col - len));
    }
    if len == col {
        line.push(ch);
    } else {
        let mut chars: Vec<char> = line.chars().collect();
        if col < chars.len() {
            chars[col] = ch;
            *line = chars.into_iter().collect();
        } else {
            line.push(ch);
        }
    }
}

fn truncate_at_char_boundary(s: &mut String, char_len: usize) {
    let current_len = s.chars().count();
    if char_len >= current_len {
        return;
    }
    let new_len = s
        .char_indices()
        .nth(char_len)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len());
    s.truncate(new_len);
}

fn parse_ansi_params(args: &str) -> Vec<u16> {
    if args.is_empty() {
        return vec![0];
    }
    args.split(';')
        .filter_map(|p| p.parse::<u16>().ok())
        .collect()
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
    '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', 'Á', 'Â', 'À', '©', '╣', '║', '╗', '╝', '¢',
    '¥', '┐', '└', '┴', '┬', '├', '─', '┼', 'ã', 'Ã', '╚', '╔', '╩', '╦', '╠', '═', '╬', '¤', 'ð',
    'Ð', 'Ê', 'Ë', 'È', 'ı', 'Í', 'Î', 'Ï', '┘', '┌', '█', '▄', '¦', 'Ì', '▀', 'Ó', 'ß', 'Ô', 'Ò',
    'õ', 'Õ', 'µ', 'þ', 'Þ', 'Ú', 'Û', 'Ù', 'ý', 'Ý', '¯', '´', '≡', '±', '‗', '¾', '¶', '§', '÷',
    '¸', '°', '¨', '·', '¹', '³', '²', '■', ' ',
];
