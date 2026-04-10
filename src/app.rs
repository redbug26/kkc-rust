use crate::config::{Config, SortMode};
use crate::file_ops;
use crate::panel::Panel;
use crate::search::{search, SearchQuery, SearchResult};
use crate::viewer::Viewer;
use anyhow::Result;
use std::collections::VecDeque;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Which panel is active
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePanel {
    Left,
    Right,
}

impl ActivePanel {
    pub fn other(self) -> Self {
        match self {
            ActivePanel::Left => ActivePanel::Right,
            ActivePanel::Right => ActivePanel::Left,
        }
    }
}

// ---------------------------------------------------------------------------
// Application mode (state machine)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum AppMode {
    /// Normal dual-panel browsing.
    Browse,
    /// Inline quick-search (type-ahead).
    QuickSearch,
    /// F3 internal viewer (normal navigation).
    Viewer(Viewer),
    /// Viewer with the '/' search bar active.
    ViewerSearching(Viewer),
    /// Search panel (Alt-F7).
    SearchPanel(SearchState),
    /// Confirmation dialog.
    Confirm(ConfirmDialog),
    /// Single-line text input dialog.
    Input(InputDialog),
    /// Directory history popup.
    DirHistory,
    /// Help overlay.
    Help,
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    Quit,
    Delete(Vec<PathBuf>),
}

#[derive(Debug, Clone)]
pub struct InputDialog {
    pub title: String,
    pub prompt: String,
    pub value: String,
    pub cursor: usize,
    pub action: InputAction,
}

#[derive(Debug, Clone)]
pub enum InputAction {
    Rename(PathBuf),
    Mkdir(PathBuf),
    /// Wildcard select (+)
    SelectPattern,
    /// Wildcard deselect (-)
    DeselectPattern,
}

