use anyhow::{Context, Result};
use crossterm::{
    cursor::MoveTo,
    queue,
    terminal::{size as terminal_size, window_size},
};
use image::{ImageFormat, ImageReader};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod syntax;
mod viewer_decode;
mod viewer_render;
mod viewer_search;

use self::viewer_decode::{
    ansi_lines, detect_mode, hex_line, preproc_op_label, preprocess_bytes, text_lines,
};
use self::viewer_render::{pad_visible, slice_visible};
use self::viewer_search::parse_hex_query;

fn viewer_positions() -> &'static Mutex<HashMap<PathBuf, ViewerPosition>> {
    static POSITIONS: OnceLock<Mutex<HashMap<PathBuf, ViewerPosition>>> = OnceLock::new();
    POSITIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// Debug logger
// ---------------------------------------------------------------------------

static DEBUG_LOG_ENABLED: AtomicBool = AtomicBool::new(false);
static DEBUG_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Call once at startup (after loading config) to enable/disable debug logging.
/// Creates the parent directory if needed and writes a startup marker.
pub fn init_debug_log(enabled: bool, path: PathBuf) {
    // Always create the parent dir so the path is usable as soon as logging is
    // turned on (either now or later via set_debug_log_enabled).
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    DEBUG_LOG_PATH.get_or_init(|| path);
    DEBUG_LOG_ENABLED.store(enabled, Ordering::Relaxed);
    if enabled {
        debug_log(&format!(
            "=== kkc debug log started (log path: {}) ===",
            DEBUG_LOG_PATH.get().unwrap().display()
        ));
    }
}

/// Toggle debug logging at runtime (e.g. from the config panel).
pub fn set_debug_log_enabled(enabled: bool) {
    let was = DEBUG_LOG_ENABLED.swap(enabled, Ordering::Relaxed);
    if enabled && !was {
        debug_log(&format!(
            "=== kkc debug log enabled (log path: {}) ===",
            DEBUG_LOG_PATH
                .get()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unset>".into())
        ));
    }
}

