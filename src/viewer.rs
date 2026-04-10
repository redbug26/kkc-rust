use anyhow::{Context, Result};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn viewer_positions() -> &'static Mutex<HashMap<PathBuf, ViewerPosition>> {
    static POSITIONS: OnceLock<Mutex<HashMap<PathBuf, ViewerPosition>>> = OnceLock::new();
    POSITIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug)]
pub struct Viewer {
    pub path: PathBuf,
    pub raw: Vec<u8>,
    pub scroll: usize,
    pub hscroll: usize,
    pub mode: ViewMode,
    pub wrap: bool,
    pub search: String,
    pub matches: Vec<usize>,
    pub match_pos: usize,
    pub html_selected_link: usize,
    pub zoomed: bool,
    pub save_position: bool,
    pub encoding: EncodingMode,
    pub line_feed: LineFeedMode,
    pub mask: MaskKind,
    pub mask_enabled: bool,
    pub preproc_ops: Vec<PreprocOp>,
    text_lines: Vec<String>,
    hex_lines: Vec<String>,
    ansi_lines: Vec<String>,
    html: HtmlDocument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Text,
    Hex,
    Ansi,
    Html,
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
    C,
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

#[derive(Debug, Clone)]
pub struct HtmlDocument {
    pub lines: Vec<HtmlLine>,
    pub anchors: HashMap<String, usize>,
    pub links: Vec<HtmlLinkRef>,
}

#[derive(Debug, Clone)]
pub struct HtmlLine {
    pub spans: Vec<HtmlSpan>,
    pub plain: String,
}

#[derive(Debug, Clone)]
pub struct HtmlSpan {
    pub text: String,
    pub href: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HtmlLinkRef {
    pub line: usize,
    pub href: String,
}

#[derive(Debug, Clone, Copy)]
struct ViewerPosition {
    scroll: usize,
    hscroll: usize,
}

impl Viewer {
    pub fn open(path: &Path, wrap: bool) -> Result<Self> {
        let raw = fs::read(path).with_context(|| format!("Reading {}", path.display()))?;
        let line_feed = LineFeedMode::Mixed;
        let encoding = EncodingMode::Cp437;
        let text_lines = text_lines(&raw, line_feed, &[], encoding);
        let hex_lines = hex_lines(&raw, encoding);
        let ansi_lines = ansi_lines(&raw, line_feed, &[], encoding);
        let html = html_document(&raw);
        let mode = detect_mode(&raw);

        let mut viewer = Self {
            path: path.to_path_buf(),
            raw,
            scroll: 0,
            hscroll: 0,
            mode,
            wrap,
            search: String::new(),
            matches: Vec::new(),
            match_pos: 0,
            html_selected_link: 0,
            zoomed: false,
            save_position: true,
            encoding,
            line_feed,
            mask: MaskKind::Ketchup,
            mask_enabled: true,
            preproc_ops: Vec::new(),
            text_lines,
            hex_lines,
            ansi_lines,
            html,
        };
        viewer.restore_position();
        viewer.rebuild_matches();
        Ok(viewer)
    }

    pub fn mode_label(&self) -> &'static str {
        match self.mode {
            ViewMode::Text => "Text",
            ViewMode::Hex => "Hex",
            ViewMode::Ansi => "Ansi",
            ViewMode::Html => "Html",
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
                MaskKind::C => "C",
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
            format!("{}+{}", preproc_op_label(self.preproc_ops[0]), self.preproc_ops.len() - 1)
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
        match self.mode {
            ViewMode::Html => self.html.lines.len().max(1),
            _ => self.current_plain_lines().len().max(1),
        }
    }

