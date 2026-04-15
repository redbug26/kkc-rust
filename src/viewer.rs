use anyhow::{Context, Result};
use crossterm::{cursor::MoveTo, queue, terminal::window_size};
use image::{ImageFormat, ImageReader};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{Cursor, Write};
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
    image: Option<ImageInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Text,
    Hex,
    Ansi,
    Eml,
    Html,
    Image,
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub format: &'static str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    kitty_png: OnceLock<Option<Vec<u8>>>,
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
        let image = detect_image_info(path, &raw);
        let mode = detect_mode(path, &raw);
        let load_decoded = !matches!(mode, ViewMode::Image);
        let text_lines = if load_decoded {
            text_lines(&raw, line_feed, &[], encoding)
        } else {
            Vec::new()
        };
        let hex_lines = if load_decoded {
            hex_lines(&raw, encoding)
        } else {
            Vec::new()
        };
        let ansi_lines = if load_decoded {
            ansi_lines(&raw, line_feed, &[], encoding)
        } else {
            Vec::new()
        };
        let eml_lines = if load_decoded {
            eml_lines(&raw)
        } else {
            Vec::new()
        };
        let eml_rendered = if load_decoded {
            eml_render_lines(&raw)
        } else {
            Vec::new()
        };
        let html = if load_decoded {
            html_document(&raw)
        } else {
            HtmlDocument {
                lines: Vec::new(),
                anchors: HashMap::new(),
                links: Vec::new(),
            }
        };

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
            image,
        };
        if matches!(viewer.mode, ViewMode::Image) {
            viewer.zoomed = true;
        }
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
        match self.mode {
            ViewMode::Html => self.html.lines.len().max(1),
            ViewMode::Eml => self.eml_lines.len().max(1),
            ViewMode::Image => 1,
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
        self.ensure_mode_decoded(mode);
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
        if matches!(
            self.mode,
            ViewMode::Text | ViewMode::Ansi | ViewMode::Html | ViewMode::Eml
        ) {
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
        self.match_pos = self
            .match_pos
            .checked_sub(1)
            .unwrap_or(self.matches.len() - 1);
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
            ViewMode::Image => vec![Line::from(Span::raw(String::new()))],
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
            ViewMode::Image => &[],
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
            ViewMode::Image => String::new(),
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
                let style = if keywords.iter().any(|kw| kw.eq_ignore_ascii_case(&token)) {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
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
        self.hex_lines = hex_lines(
            &preprocess_bytes(&self.raw, &self.preproc_ops),
            self.encoding,
        );
        self.ansi_lines = ansi_lines(&self.raw, self.line_feed, &self.preproc_ops, self.encoding);
        self.eml_lines = eml_lines(&self.raw);
        self.eml_rendered = eml_render_lines(&self.raw);
        self.html = html_document(&self.raw);
        self.image = detect_image_info(&self.path, &self.raw);
    }

    fn ensure_mode_decoded(&mut self, mode: ViewMode) {
        match mode {
            ViewMode::Text => {
                if self.text_lines.is_empty() {
                    self.text_lines =
                        text_lines(&self.raw, self.line_feed, &self.preproc_ops, self.encoding);
                }
            }
            ViewMode::Hex => {
                if self.hex_lines.is_empty() {
                    self.hex_lines = hex_lines(
                        &preprocess_bytes(&self.raw, &self.preproc_ops),
                        self.encoding,
                    );
                }
            }
            ViewMode::Ansi => {
                if self.ansi_lines.is_empty() {
                    self.ansi_lines =
                        ansi_lines(&self.raw, self.line_feed, &self.preproc_ops, self.encoding);
                }
            }
            ViewMode::Eml => {
                if self.eml_lines.is_empty() {
                    self.eml_lines = eml_lines(&self.raw);
                }
                if self.eml_rendered.is_empty() {
                    self.eml_rendered = eml_render_lines(&self.raw);
                }
            }
            ViewMode::Html => {
                if self.html.lines.is_empty()
                    && (self.html.anchors.is_empty() && self.html.links.is_empty())
                {
                    self.html = html_document(&self.raw);
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
        });
    }
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        let (width, height) = jpeg_dimensions(data);
        return Some(ImageInfo {
            format: "JPEG",
            width,
            height,
            kitty_png: OnceLock::new(),
        });
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        let (width, height) = gif_dimensions(data);
        return Some(ImageInfo {
            format: "GIF",
            width,
            height,
            kitty_png: OnceLock::new(),
        });
    }
    if data.starts_with(b"BM") {
        let (width, height) = bmp_dimensions(data);
        return Some(ImageInfo {
            format: "BMP",
            width,
            height,
            kitty_png: OnceLock::new(),
        });
    }
    if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return Some(ImageInfo {
            format: "WEBP",
            width: None,
            height: None,
            kitty_png: OnceLock::new(),
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
