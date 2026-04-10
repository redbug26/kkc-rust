use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Internal file viewer — loads and serves lines for display.
#[derive(Debug)]
pub struct Viewer {
    pub path: std::path::PathBuf,
    pub lines: Vec<String>,
    pub scroll: usize,
    pub mode: ViewMode,
    pub wrap: bool,
    /// Incremental search string.
    pub search: String,
    /// Indices of lines matching the current search.
    pub matches: Vec<usize>,
    pub match_pos: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Text,
    Hex,
}

impl Viewer {
    pub fn open(path: &Path, wrap: bool) -> Result<Self> {
        let raw = fs::read(path)
            .with_context(|| format!("Reading {}", path.display()))?;

        // Detect binary: if >30% non-UTF8 printable bytes → hex mode
        let mode = if is_likely_binary(&raw) {
            ViewMode::Hex
        } else {
            ViewMode::Text
        };

        let lines = match mode {
            ViewMode::Text => text_lines(&raw),
            ViewMode::Hex => hex_lines(&raw),
        };

        Ok(Self {
            path: path.to_path_buf(),
            lines,
            scroll: 0,
            mode,
            wrap,
            search: String::new(),
            matches: Vec::new(),
            match_pos: 0,
        })
    }

    pub fn switch_mode(&mut self) {
        let raw = fs::read(&self.path).unwrap_or_default();
        self.mode = match self.mode {
            ViewMode::Text => ViewMode::Hex,
            ViewMode::Hex => ViewMode::Text,
        };
        self.lines = match self.mode {
            ViewMode::Text => text_lines(&raw),
            ViewMode::Hex => hex_lines(&raw),
        };
        self.scroll = 0;
    }

    pub fn scroll_up(&mut self) {
        if self.scroll > 0 {
            self.scroll -= 1;
        }
    }

    pub fn scroll_down(&mut self) {
        if self.scroll + 1 < self.lines.len() {
            self.scroll += 1;
        }
    }

    pub fn page_up(&mut self, height: usize) {
        self.scroll = self.scroll.saturating_sub(height);
    }

    pub fn page_down(&mut self, height: usize) {
        let max = self.lines.len().saturating_sub(height);
        self.scroll = (self.scroll + height).min(max);
    }

    pub fn goto_start(&mut self) {
        self.scroll = 0;
    }

    pub fn goto_end(&mut self, height: usize) {
        self.scroll = self.lines.len().saturating_sub(height.max(1));
    }

    // -----------------------------------------------------------------------
    // Incremental search
    // -----------------------------------------------------------------------

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

    fn rebuild_matches(&mut self) {
        self.matches = if self.search.is_empty() {
            vec![]
        } else {
            let needle = self.search.to_lowercase();
            self.lines
                .iter()
                .enumerate()
                .filter(|(_, l)| l.to_lowercase().contains(&needle))
                .map(|(i, _)| i)
                .collect()
        };
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn text_lines(data: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(data);
    text.lines().map(|l| l.replace('\t', "    ")).collect()
}

fn hex_lines(data: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    let width = 16;
    for (i, chunk) in data.chunks(width).enumerate() {
        let offset = i * width;
        let hex: String = chunk
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii: String = chunk
            .iter()
            .map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' })
            .collect();
        lines.push(format!("{:08X}  {:<47}  {}", offset, hex, ascii));
    }
    lines
}
