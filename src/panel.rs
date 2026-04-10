use crate::config::SortMode;
use crate::file_types::FileCategory;
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// DirEntry wrapper
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<DateTime<Local>>,
    pub category: FileCategory,
    pub selected: bool,
    /// Unix permission bits (0o755 etc.)
    pub mode: u32,
}

impl Entry {
    pub fn from_path(path: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("stat: {}", path.display()))?;

        let is_symlink = metadata.file_type().is_symlink();
        // For directories/size resolve through the symlink
        let resolved = if is_symlink {
            fs::metadata(path).ok()
        } else {
            None
        };
        let real_meta = resolved.as_ref().unwrap_or(&metadata);

        let is_dir = real_meta.is_dir();
        let size = if is_dir { 0 } else { real_meta.len() };
        let modified = real_meta
            .modified()
            .ok()
            .map(|t| DateTime::<Local>::from(t));
        let mode = metadata.permissions().mode();

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let category = FileCategory::from_entry(is_dir, is_symlink, &name);

        Ok(Self {
            name,
            path: path.to_path_buf(),
            is_dir,
            is_symlink,
            size,
            modified,
            category,
            selected: false,
            mode,
        })
    }

    /// A single-char type indicator, similar to `ls -l`.
    #[allow(dead_code)]
    pub fn type_char(&self) -> char {
        if self.is_symlink {
            'l'
        } else if self.is_dir {
            'd'
        } else {
            '-'
        }
    }
}

// ---------------------------------------------------------------------------
// Panel state
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Panel {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    /// Index of the highlighted (cursor) entry.
    pub cursor: usize,
    /// Vertical scroll offset.
    pub scroll: usize,
    pub sort: SortMode,
    pub show_hidden: bool,
    /// Incremental name filter typed by the user.
    pub quicksearch: String,
}

impl Panel {
    pub fn new(path: PathBuf, sort: SortMode, show_hidden: bool) -> Self {
        let mut p = Self {
            path,
            entries: Vec::new(),
            cursor: 0,
            scroll: 0,
            sort,
            show_hidden,
            quicksearch: String::new(),
        };
        let _ = p.reload();
        p
    }

    /// Reload directory listing from disk.
    pub fn reload(&mut self) -> Result<()> {
        let rd = fs::read_dir(&self.path)
            .with_context(|| format!("Reading dir: {}", self.path.display()))?;

        let mut entries: Vec<Entry> = rd
            .filter_map(|res| res.ok())
            .filter_map(|de| Entry::from_path(&de.path()).ok())
            .filter(|e| self.show_hidden || !e.name.starts_with('.'))
            .collect();

        // Always keep ".." at the top
        let parent_entry = self.parent_entry();

        self.sort_entries(&mut entries);

        // Prepend ".."
        if let Some(pe) = parent_entry {
            entries.insert(0, pe);
        }

        // Preserve cursor on the same filename
        let old_name = self.entries.get(self.cursor).map(|e| e.name.clone());
        self.entries = entries;

        if let Some(name) = old_name {
            if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                self.cursor = idx;
            } else {
                self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
            }
        } else {
            self.cursor = 0;
        }