impl InputDialog {
    pub fn insert_char(&mut self, ch: char) {
        self.value.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            // Find the previous char boundary
            let mut prev = self.cursor - 1;
            while prev > 0 && !self.value.is_char_boundary(prev) {
                prev -= 1;
            }
            self.value.remove(prev);
            self.cursor = prev;
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor < self.value.len() {
            self.value.remove(self.cursor);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            let mut p = self.cursor - 1;
            while p > 0 && !self.value.is_char_boundary(p) {
                p -= 1;
            }
            self.cursor = p;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.value.len() {
            let mut p = self.cursor + 1;
            while p < self.value.len() && !self.value.is_char_boundary(p) {
                p += 1;
            }
            self.cursor = p;
        }
    }

    pub fn home(&mut self) { self.cursor = 0; }

    pub fn end(&mut self) { self.cursor = self.value.len(); }
}

// ---------------------------------------------------------------------------
// Search state
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct SearchState {
    pub query: String,
    pub content_query: String,
    pub input_field: usize, // 0 = pattern, 1 = content
    pub results: Vec<SearchResult>,
    pub cursor: usize,
    pub running: bool,
    pub start_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Status-bar message
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct StatusMessage {
    pub text: String,
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct App {
    pub config: Config,
    pub left: Panel,
    pub right: Panel,
    pub active: ActivePanel,
    pub mode: AppMode,
    pub status: StatusMessage,
    pub dir_history: VecDeque<PathBuf>,
    pub history_cursor: usize,
}

impl App {
    pub fn new(config: Config) -> Self {
        let left = Panel::new(
            config.left.path.clone(),
            config.left.sort,
            config.left.show_hidden,
        );
        let right = Panel::new(
            config.right.path.clone(),
            config.right.sort,
            config.right.show_hidden,
        );
        let mut history = VecDeque::with_capacity(config.dir_history_max);
        history.push_front(config.left.path.clone());

        App {
            config,
            left,
            right,
            active: ActivePanel::Left,
            mode: AppMode::Browse,
            status: StatusMessage::default(),
            dir_history: history,
            history_cursor: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Panel accessors
    // -----------------------------------------------------------------------

    pub fn active_panel(&self) -> &Panel {
        match self.active {
            ActivePanel::Left => &self.left,
            ActivePanel::Right => &self.right,
        }
    }

    pub fn active_panel_mut(&mut self) -> &mut Panel {
        match self.active {
            ActivePanel::Left => &mut self.left,
            ActivePanel::Right => &mut self.right,
        }
    }

    pub fn other_panel(&self) -> &Panel {
        match self.active {
            ActivePanel::Left => &self.right,
            ActivePanel::Right => &self.left,
        }
    }

    #[allow(dead_code)]
    pub fn other_panel_mut(&mut self) -> &mut Panel {
        match self.active {
            ActivePanel::Left => &mut self.right,
            ActivePanel::Right => &mut self.left,
        }
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    pub fn switch_panel(&mut self) {
        self.active = self.active.other();
    }

    pub fn push_dir_history(&mut self, path: PathBuf) {
        // Avoid duplicates at front
        if self.dir_history.front() != Some(&path) {
            self.dir_history.push_front(path);
            while self.dir_history.len() > self.config.dir_history_max {
                self.dir_history.pop_back();
            }
        }
    }

    pub fn enter_dir(&mut self, path: PathBuf) -> Result<()> {
        self.push_dir_history(path.clone());
        self.active_panel_mut().enter_dir(path)
    }

    pub fn go_parent(&mut self) -> Result<()> {
        let current = self.active_panel().path.clone();
        if let Some(parent) = current.parent() {
            let parent = parent.to_path_buf();
            // Set cursor to the directory we just left
            let old_name = current
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            self.push_dir_history(parent.clone());
            self.active_panel_mut().enter_dir(parent)?;
            if let Some(idx) = self
                .active_panel()
                .entries
                .iter()
                .position(|e| e.name == old_name)
            {
                self.active_panel_mut().cursor = idx;
            }
        }
        Ok(())
    }

    pub fn reload_panels(&mut self) {
        let _ = self.left.reload();
        let _ = self.right.reload();
    }

    // -----------------------------------------------------------------------
    // File operations
    // -----------------------------------------------------------------------

    pub fn cmd_copy(&mut self) -> Result<()> {
        let sources: Vec<PathBuf> = self
            .active_panel()
            .effective_selection()
            .iter()
            .map(|e| e.path.clone())
            .collect();
        if sources.is_empty() {
            return Ok(());
        }
        let dst = self.other_panel().path.clone();
        let mut errors = Vec::new();
        for src in &sources {
            if let Err(e) = file_ops::copy_entry(src, &dst, None) {
                errors.push(format!("{}: {}", src.display(), e));
            }
        }
        if self.config.auto_reload {
            self.reload_panels();
        }
        if errors.is_empty() {
            self.status.text = format!("Copied {} item(s)", sources.len());
        } else {
            self.status.text = format!("Errors: {}", errors.join("; "));
        }
        Ok(())
    }

    pub fn cmd_move(&mut self) -> Result<()> {
        let sources: Vec<PathBuf> = self
            .active_panel()
            .effective_selection()
            .iter()
            .map(|e| e.path.clone())
            .collect();
        if sources.is_empty() {
            return Ok(());
        }
        let dst = self.other_panel().path.clone();
        let mut errors = Vec::new();
        for src in &sources {
            if let Err(e) = file_ops::move_entry(src, &dst) {
                errors.push(format!("{}: {}", src.display(), e));
            }
        }
        if self.config.auto_reload {
            self.reload_panels();
        }
        if errors.is_empty() {
            self.status.text = format!("Moved {} item(s)", sources.len());
        } else {
            self.status.text = format!("Errors: {}", errors.join("; "));
        }
        Ok(())
    }

    pub fn cmd_delete_confirmed(&mut self, paths: Vec<PathBuf>) -> Result<()> {
        let mut errors = Vec::new();
        for p in &paths {
            if let Err(e) = file_ops::delete_entry(p) {
                errors.push(format!("{}: {}", p.display(), e));
            }
        }
        if self.config.auto_reload {
            self.reload_panels();
        }
        if errors.is_empty() {
            self.status.text = format!("Deleted {} item(s)", paths.len());
        } else {
            self.status.text = format!("Errors: {}", errors.join("; "));
        }
        Ok(())
    }

    /// Initiate a delete — show confirmation if enabled, else delete immediately.
    pub fn cmd_delete(&mut self) {
        let paths: Vec<PathBuf> = self
            .active_panel()
            .effective_selection()
            .iter()
            .map(|e| e.path.clone())
            .collect();
        if paths.is_empty() {
            return;
        }

        let n = paths.len();
        let label = if n == 1 {
            paths[0].file_name().unwrap_or_default().to_string_lossy().into_owned()
        } else {
            format!("{} items", n)
        };

        if self.config.confirm_delete {
            self.mode = AppMode::Confirm(crate::app::ConfirmDialog {
                title: "Delete".into(),
                message: format!("Delete {}?", label),
                action: crate::app::ConfirmAction::Delete(paths),
            });
        } else {
            let _ = self.cmd_delete_confirmed(paths);
        }
    }

    // -----------------------------------------------------------------------
    // Viewer
    // -----------------------------------------------------------------------

    pub fn open_viewer(&mut self) {
        if let Some(entry) = self.active_panel().current_entry() {
            if entry.is_dir || entry.name == ".." {
                return;
            }
            match Viewer::open(&entry.path, self.config.viewer.word_wrap) {
                Ok(v) => self.mode = AppMode::Viewer(v),
                Err(e) => self.status.text = format!("Cannot open viewer: {}", e),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Sort
    // -----------------------------------------------------------------------

    pub fn set_sort(&mut self, sort: SortMode) {
        self.active_panel_mut().sort = sort;
        let _ = self.active_panel_mut().reload();
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    pub fn open_search(&mut self) {
        let start = self.active_panel().path.clone();
        self.mode = AppMode::SearchPanel(SearchState {
            query: String::new(),
            content_query: String::new(),
            input_field: 0,
            results: Vec::new(),
            cursor: 0,
            running: false,
            start_dir: start,
        });
    }

    pub fn run_search(&mut self) {
        let AppMode::SearchPanel(ref mut state) = self.mode else {
            return;
        };
        state.results.clear();
        state.cursor = 0;
        state.running = true;

        let query = SearchQuery {
            pattern: if state.query.is_empty() { "*".into() } else { state.query.clone() },
            content: if state.content_query.is_empty() {
                None
            } else {
                Some(state.content_query.clone())
            },
            start: state.start_dir.clone(),
            follow_links: false,
        };

        let mut results = Vec::new();
        let _ = search(&query, |r| {
            results.push(r.clone());
            results.len() < 500
        });

        let AppMode::SearchPanel(ref mut state) = self.mode else {
            return;
        };
        state.results = results;
        state.running = false;
    }

    // -----------------------------------------------------------------------
    // Config persistence
    // -----------------------------------------------------------------------

    pub fn save_config(&mut self) -> Result<()> {
        self.config.left.path = self.left.path.clone();
        self.config.left.sort = self.left.sort;
        self.config.left.show_hidden = self.left.show_hidden;
        self.config.right.path = self.right.path.clone();
        self.config.right.sort = self.right.sort;
        self.config.right.show_hidden = self.right.show_hidden;
        self.config.save()
    }
}