pub fn debug_log(msg: &str) {
    if !DEBUG_LOG_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(path) = DEBUG_LOG_PATH.get() else {
        return;
    };
    use std::fs::OpenOptions;
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

/// Returns the path to the debug log file (regardless of whether logging is enabled).
pub fn debug_log_path() -> Option<PathBuf> {
    DEBUG_LOG_PATH.get().cloned()
}

#[derive(Debug)]
pub struct Viewer {
    pub path: PathBuf,
    pub raw: Vec<u8>,
    pub scroll: usize,
    pub hscroll: usize,
    pub mode: ViewMode,
    pub viewer_plugin: Option<String>,
    pub plugin_state: HashMap<String, String>,
    pub wrap: bool,
    pub search: String,
    pub matches: Vec<usize>,
    pub match_pos: usize,
    pub zoomed: bool,
    pub save_position: bool,
    pub encoding: EncodingMode,
    pub line_feed: LineFeedMode,
    pub mask: MaskKind,
    pub mask_enabled: bool,
    pub preproc_ops: Vec<PreprocOp>,
    text_lines: Vec<String>,
    ansi_lines: Vec<String>,
    image: Option<ImageInfo>,
    plugin_document_cache: RefCell<Option<PluginDocumentCache>>,
    /// Bytes-per-row for hex mode, updated lazily during rendering to match panel width.
    pub hex_bytes_per_row: Cell<usize>,
    mouse_selection: Option<MouseTextSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginDocumentCacheKey {
    plugin_name: String,
    mode: &'static str,
    state: Vec<(String, String)>,
    width: usize,
}

#[derive(Debug, Clone)]
struct PluginDocumentCache {
    key: PluginDocumentCacheKey,
    lines: Vec<Vec<crate::plugins::ViewerSpan>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Text,
    Hex,
    Ansi,
    Image,
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub format: &'static str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    kitty_png: OnceLock<Option<Vec<u8>>>,
    decoded_rgba: OnceLock<Option<(u32, u32, Vec<u8>)>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineFeedMode {
    DosCrLf,
    MacCr,
    UnixLf,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskKind {
    /// Auto-detect language from the file extension.
    Auto,
    C,
    Rust,
    JavaScript,
    Python,
    Php,
    Html,
    Css,
    Sql,
    Shell,
    Pascal,
    Assembler,
    Ketchup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingMode {
    Plain,
    Cp437,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreprocOpKind {
    Xor,
    And,
    Or,
    Neg,
    Ror,
    Add,
    Latin,
    Elite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreprocOp {
    Xor(u8),
    And(u8),
    Or(u8),
    Neg,
    Ror(u8),
    Add(u8),
    Latin,
    Elite,
}

#[derive(Debug, Clone, Copy)]
struct ViewerPosition {
    scroll: usize,
    hscroll: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MouseTextPoint {
    row: usize,
    column: usize,
}

#[derive(Debug, Clone, Copy)]
struct MouseTextSelection {
    scroll: usize,
    text_width: usize,
    visible_rows: usize,
    anchor: MouseTextPoint,
    focus: MouseTextPoint,
}

impl Viewer {
    /// Create a viewer displaying synthetic plain-text content (no file is read).
    /// Useful for placeholder views such as folder placeholders.
    pub fn placeholder(path: &Path, text: &str, wrap: bool) -> Self {
        let raw = text.as_bytes().to_vec();
        let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        Self {
            path: path.to_path_buf(),
            raw,
            scroll: 0,
            hscroll: 0,
            mode: ViewMode::Text,
            viewer_plugin: None,
            plugin_state: HashMap::new(),
            wrap,
            search: String::new(),
            matches: Vec::new(),
            match_pos: 0,
            zoomed: false,
            save_position: false,
            encoding: EncodingMode::Plain,
            line_feed: LineFeedMode::UnixLf,
            mask: MaskKind::Auto,
            mask_enabled: false,
            preproc_ops: Vec::new(),
            text_lines: lines,
            ansi_lines: Vec::new(),
            image: None,
            plugin_document_cache: RefCell::new(None),
            hex_bytes_per_row: Cell::new(16),
            mouse_selection: None,
        }
    }

    pub fn open(path: &Path, wrap: bool) -> Result<Self> {
        Self::open_with_limit(path, wrap, None)
    }

    /// Like `open`, but caps the bytes read to `max_bytes`.
    /// Used by quick-preview so that large files don't stall the UI on every
    /// cursor movement.
    pub fn open_preview(path: &Path, wrap: bool) -> Result<Self> {
        const MAX_QUICK_PREVIEW_BYTES: usize = 512 * 1024 * 8; // 4 MB
        debug_log(&format!("open_preview: {} (limit=4 MB)", path.display()));
        Self::open_with_limit(path, wrap, Some(MAX_QUICK_PREVIEW_BYTES))
    }

    fn open_with_limit(path: &Path, wrap: bool, max_bytes: Option<usize>) -> Result<Self> {
        debug_log(&format!(
            "open_with_limit: {} max_bytes={:?}",
            path.display(),
            max_bytes
        ));
        let raw = if let Some(limit) = max_bytes {
            use std::io::Read;
            let mut file =
                std::fs::File::open(path).with_context(|| format!("Reading {}", path.display()))?;
            let file_len = file.metadata().map(|m| m.len() as usize).unwrap_or(limit);
            let cap = file_len.min(limit);
            let mut buf = vec![0u8; cap];
            let n = file
                .read(&mut buf)
                .with_context(|| format!("Reading {}", path.display()))?;
            buf.truncate(n);
            buf
        } else {
            fs::read(path).with_context(|| format!("Reading {}", path.display()))?
        };
        let line_feed = LineFeedMode::Mixed;
        let encoding = EncodingMode::Cp437;
        let image = detect_image_info(path, &raw);
        let mode = detect_mode(path, &raw);
        let mut viewer = Self {
            path: path.to_path_buf(),
            raw,
            scroll: 0,
            hscroll: 0,
            mode,
            viewer_plugin: None,
            plugin_state: HashMap::new(),
            wrap,
            search: String::new(),
            matches: Vec::new(),
            match_pos: 0,
            zoomed: false,
            save_position: true,
            encoding,
            line_feed,
            mask: MaskKind::Auto,
            mask_enabled: true,
            preproc_ops: Vec::new(),
            text_lines: Vec::new(),
            ansi_lines: Vec::new(),
            image,
            plugin_document_cache: RefCell::new(None),
            hex_bytes_per_row: Cell::new(16),
            mouse_selection: None,
        };
        // Only decode the initial mode — other modes are built lazily on first access.
        viewer.ensure_mode_decoded(mode);
        if matches!(viewer.mode, ViewMode::Image) {
            viewer.zoomed = true;
        }
        if let Some(plugin_name) = crate::plugins::default_viewer_plugin_for_path(path) {
            viewer.set_viewer_plugin(plugin_name);
        }
        if max_bytes.is_none() {
            viewer.restore_position();
        }
        viewer.rebuild_matches();
        Ok(viewer)
    }

    pub fn mode_label(&self) -> &'static str {
        if self.viewer_plugin.is_some() {
            return "Plugin";
        }
        match self.mode {
            ViewMode::Text => "Text",
            ViewMode::Hex => "Hex",
            ViewMode::Ansi => "Ansi",
            ViewMode::Image => "Image",
        }
    }

    pub fn line_feed_label(&self) -> &'static str {
        match self.line_feed {
            LineFeedMode::DosCrLf => "CR/LF",
            LineFeedMode::MacCr => "CR",
            LineFeedMode::UnixLf => "LF",
            LineFeedMode::Mixed => "Mixed",
        }
    }

    pub fn mask_label(&self) -> &'static str {
        if !self.mask_enabled {
            "OFF"
        } else {
            match self.mask {
                MaskKind::Auto => "Auto",
                MaskKind::C => "C/C++",
                MaskKind::Rust => "Rust",
                MaskKind::JavaScript => "JS",
                MaskKind::Python => "Python",
                MaskKind::Php => "PHP",
                MaskKind::Html => "HTML",
                MaskKind::Css => "CSS",
                MaskKind::Sql => "SQL",
                MaskKind::Shell => "Shell",
                MaskKind::Pascal => "Pascal",
                MaskKind::Assembler => "Asm",
                MaskKind::Ketchup => "Ketchup",
            }
        }
    }

    pub fn preproc_label(&self) -> String {
        if self.preproc_ops.is_empty() {
            "None".into()
        } else if self.preproc_ops.len() == 1 {
            preproc_op_label(self.preproc_ops[0])
        } else {
            format!(
                "{}+{}",
                preproc_op_label(self.preproc_ops[0]),
                self.preproc_ops.len() - 1
            )
        }
    }

    pub fn zoom_label(&self) -> &'static str {
        if self.zoomed { "Full" } else { "Auto" }
    }

    pub fn encoding_label(&self) -> &'static str {
        match self.encoding {
            EncodingMode::Plain => "Plain",
            EncodingMode::Cp437 => "CP437",
        }
    }

    pub fn line_count(&self) -> usize {
        if let Some(count) = self.plugin_document_line_count() {
            return count.max(1);
        }
        match self.mode {
            ViewMode::Image => 1,
            ViewMode::Hex => self.hex_line_count(),
            _ => self.current_plain_lines().len().max(1),
        }
    }

    pub fn image_info(&self) -> Option<&ImageInfo> {
        self.image.as_ref()
    }

    pub fn is_image_mode(&self) -> bool {
        matches!(self.mode, ViewMode::Image)
    }

    pub fn set_mode(&mut self, mode: ViewMode) {
        self.mode = mode;
        self.viewer_plugin = None;
        self.plugin_state = HashMap::new();
        self.ensure_mode_decoded(mode);
        self.scroll = 0;
        self.hscroll = 0;
        self.rebuild_matches();
    }

    pub fn set_viewer_plugin(&mut self, plugin_name: String) {
        self.mode = ViewMode::Text;
        self.viewer_plugin = Some(plugin_name);
        self.plugin_state = HashMap::new();
        self.ensure_mode_decoded(ViewMode::Text);
        self.scroll = 0;
        self.hscroll = 0;
        self.rebuild_matches();
    }

    pub fn set_line_feed(&mut self, mode: LineFeedMode) {
        self.line_feed = mode;
        self.rebuild_decoded_lines();
        self.scroll = 0;
        self.hscroll = 0;
        self.rebuild_matches();
    }

    pub fn set_encoding(&mut self, mode: EncodingMode) {
        self.encoding = mode;
        self.rebuild_decoded_lines();
        self.rebuild_matches();
    }

    pub fn set_mask(&mut self, mask: Option<MaskKind>) {
        if let Some(mask) = mask {
            self.mask_enabled = true;
            self.mask = mask;
        } else {
            self.mask_enabled = false;
        }
    }

    pub fn push_preproc(&mut self, kind: PreprocOpKind, param: u8) {
        let op = match kind {
            PreprocOpKind::Xor => PreprocOp::Xor(param),
            PreprocOpKind::And => PreprocOp::And(param),
            PreprocOpKind::Or => PreprocOp::Or(param),
            PreprocOpKind::Neg => PreprocOp::Neg,
            PreprocOpKind::Ror => PreprocOp::Ror(param),
            PreprocOpKind::Add => PreprocOp::Add(param),
            PreprocOpKind::Latin => PreprocOp::Latin,
            PreprocOpKind::Elite => PreprocOp::Elite,
        };
        if self.preproc_ops.len() < 16 {
            self.preproc_ops.push(op);
        }
        self.rebuild_decoded_lines();
        self.scroll = 0;
        self.hscroll = 0;
        self.rebuild_matches();
    }

    pub fn clear_preproc(&mut self) {
        self.preproc_ops.clear();
        self.rebuild_decoded_lines();
        self.scroll = 0;
        self.hscroll = 0;
        self.rebuild_matches();
    }

    pub fn preproc_last_param(&self) -> Option<u8> {
        self.preproc_ops.last().and_then(|op| match *op {
            PreprocOp::Xor(v)
            | PreprocOp::And(v)
            | PreprocOp::Or(v)
            | PreprocOp::Ror(v)
            | PreprocOp::Add(v) => Some(v),
            _ => None,
        })
    }

    pub fn preproc_len(&self) -> usize {
        self.preproc_ops.len()
    }

    pub fn preproc_item_label(&self, idx: usize) -> Option<String> {
        self.preproc_ops.get(idx).copied().map(preproc_op_label)
    }

    pub fn move_preproc_up(&mut self, idx: usize) {
        if idx > 0 && idx < self.preproc_ops.len() {
            self.preproc_ops.swap(idx - 1, idx);
            self.rebuild_decoded_lines();
            self.rebuild_matches();
        }
    }

    pub fn move_preproc_down(&mut self, idx: usize) {
        if idx + 1 < self.preproc_ops.len() {
            self.preproc_ops.swap(idx, idx + 1);
            self.rebuild_decoded_lines();
            self.rebuild_matches();
        }
    }

    pub fn update_preproc_param(&mut self, idx: usize, delta: i16) {
        let Some(op) = self.preproc_ops.get_mut(idx) else {
            return;
        };
        match op {
            PreprocOp::Xor(v)
            | PreprocOp::And(v)
            | PreprocOp::Or(v)
            | PreprocOp::Ror(v)
            | PreprocOp::Add(v) => {
                *v = v.wrapping_add_signed(delta as i8);
                self.rebuild_decoded_lines();
                self.rebuild_matches();
            }
            _ => {}
        }
    }

    pub fn remove_preproc(&mut self, idx: usize) {
        if idx < self.preproc_ops.len() {
            self.preproc_ops.remove(idx);
            self.rebuild_decoded_lines();
            self.rebuild_matches();
        }
    }

    pub fn toggle_wrap(&mut self) {
        if matches!(self.mode, ViewMode::Text | ViewMode::Ansi) {
            self.wrap = !self.wrap;
        }
        self.clear_mouse_selection();
    }

    pub fn toggle_zoom(&mut self) {
        self.zoomed = !self.zoomed;
        self.clear_mouse_selection();
    }

    pub fn supports_mouse_text_selection(&self) -> bool {
        !self.wrap
            && self.viewer_plugin.is_none()
            && matches!(self.mode, ViewMode::Text | ViewMode::Ansi)
    }

    pub fn clear_mouse_selection(&mut self) {
        self.mouse_selection = None;
    }

    pub fn start_mouse_selection(
        &mut self,
        row: usize,
        column: usize,
        text_width: usize,
        visible_rows: usize,
    ) {
        if !self.supports_mouse_text_selection() || text_width == 0 || visible_rows == 0 {
            self.mouse_selection = None;
            return;
        }

        let point = MouseTextPoint {
            row: row.min(visible_rows.saturating_sub(1)),
            column: column.min(text_width),
        };
        self.mouse_selection = Some(MouseTextSelection {
            scroll: self.scroll,
            text_width,
            visible_rows,
            anchor: point,
            focus: point,
        });
    }

    pub fn update_mouse_selection(&mut self, row: usize, column: usize) {
        let Some(selection) = self.mouse_selection.as_mut() else {
            return;
        };
        selection.focus = MouseTextPoint {
            row: row.min(selection.visible_rows.saturating_sub(1)),
            column: column.min(selection.text_width),
        };
    }

    pub fn selection_display_segments_for_visible_row(
        &self,
        row: usize,
        text_width: usize,
        visible_rows: usize,
    ) -> Option<(String, String, String)> {
        let (start_col, end_col) = self.selection_range_for_visible_row(row, text_width, visible_rows)?;
        let display = self.visible_display_line(row, text_width)?;
        Some((
            slice_display_columns(&display, 0, start_col),
            slice_display_columns(&display, start_col, end_col),
            slice_display_columns(&display, end_col, text_width),
        ))
    }

    pub fn selected_visible_text(&self, text_width: usize, visible_rows: usize) -> Option<String> {
        let selection = self.active_mouse_selection(text_width, visible_rows)?;
        let (start, end) = ordered_mouse_points(selection.anchor, selection.focus);
        if start == end {
            return None;
        }

        let mut lines = Vec::new();
        for row in start.row..=end.row {
            let Some((start_col, end_col)) = self.selection_range_for_visible_row(row, text_width, visible_rows) else {
                continue;
            };
            let Some(display) = self.visible_display_line(row, text_width) else {
                continue;
            };
            lines.push(slice_display_columns(&display, start_col, end_col).trim_end_matches(' ').to_string());
        }
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    fn active_mouse_selection(
        &self,
        text_width: usize,
        visible_rows: usize,
    ) -> Option<MouseTextSelection> {
        let selection = self.mouse_selection?;
        if selection.scroll != self.scroll
            || selection.text_width != text_width
            || selection.visible_rows != visible_rows
            || !self.supports_mouse_text_selection()
        {
            return None;
        }
        Some(selection)
    }

    fn selection_range_for_visible_row(
        &self,
        row: usize,
        text_width: usize,
        visible_rows: usize,
    ) -> Option<(usize, usize)> {
        let selection = self.active_mouse_selection(text_width, visible_rows)?;
        let (start, end) = ordered_mouse_points(selection.anchor, selection.focus);
        if start == end || row < start.row || row > end.row {
            return None;
        }

        let start_col = if row == start.row { start.column } else { 0 };
        let end_col = if row == end.row { end.column } else { text_width };
        (start_col < end_col).then_some((start_col, end_col))
    }

    fn visible_display_line(&self, row: usize, text_width: usize) -> Option<String> {
        let abs_idx = self.scroll + row;
        if abs_idx >= self.line_count() {
            return None;
        }
        Some(self.display_line(&self.plain_line_at(abs_idx), text_width))
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        if self.scroll + 1 < self.line_count() {
            self.scroll += 1;
        }
    }

    pub fn page_up(&mut self, height: usize) {
        self.scroll = self.scroll.saturating_sub(height);
    }

    pub fn page_down(&mut self, height: usize) {
        let max = self.line_count().saturating_sub(height.max(1));
        self.scroll = (self.scroll + height).min(max);
    }

    pub fn goto_start(&mut self) {
        self.scroll = 0;
    }

    pub fn goto_first_non_blank(&mut self) {
        if matches!(self.mode, ViewMode::Text | ViewMode::Ansi) && self.viewer_plugin.is_none() {
            let line = self
                .current_plain_lines()
                .iter()
                .position(|line| !line.trim().is_empty())
                .unwrap_or(0);
            self.goto_line(line);
        } else {
            self.goto_start();
        }
    }

    pub fn goto_end(&mut self, height: usize) {
        self.scroll = self.line_count().saturating_sub(height.max(1));
    }

    /// How many logical lines fit in `display_rows` terminal rows from the
    /// current scroll position, accounting for word-wrap.
    ///
    /// In non-wrap modes every logical line is exactly one row, so the result
    /// equals `display_rows`. In wrap text/ansi mode a long logical line
    /// occupies `ceil(len / text_width)` rows, so fewer logical lines fit.
    pub fn page_lines_for(&self, display_rows: usize, text_width: usize) -> usize {
        if display_rows == 0 {
            return 1;
        }
        if !self.wrap
            || !matches!(self.mode, ViewMode::Text | ViewMode::Ansi)
            || self.viewer_plugin.is_some()
            || text_width == 0
        {
            return display_rows;
        }
        let lines = self.current_plain_lines();
        let mut rows_used = 0usize;
        let mut count = 0usize;
        for i in self.scroll..lines.len() {
            let line_chars = lines[i].chars().count();
            let rows_for_line = ((line_chars + text_width - 1) / text_width).max(1);
            if rows_used + rows_for_line > display_rows {
                break;
            }
            rows_used += rows_for_line;
            count += 1;
        }
        count.max(1)
    }

    /// Jump to a 0-based line index, clamped to the valid range.
    pub fn goto_line(&mut self, line: usize) {
        self.scroll = line.min(self.line_count().saturating_sub(1));
    }

    /// Width (in terminal columns) of the line-number gutter for text/ansi modes.
    /// e.g. for 999 lines → "999│ " = 5, for 9 lines → "9│ " = 3.
    /// Returns 0 for hex/image/plugin-document modes.
    pub fn line_number_width(&self) -> usize {
        if !matches!(self.mode, ViewMode::Text | ViewMode::Ansi) || self.viewer_plugin.is_some() {
            return 0;
        }
        let n = self.line_count().max(1);
        let digits = n.ilog10() as usize + 1;
        digits + 2 // digits + "│ "
    }

    pub fn scroll_left(&mut self, amount: usize) {
        if matches!(self.mode, ViewMode::Text | ViewMode::Ansi) && !self.wrap {
            self.hscroll = self.hscroll.saturating_sub(amount);
        }
    }

    pub fn scroll_right(&mut self, amount: usize) {
        if matches!(self.mode, ViewMode::Text | ViewMode::Ansi) && !self.wrap {
            self.hscroll = self.hscroll.saturating_add(amount);
        }
    }

    pub fn scroll_left_max(&mut self) {
        if matches!(self.mode, ViewMode::Text | ViewMode::Ansi) {
            self.hscroll = 0;
        }
    }

    pub fn search_set(&mut self, s: &str) {
        self.search = s.to_string();
        self.rebuild_matches();
        if !self.matches.is_empty() {
            self.match_pos = 0;
            self.scroll = self.matches[0];
        }
    }

    pub fn search_next(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.match_pos = (self.match_pos + 1) % self.matches.len();
        self.scroll = self.matches[self.match_pos];
    }

    pub fn search_prev(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.match_pos = self
            .match_pos
            .checked_sub(1)
            .unwrap_or(self.matches.len() - 1);
        self.scroll = self.matches[self.match_pos];
    }

    pub fn render_lines(
        &self,
        selected_width: usize,
        start: usize,
        height: usize,
    ) -> Vec<Line<'static>> {
        if let Some(lines) = self.render_plugin_document_lines(start, height, selected_width) {
            return lines;
        }
        match self.mode {
            ViewMode::Text | ViewMode::Ansi => {
                self.render_text_like_lines(selected_width, start, height)
            }
            ViewMode::Image => self.render_image_fallback_lines(selected_width, height),
            ViewMode::Hex => self.render_hex_lines(selected_width, start, height),
        }
    }

    fn render_image_fallback_lines(
        &self,
        selected_width: usize,
        height: usize,
    ) -> Vec<Line<'static>> {
        if selected_width == 0 || height == 0 {
            return Vec::new();
        }
        let Some(image) = self.image_info() else {
            return vec![Line::from(Span::styled(
                "Image unavailable",
                Style::default().fg(Color::Yellow),
            ))];
        };

        let decoded = image
            .decoded_rgba
            .get_or_init(|| decode_rgba_for_fallback(&self.raw, image.format).ok());
        let Some((src_w, src_h, pixels)) = decoded.as_ref() else {
            return vec![Line::from(Span::styled(
                "Image decode failed",
                Style::default().fg(Color::Yellow),
            ))];
        };

        let src_w = *src_w as usize;
        let src_h = *src_h as usize;
        if src_w == 0 || src_h == 0 {
            return vec![Line::from(Span::raw(String::new()))];
        }

        // Quarter-block rendering: 2×2 sub-pixels per terminal cell.
        // Terminal cells are typically ~2:1 (height:width), so we compute the
        // aspect-correct scale in "half-row pixel units" (1 unit = 1 col width ≈
        // half a row height), exactly as the old ▀/▄ approach did.
        // Then each terminal column gets 2 horizontal sub-pixels (extra detail),
        // while each half-row unit stays at 1 vertical sub-pixel.
        let max_pu_w = selected_width; // 1 pixel-unit per column
        let max_pu_h = height.saturating_mul(2); // 2 pixel-units per row (half-rows)
        let scale_x = max_pu_w as f32 / src_w as f32;
        let scale_y = max_pu_h as f32 / src_h as f32;
        let scale = scale_x.min(scale_y).max(0.0001);
        // target dimensions in pixel-units (aspect-correct, square at 2:1 cells)
        let target_pu_w = ((src_w as f32) * scale).floor().max(1.0) as usize;
        let target_pu_h = ((src_h as f32) * scale).floor().max(1.0) as usize;
        // sub-pixel dimensions: horizontal uses 2 per column for extra detail
        let target_w = target_pu_w * 2; // sub-pixels wide
        let target_h = target_pu_h; // sub-pixels tall (= half-rows)
        let term_cols = target_pu_w;
        let term_rows = target_h.div_ceil(2).max(1);

        let pad_x = selected_width.saturating_sub(term_cols) / 2;
        let pad_y = height.saturating_sub(term_rows) / 2;

        // Quarter-block characters indexed by 4-bit pattern.
        // Bit layout: bit3=TL, bit2=TR, bit1=BL, bit0=BR  (1=fg, 0=bg)
        #[rustfmt::skip]
        const QUAD: [&str; 16] = [
            " ",  // 0000
            "▗",  // 0001  BR
            "▖",  // 0010  BL
            "▄",  // 0011  BL+BR  (lower half)
            "▝",  // 0100  TR
            "▐",  // 0101  TR+BR  (right half)
            "▞",  // 0110  TR+BL  (diagonal /)
            "▟",  // 0111  TR+BL+BR
            "▘",  // 1000  TL
            "▚",  // 1001  TL+BR  (diagonal \)
            "▌",  // 1010  TL+BL  (left half)
            "▙",  // 1011  TL+BL+BR
            "▀",  // 1100  TL+TR  (upper half)
            "▜",  // 1101  TL+TR+BR
            "▛",  // 1110  TL+TR+BL
            "█",  // 1111  full
        ];

        // Map a sub-pixel (tx, ty) in target space → source pixel.
        let sample = |tx: usize, ty: usize| -> [u8; 4] {
            let sx = (tx * src_w / target_w).min(src_w - 1);
            let sy = (ty * src_h / target_h).min(src_h - 1);
            rgba_at(pixels, src_w, sx, sy)
        };

        // Given 4 RGBA pixels [TL, TR, BL, BR], pick 2 representative colors
        // (fg and bg) and return the block character + colors.
        let quantize = |block: [[u8; 4]; 4]| -> (&'static str, Color, Color) {
            // Luminance (0=transparent treated as black)
            let lum = |p: [u8; 4]| -> u16 {
                if p[3] < 64 {
                    return 0;
                }
                ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u16
            };
            let lums = block.map(lum);
            let max_l = *lums.iter().max().unwrap();
            let min_l = *lums.iter().min().unwrap();
            let mid = (max_l as u32 + min_l as u32) / 2;

            let mut fg_r = 0u32;
            let mut fg_g = 0u32;
            let mut fg_b = 0u32;
            let mut fg_n = 0u32;
            let mut bg_r = 0u32;
            let mut bg_g = 0u32;
            let mut bg_b = 0u32;
            let mut bg_n = 0u32;
            let mut pattern = 0u8;

            // bit order: TL=3, TR=2, BL=1, BR=0
            for (i, (p, l)) in block.iter().zip(lums.iter()).enumerate() {
                let is_fg = *l as u32 > mid || (max_l == min_l && i < 2);
                if is_fg {
                    pattern |= 1 << (3 - i);
                    if p[3] >= 64 {
                        fg_r += p[0] as u32;
                        fg_g += p[1] as u32;
                        fg_b += p[2] as u32;
                        fg_n += 1;
                    }
                } else if p[3] >= 64 {
                    bg_r += p[0] as u32;
                    bg_g += p[1] as u32;
                    bg_b += p[2] as u32;
                    bg_n += 1;
                }
            }

            let fg = if fg_n > 0 {
                Color::Rgb(
                    (fg_r / fg_n) as u8,
                    (fg_g / fg_n) as u8,
                    (fg_b / fg_n) as u8,
                )
            } else {
                Color::Black
            };
            let bg = if bg_n > 0 {
                Color::Rgb(
                    (bg_r / bg_n) as u8,
                    (bg_g / bg_n) as u8,
                    (bg_b / bg_n) as u8,
                )
            } else {
                Color::Black
            };

            // If all 4 pixels are transparent, return a blank.
            let all_transparent = block.iter().all(|p| p[3] < 64);
            if all_transparent {
                return (" ", Color::Black, Color::Black);
            }

            (QUAD[pattern as usize], fg, bg)
        };

        let mut out = Vec::with_capacity(height);

        for _ in 0..pad_y {
            out.push(Line::from(Span::raw(" ".repeat(selected_width))));
        }

        for row in 0..term_rows {
            let mut spans: Vec<Span<'static>> = Vec::new();
            if pad_x > 0 {
                spans.push(Span::raw(" ".repeat(pad_x)));
            }

            let py0 = row * 2;
            let py1 = py0 + 1;

            for col in 0..term_cols {
                let px0 = col * 2;
                let px1 = px0 + 1;
                let block = [
                    sample(px0, py0), // TL
                    sample(px1, py0), // TR
                    sample(px0, py1), // BL
                    sample(px1, py1), // BR
                ];
                let (ch, fg, bg) = quantize(block);
                spans.push(Span::styled(ch, Style::default().fg(fg).bg(bg)));
            }

            let used = pad_x + term_cols;
            if used < selected_width {
                spans.push(Span::raw(" ".repeat(selected_width - used)));
            }
            out.push(Line::from(spans));
        }

        while out.len() < height {
            out.push(Line::from(Span::raw(" ".repeat(selected_width))));
        }

        out.truncate(height);
        out
    }

    fn render_text_like_lines(
        &self,
        selected_width: usize,
        start: usize,
        height: usize,
    ) -> Vec<Line<'static>> {
        let lines = self.current_plain_lines();
        let display_lines = lines
            .iter()
            .skip(start)
            .take(height)
            .map(|line| self.display_line(line, selected_width))
            .collect::<Vec<_>>();

        if let Some(highlighted) = self.render_plugin_lines(&display_lines, selected_width) {
            return highlighted;
        }

        // Syntax highlight ─────────────────────────────────────────────────
        if self.mask_enabled {
            if let Some(lang) = syntax::effective_lang(self.mask, &self.path) {
                // Pre-scan lines before the visible area to determine the
                // block-comment state at `start`.
                let mut bc = false;
                for orig in lines.iter().take(start) {
                    syntax::scan_line_state(orig, lang, &mut bc);
                }
                // Render each visible line; highlight_line also advances `bc`.
                // We use the original line to update state for accuracy, but
                // render the (possibly hscroll-clipped) display line.
                return lines
                    .iter()
                    .skip(start)
                    .take(height)
                    .zip(display_lines.into_iter())
                    .map(|(orig, display)| {
                        // Save state, render display line, restore and re-advance
                        // using the original line so multi-line comment tracking
                        // is not confused by horizontal scrolling.
                        let bc_before = bc;
                        let rendered = syntax::highlight_line(&display, lang, &mut bc);
                        bc = bc_before;
                        syntax::scan_line_state(orig, lang, &mut bc);
                        rendered
                    })
                    .collect();
            }
        }

        display_lines
            .into_iter()
            .map(|line| Line::from(Span::raw(line)))
            .collect()
    }

    pub fn current_plain_lines(&self) -> &[String] {
        match self.mode {
            ViewMode::Text => &self.text_lines,
            ViewMode::Hex => &[],
            ViewMode::Ansi => &self.ansi_lines,
            ViewMode::Image => &[],
        }
    }

    fn plain_line_at(&self, idx: usize) -> String {
        match self.mode {
            ViewMode::Text => self.text_lines.get(idx).cloned().unwrap_or_default(),
            ViewMode::Hex => self.hex_plain_line_at(idx),
            ViewMode::Ansi => self.ansi_lines.get(idx).cloned().unwrap_or_default(),
            ViewMode::Image => String::new(),
        }
    }

    fn hex_line_count(&self) -> usize {
        let bpr = self.hex_bytes_per_row.get();
        self.raw.len().div_ceil(bpr).max(1)
    }

    fn hex_plain_line_at(&self, idx: usize) -> String {
        let bpr = self.hex_bytes_per_row.get();
        let offset = idx.saturating_mul(bpr);
        if offset >= self.raw.len() {
            return String::new();
        }
        let end = offset.saturating_add(bpr).min(self.raw.len());
        let chunk = preprocess_bytes(&self.raw[offset..end], &self.preproc_ops);
        hex_line(offset, &chunk, bpr, self.encoding)
    }

    fn render_hex_lines(
        &self,
        selected_width: usize,
        start: usize,
        height: usize,
    ) -> Vec<Line<'static>> {
        // Compute bytes-per-row from the available panel width and cache it so that
        // hex_line_count() and scroll arithmetic stay consistent across frames.
        // Formula: total line width = 8 (offset) + 2 + (bpr*3-1) + 2 + bpr = 11 + 4*bpr
        let bpr = ((selected_width.saturating_sub(11)) / 4).max(1).min(16);
        self.hex_bytes_per_row.set(bpr);
        let end = start.saturating_add(height).min(self.hex_line_count());
        (start..end)
            .map(|idx| {
                let line = self.hex_plain_line_at(idx);
                let display = if self.wrap {
                    line
                } else {
                    let shifted = slice_visible(&line, self.hscroll, selected_width);
                    pad_visible(&shifted, selected_width)
                };
                Line::from(Span::raw(display))
            })
            .collect()
    }

    fn rebuild_matches(&mut self) {
        if self.search.is_empty() {
            self.matches.clear();
            self.match_pos = 0;
            return;
        }
        self.matches = if matches!(self.mode, ViewMode::Hex) {
            self.rebuild_hex_matches()
        } else if let Some(lines) = self.plugin_document_plain_lines(self.plugin_document_width()) {
            let needle = self.search.to_lowercase();
            lines
                .iter()
                .enumerate()
                .filter_map(|(idx, line)| line.to_lowercase().contains(&needle).then_some(idx))
                .collect()
        } else {
            let needle = self.search.to_lowercase();
            (0..self.line_count())
                .filter(|&idx| self.plain_line_at(idx).to_lowercase().contains(&needle))
                .collect()
        };
        if self.match_pos >= self.matches.len() {
            self.match_pos = 0;
        }
    }

    fn rebuild_hex_matches(&self) -> Vec<usize> {
        let bpr = self.hex_bytes_per_row.get();
        if let Some(bytes) = parse_hex_query(&self.search) {
            if bytes.is_empty() || bytes.len() > self.raw.len() {
                return Vec::new();
            }
            let mut matches = Vec::new();
            for start in 0..=self.raw.len() - bytes.len() {
                if self.raw[start..start + bytes.len()] == *bytes {
                    matches.push(start / bpr);
                }
            }
            matches.sort_unstable();
            matches.dedup();
            matches
        } else {
            let needle = self.search.to_ascii_lowercase().into_bytes();
            if needle.is_empty() || needle.len() > self.raw.len() {
                return Vec::new();
            }
            let mut matches = Vec::new();
            for start in 0..=self.raw.len() - needle.len() {
                if self.raw[start..start + needle.len()]
                    .iter()
                    .zip(&needle)
                    .all(|(hay, needle)| hay.to_ascii_lowercase() == *needle)
                {
                    matches.push(start / bpr);
                }
            }
            matches.sort_unstable();
            matches.dedup();
            matches
        }
    }

    fn display_line(&self, line: &str, width: usize) -> String {
        if self.wrap {
            line.to_string()
        } else {
            let shifted = slice_visible(line, self.hscroll, width);
            pad_visible(&shifted, width)
        }
    }

    fn render_plugin_lines(
        &self,
        display_lines: &[String],
        width: usize,
    ) -> Option<Vec<Line<'static>>> {
        let mode = match self.mode {
            ViewMode::Text => "text",
            ViewMode::Ansi => "ansi",
            _ => return None,
        };
        let plugin_name = self.viewer_plugin.as_deref()?;
        let highlighted =
            crate::plugins::highlight_viewer_lines(&self.path, mode, plugin_name, display_lines)?;
        Some(
            highlighted
                .into_iter()
                .map(|spans| viewer_plugin_line(&spans, width))
                .collect(),
        )
    }

    fn plugin_document_line_count(&self) -> Option<usize> {
        if let Some(cache) = self.plugin_document_cache.borrow().as_ref() {
            return Some(cache.lines.len());
        }
        self.ensure_plugin_document_cache(self.plugin_document_width())?;
        self.plugin_document_cache
            .borrow()
            .as_ref()
            .map(|cache| cache.lines.len())
    }

    fn plugin_document_width(&self) -> usize {
        terminal_size()
            .map(|(cols, _)| cols.saturating_sub(2).max(1) as usize)
            .unwrap_or(120)
    }

    fn plugin_document_plain_lines(&self, width: usize) -> Option<Vec<String>> {
        self.ensure_plugin_document_cache(width)?;
        self.plugin_document_cache.borrow().as_ref().map(|cache| {
            cache
                .lines
                .iter()
                .map(|line| {
                    line.iter()
                        .map(|span| sanitize_plugin_text(&span.text))
                        .collect::<String>()
                })
                .collect()
        })
    }

    fn render_plugin_document_lines(
        &self,
        start: usize,
        height: usize,
        width: usize,
    ) -> Option<Vec<Line<'static>>> {
        self.ensure_plugin_document_cache(width)?;
        self.plugin_document_cache.borrow().as_ref().map(|cache| {
            cache
                .lines
                .iter()
                .skip(start)
                .take(height)
                .map(|line| viewer_plugin_line(line, width))
                .collect()
        })
    }

    fn ensure_plugin_document_cache(&self, width: usize) -> Option<()> {
        let mode = self.viewer_mode_key()?;
        let plugin_name = self.viewer_plugin.as_deref()?;
        let key = PluginDocumentCacheKey {
            plugin_name: plugin_name.to_string(),
            mode,
            state: plugin_state_cache_key(&self.plugin_state),
            width,
        };
        if let Some(cache) = self.plugin_document_cache.borrow().as_ref() {
            if cache.key == key {
                return Some(());
            }
        }

        let lines = crate::plugins::render_viewer_document(
            &self.path,
            mode,
            plugin_name,
            &self.plugin_state,
            width,
        )?;
        *self.plugin_document_cache.borrow_mut() = Some(PluginDocumentCache { key, lines });
        Some(())
    }

    fn viewer_mode_key(&self) -> Option<&'static str> {
        match self.mode {
            ViewMode::Text => Some("text"),
            ViewMode::Ansi => Some("ansi"),
            _ => None,
        }
    }

    /// Forward a key event to the active viewer plugin.
    /// Returns `true` if the plugin consumed the key (skip normal handling).
    pub fn handle_plugin_key(&mut self, key: &str) -> bool {
        let mode = match self.viewer_mode_key() {
            Some(m) => m,
            None => return false,
        };
        let plugin_name = match self.viewer_plugin.as_deref() {
            Some(p) => p.to_string(),
            None => return false,
        };
        match crate::plugins::handle_viewer_key(
            &self.path,
            mode,
            &plugin_name,
            key,
            &self.plugin_state,
        ) {
            Some((consumed, new_state)) => {
                self.plugin_state = new_state;
                self.rebuild_matches();
                consumed
            }
            None => false,
        }
    }

    fn rebuild_decoded_lines(&mut self) {
        // Clear cached lines — other modes will be rebuilt lazily when accessed.
        self.text_lines = Vec::new();
        self.ansi_lines = Vec::new();
        self.image = detect_image_info(&self.path, &self.raw);
        // Immediately rebuild only the currently active mode.
        let mode = self.mode;
        self.ensure_mode_decoded(mode);
    }

    fn ensure_mode_decoded(&mut self, mode: ViewMode) {
        match mode {
            ViewMode::Text => {
                if self.text_lines.is_empty() {
                    self.text_lines =
                        text_lines(&self.raw, self.line_feed, &self.preproc_ops, self.encoding);
                }
            }
            ViewMode::Hex => {}
            ViewMode::Ansi => {
                if self.ansi_lines.is_empty() {
                    self.ansi_lines =
                        ansi_lines(&self.raw, self.line_feed, &self.preproc_ops, self.encoding);
                }
            }
            ViewMode::Image => {}
        }
    }

    pub fn save_position(&self) {
        if !self.save_position {
            return;
        }
        if let Ok(mut positions) = viewer_positions().lock() {
            positions.insert(
                self.path.clone(),
                ViewerPosition {
                    scroll: self.scroll,
                    hscroll: self.hscroll,
                },
            );
        }
    }

    fn restore_position(&mut self) {
        if !self.save_position {
            return;
        }
        if let Ok(positions) = viewer_positions().lock()
            && let Some(pos) = positions.get(&self.path).copied()
        {
            self.scroll = pos.scroll.min(self.line_count().saturating_sub(1));
            self.hscroll = pos.hscroll;
        }
    }
}