    pub fn set_mode(&mut self, mode: ViewMode) {
        self.mode = mode;
        self.scroll = 0;
        self.hscroll = 0;
        self.html_selected_link = 0;
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
            PreprocOp::Xor(v) | PreprocOp::And(v) | PreprocOp::Or(v) | PreprocOp::Ror(v) | PreprocOp::Add(v) => Some(v),
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
        let Some(op) = self.preproc_ops.get_mut(idx) else { return; };
        match op {
            PreprocOp::Xor(v) | PreprocOp::And(v) | PreprocOp::Or(v) | PreprocOp::Ror(v) | PreprocOp::Add(v) => {
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
        if matches!(self.mode, ViewMode::Text | ViewMode::Ansi | ViewMode::Html) {
            self.wrap = !self.wrap;
        }
    }

    pub fn toggle_zoom(&mut self) {
        self.zoomed = !self.zoomed;
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

    pub fn goto_end(&mut self, height: usize) {
        self.scroll = self.line_count().saturating_sub(height.max(1));
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
        self.match_pos = self.match_pos.checked_sub(1).unwrap_or(self.matches.len() - 1);
        self.scroll = self.matches[self.match_pos];
    }

    pub fn html_next_link(&mut self) {
        if !matches!(self.mode, ViewMode::Html) || self.html.links.is_empty() {
            return;
        }
        self.html_selected_link = (self.html_selected_link + 1) % self.html.links.len();
        self.scroll = self.html.links[self.html_selected_link].line;
    }

    pub fn html_prev_link(&mut self) {
        if !matches!(self.mode, ViewMode::Html) || self.html.links.is_empty() {
            return;
        }
        self.html_selected_link = if self.html_selected_link == 0 {
            self.html.links.len() - 1
        } else {
            self.html_selected_link - 1
        };
        self.scroll = self.html.links[self.html_selected_link].line;
    }

    pub fn html_follow_link(&mut self) -> bool {
        if !matches!(self.mode, ViewMode::Html) || self.html.links.is_empty() {
            return false;
        }
        let href = self.html.links[self.html_selected_link].href.clone();
        if let Some(anchor) = href.strip_prefix('#') {
            if let Some(line) = self.html.anchors.get(&anchor.to_ascii_lowercase()) {
                self.scroll = *line;
                return true;
            }
        }
        false
    }

    pub fn render_lines(&self, selected_width: usize) -> Vec<Line<'static>> {
        match self.mode {
            ViewMode::Html => self.render_html_lines(),
            ViewMode::Text | ViewMode::Ansi => self
                .current_plain_lines()
                .iter()
                .map(|line| self.render_masked_line(line, selected_width))
                .collect(),
            ViewMode::Hex => self
                .current_plain_lines()
                .iter()
                .map(|line| {
                    let display = if self.wrap {
                        line.clone()
                    } else {
                        let shifted = slice_visible(line, self.hscroll, selected_width);
                        pad_visible(&shifted, selected_width)
                    };
                    Line::from(Span::raw(display))
                })
                .collect(),
        }
    }

    pub fn current_plain_lines(&self) -> &[String] {
        match self.mode {
            ViewMode::Text => &self.text_lines,
            ViewMode::Hex => &self.hex_lines,
            ViewMode::Ansi => &self.ansi_lines,
            ViewMode::Html => &[],
        }
    }

    fn plain_line_at(&self, idx: usize) -> String {
        match self.mode {
            ViewMode::Text => self.text_lines.get(idx).cloned().unwrap_or_default(),
            ViewMode::Hex => self.hex_lines.get(idx).cloned().unwrap_or_default(),
            ViewMode::Ansi => self.ansi_lines.get(idx).cloned().unwrap_or_default(),
            ViewMode::Html => self
                .html
                .lines
                .get(idx)
                .map(|line| line.plain.clone())
                .unwrap_or_default(),
        }
    }

    fn rebuild_matches(&mut self) {
        if self.search.is_empty() {
            self.matches.clear();
            self.match_pos = 0;
            return;
        }
        self.matches = if matches!(self.mode, ViewMode::Hex) {
            self.rebuild_hex_matches()
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
        if let Some(bytes) = parse_hex_query(&self.search) {
            if bytes.is_empty() || bytes.len() > self.raw.len() {
                return Vec::new();
            }
            let mut matches = Vec::new();
            for start in 0..=self.raw.len() - bytes.len() {
                if self.raw[start..start + bytes.len()] == *bytes {
                    matches.push(start / 16);
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
            let hay = self
                .raw
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect::<Vec<_>>();
            let mut matches = Vec::new();
            for start in 0..=hay.len() - needle.len() {
                if hay[start..start + needle.len()] == needle {
                    matches.push(start / 16);
                }
            }
            matches.sort_unstable();
            matches.dedup();
            matches
        }
    }

    fn render_html_lines(&self) -> Vec<Line<'static>> {
        let mut current_link = 0usize;
        self.html
            .lines
            .iter()
            .map(|line| {
                let spans = line
                    .spans
                    .iter()
                    .map(|span| {
                        let style = if span.href.is_some() {
                            let style = if current_link == self.html_selected_link {
                                Style::default()
                                    .fg(Color::Black)
                                    .bg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::UNDERLINED)
                            };
                            current_link += 1;
                            style
                        } else {
                            Style::default().fg(Color::White)
                        };
                        Span::styled(span.text.clone(), style)
                    })
                    .collect::<Vec<_>>();
                Line::from(spans)
            })
            .collect()
    }

    fn render_masked_line(&self, line: &str, width: usize) -> Line<'static> {
        let display = if self.wrap {
            line.to_string()
        } else {
            let shifted = slice_visible(line, self.hscroll, width);
            pad_visible(&shifted, width)
        };
        if !self.mask_enabled {
            return Line::from(Span::raw(display));
        }

        let keywords = mask_keywords(self.mask);
        let mut spans = Vec::new();
        let chars: Vec<char> = display.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            let ch = chars[i];
            if ch.is_ascii_alphanumeric() || ch == '_' {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let token: String = chars[start..i].iter().collect();
                let style = if keywords
                    .iter()
                    .any(|kw| kw.eq_ignore_ascii_case(&token))
                {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                spans.push(Span::styled(token, style));
            } else {
                spans.push(Span::styled(
                    ch.to_string(),
                    Style::default().fg(Color::White),
                ));
                i += 1;
            }
        }
        Line::from(spans)
    }

    fn rebuild_decoded_lines(&mut self) {
        self.text_lines = text_lines(&self.raw, self.line_feed, &self.preproc_ops, self.encoding);
        self.hex_lines = hex_lines(&preprocess_bytes(&self.raw, &self.preproc_ops), self.encoding);
        self.ansi_lines = ansi_lines(&self.raw, self.line_feed, &self.preproc_ops, self.encoding);
    }

    pub fn save_position(&self) {
        if !self.save_position {
            return;
        }
        if let Ok(mut positions) = viewer_positions().lock() {
            positions.insert(
                self.path.clone(),
                ViewerPosition { scroll: self.scroll, hscroll: self.hscroll },
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

fn detect_mode(data: &[u8]) -> ViewMode {
    if looks_like_html(data) {
        ViewMode::Html
    } else if contains_ansi_escape(data) {
        ViewMode::Ansi
    } else if is_likely_binary(data) {
        ViewMode::Hex
    } else {
        ViewMode::Text
    }
}

fn contains_ansi_escape(data: &[u8]) -> bool {
    data.windows(2).any(|w| w == [0x1b, b'['])
}

fn looks_like_html(data: &[u8]) -> bool {
    let sample = String::from_utf8_lossy(&data[..data.len().min(8192)]).to_lowercase();
    sample.contains("<html") || sample.contains("<body") || sample.contains("<a href") || sample.contains("<!doctype html")
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

fn text_lines(data: &[u8], line_feed: LineFeedMode, preproc_ops: &[PreprocOp], encoding: EncodingMode) -> Vec<String> {
    let processed = preprocess_bytes(data, preproc_ops);
    let lines = split_line_bytes(&processed, line_feed)
        .into_iter()
        .map(|bytes| bytes.into_iter().map(|b| byte_to_display_char(b, encoding)).collect::<String>().replace('\t', "    "))
        .collect::<Vec<_>>();
    if lines.is_empty() { vec![String::new()] } else { lines }
}

fn ansi_lines(data: &[u8], line_feed: LineFeedMode, preproc_ops: &[PreprocOp], encoding: EncodingMode) -> Vec<String> {
    let processed = preprocess_bytes(data, preproc_ops);
    let text = ansi_to_text(&processed, line_feed, encoding);
    if text.is_empty() { vec![String::new()] } else { text }
}

fn hex_lines(data: &[u8], encoding: EncodingMode) -> Vec<String> {
    let mut lines = Vec::new();
    let width = 16;
    for (i, chunk) in data.chunks(width).enumerate() {
        let offset = i * width;
        let hex = chunk
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii: String = chunk
            .iter()
            .map(|&b| if b < 0x20 || b == 0x7f { '.' } else { byte_to_display_char(b, encoding) })
            .collect();
        lines.push(format!("{:08X}  {:<47}  {}", offset, hex, ascii));
    }
    if lines.is_empty() { vec![String::new()] } else { lines }
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

fn preprocess_bytes(data: &[u8], preproc_ops: &[PreprocOp]) -> Vec<u8> {
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
            if b.is_ascii() { b as char } else { '.' }
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
                    if let Some(line) = lines.get_mut(row)
                        && col < line.len()
                    {
                        line.truncate(col);
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
                if matches!(line_feed, LineFeedMode::DosCrLf | LineFeedMode::UnixLf | LineFeedMode::Mixed) {
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

fn parse_ansi_params(args: &str) -> Vec<u16> {
    if args.is_empty() {
        return vec![0];
    }
    args.split(';').filter_map(|p| p.parse::<u16>().ok()).collect()
}

fn preproc_op_label(op: PreprocOp) -> String {
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
    '\0','☺','☻','♥','♦','♣','♠','•','◘','○','◙','♂','♀','♪','♫','☼',
    '►','◄','↕','‼','¶','§','▬','↨','↑','↓','→','←','∟','↔','▲','▼',
    ' ','!','"','#','$','%','&','\'','(',')','*','+',',','-','.','/',
    '0','1','2','3','4','5','6','7','8','9',':',';','<','=','>','?',
    '@','A','B','C','D','E','F','G','H','I','J','K','L','M','N','O',
    'P','Q','R','S','T','U','V','W','X','Y','Z','[','\\',']','^','_',
    '`','a','b','c','d','e','f','g','h','i','j','k','l','m','n','o',
    'p','q','r','s','t','u','v','w','x','y','z','{','|','}','~','⌂',
    'Ç','ü','é','â','ä','à','å','ç','ê','ë','è','ï','î','ì','Ä','Å',
    'É','æ','Æ','ô','ö','ò','û','ù','ÿ','Ö','Ü','¢','£','¥','₧','ƒ',
    'á','í','ó','ú','ñ','Ñ','ª','º','¿','⌐','¬','½','¼','¡','«','»',
    '░','▒','▓','│','┤','Á','Â','À','©','╣','║','╗','╝','¢','¥','┐',
    '└','┴','┬','├','─','┼','ã','Ã','╚','╔','╩','╦','╠','═','╬','¤',
    'ð','Ð','Ê','Ë','È','ı','Í','Î','Ï','┘','┌','█','▄','¦','Ì','▀',
    'Ó','ß','Ô','Ò','õ','Õ','µ','þ','Þ','Ú','Û','Ù','ý','Ý','¯','´',
    '≡','±','‗','¾','¶','§','÷','¸','°','¨','·','¹','³','²','■',' ',
];

fn mask_keywords(mask: MaskKind) -> &'static [&'static str] {
    match mask {
        MaskKind::C => &[
            "asm", "break", "case", "char", "const", "continue", "default", "do", "double",
            "else", "enum", "extern", "float", "for", "goto", "if", "int", "long", "register",
            "return", "short", "signed", "sizeof", "static", "struct", "switch", "typedef",
            "union", "unsigned", "void", "volatile", "while",
        ],
        MaskKind::Pascal => &[
            "absolute", "and", "array", "begin", "case", "const", "div", "do", "downto", "else",
            "end", "file", "for", "function", "goto", "if", "implementation", "in", "inline",
            "interface", "label", "mod", "nil", "not", "of", "or", "packed", "procedure",
            "program", "record", "repeat", "set", "string", "then", "to", "type", "unit",
            "until", "uses", "var", "while", "with", "xor",
        ],
        MaskKind::Assembler => &[
            "mov", "push", "pop", "call", "ret", "cmp", "jmp", "je", "jne", "ja", "jb", "jg",
            "jl", "add", "sub", "mul", "div", "xor", "or", "and", "lea", "int", "db", "dw",
            "dd", "endp", "ends", "assume", "xlatb", "nop",
        ],
        MaskKind::Ketchup => &[
            "blackward", "ketchup", "killers", "redbug", "access", "darkangel", "off", "topy",
            "kennet", "typeone", "pulpe", "tyby", "djamm", "vatin", "marjorie", "katana",
            "ecstasy", "cray", "magicfred", "cobra", "z",
        ],
    }
}

fn html_document(data: &[u8]) -> HtmlDocument {
    let input = String::from_utf8_lossy(data).into_owned();
    let chars: Vec<char> = input.chars().collect();
    let mut lines: Vec<HtmlLine> = vec![HtmlLine { spans: Vec::new(), plain: String::new() }];
    let mut anchors = HashMap::new();
    let mut links = Vec::new();
    let mut in_pre = false;
    let mut current_href: Option<String> = None;
    let mut i = 0usize;
    let mut collapse_space = true;

    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '>' {
                j += 1;
            }
            if j >= chars.len() {
                break;
            }
            let raw_tag: String = chars[i + 1..j].iter().collect();
            let tag = raw_tag.trim();
            let lower = tag.to_ascii_lowercase();

            if lower.starts_with("a ") {
                if let Some(name) = attr_value(tag, "name").or_else(|| attr_value(tag, "id")) {
                    anchors.insert(name.to_ascii_lowercase(), lines.len().saturating_sub(1));
                }
                current_href = attr_value(tag, "href");
            } else if lower.starts_with("/a") {
                current_href = None;
            } else if lower.starts_with("br")
                || lower.starts_with("/p")
                || lower.starts_with("p")
                || lower.starts_with("/div")
                || lower.starts_with("div")
                || lower.starts_with("/h")
                || lower.starts_with("h1")
                || lower.starts_with("h2")
                || lower.starts_with("h3")
                || lower.starts_with("li")
                || lower.starts_with("hr")
            {
                push_html_line(&mut lines);
                collapse_space = true;
            } else if lower.starts_with("pre") {
                in_pre = true;
                push_html_line(&mut lines);
            } else if lower.starts_with("/pre") {
                in_pre = false;
                push_html_line(&mut lines);
            }

            i = j + 1;
            continue;
        }

        if chars[i] == '&' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != ';' && j - i < 10 {
                j += 1;
            }
            if j < chars.len() && chars[j] == ';' {
                let entity: String = chars[i + 1..j].iter().collect();
                append_html_text(&mut lines, &decode_entity(&entity), current_href.clone(), &mut links, &mut collapse_space, in_pre);
                i = j + 1;
                continue;
            }
        }

        let ch = chars[i];
        if ch == '\n' {
            push_html_line(&mut lines);
            collapse_space = true;
        } else {
            append_html_text(&mut lines, &ch.to_string(), current_href.clone(), &mut links, &mut collapse_space, in_pre);
        }
        i += 1;
    }

    while lines.last().is_some_and(|line| line.spans.is_empty() && line.plain.is_empty()) && lines.len() > 1 {
        lines.pop();
    }

    HtmlDocument { lines, anchors, links }
}

fn append_html_text(
    lines: &mut [HtmlLine],
    text: &str,
    href: Option<String>,
    links: &mut Vec<HtmlLinkRef>,
    collapse_space: &mut bool,
    in_pre: bool,
) {
    let line_idx = lines.len() - 1;
    let line = lines.last_mut().expect("html line exists");
    let normalized = if in_pre {
        text.to_string()
    } else if text.chars().all(char::is_whitespace) {
        if *collapse_space {
            String::new()
        } else {
            *collapse_space = true;
            " ".to_string()
        }
    } else {
        *collapse_space = false;
        text.to_string()
    };

    if normalized.is_empty() {
        return;
    }

    if let Some(last) = line.spans.last_mut()
        && last.href == href
    {
        last.text.push_str(&normalized);
    } else {
        if let Some(target) = href.clone() {
            links.push(HtmlLinkRef { line: line_idx, href: target });
        }
        line.spans.push(HtmlSpan { text: normalized.clone(), href });
    }
    line.plain.push_str(&normalized);
}

fn push_html_line(lines: &mut Vec<HtmlLine>) {
    if lines.last().is_some_and(|line| line.spans.is_empty() && line.plain.is_empty()) {
        return;
    }
    lines.push(HtmlLine { spans: Vec::new(), plain: String::new() });
}

fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{}=", attr);
    let start = lower.find(&needle)?;
    let value = &tag[start + needle.len()..];
    let value = value.trim_start();
    if let Some(rest) = value.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else if let Some(rest) = value.strip_prefix('\'') {
        let end = rest.find('\'')?;
        Some(rest[..end].to_string())
    } else {
        let end = value.find(char::is_whitespace).unwrap_or(value.len());
        Some(value[..end].to_string())
    }
}

fn decode_entity(entity: &str) -> String {
    match entity.to_ascii_lowercase().as_str() {
        "nbsp" => " ".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "amp" => "&".into(),
        "quot" => "\"".into(),
        _ => format!("&{};", entity),
    }
}

fn parse_hex_query(query: &str) -> Option<Vec<u8>> {
    let compact = query
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect::<String>();
    if compact.is_empty() || compact.len() % 2 != 0 || !compact.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    let bytes = compact.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let pair = std::str::from_utf8(&bytes[i..i + 2]).ok()?;
        out.push(u8::from_str_radix(pair, 16).ok()?);
        i += 2;
    }
    Some(out)
}

fn slice_visible(s: &str, skip: usize, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut seen = 0usize;
    let mut kept = 0usize;
    for ch in s.chars() {
        if seen < skip {
            seen += 1;
            continue;
        }
        if kept >= max {
            break;
        }
        out.push(ch);
        kept += 1;
    }
    out
}

fn pad_visible(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = s.to_string();
    let mut count = out.chars().count();
    while count < max {
        out.push(' ');
        count += 1;
    }
    out
}
