use anyhow::{Context, Result};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

mod viewer_decode;
mod viewer_eml;
mod viewer_html;
mod viewer_render;
mod viewer_search;

use self::viewer_decode::{
    ansi_lines, detect_mode, hex_lines, preproc_op_label, preprocess_bytes, text_lines,
};
use self::viewer_eml::{eml_lines, eml_render_lines};
use self::viewer_html::html_document;
use self::viewer_render::{mask_keywords, pad_visible, slice_visible};
use self::viewer_search::parse_hex_query;

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
    eml_lines: Vec<String>,
    eml_rendered: Vec<Line<'static>>,
    html: HtmlDocument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Text,
    Hex,
    Ansi,
    Eml,
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
        let eml_lines = eml_lines(&raw);
        let eml_rendered = eml_render_lines(&raw);
        let html = html_document(&raw);
        let mode = detect_mode(path, &raw);

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
            eml_lines,
            eml_rendered,
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
            ViewMode::Eml => "EML",
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
            ViewMode::Eml => self.eml_lines.len().max(1),
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
        if matches!(self.mode, ViewMode::Text | ViewMode::Ansi | ViewMode::Html | ViewMode::Eml) {
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
        if matches!(self.mode, ViewMode::Text | ViewMode::Ansi | ViewMode::Eml) && !self.wrap {
            self.hscroll = self.hscroll.saturating_sub(amount);
        }
    }

    pub fn scroll_right(&mut self, amount: usize) {
        if matches!(self.mode, ViewMode::Text | ViewMode::Ansi | ViewMode::Eml) && !self.wrap {
            self.hscroll = self.hscroll.saturating_add(amount);
        }
    }

    pub fn scroll_left_max(&mut self) {
        if matches!(self.mode, ViewMode::Text | ViewMode::Ansi | ViewMode::Eml) {
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
            ViewMode::Eml => self.eml_rendered.clone(),
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
            ViewMode::Eml => &self.eml_lines,
            ViewMode::Html => &[],
        }
    }

    fn plain_line_at(&self, idx: usize) -> String {
        match self.mode {
            ViewMode::Text => self.text_lines.get(idx).cloned().unwrap_or_default(),
            ViewMode::Hex => self.hex_lines.get(idx).cloned().unwrap_or_default(),
            ViewMode::Ansi => self.ansi_lines.get(idx).cloned().unwrap_or_default(),
            ViewMode::Eml => self.eml_lines.get(idx).cloned().unwrap_or_default(),
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
        self.eml_lines = eml_lines(&self.raw);
        self.eml_rendered = eml_render_lines(&self.raw);
        self.html = html_document(&self.raw);
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