fn ordered_mouse_points(a: MouseTextPoint, b: MouseTextPoint) -> (MouseTextPoint, MouseTextPoint) {
    if (a.row, a.column) <= (b.row, b.column) {
        (a, b)
    } else {
        (b, a)
    }
}

fn slice_display_columns(s: &str, start: usize, end: usize) -> String {
    if start >= end {
        return String::new();
    }

    let mut out = String::new();
    let mut width = 0usize;
    for ch in s.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1);
        let next = width + ch_width;
        if next <= start {
            width = next;
            continue;
        }
        if width >= end {
            break;
        }
        out.push(ch);
        width = next;
    }
    out
}

fn viewer_plugin_color(name: &str) -> Color {
    match name.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => Color::White,
    }
}

fn viewer_plugin_style(span: &crate::plugins::ViewerSpan) -> Style {
    let mut style = Style::default().fg(viewer_plugin_color(&span.fg));
    if let Some(bg) = &span.bg {
        style = style.bg(viewer_plugin_color(bg));
    }
    if span.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

fn take_display_width(s: &str, max_width: usize) -> (String, usize) {
    let mut out = String::new();
    let mut width = 0usize;
    for ch in s.chars() {
        let ch_width = ch.width().unwrap_or(1);
        if width + ch_width > max_width {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    (out, width)
}

fn sanitize_plugin_text(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\t' => out.push_str("    "),
            ch if ch.is_control() => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

fn plugin_state_cache_key(state: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut entries = state
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    entries.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    entries
}

fn viewer_plugin_line(spans: &[crate::plugins::ViewerSpan], width: usize) -> Line<'static> {
    if width == 0 {
        return Line::from(String::new());
    }

    let mut out = Vec::new();
    let mut used = 0usize;
    for span in spans {
        if used >= width {
            break;
        }

        let style = viewer_plugin_style(&span);
        let available = width - used;
        let safe_text = sanitize_plugin_text(&span.text);
        let span_width = UnicodeWidthStr::width(safe_text.as_str());
        let (text, rendered_width) = if span_width <= available {
            (safe_text, span_width)
        } else {
            take_display_width(&safe_text, available)
        };
        if !text.is_empty() {
            out.push(Span::styled(text, style));
            used += rendered_width;
        }
    }

    if used < width {
        out.push(Span::styled(
            " ".repeat(width - used),
            Style::default().fg(Color::White).bg(Color::Black),
        ));
    }

    Line::from(out)
}

pub fn kitty_graphics_supported() -> bool {
    env::var_os("KITTY_WINDOW_ID").is_some()
        || env::var("TERM")
            .map(|term| term.contains("kitty"))
            .unwrap_or(false)
        || matches!(
            env::var("TERM_PROGRAM").ok().as_deref(),
            Some("ghostty") | Some("WezTerm")
        )
}

pub fn clear_kitty_images<W: Write>(out: &mut W) -> Result<()> {
    write!(out, "\x1b_Ga=d,d=A\x1b\\")?;
    out.flush()?;
    Ok(())
}

fn rgba_at(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    let idx = (y * width + x) * 4;
    if idx + 3 < pixels.len() {
        [
            pixels[idx],
            pixels[idx + 1],
            pixels[idx + 2],
            pixels[idx + 3],
        ]
    } else {
        [0, 0, 0, 0]
    }
}

fn decode_rgba_for_fallback(raw: &[u8], _format: &'static str) -> Result<(u32, u32, Vec<u8>)> {
    let dyn_img = image::load_from_memory(raw).context("decoding image for fallback renderer")?;
    let rgba = dyn_img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((w, h, rgba.into_raw()))
}

pub fn render_kitty_image<W: Write>(out: &mut W, viewer: &Viewer, area: Rect) -> Result<()> {
    if !viewer.is_image_mode() || area.width == 0 || area.height == 0 {
        return Ok(());
    }

    let Some(image) = viewer.image_info() else {
        return Ok(());
    };
    let fit = fit_image_to_area(area, image.width, image.height);
    queue!(out, MoveTo(fit.x, fit.y))?;

    const RAW_CHUNK_LEN: usize = 3072;
    let payload = image
        .kitty_png
        .get_or_init(|| build_kitty_png_payload(&viewer.raw, image.format).ok());
    let Some(payload) = payload.as_ref() else {
        return Ok(());
    };
    let mut chunks = payload.chunks(RAW_CHUNK_LEN).peekable();
    let mut first = true;
    while let Some(chunk) = chunks.next() {
        let payload = base64::encode(chunk);
        if first {
            write!(
                out,
                "\x1b_Ga=T,f=100,i=1,c={},r={},m={};{}\x1b\\",
                fit.width,
                fit.height,
                usize::from(chunks.peek().is_some()),
                payload,
            )?;
            first = false;
        } else {
            write!(
                out,
                "\x1b_Gm={};{}\x1b\\",
                usize::from(chunks.peek().is_some()),
                payload,
            )?;
        }
    }
    out.flush()?;
    Ok(())
}

fn fit_image_to_area(area: Rect, image_width: Option<u32>, image_height: Option<u32>) -> Rect {
    let Some(img_w) = image_width.filter(|&w| w > 0) else {
        return area;
    };
    let Some(img_h) = image_height.filter(|&h| h > 0) else {
        return area;
    };

    let (cell_px_w, cell_px_h) = window_size()
        .ok()
        .filter(|ws| ws.columns > 0 && ws.rows > 0 && ws.width > 0 && ws.height > 0)
        .map(|ws| {
            (
                (ws.width as f32 / ws.columns as f32).max(1.0),
                (ws.height as f32 / ws.rows as f32).max(1.0),
            )
        })
        .unwrap_or((8.0, 16.0));

    let max_px_w = area.width as f32 * cell_px_w;
    let max_px_h = area.height as f32 * cell_px_h;
    let scale = (max_px_w / img_w as f32)
        .min(max_px_h / img_h as f32)
        .max(0.0);

    if scale <= 0.0 {
        return area;
    }

    let fitted_cols = ((img_w as f32 * scale) / cell_px_w).ceil() as u16;
    let fitted_rows = ((img_h as f32 * scale) / cell_px_h).ceil() as u16;
    let width = fitted_cols.clamp(1, area.width);
    let height = fitted_rows.clamp(1, area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn detect_image_info(path: &Path, data: &[u8]) -> Option<ImageInfo> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let (width, height) = png_dimensions(data);
        return Some(ImageInfo {
            format: "PNG",
            width,
            height,
            kitty_png: OnceLock::new(),
            decoded_rgba: OnceLock::new(),
        });
    }
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        let (width, height) = jpeg_dimensions(data);
        return Some(ImageInfo {
            format: "JPEG",
            width,
            height,
            kitty_png: OnceLock::new(),
            decoded_rgba: OnceLock::new(),
        });
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        let (width, height) = gif_dimensions(data);
        return Some(ImageInfo {
            format: "GIF",
            width,
            height,
            kitty_png: OnceLock::new(),
            decoded_rgba: OnceLock::new(),
        });
    }
    if data.starts_with(b"BM") {
        let (width, height) = bmp_dimensions(data);
        return Some(ImageInfo {
            format: "BMP",
            width,
            height,
            kitty_png: OnceLock::new(),
            decoded_rgba: OnceLock::new(),
        });
    }
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some(ImageInfo {
            format: "WEBP",
            width: None,
            height: None,
            kitty_png: OnceLock::new(),
            decoded_rgba: OnceLock::new(),
        });
    }
    if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp"
    ) {
        return Some(ImageInfo {
            format: match ext.as_str() {
                "png" => "PNG",
                "jpg" | "jpeg" => "JPEG",
                "gif" => "GIF",
                "bmp" => "BMP",
                "webp" => "WEBP",
                _ => "Image",
            },
            width: None,
            height: None,
            kitty_png: OnceLock::new(),
            decoded_rgba: OnceLock::new(),
        });
    }
    None
}