        Ok(())
    }

    fn parent_entry(&self) -> Option<Entry> {
        let parent = self.path.parent()?;
        Some(Entry {
            name: "..".into(),
            path: parent.to_path_buf(),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
            category: FileCategory::Directory,
            selected: false,
            mode: 0o755,
        })
    }

    fn sort_entries(&self, entries: &mut Vec<Entry>) {
        match self.sort {
            SortMode::Name => {
                entries.sort_by(|a, b| {
                    // directories first
                    b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            SortMode::Extension => {
                entries.sort_by(|a, b| {
                    let ext_a = ext_of(&a.name);
                    let ext_b = ext_of(&b.name);
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then(ext_a.cmp(&ext_b))
                        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            SortMode::Date => {
                entries.sort_by(|a, b| {
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then(b.modified.cmp(&a.modified))
                });
            }
            SortMode::Size => {
                entries.sort_by(|a, b| {
                    b.is_dir.cmp(&a.is_dir).then(b.size.cmp(&a.size))
                });
            }
            SortMode::Unsorted => {}
        }
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
        }
    }

    pub fn move_page_up(&mut self, page_size: usize) {
        self.cursor = self.cursor.saturating_sub(page_size);
    }

    pub fn move_page_down(&mut self, page_size: usize) {
        let max = self.entries.len().saturating_sub(1);
        self.cursor = (self.cursor + page_size).min(max);
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.entries.len().saturating_sub(1);
    }

    /// Update scroll so cursor stays visible in a window of `height` rows.
    pub fn clamp_scroll(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + height {
            self.scroll = self.cursor - height + 1;
        }
    }

    // -----------------------------------------------------------------------
    // Selection
    // -----------------------------------------------------------------------

    pub fn toggle_selected(&mut self) {
        if let Some(e) = self.entries.get_mut(self.cursor) {
            if e.name != ".." {
                e.selected = !e.selected;
            }
        }
    }

    #[allow(dead_code)]
    pub fn select_all(&mut self) {
        for e in &mut self.entries {
            if e.name != ".." {
                e.selected = true;
            }
        }
    }

    pub fn deselect_all(&mut self) {
        for e in &mut self.entries {
            e.selected = false;
        }
    }

    pub fn invert_selection(&mut self) {
        for e in &mut self.entries {
            if e.name != ".." {
                e.selected = !e.selected;
            }
        }
    }

    /// Select entries whose name matches `pattern` (glob-style `*` and `?`).
    pub fn select_pattern(&mut self, pattern: &str, value: bool) {
        let pat = pattern.to_lowercase();
        for e in &mut self.entries {
            if e.name == ".." {
                continue;
            }
            if glob_match(&pat, &e.name.to_lowercase()) {
                e.selected = value;
            }
        }
    }

    /// Returns selected entries, or the current entry if none are selected.
    pub fn effective_selection(&self) -> Vec<&Entry> {
        let selected: Vec<&Entry> = self.entries.iter().filter(|e| e.selected).collect();
        if selected.is_empty() {
            if let Some(e) = self.entries.get(self.cursor) {
                if e.name != ".." {
                    return vec![e];
                }
            }
            vec![]
        } else {
            selected
        }
    }

    // -----------------------------------------------------------------------
    // Navigation into directories
    // -----------------------------------------------------------------------

    pub fn enter_dir(&mut self, path: PathBuf) -> Result<()> {
        self.path = path;
        self.cursor = 0;
        self.scroll = 0;
        self.quicksearch.clear();
        // Changing directory is a fresh listing; do not preserve a filename
        // from the previous directory when reloading.
        self.entries.clear();
        self.reload()
    }

    // -----------------------------------------------------------------------
    // Quicksearch
    // -----------------------------------------------------------------------

    pub fn quicksearch_append(&mut self, ch: char) {
        self.quicksearch.push(ch);
    }

    pub fn quicksearch_pop(&mut self) {
        self.quicksearch.pop();
    }

    pub fn quicksearch_clear(&mut self) {
        self.quicksearch.clear();
    }

    /// Returns the index of the first entry whose name starts with `quicksearch`.
    pub fn quicksearch_index(&self) -> Option<usize> {
        if self.quicksearch.is_empty() {
            return None;
        }
        let qs = self.quicksearch.to_lowercase();
        self.entries
            .iter()
            .position(|e| e.name.to_lowercase().starts_with(&qs))
    }

    // -----------------------------------------------------------------------
    // Stats
    // -----------------------------------------------------------------------

    pub fn selected_count(&self) -> usize {
        self.entries.iter().filter(|e| e.selected).count()
    }

    pub fn selected_bytes(&self) -> u64 {
        self.entries.iter().filter(|e| e.selected).map(|e| e.size).sum()
    }

    pub fn current_entry(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ext_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Very simple glob: `*` matches anything, `?` matches one char.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_rec(&p, &t, 0, 0)
}

fn glob_rec(p: &[char], t: &[char], pi: usize, ti: usize) -> bool {
    if pi == p.len() {
        return ti == t.len();
    }
    match p[pi] {
        '*' => {
            // skip consecutive stars
            let mut next = pi + 1;
            while next < p.len() && p[next] == '*' {
                next += 1;
            }
            if next == p.len() {
                return true;
            }
            for i in ti..=t.len() {
                if glob_rec(p, t, next, i) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if ti < t.len() {
                glob_rec(p, t, pi + 1, ti + 1)
            } else {
                false
            }
        }
        c => {
            if ti < t.len() && t[ti] == c {
                glob_rec(p, t, pi + 1, ti + 1)
            } else {
                false
            }
        }
    }
}