fn build_kitty_png_payload(raw: &[u8], format: &'static str) -> Result<Vec<u8>> {
    if format == "PNG" {
        return Ok(raw.to_vec());
    }

    let guessed = ImageReader::new(Cursor::new(raw))
        .with_guessed_format()
        .context("guessing image format")?;
    let image = guessed.decode().context("decoding image for viewer")?;
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, ImageFormat::Png)
        .context("encoding PNG for kitty graphics")?;
    Ok(out.into_inner())
}

fn png_dimensions(data: &[u8]) -> (Option<u32>, Option<u32>) {
    if data.len() < 24 {
        return (None, None);
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    (Some(width), Some(height))
}

fn gif_dimensions(data: &[u8]) -> (Option<u32>, Option<u32>) {
    if data.len() < 10 {
        return (None, None);
    }
    let width = u16::from_le_bytes([data[6], data[7]]) as u32;
    let height = u16::from_le_bytes([data[8], data[9]]) as u32;
    (Some(width), Some(height))
}

fn bmp_dimensions(data: &[u8]) -> (Option<u32>, Option<u32>) {
    if data.len() < 26 {
        return (None, None);
    }
    let width = u32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let height = u32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    (Some(width), Some(height))
}

fn jpeg_dimensions(data: &[u8]) -> (Option<u32>, Option<u32>) {
    let mut i = 2usize;
    while i + 8 < data.len() {
        if data[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        i += 2;
        if marker == 0xD8 || marker == 0xD9 {
            continue;
        }
        if i + 2 > data.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
        if seg_len < 2 || i + seg_len > data.len() {
            break;
        }
        if matches!(
            marker,
            0xC0 | 0xC1
                | 0xC2
                | 0xC3
                | 0xC5
                | 0xC6
                | 0xC7
                | 0xC9
                | 0xCA
                | 0xCB
                | 0xCD
                | 0xCE
                | 0xCF
        ) && seg_len >= 7
        {
            let height = u16::from_be_bytes([data[i + 3], data[i + 4]]) as u32;
            let width = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            return (Some(width), Some(height));
        }
        i += seg_len;
    }
    (None, None)
}
