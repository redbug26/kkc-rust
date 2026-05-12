mod command_palette;
mod dialogs;
mod helpers;
mod menu;
mod panel_tabs;
mod remote_edit;

pub use self::command_palette::{
    CommandPaletteState, PALETTE_DATA, PALETTE_SEP, normalize_shortcut, palette_label_for_action,
    palette_shortname_for_action, shortcut_from_key_event,
};
pub use self::dialogs::{
    AssocInputAction, AssocInputDialog, ConfirmAction, ConfirmDialog, InputAction, InputDialog,
    RemoteDeleteTarget, SearchState, TextInputState,
};
use self::helpers::{
    cleanup_temp_download, draw_busy_status, panel_config_needs_profiles, same_remote_target,
    spawn_remote_connect_task,
};
pub use self::menu::{
    MENU_DATA, MENU_HEADERS, MenuAction, MenuEntry, MenuState, StoreDetectChoice, StoreDetectItem,
    StoreDetectState, StoreInstallMethodsState, StoreInstallPaletteState, StoreInstallProgress,
    AudioPlayerPaletteState, ViewerGotoState, ViewerMenuKind, ViewerMenuState,
    ViewerPluginPaletteState,
};
use self::panel_tabs::{PanelTabs, panel_config_for_save, restore_panel_side};
pub use self::remote_edit::{RemoteEditKind, RemoteEditState};
use crate::about::AboutState;
use crate::compare::CompareBuffer;
use crate::config::{ActivePanelSide, Config, PanelConfig, PanelViewType, SortMode};
use crate::copy::{
    CopyDestination, CopyDialogState, CopyJob, CopyProgressState, CopyScanTask, CopySource,
    CopyTask, CopyTaskMessage, count_local_files, spawn_copy_scan, spawn_copy_task,
};
use crate::file_ops::{self, CopyOptions};
use crate::help::HelpState;
use crate::idf::render_idf_card;
pub use crate::matrix_screensaver::MatrixScreensaverState;
use crate::panel::Panel;
use crate::remote::{
    RemoteEntry, RemoteKind, RemoteProfile, RemoteSource, delete_path as remote_delete_path,
    download_into_dir, download_to_temp, join_remote, load_profiles, normalize_remote_cwd,
    prepare_connection, rename_path as remote_rename_path, save_profile, upload_into_dir,
};
use crate::search::{
    SearchBackend, SearchQuery, SearchResult, search, search_locate, search_spotlight,
};
use crate::terminal::{CmdLine, RunningCmd, TerminalState};
use crate::tree_mode::{TreeScanMessage, TreeViewState};
use crate::viewer::{ViewMode, Viewer};
use anyhow::Result;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType, size},
};
use std::collections::VecDeque;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

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
    /// Viewer with the Ctrl-G goto-line input active.
    ViewerGotoLine(Viewer, String),
    /// Viewer with Helix-style goto dropdown active.
    ViewerGoto(Viewer, ViewerGotoState),
    /// Viewer with a popup choice menu.
    ViewerMenu(Viewer, ViewerMenuState),
    /// Viewer plugin picker with a quick-palette filter.
    ViewerPluginPalette(Viewer, ViewerPluginPaletteState),
    /// Audio plugin picker for the current file MIME type.
    AudioPlayerPalette(Viewer, AudioPlayerPaletteState),
    /// Search panel (Alt-F7).
    SearchPanel(SearchState),
    /// Friendly side-by-side comparison panel for left/right files.
    ComparePanel(ComparePanelState),
    /// Cached user-directory tree browser.
    TreeView(TreeViewState),
    /// Confirmation dialog.
    Confirm(ConfirmDialog),
    /// Single-line text input dialog.
    Input(InputDialog),
    /// Association input dialog (single-line MIME type or multi-line openers).
    AssocInput(AssocInputDialog),
    /// Directory bookmarks popup.
    DirBookmarks,
    /// Help overlay.
    Help(HelpState),
    /// Menu bar / dropdown (F2).
    Menu(MenuState),
    /// Configuration screen (Options > Setup).
    Config(ConfigState),
    /// Plugin list (Options > Plugins).
    Plugins(PluginsState),
    /// Context actions returned by Lua action plugins (Ctrl-A).
    ActionPalette(ActionPaletteState),
    /// Command palette (Ctrl-P) – searchable list of all menu commands.
    CommandPalette(CommandPaletteState),
    /// Store plugin install palette with searchable plugin list.
    StoreInstallPalette(StoreInstallPaletteState),
    /// Choose from multiple registered openers.
    Opener(OpenerState),
    /// File-type association editor (Options > Associations).
    AssocEditor(AssocEditorState),
    /// Remote connection picker (Ctrl-F).
    RemoteConnect(RemoteConnectState),
    /// Add a new remote connection.
    RemoteEdit(RemoteEditState),
    /// Protocol picker dropdown for adding a new remote connection.
    RemoteAddMenu(usize),
    /// Connecting to a remote backend in the background.
    RemoteConnecting(RemoteConnectingState),
    /// Copy dialog and options.
    CopyDialog(CopyDialogState),
    /// Copy progress popup.
    CopyProgress(CopyProgressState),
    /// Pseudo-terminal mode (Ctrl-U / Esc toggle).
    Terminal,
    /// About / credits dialog (animated).
    About(AboutState),
    /// Fullscreen Matrix-style digital rain. Exits on any key/mouse event.
    MatrixScreensaver(MatrixScreensaverState),
}

// ---------------------------------------------------------------------------
// Config screen
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PluginsState {
    pub plugins: Vec<crate::plugins::PluginInfo>,
    pub plugins_dir: PathBuf,
    pub cursor: usize,
    pub query: String,
}

#[derive(Debug, Clone)]
pub struct ActionPaletteState {
    pub actions: Vec<crate::plugins::ActionItem>,
    pub cwd: PathBuf,
    pub cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareRowKind {
    Equal,
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone)]
pub struct CompareRow {
    pub kind: CompareRowKind,
    pub left_no: Option<usize>,
    pub right_no: Option<usize>,
    pub left_text: String,
    pub right_text: String,
}

#[derive(Debug, Clone)]
pub struct ComparePanelState {
    pub left_label: String,
    pub right_label: String,
    pub left_buffer: CompareBuffer,
    pub right_buffer: CompareBuffer,
    pub show_only_differences: bool,
    pub ignore_whitespace: bool,
    pub ignore_crlf: bool,
    pub summary: String,
    pub message: Option<String>,
    pub rows: Vec<CompareRow>,
    pub cursor: usize,
    pub scroll: usize,
    pub search_query: String,
    pub search_cursor: usize,
    pub search_active: bool,
}

impl ComparePanelState {
    pub fn move_prev(&mut self) {
        if self.rows.is_empty() {
            self.cursor = 0;
            self.scroll = 0;
            return;
        }
        self.cursor = if self.cursor == 0 {
            self.rows.len() - 1
        } else {
            self.cursor - 1
        };
    }

    pub fn move_next(&mut self) {
        if self.rows.is_empty() {
            self.cursor = 0;
            self.scroll = 0;
            return;
        }
        self.cursor = (self.cursor + 1) % self.rows.len();
    }
}

impl TextInputState for ComparePanelState {
    fn value(&self) -> &String {
        &self.search_query
    }

    fn value_mut(&mut self) -> &mut String {
        &mut self.search_query
    }

    fn cursor(&self) -> usize {
        self.search_cursor
    }

    fn cursor_mut(&mut self) -> &mut usize {
        &mut self.search_cursor
    }
}

impl ActionPaletteState {
    pub fn load(cwd: PathBuf) -> Self {
        Self {
            actions: crate::plugins::action_items(&cwd),
            cwd,
            cursor: 0,
        }
    }
}

impl PluginsState {
    pub fn load() -> Self {
        let plugins_dir = crate::plugins::plugins_dir().unwrap_or_else(|_| PathBuf::new());
        let mut plugins = crate::plugins::plugin_infos();
        plugins.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Self {
            plugins,
            plugins_dir,
            cursor: 0,
            query: String::new(),
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.query.trim().is_empty() {
            return (0..self.plugins.len()).collect();
        }

        let tokens: Vec<String> = self
            .query
            .split_whitespace()
            .map(|token| token.to_lowercase())
            .filter(|token| !token.is_empty())
            .collect();
        if tokens.is_empty() {
            return (0..self.plugins.len()).collect();
        }

        let first = &tokens[0];
        let rest = &tokens[1..];
        let mut starts = Vec::new();
        let mut contains = Vec::new();

        for (idx, item) in self.plugins.iter().enumerate() {
            let source = crate::plugins::plugin_source_label(&item.dir, &self.plugins_dir);
            let searchable = format!(
                "{} {} {} {} {} {}",
                item.name,
                item.kind,
                item.version,
                item.description,
                item.extensions.join(" "),
                source,
            );
            let lowered = searchable.to_lowercase();
            if !rest.iter().all(|token| lowered.contains(token.as_str())) {
                continue;
            }
            if item.name.to_lowercase().starts_with(first.as_str()) {
                starts.push(idx);
            } else if lowered.contains(first.as_str()) {
                contains.push(idx);
            }
        }

        starts.extend(contains);
        starts
    }

    pub fn append_query(&mut self, ch: char) {
        self.query.push(ch);
        self.cursor = 0;
        self.clamp_cursor();
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.cursor = 0;
        self.clamp_cursor();
    }

    pub fn move_prev(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = if self.cursor == 0 {
            len - 1
        } else {
            self.cursor - 1
        };
        self.clamp_cursor();
    }

    pub fn move_next(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = (self.cursor + 1) % len;
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.cursor = 0;
        } else {
            self.cursor = self.cursor.min(len.saturating_sub(1));
        }
    }
}

/// State for the full configuration screen.
#[derive(Debug, Clone)]
pub struct ConfigState {
    // checkboxes — Behaviour
    pub confirm_exit: bool,
    pub confirm_delete: bool,
    pub auto_reload: bool,
    pub insert_moves_down: bool,
    pub select_dirs: bool,
    // checkboxes — Display
    pub show_hidden: bool,
    pub color_by_type: bool,
    pub show_cloud_icons: bool,
    pub show_file_icons: bool,
    pub show_fkey_bar: bool,
    // checkboxes — Viewer
    pub word_wrap: bool,
    pub default_zoom: bool,
    pub debug_log: bool,
    // text fields
    pub screensaver_idle_minutes: String,
    pub editor: String,
    pub pager: String,
    pub dir_history_max: String,
    // cursor inside the form (0-based, covers checkboxes then text fields)
    pub cursor: usize,
    pub tab: usize,
}

impl ConfigState {
    pub const TAB_BEHAVIOUR: usize = 0;
    pub const TAB_DISPLAY: usize = 1;
    pub const TAB_VIEWER: usize = 2;
    pub const TAB_EXTERNAL: usize = 3;
    pub const TAB_COUNT: usize = 4;

    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self {
            confirm_exit: cfg.confirm_exit,
            confirm_delete: cfg.confirm_delete,
            auto_reload: cfg.auto_reload,
            insert_moves_down: cfg.insert_moves_down,
            select_dirs: cfg.select_dirs,
            show_hidden: cfg.left.show_hidden,
            color_by_type: cfg.color_by_type,
            show_cloud_icons: cfg.show_cloud_icons,
            show_file_icons: cfg.show_file_icons,
            show_fkey_bar: cfg.show_fkey_bar,
            word_wrap: cfg.viewer.word_wrap,
            default_zoom: cfg.viewer.default_zoom,
            debug_log: cfg.debug_log,
            screensaver_idle_minutes: cfg.screensaver_idle_minutes.to_string(),
            editor: cfg.editor.clone(),
            pager: cfg.pager.clone(),
            dir_history_max: cfg.dir_history_max.to_string(),
            cursor: 0,
            tab: Self::TAB_BEHAVIOUR,
        }
    }

    /// Apply the form values back into a Config.
    pub fn apply_to(&self, cfg: &mut crate::config::Config) {
        cfg.confirm_exit = self.confirm_exit;
        cfg.confirm_delete = self.confirm_delete;
        cfg.auto_reload = self.auto_reload;
        cfg.insert_moves_down = self.insert_moves_down;
        cfg.select_dirs = self.select_dirs;
        cfg.left.show_hidden = self.show_hidden;
        cfg.right.show_hidden = self.show_hidden;
        cfg.color_by_type = self.color_by_type;
        cfg.show_cloud_icons = self.show_cloud_icons;
        cfg.show_file_icons = self.show_file_icons;
        cfg.show_fkey_bar = self.show_fkey_bar;
        cfg.viewer.word_wrap = self.word_wrap;
        cfg.viewer.default_zoom = self.default_zoom;
        cfg.debug_log = self.debug_log;
        if let Ok(n) = self.screensaver_idle_minutes.trim().parse::<u64>() {
            cfg.screensaver_idle_minutes = n;
        }
        if !self.editor.trim().is_empty() {
            cfg.editor = self.editor.trim().to_owned();
        }
        if !self.pager.trim().is_empty() {
            cfg.pager = self.pager.trim().to_owned();
        }
        if let Ok(n) = self.dir_history_max.trim().parse::<usize>() {
            if n > 0 {
                cfg.dir_history_max = n;
            }
        }
    }

    pub const NUM_CHECKBOXES: usize = 13; // 5 behaviour + 5 display + 3 viewer
    pub const NUM_TOTAL: usize = 19; // 13 + 4 text + OK + Cancel

    pub fn ok_cursor() -> usize {
        Self::NUM_CHECKBOXES + 4
    }

    pub fn cancel_cursor() -> usize {
        Self::NUM_CHECKBOXES + 5
    }

    pub fn tab_range(tab: usize) -> std::ops::RangeInclusive<usize> {
        match tab {
            Self::TAB_BEHAVIOUR => 0..=4,
            Self::TAB_DISPLAY => 5..=9,
            Self::TAB_VIEWER => 10..=12,
            Self::TAB_EXTERNAL => 13..=16,
            _ => 0..=4,
        }
    }

    pub fn first_cursor_for_tab(tab: usize) -> usize {
        *Self::tab_range(tab).start()
    }

    pub fn last_cursor_for_tab(tab: usize) -> usize {
        *Self::tab_range(tab).end()
    }

    pub fn tab_for_cursor(cursor: usize) -> usize {
        match cursor {
            0..=4 => Self::TAB_BEHAVIOUR,
            5..=9 => Self::TAB_DISPLAY,
            10..=12 => Self::TAB_VIEWER,
            13..=16 => Self::TAB_EXTERNAL,
            _ => Self::TAB_BEHAVIOUR,
        }
    }

    pub fn set_tab(&mut self, tab: usize) {
        self.tab = tab.min(Self::TAB_COUNT - 1);
        self.cursor = Self::first_cursor_for_tab(self.tab);
    }

    pub fn sync_tab_to_cursor(&mut self) {
        if self.cursor < Self::ok_cursor() {
            self.tab = Self::tab_for_cursor(self.cursor);
        }
    }
}

// ---------------------------------------------------------------------------
// Opener picker state
// ---------------------------------------------------------------------------

/// State for the popup picker shown when multiple openers match a file.
#[derive(Debug, Clone)]
pub enum OpenerActionKind {
    System,
    Association { command: String },
    Archive,
}

#[derive(Debug, Clone)]
pub struct OpenerActionItem {
    pub category: &'static str,
    pub label: String,
    pub detail: String,
    pub kind: OpenerActionKind,
}

#[derive(Debug, Clone)]
pub struct OpenerState {
    pub items: Vec<OpenerActionItem>,
    pub query: String,
    pub match_pos: usize,
    pub path: std::path::PathBuf,
}

impl OpenerState {
    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.query.trim().is_empty() {
            return (0..self.items.len()).collect();
        }

        let tokens = self
            .query
            .split_whitespace()
            .map(|token| token.to_ascii_lowercase())
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        if tokens.is_empty() {
            return (0..self.items.len()).collect();
        }

        let first = &tokens[0];
        let rest = &tokens[1..];
        let mut starts = Vec::new();
        let mut contains = Vec::new();
        for (idx, item) in self.items.iter().enumerate() {
            let searchable =
                format!("{} {} {}", item.category, item.label, item.detail).to_ascii_lowercase();
            if !rest.iter().all(|token| searchable.contains(token)) {
                continue;
            }
            if item.label.to_ascii_lowercase().starts_with(first)
                || item.category.to_ascii_lowercase().starts_with(first)
            {
                starts.push(idx);
            } else if searchable.contains(first) {
                contains.push(idx);
            }
        }
        starts.extend(contains);
        starts
    }

    pub fn append_query(&mut self, ch: char) {
        self.query.push(ch);
        self.match_pos = 0;
        self.clamp_match();
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.match_pos = 0;
        self.clamp_match();
    }

    pub fn move_prev(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.match_pos = 0;
        } else if self.match_pos == 0 {
            self.match_pos = len - 1;
        } else {
            self.match_pos -= 1;
        }
    }

    pub fn move_next(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.match_pos = 0;
        } else {
            self.match_pos = (self.match_pos + 1) % len;
        }
    }

    pub fn selected_item(&self) -> Option<&OpenerActionItem> {
        self.filtered_indices()
            .get(self.match_pos)
            .and_then(|idx| self.items.get(*idx))
    }

    fn clamp_match(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.match_pos = 0;
        } else {
            self.match_pos = self.match_pos.min(len.saturating_sub(1));
        }
    }
}

// ---------------------------------------------------------------------------
// Association editor state
// ---------------------------------------------------------------------------

/// State for the full-screen association editor.
#[derive(Debug, Clone)]
pub struct AssocEditorState {
    /// (MIME type, openers) pairs - mirrors config.file_assoc.
    pub assocs: Vec<(String, Vec<String>)>,
    pub query: String,
    pub match_pos: usize,
}

#[derive(Debug, Clone)]
pub struct RemoteConnectState {
    pub items: Vec<RemoteProfile>,
    pub cursor: usize,
    pub query: String,
    pub match_pos: usize,
}

impl RemoteConnectState {
    pub fn load() -> Self {
        let mut state = Self {
            items: load_profiles().unwrap_or_default(),
            cursor: 0,
            query: String::new(),
            match_pos: 0,
        };
        state.sync_cursor();
        state
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.query.trim().is_empty() {
            return (0..self.items.len()).collect();
        }

        let tokens: Vec<String> = self
            .query
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.is_empty() {
            return (0..self.items.len()).collect();
        }

        let first = &tokens[0];
        let rest = &tokens[1..];

        let mut starts = Vec::new();
        let mut contains = Vec::new();

        for (idx, item) in self.items.iter().enumerate() {
            let protocol = item.protocol().name();
            let source = match item.source {
                RemoteSource::SshConfig => "ssh",
                RemoteSource::UserToml => "toml",
                RemoteSource::PluginAuto => "plugin",
            };
            let searchable = format!(
                "{} {} {} {}",
                item.name,
                item.host_label(),
                protocol,
                source
            );
            let lowered = searchable.to_lowercase();
            if !rest.iter().all(|token| lowered.contains(token.as_str())) {
                continue;
            }
            if lowered.starts_with(first.as_str())
                || item.name.to_lowercase().starts_with(first.as_str())
            {
                starts.push(idx);
            } else if lowered.contains(first.as_str()) {
                contains.push(idx);
            }
        }

        starts.extend(contains);
        starts
    }

    pub fn sync_cursor(&mut self) {
        let matches = self.filtered_indices();
        if matches.is_empty() {
            self.match_pos = 0;
            self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
            return;
        }
        self.match_pos = self.match_pos.min(matches.len().saturating_sub(1));
        self.cursor = matches[self.match_pos];
    }

    pub fn append_query(&mut self, ch: char) {
        self.query.push(ch);
        self.match_pos = 0;
        self.sync_cursor();
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.match_pos = 0;
        self.sync_cursor();
    }

    pub fn move_prev(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            return;
        }
        if self.match_pos == 0 {
            self.match_pos = len - 1;
        } else {
            self.match_pos -= 1;
        }
        self.sync_cursor();
    }

    pub fn move_next(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            return;
        }
        self.match_pos = (self.match_pos + 1) % len;
        self.sync_cursor();
    }
}

#[derive(Debug, Clone)]
pub enum BookmarkListItem {
    Existing(usize),
    AddCurrentDir(PathBuf),
}

#[derive(Debug, Clone)]
pub struct RemoteConnectingState {
    pub profile_name: String,
    pub protocol_label: &'static str,
    pub phase: String,
}

#[derive(Debug)]
struct RemoteConnectTask {
    rx: Receiver<RemoteConnectMessage>,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug)]
enum RemoteConnectMessage {
    Progress(String),
    Connected {
        profile: RemoteProfile,
        cwd: String,
        entries: Vec<RemoteEntry>,
    },
    Failed(String),
}

#[derive(Debug)]
struct StoreInstallTask {
    rx: Receiver<StoreInstallMessage>,
    item: crate::plugins::StorePluginInfo,
    index_path: PathBuf,
}

#[derive(Debug)]
enum StoreInstallMessage {
    Progress { percent: u8, phase: String },
    Finished(std::result::Result<String, String>),
}

#[derive(Debug)]
struct QuickPreviewTask {
    rx: Receiver<QuickPreviewMessage>,
    request_id: u64,
    path: PathBuf,
}

#[derive(Debug)]
enum QuickPreviewMessage {
    Loaded(std::result::Result<Viewer, String>),
}

impl AssocEditorState {
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        let mut assocs = cfg
            .file_assoc
            .iter()
            .map(|a| (a.mime_type.clone(), a.openers.clone()))
            .collect::<Vec<_>>();
        assocs
            .sort_by(|(mime_a, _), (mime_b, _)| mime_a.to_lowercase().cmp(&mime_b.to_lowercase()));

        Self {
            assocs,
            query: String::new(),
            match_pos: 0,
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.query.trim().is_empty() {
            return (0..self.assocs.len()).collect();
        }

        let tokens = self
            .query
            .split_whitespace()
            .map(|token| token.to_ascii_lowercase())
            .filter(|token| !token.is_empty())
            .collect::<Vec<_>>();
        if tokens.is_empty() {
            return (0..self.assocs.len()).collect();
        }

        self.assocs
            .iter()
            .enumerate()
            .filter_map(|(idx, (mime_type, openers))| {
                let haystack = format!(
                    "{} {}",
                    mime_type.to_ascii_lowercase(),
                    openers.join(" ").to_ascii_lowercase()
                );
                tokens
                    .iter()
                    .all(|token| haystack.contains(token))
                    .then_some(idx)
            })
            .collect()
    }

    pub fn clamp_match(&mut self) {
        let total = self.filtered_indices().len();
        if total == 0 {
            self.match_pos = 0;
        } else {
            self.match_pos = self.match_pos.min(total.saturating_sub(1));
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.filtered_indices().get(self.match_pos).copied()
    }

    pub fn move_prev(&mut self) {
        if self.match_pos > 0 {
            self.match_pos -= 1;
        }
    }

    pub fn move_next(&mut self) {
        let total = self.filtered_indices().len();
        if self.match_pos + 1 < total {
            self.match_pos += 1;
        }
    }

    pub fn push_query(&mut self, ch: char) {
        self.query.push(ch);
        self.clamp_match();
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.clamp_match();
    }
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Status-bar message
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct StatusMessage {
    pub text: String,
    /// When the current text was last set (used for auto-clear after 30 s).
    pub set_at: Option<std::time::Instant>,
    /// When the status-copy icon was triggered by mouse copy action.
    pub copy_icon_at: Option<std::time::Instant>,
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct App {
    pub config: Config,
    pub left: Panel,
    pub right: Panel,
    left_tabs: PanelTabs,
    right_tabs: PanelTabs,
    pub active: ActivePanel,
    pub file_preview_info: bool,
    pub file_id_active: bool,
    pub file_id_scroll: u16,
    pub mode: AppMode,
    pub status: StatusMessage,
    pub dir_history: VecDeque<PathBuf>,
    pub bookmarks: Vec<PathBuf>,
    pub bookmark_cursor: usize,
    pub bookmark_query: String,
    pub bookmark_match_pos: usize,
    remote_connect_task: Option<RemoteConnectTask>,
    remote_connect_return: Option<RemoteConnectState>,
    pending_remote_cwd: Option<String>,
    copy_scan: Option<CopyScanTask>,
    copy_task: Option<CopyTask>,
    store_install_task: Option<StoreInstallTask>,
    store_install_queue: VecDeque<crate::plugins::StorePluginInfo>,
    quick_preview_task: Option<QuickPreviewTask>,
    quick_preview_request_id: u64,
    /// Set to true after spawning an external program so the main loop can
    /// call terminal.clear() before the next draw.
    pub needs_clear: bool,
    /// Set to true when the user presses Ctrl+G; the main loop will capture
    /// the rendered frame to a GIF and reset this flag.
    pub capture_gif: bool,
    /// Persistent pseudo-terminal state (survives mode switches and quit/reopen).
    pub terminal: TerminalState,
    /// Streaming output from a running external command.
    pub running_cmd: Option<RunningCmd>,
    /// Quick-preview viewer shown in the other panel (toggled via Ctrl-P > Quick Preview).
    pub quick_preview: Option<Viewer>,
    /// Whether keyboard focus is in the quick-preview panel (Tab to enter, Tab/Esc to leave).
    pub quick_preview_active: bool,
    /// Forced view mode for quick-preview (`None` = auto-detect).
    pub quick_preview_forced_mode: Option<ViewMode>,
    /// Recently-used command palette entries (fn_name values), most-recent first.
    pub palette_recent: Vec<String>,
    /// When true, the main loop calls terminal.clear() before the next draw to force
    /// a full repaint (e.g. after a Lua app that drew directly on the main terminal).
    pub needs_full_redraw: bool,
    /// Center-column buttons shown between the two panels.
    pub center_buttons: Vec<MenuAction>,
    /// Last command action executed via menu/palette/shortcuts.
    pub last_menu_action: Option<MenuAction>,
}

pub fn default_center_button_actions() -> Vec<MenuAction> {
    vec![
        MenuAction::ViewFile,
        MenuAction::EditFile,
        MenuAction::OpenMenu,
        MenuAction::SwapPanels,
        MenuAction::RemoteConnect,
        MenuAction::DirBookmarks,
        MenuAction::SelectPattern,
        MenuAction::FileIdPreview,
        MenuAction::QuickPreview,
    ]
}

pub fn center_button_label(action: MenuAction) -> &'static str {
    palette_shortname_for_action(action)
}

impl App {
    pub fn new(config: Config) -> Self {
        let app_start = std::time::Instant::now();
        crate::viewer::debug_log("startup: App::new begin");
        let profiles_start = std::time::Instant::now();
        let profiles = if panel_config_needs_profiles(&config.left)
            || panel_config_needs_profiles(&config.right)
        {
            let profiles = load_profiles().unwrap_or_default();
            crate::viewer::debug_log(&format!(
                "startup: loaded {} remote profile(s) for restored remote panel(s) in {:.3} ms",
                profiles.len(),
                profiles_start.elapsed().as_secs_f64() * 1000.0
            ));
            profiles
        } else {
            crate::viewer::debug_log(&format!(
                "startup: skipped remote profile load for local panels in {:.3} ms",
                profiles_start.elapsed().as_secs_f64() * 1000.0
            ));
            Vec::new()
        };
        let panels_start = std::time::Instant::now();
        let (left, left_tabs) = restore_panel_side(&config.left, &profiles);
        let (right, right_tabs) = restore_panel_side(&config.right, &profiles);
        crate::viewer::debug_log(&format!(
            "startup: restored panels in {:.3} ms",
            panels_start.elapsed().as_secs_f64() * 1000.0
        ));
        let max = config.dir_history_max;
        let mut history: VecDeque<PathBuf> = config.dir_history.iter().cloned().take(max).collect();
        // Always seed with the left panel path if history is empty
        if history.is_empty() {
            history.push_front(config.left.path.clone());
        }

        let bookmarks = {
            let mut bm = config.bookmarks.clone();
            let home = directories::UserDirs::new()
                .map(|u| u.home_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from("/"));
            if bm.is_empty() {
                bm.push(home);
            }
            bm
        };

        let terminal_cache_start = std::time::Instant::now();
        let (term_history, term_output) = crate::terminal::load_terminal_cache();
        crate::viewer::debug_log(&format!(
            "startup: loaded terminal cache ({} history, {} output) in {:.3} ms",
            term_history.len(),
            term_output.len(),
            terminal_cache_start.elapsed().as_secs_f64() * 1000.0
        ));

        let plugins_start = std::time::Instant::now();
        let plugin_status = crate::plugins::initialize()
            .err()
            .map(|err| format!("Plugin loading failed: {err}"));
        crate::viewer::debug_log(&format!(
            "startup: plugin initialization completed in {:.3} ms{}",
            plugins_start.elapsed().as_secs_f64() * 1000.0,
            if plugin_status.is_some() {
                " with error"
            } else {
                ""
            }
        ));

        let lua_apps_start = std::time::Instant::now();
        let lua_app_status = crate::lua_apps::initialize()
            .err()
            .map(|err| format!("Lua app initialization failed: {err}"));
        crate::viewer::debug_log(&format!(
            "startup: lua app initialization completed in {:.3} ms{}",
            lua_apps_start.elapsed().as_secs_f64() * 1000.0,
            if lua_app_status.is_some() {
                " with error"
            } else {
                ""
            }
        ));

        let startup_status = match (plugin_status, lua_app_status) {
            (Some(a), Some(b)) => Some(format!("{a}\n{b}")),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        let palette_recent = config.palette_recent.clone();
        let restored_active_panel = match config.active_panel {
            ActivePanelSide::Left => ActivePanel::Left,
            ActivePanelSide::Right => ActivePanel::Right,
        };

        let mut app = App {
            config,
            left,
            right,
            left_tabs,
            right_tabs,
            active: restored_active_panel,
            file_preview_info: false,
            file_id_active: false,
            file_id_scroll: 0,
            mode: if let Some(msg) = startup_status {
                AppMode::Confirm(ConfirmDialog {
                    title: String::new(),
                    message: msg,
                    action: ConfirmAction::Message,
                })
            } else {
                AppMode::Browse
            },
            status: StatusMessage::default(),
            dir_history: history,
            bookmarks,
            bookmark_cursor: 0,
            bookmark_query: String::new(),
            bookmark_match_pos: 0,
            remote_connect_task: None,
            remote_connect_return: None,
            pending_remote_cwd: None,
            copy_scan: None,
            copy_task: None,
            store_install_task: None,
            store_install_queue: VecDeque::new(),
            quick_preview_task: None,
            quick_preview_request_id: 0,
            needs_clear: false,
            capture_gif: false,
            terminal: TerminalState {
                history: term_history,
                output: term_output,
                ..TerminalState::new()
            },
            running_cmd: None,
            quick_preview: None,
            quick_preview_active: false,
            quick_preview_forced_mode: None,
            palette_recent,
            needs_full_redraw: false,
            center_buttons: default_center_button_actions(),
            last_menu_action: None,
        };

        match app.config.panel_view_type {
            PanelViewType::Normal => {}
            PanelViewType::FilePreviewInfo => {
                app.file_preview_info = true;
            }
            PanelViewType::QuickPreview => {
                let preview_start = std::time::Instant::now();
                if let Some(entry) = app.active_panel().current_entry().cloned() {
                    app.start_quick_preview_for_entry(entry);
                }
                crate::viewer::debug_log(&format!(
                    "startup: restored quick preview in {:.3} ms",
                    preview_start.elapsed().as_secs_f64() * 1000.0
                ));
            }
        }

        crate::viewer::debug_log(&format!(
            "startup: App::new end in {:.3} ms",
            app_start.elapsed().as_secs_f64() * 1000.0
        ));
        app
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

    pub fn send_active_entry_to_other_panel(&mut self) -> Result<()> {
        let Some(entry) = self.active_panel().current_entry().cloned() else {
            return Ok(());
        };
        let source_dir = self.active_panel().path.clone();
        let source_is_remote = self.active_panel().is_remote_view();
        let other_is_remote = self.other_panel().is_remote_view();

        self.close_quick_preview();
        self.close_file_id_view();

        if source_is_remote || other_is_remote {
            self.notify("Arrow panel sync is only supported for local panels");
            return Ok(());
        }

        if entry.is_dir {
            self.other_panel_mut().enter_dir(entry.path)?;
        } else {
            let name = entry.name;
            let other = self.other_panel_mut();
            other.enter_dir(source_dir)?;
            other.restore_cursor_by_name(&name);
        }
        self.switch_panel();
        Ok(())
    }

    pub fn swap_panels(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
        std::mem::swap(&mut self.left_tabs, &mut self.right_tabs);
    }

    pub fn active_panel_tab_index(&self) -> usize {
        match self.active {
            ActivePanel::Left => self.left_tabs.current_index(),
            ActivePanel::Right => self.right_tabs.current_index(),
        }
    }

    pub fn active_panel_tab_count(&self) -> usize {
        match self.active {
            ActivePanel::Left => self.left_tabs.count(),
            ActivePanel::Right => self.right_tabs.count(),
        }
    }

    pub fn left_panel_tab_index(&self) -> usize {
        self.left_tabs.current_index()
    }

    pub fn left_panel_tab_count(&self) -> usize {
        self.left_tabs.count()
    }

    pub fn right_panel_tab_index(&self) -> usize {
        self.right_tabs.current_index()
    }

    pub fn right_panel_tab_count(&self) -> usize {
        self.right_tabs.count()
    }

    pub fn new_active_tab(&mut self) {
        match self.active {
            ActivePanel::Left => PanelTabs::new_tab(&mut self.left, &mut self.left_tabs),
            ActivePanel::Right => PanelTabs::new_tab(&mut self.right, &mut self.right_tabs),
        }
        self.set_status(format!(
            "Tab {}/{}",
            self.active_panel_tab_index() + 1,
            self.active_panel_tab_count()
        ));
    }

    pub fn close_active_tab(&mut self) {
        let closed = match self.active {
            ActivePanel::Left => PanelTabs::close_tab(&mut self.left, &mut self.left_tabs),
            ActivePanel::Right => PanelTabs::close_tab(&mut self.right, &mut self.right_tabs),
        };
        if closed {
            self.set_status(format!(
                "Tab {}/{}",
                self.active_panel_tab_index() + 1,
                self.active_panel_tab_count()
            ));
        } else {
            self.notify("Cannot close last tab");
        }
    }

    pub fn next_active_tab(&mut self) {
        let switched = match self.active {
            ActivePanel::Left => PanelTabs::next_tab(&mut self.left, &mut self.left_tabs),
            ActivePanel::Right => PanelTabs::next_tab(&mut self.right, &mut self.right_tabs),
        };
        if switched {
            self.set_status(format!(
                "Tab {}/{}",
                self.active_panel_tab_index() + 1,
                self.active_panel_tab_count()
            ));
        }
    }

    /// Show a notification dialog (dismissible with Enter/Esc).
    pub fn notify(&mut self, message: impl Into<String>) {
        self.mode = AppMode::Confirm(ConfirmDialog {
            title: String::new(),
            message: message.into(),
            action: ConfirmAction::Message,
        });
    }

    pub fn open_dir_bookmarks(&mut self) {
        self.bookmark_query.clear();
        let current = self.current_bookmark_candidate();
        self.bookmark_cursor = self
            .bookmarks
            .iter()
            .position(|bookmark| *bookmark == current)
            .unwrap_or(0);
        self.bookmark_match_pos = self
            .filtered_bookmark_items()
            .iter()
            .position(|item| matches!(item, BookmarkListItem::Existing(idx) if *idx == self.bookmark_cursor))
            .unwrap_or(0);
        self.sync_bookmark_cursor();
        self.mode = AppMode::DirBookmarks;
    }

    pub fn current_bookmark_candidate(&self) -> PathBuf {
        if let Some(profile) = self.active_panel().remote_profile() {
            let cwd = self.active_panel().remote_cwd().unwrap_or("/");
            PathBuf::from(format!(
                "remote://{}/{}",
                profile.name,
                cwd.trim_start_matches('/')
            ))
        } else {
            self.active_panel().path.clone()
        }
    }

    pub fn add_current_dir_bookmark(&mut self) -> bool {
        let cur = self.current_bookmark_candidate();
        if !self.bookmarks.contains(&cur) {
            self.bookmarks.push(cur);
            self.bookmark_cursor = self.bookmarks.len() - 1;
            self.bookmark_match_pos = 0;
            self.sync_bookmark_cursor();
            true
        } else {
            false
        }
    }

    pub fn filtered_bookmark_items(&self) -> Vec<BookmarkListItem> {
        let tokens: Vec<String> = self
            .bookmark_query
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();

        let matches_tokens = |label: &str| -> bool {
            if tokens.is_empty() {
                return true;
            }
            let lowered = label.to_lowercase();
            let first = &tokens[0];
            let rest = &tokens[1..];
            lowered.contains(first.as_str())
                && rest.iter().all(|token| lowered.contains(token.as_str()))
        };
        let starts_with_first = |label: &str| -> bool {
            if tokens.is_empty() {
                return true;
            }
            label.to_lowercase().starts_with(tokens[0].as_str())
        };

        let mut starts = Vec::new();
        let mut contains = Vec::new();

        let current = self.current_bookmark_candidate();
        if !self.bookmarks.contains(&current) {
            let label = format!("<add current dir> {}", current.to_string_lossy());
            if matches_tokens(&label) {
                let item = BookmarkListItem::AddCurrentDir(current);
                if starts_with_first(&label) {
                    starts.push(item);
                } else {
                    contains.push(item);
                }
            }
        }

        for (idx, bookmark) in self.bookmarks.iter().enumerate() {
            let label = bookmark.to_string_lossy();
            if !matches_tokens(&label) {
                continue;
            }
            let item = BookmarkListItem::Existing(idx);
            if starts_with_first(&label) {
                starts.push(item);
            } else {
                contains.push(item);
            }
        }

        starts.extend(contains);
        starts
    }

    pub fn sync_bookmark_cursor(&mut self) {
        let matches = self.filtered_bookmark_items();
        if matches.is_empty() {
            self.bookmark_match_pos = 0;
            self.bookmark_cursor = self
                .bookmark_cursor
                .min(self.bookmarks.len().saturating_sub(1));
            return;
        }
        self.bookmark_match_pos = self.bookmark_match_pos.min(matches.len().saturating_sub(1));
        if let BookmarkListItem::Existing(idx) = matches[self.bookmark_match_pos] {
            self.bookmark_cursor = idx;
        }
    }

    pub fn append_bookmark_query(&mut self, ch: char) {
        self.bookmark_query.push(ch);
        self.bookmark_match_pos = 0;
        self.sync_bookmark_cursor();
    }

    pub fn pop_bookmark_query(&mut self) {
        self.bookmark_query.pop();
        self.bookmark_match_pos = 0;
        self.sync_bookmark_cursor();
    }

    pub fn move_prev_bookmark(&mut self) {
        let len = self.filtered_bookmark_items().len();
        if len == 0 {
            return;
        }
        if self.bookmark_match_pos == 0 {
            self.bookmark_match_pos = len - 1;
        } else {
            self.bookmark_match_pos -= 1;
        }
        self.sync_bookmark_cursor();
    }

    pub fn move_next_bookmark(&mut self) {
        let len = self.filtered_bookmark_items().len();
        if len == 0 {
            return;
        }
        if self.bookmark_match_pos + 1 >= len {
            self.bookmark_match_pos = 0;
        } else {
            self.bookmark_match_pos += 1;
        }
        self.sync_bookmark_cursor();
    }

    pub fn open_remote_connect(&mut self) {
        self.remote_connect_task = None;
        self.remote_connect_return = None;
        self.mode = AppMode::RemoteConnect(RemoteConnectState::load());
    }

    pub fn open_remote_add_menu(&mut self) {
        self.mode = AppMode::RemoteAddMenu(0);
    }

    pub fn open_remote_edit(&mut self) {
        self.open_remote_edit_profile();
    }

    pub fn open_copy_dialog(&mut self) {
        if self.other_panel().is_archive_view() {
            let Some(archive_path) = self.other_panel().archive_path() else {
                self.notify("Archive destination is not available");
                return;
            };
            if self.active_panel().is_archive_view() || self.active_panel().is_remote_view() {
                self.notify("Copy to archive is supported from local files only");
                return;
            }
            if !crate::plugins::supports_archive_add_files(archive_path) {
                self.notify("Copy to this archive format is not supported");
                return;
            }
            if self
                .active_panel()
                .effective_selection()
                .iter()
                .any(|entry| entry.is_dir)
            {
                self.notify("Copying directories to archive is not supported");
                return;
            }
        }
        if self.other_panel().is_archive_view() && self.active_panel().is_remote_view() {
            self.notify("Copy from remote to archive is not supported");
            return;
        }
        if self.active_panel().is_archive_view() && self.other_panel().is_remote_view() {
            self.notify("Copy from archive to remote is not supported");
            return;
        }
        let selection = self
            .active_panel()
            .effective_selection()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let mut file_count = 0usize;
        let mut total_bytes = 0u64;
        let mut stats_pending = false;
        let destination = if self.other_panel().is_remote_view() {
            self.other_panel().remote_cwd().unwrap_or("/").to_string()
        } else {
            self.other_panel().path.to_string_lossy().into_owned()
        };
        self.copy_scan = None;
        if let Some(profile) = self.active_panel().remote_profile() {
            let items = selection
                .iter()
                .map(|entry| (entry.path.to_string_lossy().into_owned(), entry.is_dir))
                .collect::<Vec<_>>();
            self.copy_scan = Some(spawn_copy_scan(profile, items));
            stats_pending = true;
        } else {
            for entry in &selection {
                file_count += count_local_files(&entry.path);
                total_bytes += file_ops::entry_size(&entry.path);
            }
        }
        self.mode = AppMode::CopyDialog(CopyDialogState::new(
            destination,
            file_count,
            total_bytes,
            stats_pending,
        ));
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
        crate::viewer::debug_log(&format!(
            "[enter_dir] path={}, is_remote={}",
            path.display(),
            self.active_panel().is_remote_view()
        ));
        self.push_dir_history(path.clone());
        if self.active_panel().is_remote_view() {
            self.run_with_busy("Remote: changing directory...", |app| {
                app.active_panel_mut().enter_dir(path)
            })
        } else {
            self.active_panel_mut().enter_dir(path)
        }
    }

    pub fn enter_archive(&mut self, path: PathBuf) -> Result<()> {
        self.push_dir_history(path.clone());
        self.active_panel_mut().enter_archive(path)
    }

    pub fn start_remote_connect(
        &mut self,
        profile: RemoteProfile,
        return_state: RemoteConnectState,
    ) {
        self.pending_remote_cwd = None;
        self.file_preview_info = false;
        self.remote_connect_return = Some(return_state);
        let protocol_label = profile.protocol().label();
        self.remote_connect_task = Some(spawn_remote_connect_task(
            profile.clone(),
            self.active_panel().show_hidden,
        ));
        self.mode = AppMode::RemoteConnecting(RemoteConnectingState {
            profile_name: profile.name,
            protocol_label,
            phase: "Preparing connection...".into(),
        });
    }

    pub fn start_remote_connect_with_cwd(&mut self, profile: RemoteProfile, target_cwd: String) {
        self.pending_remote_cwd = Some(target_cwd);
        self.file_preview_info = false;
        self.remote_connect_return = None;
        let protocol_label = profile.protocol().label();
        self.remote_connect_task = Some(spawn_remote_connect_task(
            profile.clone(),
            self.active_panel().show_hidden,
        ));
        self.mode = AppMode::RemoteConnecting(RemoteConnectingState {
            profile_name: profile.name,
            protocol_label,
            phase: "Preparing connection...".into(),
        });
    }

    pub fn cancel_remote_connect(&mut self) {
        if let Some(task) = &self.remote_connect_task {
            task.cancel.store(true, Ordering::Relaxed);
        }
        self.remote_connect_task = None;
        self.mode = AppMode::RemoteConnect(
            self.remote_connect_return
                .take()
                .unwrap_or_else(RemoteConnectState::load),
        );
    }

    pub fn save_remote_profile(
        &mut self,
        profile: RemoteProfile,
        old_name: Option<String>,
    ) -> Result<()> {
        save_profile(&profile, old_name.as_deref())?;
        Ok(())
    }

    pub fn open_remote_edit_profile(&mut self) {
        let profile = if let AppMode::RemoteConnect(ref s) = self.mode {
            s.filtered_indices()
                .get(s.match_pos)
                .and_then(|idx| s.items.get(*idx))
                .filter(|p| {
                    matches!(p.source, RemoteSource::UserToml | RemoteSource::PluginAuto)
                        && matches!(
                            p.kind,
                            RemoteKind::Sftp(_) | RemoteKind::Smb(_) | RemoteKind::RemotePlugin(_)
                        )
                })
                .cloned()
        } else {
            None
        };
        if let Some(profile) = profile {
            self.mode = AppMode::RemoteEdit(RemoteEditState::from_profile(&profile));
        } else {
            self.notify("Only user-defined connections can be edited");
        }
    }

    pub fn go_parent(&mut self) -> Result<()> {
        if self.active_panel().is_remote_view() {
            let current = self.active_panel().path.clone();
            let old_name = current
                .file_name()
                .and_then(|name| name.to_str())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());
            let raw_parent = current
                .parent()
                .unwrap_or(std::path::Path::new("/"))
                .to_path_buf();
            let parent = if let Some(profile) = self.active_panel().remote_profile() {
                PathBuf::from(normalize_remote_cwd(
                    &profile,
                    &raw_parent.to_string_lossy(),
                ))
            } else {
                raw_parent
            };
            self.run_with_busy("Remote: changing directory...", |app| {
                app.active_panel_mut().enter_dir(parent)
            })?;
            if let Some(old_name) = old_name
                && let Some(idx) = self
                    .active_panel()
                    .entries
                    .iter()
                    .position(|e| e.name == old_name)
            {
                let panel = self.active_panel_mut();
                panel.cursor = idx;
                if panel.cursor < panel.scroll {
                    panel.scroll = panel.cursor;
                }
            }
            return Ok(());
        }
        if self.active_panel().is_archive_root() {
            if let Some((parent, archive_name)) = self.active_panel_mut().leave_archive() {
                self.push_dir_history(parent);
                if let Some(idx) = self
                    .active_panel()
                    .entries
                    .iter()
                    .position(|e| e.name == archive_name)
                {
                    let panel = self.active_panel_mut();
                    panel.cursor = idx;
                    if panel.cursor < panel.scroll {
                        panel.scroll = panel.cursor;
                    }
                }
            }
            return Ok(());
        }
        let current = self.active_panel().path.clone();
        if let Some(parent) = current.parent() {
            let parent = parent.to_path_buf();
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
                let panel = self.active_panel_mut();
                panel.cursor = idx;
                if panel.cursor < panel.scroll {
                    panel.scroll = panel.cursor;
                }
            }
        }
        Ok(())
    }

    pub fn reload_panels(&mut self) {
        let _ = self.left.reload();
        let _ = self.right.reload();
    }

    pub fn poll_background_tasks(&mut self) {
        self.poll_running_cmd();
        self.poll_search();
        self.poll_tree_view();
        self.poll_store_install();
        self.poll_quick_preview();
        self.poll_audio_autoadvance();
        // Auto-clear status bar text after 30 seconds.
        if let Some(set_at) = self.status.set_at {
            if set_at.elapsed() >= std::time::Duration::from_secs(30) {
                self.status.text.clear();
                self.status.set_at = None;
            }
        }
        if let Some(icon_at) = self.status.copy_icon_at {
            if icon_at.elapsed() >= std::time::Duration::from_secs(10) {
                self.status.copy_icon_at = None;
            }
        }
        let mut remote_connect_result: Option<RemoteConnectMessage> = None;
        if let Some(task) = &self.remote_connect_task {
            match task.rx.try_recv() {
                Ok(RemoteConnectMessage::Progress(phase)) => {
                    if let AppMode::RemoteConnecting(state) = &mut self.mode {
                        state.phase = phase;
                    }
                }
                Ok(msg) => remote_connect_result = Some(msg),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    remote_connect_result = Some(RemoteConnectMessage::Failed(
                        "Remote connect worker disconnected".into(),
                    ));
                }
            }
        }
        if let Some(msg) = remote_connect_result {
            self.remote_connect_task = None;
            match msg {
                RemoteConnectMessage::Progress(_) => {}
                RemoteConnectMessage::Connected {
                    profile,
                    cwd,
                    entries,
                } => {
                    self.active_panel_mut()
                        .mount_remote_prefetched(profile, cwd, entries);
                    if let Some(target) = self.pending_remote_cwd.take() {
                        let _ = self.active_panel_mut().enter_dir(PathBuf::from(target));
                    }
                    self.remote_connect_return = None;
                    self.mode = AppMode::Browse;
                }
                RemoteConnectMessage::Failed(err) => {
                    let return_state = self
                        .remote_connect_return
                        .take()
                        .unwrap_or_else(RemoteConnectState::load);
                    self.mode = AppMode::Confirm(ConfirmDialog {
                        title: String::new(),
                        message: format!("Remote connect failed: {}", err),
                        action: ConfirmAction::MessageThen(Box::new(AppMode::RemoteConnect(
                            return_state,
                        ))),
                    });
                }
            }
        }

        let mut start_copy: Option<CopyDialogState> = None;
        let mut clear_copy_scan = false;
        if let Some(task) = &self.copy_scan {
            loop {
                match task.rx.try_recv() {
                    Ok(update) => {
                        if let AppMode::CopyDialog(state) = &mut self.mode {
                            state.file_count = update.stats.files;
                            state.total_bytes = update.stats.bytes;
                            if let Some((path, bytes)) = update.finished_entry {
                                state.entry_bytes.insert(path, bytes);
                            }
                            state.stats_pending = !update.done;
                            if update.done && state.waiting_to_start {
                                start_copy = Some(state.clone());
                            }
                        }
                        if update.done {
                            clear_copy_scan = true;
                            break;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if let AppMode::CopyDialog(state) = &mut self.mode {
                            state.stats_pending = false;
                        }
                        clear_copy_scan = true;
                        break;
                    }
                }
            }
        }
        if clear_copy_scan {
            self.copy_scan = None;
        }
        if let Some(state) = start_copy {
            let _ = self.execute_copy_dialog(state);
        }
        let mut finish_message: Option<(usize, Vec<String>, bool)> = None;
        if let Some(task) = &self.copy_task {
            loop {
                match task.rx.try_recv() {
                    Ok(CopyTaskMessage::Progress(progress)) => {
                        self.mode = AppMode::CopyProgress(progress);
                    }
                    Ok(CopyTaskMessage::Finished {
                        copied_items,
                        errors,
                        aborted,
                    }) => {
                        finish_message = Some((copied_items, errors, aborted));
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finish_message = Some((0, vec!["Copy worker disconnected".into()], false));
                        break;
                    }
                }
            }
        }
        if let Some((copied_items, errors, aborted)) = finish_message {
            self.copy_task = None;
            self.mode = AppMode::Browse;
            if self.config.auto_reload {
                self.reload_panels();
            }
            let msg = if aborted {
                "Copy aborted".to_string()
            } else if errors.is_empty() {
                format!("Copied {} item(s)", copied_items)
            } else {
                format!("Errors: {}", errors.join("; "))
            };
            self.notify(msg);
        }
    }

    fn poll_audio_autoadvance(&mut self) {
        let Some(path) = (match &self.mode {
            AppMode::Viewer(viewer) if matches!(viewer.mode, ViewMode::Module) => {
                Some(viewer.path.clone())
            }
            _ => None,
        }) else {
            return;
        };

        if !crate::tracker_audio::playback_finished_for_path(&path) {
            return;
        }

        crate::tracker_audio::stop_module_if_path(&path);
        let Some(next_idx) = self.next_audio_entry_index() else {
            self.notify("Audio playback finished");
            return;
        };

        let panel = self.active_panel_mut();
        panel.cursor = next_idx;
        if panel.cursor < panel.scroll {
            panel.scroll = panel.cursor;
        }
        self.open_viewer();
    }

    fn next_audio_entry_index(&self) -> Option<usize> {
        let panel = self.active_panel();
        let start = panel.cursor.saturating_add(1);
        panel
            .entries
            .iter()
            .enumerate()
            .skip(start)
            .find_map(|(idx, entry)| {
                (!entry.is_dir
                    && entry.name != ".."
                    && !entry.cloud_only
                    && crate::tracker_audio::is_audio_path(&entry.path))
                .then_some(idx)
            })
    }

    fn poll_store_install(&mut self) {
        let mut finished: Option<(
            crate::plugins::StorePluginInfo,
            PathBuf,
            std::result::Result<String, String>,
        )> = None;

        if let Some(task) = &self.store_install_task {
            loop {
                match task.rx.try_recv() {
                    Ok(StoreInstallMessage::Progress { percent, phase }) => {
                        if let AppMode::StoreInstallPalette(state) = &mut self.mode
                            && let Some(progress) = &mut state.progress
                        {
                            progress.percent = percent.min(100);
                            progress.phase = phase;
                        }
                    }
                    Ok(StoreInstallMessage::Finished(result)) => {
                        finished = Some((task.item.clone(), task.index_path.clone(), result));
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = Some((
                            task.item.clone(),
                            task.index_path.clone(),
                            Err("Store install worker disconnected".to_string()),
                        ));
                        break;
                    }
                }
            }
        }

        let Some((item, index_path, result)) = finished else {
            return;
        };
        self.store_install_task = None;

        match result {
            Ok(installed_name) => {
                if matches!(item.item_kind, crate::plugins::StoreItemKind::Application) {
                    let configured = self.configure_application_associations(&item);
                    let mut state = StoreInstallPaletteState::load(index_path)
                        .unwrap_or_else(|_| self.current_store_state_without_progress());
                    state.progress = None;
                    if let Some(next) = self.store_install_queue.pop_front() {
                        self.start_store_install(
                            state,
                            next,
                            "Installing missing application from store".to_string(),
                        );
                        return;
                    }
                    self.mode = AppMode::StoreInstallPalette(state);
                    if configured == 0 {
                        self.notify(format!("Application installed: {}", installed_name));
                    } else {
                        self.notify(format!(
                            "Application installed: {}; {} MIME association(s) configured",
                            installed_name, configured
                        ));
                    }
                } else {
                    self.reload_panels();
                    let mut state = StoreInstallPaletteState::load(index_path)
                        .unwrap_or_else(|_| self.current_store_state_without_progress());
                    if let Some(pos) = state
                        .filtered_indices()
                        .iter()
                        .position(|idx| state.items[*idx].id == item.id)
                    {
                        state.match_pos = pos;
                    }
                    state.clamp_match();
                    self.mode = AppMode::StoreInstallPalette(state);
                    self.notify(format!("Plugin installed: {}", installed_name));
                }
            }
            Err(err) => {
                if let AppMode::StoreInstallPalette(state) = &mut self.mode {
                    state.progress = None;
                }
                self.notify(format!("Store install error: {}", err));
            }
        }
    }

    fn poll_quick_preview(&mut self) {
        let Some(task) = &self.quick_preview_task else {
            return;
        };

        let message = match task.rx.try_recv() {
            Ok(message) => Some((task.request_id, task.path.clone(), message)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some((
                task.request_id,
                task.path.clone(),
                QuickPreviewMessage::Loaded(Err("Preview worker disconnected".to_string())),
            )),
        };

        let Some((request_id, path, QuickPreviewMessage::Loaded(result))) = message else {
            return;
        };

        self.quick_preview_task = None;
        if request_id != self.quick_preview_request_id {
            return;
        }
        if self.quick_preview.as_ref().map(|v| v.path.as_path()) != Some(path.as_path()) {
            return;
        }

        match result {
            Ok(mut viewer) => {
                viewer.zoomed = true;
                if let Some(mode) = self.quick_preview_forced_mode {
                    viewer.set_mode(mode);
                }
                self.quick_preview = Some(viewer);
            }
            Err(err) => {
                let mut viewer = Viewer::placeholder(
                    &path,
                    &format!("Cannot open preview\n{err}"),
                    self.config.viewer.word_wrap,
                );
                viewer.zoomed = true;
                self.quick_preview = Some(viewer);
            }
        }
    }

    fn current_store_state_without_progress(&self) -> StoreInstallPaletteState {
        if let AppMode::StoreInstallPalette(state) = &self.mode {
            let mut state = state.clone();
            state.progress = None;
            state
        } else {
            StoreInstallPaletteState::load(crate::plugins::store_index_path()).unwrap_or_else(
                |_| StoreInstallPaletteState {
                    index_path: crate::plugins::store_index_path(),
                    index_info: crate::plugins::StoreIndexInfo::default(),
                    items: Vec::new(),
                    plugins_dir: PathBuf::new(),
                    installed_versions: std::collections::HashMap::new(),
                    installed_app_versions: std::collections::HashMap::new(),
                    installed_only: false,
                    query: String::new(),
                    match_pos: 0,
                    scroll_offset: std::cell::Cell::new(0),
                    progress: None,
                    detect: None,
                    methods: None,
                },
            )
        }
    }

    pub fn start_store_install(
        &mut self,
        mut state: StoreInstallPaletteState,
        item: crate::plugins::StorePluginInfo,
        title: String,
    ) {
        let index_path = state.index_path.clone();
        let worker_index_path = index_path.clone();
        let item_id = item.id.clone();
        let item_name = item.name.clone();
        let (tx, rx) = mpsc::channel();

        state.progress = Some(StoreInstallProgress {
            title: title.clone(),
            item_name,
            percent: 0,
            phase: "Starting...".to_string(),
        });
        self.mode = AppMode::StoreInstallPalette(state);

        std::thread::spawn(move || {
            let report = |percent: u8, phase: &str| {
                let _ = tx.send(StoreInstallMessage::Progress {
                    percent: percent.min(100),
                    phase: phase.to_string(),
                });
            };
            let result = crate::plugins::install_plugin_from_store_with_progress(
                &worker_index_path,
                &item_id,
                |p, phase| {
                    report(p, phase);
                },
            )
            .map_err(|err| err.to_string());
            let _ = tx.send(StoreInstallMessage::Finished(result));
        });

        self.store_install_task = Some(StoreInstallTask {
            rx,
            item,
            index_path,
        });
    }

    pub fn open_store_detection_dialog(&mut self, mut state: StoreInstallPaletteState) {
        let query = state.query.clone();
        let selected_id = state
            .filtered_indices()
            .get(state.match_pos)
            .and_then(|idx| state.items.get(*idx))
            .map(|item| item.id.clone());
        let detected = match crate::plugins::detect_installed_store_applications(&state.items) {
            Ok(items) => items,
            Err(err) => {
                self.notify(format!("Store detection failed: {}", err));
                self.mode = AppMode::StoreInstallPalette(state);
                return;
            }
        };

        let mut configured = 0;
        for item in &detected {
            configured += self.configure_application_associations(item);
        }

        let missing = match crate::plugins::missing_remembered_store_applications(&state.items) {
            Ok(items) => items,
            Err(err) => {
                self.notify(format!("Store detection failed: {}", err));
                self.mode = AppMode::StoreInstallPalette(state);
                return;
            }
        };

        state = StoreInstallPaletteState::load(state.index_path.clone()).unwrap_or(state);
        state.query = query;
        if let Some(selected_id) = selected_id
            && let Some(pos) = state
                .filtered_indices()
                .iter()
                .position(|idx| state.items[*idx].id == selected_id)
        {
            state.match_pos = pos;
        }
        state.clamp_match();

        state.detect = Some(StoreDetectState {
            items: missing
                .into_iter()
                .map(|app| StoreDetectItem {
                    app,
                    choice: StoreDetectChoice::Keep,
                })
                .collect(),
            cursor: 0,
            detected_count: detected.len(),
        });
        if configured > 0 {
            self.notify(format!(
                "Detected {} installed app(s); {} MIME association(s) updated",
                detected.len(),
                configured
            ));
        }
        self.mode = AppMode::StoreInstallPalette(state);
    }

    pub fn open_store_install_methods_dialog(&mut self, mut state: StoreInstallPaletteState) {
        match crate::plugins::list_store_install_method_capabilities(&state.index_path) {
            Ok(methods) => {
                let count = methods.len();
                state.methods = Some(StoreInstallMethodsState { methods });
                self.notify(format!("Store install methods for this OS: {}", count));
                self.mode = AppMode::StoreInstallPalette(state);
            }
            Err(err) => {
                self.notify(format!("Store method detection failed: {}", err));
                self.mode = AppMode::StoreInstallPalette(state);
            }
        }
    }

    pub fn apply_store_detection_choices(&mut self, mut state: StoreInstallPaletteState) {
        let Some(detect) = state.detect.take() else {
            self.mode = AppMode::StoreInstallPalette(state);
            return;
        };
        let query = state.query.clone();
        let selected_id = state
            .filtered_indices()
            .get(state.match_pos)
            .and_then(|idx| state.items.get(*idx))
            .map(|item| item.id.clone());

        let mut install_queue = Vec::new();
        let mut removed = 0usize;
        for item in detect.items {
            match item.choice {
                StoreDetectChoice::Keep => {}
                StoreDetectChoice::Remove => {
                    if self.remove_application_associations(&item.app) {
                        let _ = self.save_config();
                    }
                    if crate::plugins::remove_store_application(&item.app.id).unwrap_or(false) {
                        removed += 1;
                    }
                }
                StoreDetectChoice::Install => install_queue.push(item.app),
            }
        }

        let state = Self::refresh_store_state_preserving_selection(state, query, selected_id);
        if let Some(first) = install_queue.first().cloned() {
            self.store_install_queue = install_queue.into_iter().skip(1).collect();
            self.start_store_install(
                state,
                first,
                "Installing missing application from store".to_string(),
            );
            if removed > 0 {
                self.notify(format!(
                    "Removed {} store entry(s); installing selected app",
                    removed
                ));
            }
        } else {
            if removed > 0 {
                self.notify(format!("Removed {} store entry(s)", removed));
            }
            self.mode = AppMode::StoreInstallPalette(state);
        }
    }

    fn refresh_store_state_preserving_selection(
        fallback: StoreInstallPaletteState,
        query: String,
        selected_id: Option<String>,
    ) -> StoreInstallPaletteState {
        let mut state =
            StoreInstallPaletteState::load(fallback.index_path.clone()).unwrap_or(fallback);
        state.query = query;
        if let Some(selected_id) = selected_id
            && let Some(pos) = state
                .filtered_indices()
                .iter()
                .position(|idx| state.items[*idx].id == selected_id)
        {
            state.match_pos = pos;
        }
        state.clamp_match();
        state.progress = None;
        state.detect = None;
        state
    }

    fn configure_application_associations(
        &mut self,
        item: &crate::plugins::StorePluginInfo,
    ) -> usize {
        let Some(opener) = store_application_opener(item) else {
            return 0;
        };
        let _ = crate::plugins::remember_store_application(item);
        if item.mime_types.is_empty() {
            return 0;
        }

        let mut configured = 0;
        for mime_type in &item.mime_types {
            let before = self.config.openers_for_mime(mime_type).len();
            self.config.add_opener_for_mime(mime_type, opener.clone());
            let after = self.config.openers_for_mime(mime_type).len();
            if after > before {
                configured += 1;
            }
        }

        if configured > 0 {
            let _ = self.save_config();
        }
        configured
    }

    fn remove_application_associations(&mut self, item: &crate::plugins::StorePluginInfo) -> bool {
        let Some(opener) = store_application_opener(item) else {
            return false;
        };
        let mut changed = false;
        for mime_type in &item.mime_types {
            changed |= self.config.remove_opener_for_mime(mime_type, &opener);
        }
        changed
    }

    /// Drain lines from a running streaming command into the terminal scrollback.
    pub fn poll_running_cmd(&mut self) {
        let Some(task) = &mut self.running_cmd else {
            return;
        };
        loop {
            match task.rx.try_recv() {
                Ok(CmdLine::Out(line)) => self.terminal.push_output(line),
                Ok(CmdLine::Err(line)) => self.terminal.push_output(line),
                Ok(CmdLine::Done(code)) => {
                    task.done = true;
                    if let Some(c) = code {
                        if c != 0 {
                            self.terminal.push_output(format!("[exit {}]", c));
                        }
                    }
                }
                Err(TryRecvError::Empty) => {
                    if task.done {
                        self.running_cmd = None;
                    }
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    self.running_cmd = None;
                    break;
                }
            }
        }
    }

    /// Drain results arriving from the background search thread.
    pub fn poll_search(&mut self) {
        if !matches!(self.mode, AppMode::SearchPanel(_)) {
            return;
        }
        let mut done = false;
        if let AppMode::SearchPanel(ref mut s) = self.mode {
            if s.running {
                if let Some(ref rx) = s.search_rx {
                    // Drain up to 200 results per tick to stay responsive
                    for _ in 0..200 {
                        match rx.try_recv() {
                            Ok(result) => {
                                s.dirs_visited += 1;
                                if s.results.len() < 1000 {
                                    s.results.push(result);
                                }
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                done = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        if done {
            if let AppMode::SearchPanel(ref mut s) = self.mode {
                s.running = false;
                s.search_rx = None;
                s.cancel_flag = None;
                // Auto-focus results list when search completes
                if !s.results.is_empty() && s.input_field < 3 {
                    s.input_field = 3;
                }
            }
        }
    }

    pub fn poll_tree_view(&mut self) {
        let AppMode::TreeView(state) = &mut self.mode else {
            return;
        };
        if !state.scanning {
            return;
        }

        let mut terminal_msg = None;
        if let Some(rx) = &state.scan_rx {
            for _ in 0..100 {
                match rx.try_recv() {
                    Ok(TreeScanMessage::Progress {
                        visited,
                        progress,
                        levels,
                        current,
                    }) => {
                        state.visited = visited;
                        state.progress = progress;
                        state.progress_levels = levels;
                        state.current = Some(current);
                    }
                    Ok(msg) => {
                        terminal_msg = Some(msg);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        terminal_msg = Some(TreeScanMessage::Failed(
                            "Tree scan worker disconnected".into(),
                        ));
                        break;
                    }
                }
            }
        }

        if let Some(msg) = terminal_msg {
            state.scanning = false;
            state.scan_rx = None;
            state.cancel_flag = None;
            match msg {
                TreeScanMessage::Finished {
                    entries,
                    scanned_at,
                } => {
                    state.set_entries(entries, Some(scanned_at));
                    state.progress = 1.0;
                    state.progress_levels.clear();
                    state.current = None;
                }
                TreeScanMessage::Cancelled => {
                    self.set_status("Tree scan cancelled");
                }
                TreeScanMessage::Failed(err) => {
                    self.set_status(format!("Tree scan error: {}", err));
                }
                TreeScanMessage::Progress { .. } => {}
            }
        }
    }

    /// Cancel any running background search and close the search panel.
    pub fn cancel_search(&mut self) {
        if let AppMode::SearchPanel(ref mut s) = self.mode {
            if let Some(ref flag) = s.cancel_flag {
                flag.store(true, Ordering::Relaxed);
            }
            s.running = false;
            s.search_rx = None;
            s.cancel_flag = None;
        }
    }

    pub fn cancel_copy_scan(&mut self) {
        if let Some(task) = &self.copy_scan {
            task.cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.copy_scan = None;
    }

    pub fn cancel_copy_task(&mut self) {
        if let Some(task) = &self.copy_task {
            task.cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    // -----------------------------------------------------------------------
    // File operations
    // -----------------------------------------------------------------------

    pub fn execute_copy_dialog(&mut self, state: CopyDialogState) -> Result<()> {
        let sources = self
            .active_panel()
            .effective_selection()
            .iter()
            .map(|e| (*e).clone())
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return Ok(());
        }

        if let Some(archive_path) = self.other_panel().archive_path().map(Path::to_path_buf) {
            let source_paths = sources
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>();
            if crate::plugins::add_files_to_archive(&archive_path, &source_paths)? {
                self.other_panel_mut().enter_archive(archive_path.clone())?;
                self.notify(format!("Copied {} file(s) to archive", source_paths.len()));
                return Ok(());
            }
            self.notify("Copy to this archive format is not supported");
            return Ok(());
        }

        self.copy_scan = None;
        let options = CopyOptions {
            overwrite: state.overwrite,
            newer_only: state.newer_only,
            keep_attributes: state.keep_attributes,
        };
        let src_remote = self.active_panel().remote_profile();
        let jobs = sources
            .into_iter()
            .map(|entry| CopyJob {
                total_bytes: if src_remote.is_some() {
                    state
                        .entry_bytes
                        .get(&entry.path.to_string_lossy().into_owned())
                        .copied()
                        .unwrap_or(entry.size)
                } else {
                    file_ops::entry_size(&entry.path)
                },
                source: match &src_remote {
                    Some(profile) => CopySource::Remote {
                        profile: profile.clone(),
                        path: entry.path.to_string_lossy().into_owned(),
                    },
                    None => CopySource::Local(entry.path.clone()),
                },
                entry,
            })
            .collect::<Vec<_>>();
        let destination = match self.other_panel().remote_profile() {
            Some(profile) => CopyDestination::Remote {
                profile,
                cwd: state.destination,
            },
            None => CopyDestination::Local(PathBuf::from(state.destination)),
        };
        self.copy_task = Some(spawn_copy_task(jobs, destination, options));
        self.mode = AppMode::CopyProgress(CopyProgressState {
            current_name: String::new(),
            item_index: 0,
            item_count: 0,
            file_done: 0,
            file_total: 0,
            total_done: 0,
            total_bytes: state.total_bytes,
            remaining_secs: None,
        });
        Ok(())
    }

    pub fn cmd_move(&mut self) -> Result<()> {
        if self.active_panel().is_archive_view() || self.other_panel().is_archive_view() {
            self.notify("Move in archive is not supported");
            return Ok(());
        }
        let sources = self
            .active_panel()
            .effective_selection()
            .iter()
            .map(|e| (*e).clone())
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return Ok(());
        }
        let mut errors = Vec::new();
        let needs_busy =
            self.active_panel().is_remote_view() || self.other_panel().is_remote_view();
        for entry in &sources {
            let result = if needs_busy {
                self.run_with_busy("Remote: moving...", |app| app.move_between_panels(entry))
            } else {
                self.move_between_panels(entry)
            };
            if let Err(e) = result {
                errors.push(format!("{}: {}", entry.name, e));
            }
        }
        if self.config.auto_reload {
            self.reload_panels();
        }
        if errors.is_empty() {
            self.notify(format!("Moved {} item(s)", sources.len()));
        } else {
            self.notify(format!("Errors: {}", errors.join("; ")));
        }
        Ok(())
    }

    pub fn cmd_delete_confirmed(&mut self, paths: Vec<PathBuf>) -> Result<()> {
        if self.active_panel().is_archive_view() {
            self.notify("Delete in archive is not supported");
            return Ok(());
        }
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
            self.notify(format!("Deleted {} item(s)", paths.len()));
        } else {
            self.notify(format!("Errors: {}", errors.join("; ")));
        }
        Ok(())
    }

    pub fn cmd_delete_remote_confirmed(&mut self, targets: Vec<RemoteDeleteTarget>) -> Result<()> {
        let mut errors = Vec::new();
        for target in &targets {
            let result = self.run_with_busy("Remote: deleting...", |_| {
                remote_delete_path(&target.profile, &target.path, target.is_dir)
            });
            if let Err(e) = result {
                errors.push(format!("{}: {}", target.path, e));
            }
        }
        if self.config.auto_reload {
            self.reload_panels();
        }
        if errors.is_empty() {
            self.notify(format!("Deleted {} item(s)", targets.len()));
        } else {
            self.notify(format!("Errors: {}", errors.join("; ")));
        }
        Ok(())
    }

    /// Force cloud-only placeholder files to be downloaded to local storage.
    ///
    /// Cloud providers such as iCloud, Dropbox, and OneDrive keep files as
    /// thin placeholders (zero local blocks) until they are first accessed.
    /// This command reads each file in full, which signals the OS / provider
    /// daemon to materialise the real data on disk.
    ///
    /// The set of files to process is determined by [`Panel::effective_selection`]:
    /// selected entries take priority; when nothing is selected the entry under
    /// the cursor is used.  Entries that are not cloud-only are silently skipped.
    /// After all downloads complete, the active panel is reloaded when
    /// `auto_reload` is enabled so the cloud-only indicators disappear.
    pub fn cmd_download_cloud_files(&mut self) {
        let entries: Vec<_> = self
            .active_panel()
            .effective_selection()
            .into_iter()
            .filter(|e| e.cloud_only)
            .map(|e| e.path.clone())
            .collect();

        if entries.is_empty() {
            self.notify("No cloud-only file selected");
            return;
        }

        let mut errors = Vec::new();
        for path in &entries {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let msg = format!("Downloading {}…", name);
            let result = self.run_with_busy(&msg, |_| {
                std::fs::read(path)?;
                Ok(())
            });
            if let Err(e) = result {
                errors.push(format!("{}: {}", name, e));
            }
        }

        if self.config.auto_reload {
            self.reload_panels();
        }
        if errors.is_empty() {
            self.notify(format!("Downloaded {} file(s)", entries.len()));
        } else {
            self.notify(format!("Errors: {}", errors.join("; ")));
        }
    }

    pub fn cmd_create_selection_m3u(&mut self) {
        if self.active_panel().is_remote_view() || self.active_panel().is_archive_view() {
            self.notify("Create m3u is available on local directories only");
            return;
        }

        let panel_path = self.active_panel().path.clone();
        let playlist_name = panel_path
            .file_name()
            .map(|name| format!("{}.m3u", name.to_string_lossy()))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "playlist.m3u".to_string());
        let output_path = panel_path.join(playlist_name);
        let mut entries = self
            .active_panel()
            .effective_selection()
            .into_iter()
            .filter(|entry| {
                !entry.is_dir
                    && entry.name != ".."
                    && entry.name != "[disconnect]"
                    && entry.path != output_path
            })
            .map(|entry| entry.name.clone())
            .collect::<Vec<_>>();

        if entries.is_empty() {
            self.notify("No file selected");
            return;
        }

        entries.sort();
        let mut text = entries.join("\n");
        text.push('\n');

        match fs::write(&output_path, text) {
            Ok(()) => {
                if self.config.auto_reload {
                    self.reload_panels();
                }
                self.notify(format!(
                    "Created {} with {} entr{}",
                    output_path.display(),
                    entries.len(),
                    if entries.len() > 1 { "ies" } else { "y" }
                ));
            }
            Err(e) => self.notify(format!("Cannot create m3u: {}", e)),
        }
    }

    /// Initiate a delete — show confirmation if enabled, else delete immediately.
    pub fn cmd_delete(&mut self) {
        let entries = self
            .active_panel()
            .effective_selection()
            .iter()
            .map(|e| (*e).clone())
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return;
        }

        let n = entries.len();
        let label = if n == 1 {
            entries[0].name.clone()
        } else {
            format!("{} items", n)
        };

        let paths = entries.iter().map(|e| e.path.clone()).collect::<Vec<_>>();
        let remote_targets = self
            .active_panel()
            .remote_profile()
            .map(|profile| {
                entries
                    .iter()
                    .map(|entry| RemoteDeleteTarget {
                        profile: profile.clone(),
                        path: entry.path.to_string_lossy().into_owned(),
                        is_dir: entry.is_dir,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if self.config.confirm_delete {
            self.mode = AppMode::Confirm(crate::app::ConfirmDialog {
                title: "Delete".into(),
                message: format!("Delete {}?", label),
                action: if self.active_panel().is_remote_view() {
                    crate::app::ConfirmAction::DeleteRemote(remote_targets)
                } else {
                    crate::app::ConfirmAction::Delete(paths)
                },
            });
        } else if self.active_panel().is_remote_view() {
            let _ = self.cmd_delete_remote_confirmed(remote_targets);
        } else {
            let _ = self.cmd_delete_confirmed(paths);
        }
    }

    // -----------------------------------------------------------------------
    // Viewer
    // -----------------------------------------------------------------------

    pub fn open_adjacent_viewer_file(&mut self, direction: isize) {
        let Some(idx) = self.adjacent_viewer_file_index(direction) else {
            if direction < 0 {
                self.notify("No previous file");
            } else {
                self.notify("No next file");
            }
            return;
        };

        let panel = self.active_panel_mut();
        panel.cursor = idx;
        if panel.cursor < panel.scroll {
            panel.scroll = panel.cursor;
        }
        self.open_viewer();
    }

    fn adjacent_viewer_file_index(&self, direction: isize) -> Option<usize> {
        let panel = self.active_panel();
        if direction < 0 {
            panel
                .entries
                .iter()
                .enumerate()
                .take(panel.cursor)
                .rev()
                .find_map(|(idx, entry)| viewer_navigable_entry(entry).then_some(idx))
                .or_else(|| {
                    panel
                        .entries
                        .iter()
                        .enumerate()
                        .rev()
                        .find_map(|(idx, entry)| viewer_navigable_entry(entry).then_some(idx))
                })
        } else {
            panel
                .entries
                .iter()
                .enumerate()
                .skip(panel.cursor.saturating_add(1))
                .find_map(|(idx, entry)| viewer_navigable_entry(entry).then_some(idx))
                .or_else(|| {
                    panel
                        .entries
                        .iter()
                        .enumerate()
                        .find_map(|(idx, entry)| viewer_navigable_entry(entry).then_some(idx))
                })
        }
    }

    pub fn open_viewer(&mut self) {
        if let Some(entry) = self.active_panel().current_entry().cloned() {
            if entry.is_dir || entry.name == ".." {
                let v = Viewer::placeholder(&entry.path, "Folder", self.config.viewer.word_wrap);
                self.mode = AppMode::Viewer(v);
                return;
            }
            if entry.cloud_only && !self.active_panel().is_remote_view() {
                let mut v = Viewer::placeholder(
                    &entry.path,
                    "Cloud-only file\nViewer disabled to avoid downloading it.",
                    self.config.viewer.word_wrap,
                );
                v.zoomed = self.config.viewer.default_zoom;
                self.mode = AppMode::Viewer(v);
                return;
            }
            let view_path = if self.active_panel().is_remote_view() {
                let Some(profile) = self.active_panel().remote_profile() else {
                    self.notify("Remote profile missing");
                    return;
                };
                match self.run_with_busy("Remote: downloading file...", |_| {
                    download_to_temp(&profile, &entry.path.to_string_lossy(), false)
                }) {
                    Ok(path) => path,
                    Err(e) => {
                        self.notify(format!("Remote download failed: {}", e));
                        return;
                    }
                }
            } else {
                entry.path.clone()
            };
            match Viewer::open(&view_path, self.config.viewer.word_wrap) {
                Ok(mut v) => {
                    // Classic 80x25 ANSI art fits best in the panel, even when default zoom is on.
                    if v.is_fixed_ansi_canvas() {
                        v.zoomed = false;
                    } else if !matches!(v.mode, ViewMode::Image) {
                        v.zoomed = self.config.viewer.default_zoom;
                    }
                    self.mode = AppMode::Viewer(v);
                }
                Err(e) => self.notify(format!("Cannot open viewer: {}", e)),
            }
        }
    }

    pub fn open_file_id_view(&mut self) {
        let enable = !self.file_preview_info;
        self.file_preview_info = enable;
        self.file_id_active = false;
        self.file_id_scroll = 0;
        if enable {
            self.close_quick_preview();
        }
    }

    pub fn close_file_id_view(&mut self) {
        self.file_preview_info = false;
        self.file_id_active = false;
        self.file_id_scroll = 0;
    }

    pub fn close_quick_preview(&mut self) {
        self.quick_preview_request_id = self.quick_preview_request_id.wrapping_add(1);
        self.quick_preview_task = None;
        self.quick_preview = None;
        self.quick_preview_active = false;
        self.quick_preview_forced_mode = None;
    }

    pub fn toggle_quick_preview(&mut self) -> Result<()> {
        if self.quick_preview.is_some() || self.quick_preview_task.is_some() {
            self.close_quick_preview();
            return Ok(());
        }

        let Some(entry) = self.active_panel().current_entry().cloned() else {
            return Ok(());
        };
        self.file_preview_info = false;
        self.file_id_active = false;
        self.file_id_scroll = 0;

        self.start_quick_preview_for_entry(entry);
        Ok(())
    }

    /// Refresh the quick-preview viewer when the cursor moves to a new file.
    /// Does nothing if quick preview is disabled.
    pub fn refresh_quick_preview(&mut self) {
        if self.quick_preview.is_none() && self.quick_preview_task.is_none() {
            return;
        }
        match self.active_panel().current_entry().cloned() {
            Some(entry) => self.start_quick_preview_for_entry(entry),
            None => {
                self.quick_preview_request_id = self.quick_preview_request_id.wrapping_add(1);
                self.quick_preview_task = None;
                self.quick_preview = None;
                self.quick_preview_active = false;
            }
        }
    }

    fn start_quick_preview_for_entry(&mut self, entry: crate::panel::Entry) {
        self.quick_preview_request_id = self.quick_preview_request_id.wrapping_add(1);
        self.quick_preview_task = None;

        let wrap = self.config.viewer.word_wrap;
        let mut viewer = if entry.is_dir || entry.name == ".." {
            Viewer::placeholder(&entry.path, "Folder", wrap)
        } else if entry.cloud_only {
            Viewer::placeholder(
                &entry.path,
                "Cloud-only file\nPreview disabled to avoid downloading it.",
                wrap,
            )
        } else {
            Viewer::placeholder(&entry.path, "Loading preview...", wrap)
        };
        viewer.zoomed = true;
        self.quick_preview = Some(viewer);

        if entry.is_dir || entry.name == ".." || entry.cloud_only {
            return;
        }

        let request_id = self.quick_preview_request_id;
        let path = entry.path;
        let worker_path = path.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = Viewer::open_preview(&worker_path, wrap).map_err(|err| err.to_string());
            let _ = tx.send(QuickPreviewMessage::Loaded(result));
        });
        self.quick_preview_task = Some(QuickPreviewTask {
            rx,
            request_id,
            path,
        });
    }

    /// Scroll the quick-preview viewer up one line.
    pub fn quick_preview_scroll_up(&mut self) {
        if let Some(v) = self.quick_preview.as_mut() {
            v.scroll_up();
        }
    }

    /// Scroll the quick-preview viewer down one line.
    pub fn quick_preview_scroll_down(&mut self) {
        if let Some(v) = self.quick_preview.as_mut() {
            v.scroll_down();
        }
    }

    pub fn file_id_scroll_up(&mut self) {
        self.file_id_scroll = self.file_id_scroll.saturating_sub(1);
    }

    pub fn file_id_scroll_down(&mut self) {
        self.file_id_scroll = self.file_id_scroll.saturating_add(1);
    }

    pub fn file_id_scroll_page_up(&mut self, page: u16) {
        self.file_id_scroll = self.file_id_scroll.saturating_sub(page);
    }

    pub fn file_id_scroll_page_down(&mut self, page: u16) {
        self.file_id_scroll = self.file_id_scroll.saturating_add(page);
    }

    pub fn file_id_home(&mut self) {
        self.file_id_scroll = 0;
    }

    pub fn build_file_id_preview(&self) -> String {
        if let Some(path) = self.active_panel().find_file_id_path() {
            if let Ok(bytes) = fs::read(&path) {
                return String::from_utf8_lossy(&bytes)
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .replace('\t', "    ");
            }
        }

        let Some(entry) = self.active_panel().current_entry() else {
            return "No FILE_ID.DIZ.".into();
        };
        if entry.name == ".." {
            return "No FILE_ID.DIZ.".into();
        }
        if entry.cloud_only {
            return "Cloud-only file.\nFileID disabled to avoid downloading it.".into();
        }

        let mut card = render_idf_card(&entry.path).unwrap_or_else(|| "No FILE_ID.DIZ.".into());
        // Append available viewer plugins
        let viewers = crate::plugins::viewer_plugins_for_path(&entry.path);
        if !viewers.is_empty() {
            card.push_str(&format!("Viewers: {}\n", viewers.join(", ")));
        }
        card
    }

    /// Set the status bar text and record its timestamp for auto-clear.
    pub fn set_status(&mut self, text: impl Into<String>) {
        self.status.text = text.into();
        self.status.set_at = Some(std::time::Instant::now());
    }

    /// Trigger the transient copy icon in the status bar for 10 seconds.
    pub fn trigger_status_copy_icon(&mut self) {
        self.status.copy_icon_at = Some(std::time::Instant::now());
    }

    pub fn status_copy_icon_visible(&self) -> bool {
        self.status
            .copy_icon_at
            .map(|at| at.elapsed() < std::time::Duration::from_secs(10))
            .unwrap_or(false)
    }

    pub fn run_with_busy<T, F>(&mut self, message: &str, op: F) -> Result<T>
    where
        F: FnOnce(&mut Self) -> Result<T>,
    {
        let previous_status = self.status.text.clone();
        // Don't update set_at for transient busy messages so the original
        // 30-second window is preserved when the previous message is restored.
        self.status.text = message.to_string();
        let _ = draw_busy_status(message, self.config.show_fkey_bar);
        let result = op(self);
        if self.status.text == message {
            self.status.text = previous_status;
        }
        result
    }

    fn move_between_panels(&self, entry: &crate::panel::Entry) -> Result<()> {
        let src_remote = self.active_panel().remote_profile();
        let dst_remote = self.other_panel().remote_profile();

        match (src_remote, dst_remote) {
            (None, None) => file_ops::move_entry(&entry.path, &self.other_panel().path),
            (None, Some(profile)) => {
                upload_into_dir(
                    &profile,
                    &entry.path,
                    self.other_panel().remote_cwd().unwrap_or("/"),
                    entry.is_dir,
                )?;
                file_ops::delete_entry(&entry.path)
            }
            (Some(profile), None) => {
                download_into_dir(
                    &profile,
                    &entry.path.to_string_lossy(),
                    &self.other_panel().path,
                    entry.is_dir,
                )?;
                remote_delete_path(&profile, &entry.path.to_string_lossy(), entry.is_dir)
            }
            (Some(src_profile), Some(dst_profile))
                if same_remote_target(&src_profile, &dst_profile) =>
            {
                let dst_path =
                    join_remote(self.other_panel().remote_cwd().unwrap_or("/"), &entry.name);
                remote_rename_path(&src_profile, &entry.path.to_string_lossy(), &dst_path)
            }
            (Some(src_profile), Some(dst_profile)) => {
                let tmp =
                    download_to_temp(&src_profile, &entry.path.to_string_lossy(), entry.is_dir)?;
                upload_into_dir(
                    &dst_profile,
                    &tmp,
                    self.other_panel().remote_cwd().unwrap_or("/"),
                    entry.is_dir,
                )?;
                remote_delete_path(&src_profile, &entry.path.to_string_lossy(), entry.is_dir)?;
                cleanup_temp_download(&tmp);
                Ok(())
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
        let start = self.active_panel().persisted_path();
        let dir_str = start.to_string_lossy().into_owned();
        self.mode = AppMode::SearchPanel(SearchState {
            query: "*".into(),
            content_query: String::new(),
            dir_query: dir_str,
            input_field: 0,
            results: Vec::new(),
            cursor: 0,
            scroll: 0,
            running: false,
            start_dir: start,
            backend: SearchBackend::best_default(),
            follow_links: false,
            search_rx: None,
            cancel_flag: None,
            dirs_visited: 0,
        });
    }

    pub fn open_tree_view(&mut self) {
        let Some(user_dirs) = directories::UserDirs::new() else {
            self.notify("Cannot determine user directory");
            return;
        };
        self.close_quick_preview();
        self.close_file_id_view();
        self.mode = AppMode::TreeView(TreeViewState::load_or_scan(
            user_dirs.home_dir().to_path_buf(),
        ));
    }

    pub fn open_help(&mut self) {
        self.mode = AppMode::Help(HelpState::load());
    }

    pub fn run_search(&mut self) {
        let AppMode::SearchPanel(ref mut state) = self.mode else {
            return;
        };

        // Cancel any previous search thread
        if let Some(ref flag) = state.cancel_flag {
            flag.store(true, Ordering::Relaxed);
        }
        state.search_rx = None;
        state.cancel_flag = None;

        // Resolve start directory from dir_query
        let start = {
            let p = std::path::Path::new(&state.dir_query);
            if p.is_dir() {
                p.to_path_buf()
            } else {
                state.start_dir.clone()
            }
        };
        state.start_dir = start.clone();
        state.results.clear();
        state.cursor = 0;
        state.scroll = 0;
        state.dirs_visited = 0;
        state.running = true;

        // Parse multiple patterns separated by ';'
        let patterns: Vec<String> = if state.query.is_empty() || state.query == "*" {
            vec!["*".into()]
        } else {
            state
                .query
                .split(';')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        };

        let content = if state.content_query.is_empty() {
            None
        } else {
            Some(state.content_query.clone())
        };

        let follow_links = state.follow_links;
        let backend = state.backend;
        let (tx, rx) = std::sync::mpsc::channel::<SearchResult>();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel);

        // If multiple patterns, search each and combine (deduplicate)
        if patterns.len() == 1 {
            // Single pattern: direct search
            let query = SearchQuery {
                pattern: patterns[0].clone(),
                content: content.clone(),
                start: start.clone(),
                follow_links,
            };

            match backend {
                SearchBackend::Walk => {
                    std::thread::spawn(move || {
                        let _ = search(&query, |r| {
                            if cancel_clone.load(Ordering::Relaxed) {
                                return false;
                            }
                            tx.send(r.clone()).is_ok()
                        });
                    });
                }
                SearchBackend::Spotlight => {
                    std::thread::spawn(move || {
                        let results = search_spotlight(&query, 1000);
                        for r in results {
                            if cancel_clone.load(Ordering::Relaxed) {
                                break;
                            }
                            if tx.send(r).is_err() {
                                break;
                            }
                        }
                    });
                }
                SearchBackend::Locate => {
                    std::thread::spawn(move || {
                        let results = search_locate(&query, 1000);
                        for r in results {
                            if cancel_clone.load(Ordering::Relaxed) {
                                break;
                            }
                            if tx.send(r).is_err() {
                                break;
                            }
                        }
                    });
                }
            }
        } else {
            // Multiple patterns: combine results and deduplicate
            std::thread::spawn(move || {
                use std::collections::HashSet;

                let mut seen_paths = HashSet::new();

                for pattern in patterns {
                    if cancel_clone.load(Ordering::Relaxed) {
                        break;
                    }

                    let query = SearchQuery {
                        pattern,
                        content: content.clone(),
                        start: start.clone(),
                        follow_links,
                    };

                    let results = match backend {
                        SearchBackend::Walk => {
                            let mut acc = Vec::new();
                            let _ = search(&query, |r| {
                                if cancel_clone.load(Ordering::Relaxed) {
                                    return false;
                                }
                                acc.push(r.clone());
                                true
                            });
                            acc
                        }
                        SearchBackend::Spotlight => search_spotlight(&query, 1000),
                        SearchBackend::Locate => search_locate(&query, 1000),
                    };

                    for r in results {
                        if cancel_clone.load(Ordering::Relaxed) {
                            break;
                        }
                        let path_str = r.path.to_string_lossy().to_string();
                        if !seen_paths.contains(&path_str) {
                            seen_paths.insert(path_str);
                            if tx.send(r).is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        }

        state.search_rx = Some(rx);
        state.cancel_flag = Some(cancel);
    }

    // -----------------------------------------------------------------------
    // Config persistence
    // -----------------------------------------------------------------------

    pub fn save_config(&mut self) -> Result<()> {
        self.normalize_shortcut_overrides();
        self.config.bookmarks = self.bookmarks.clone();
        match self.config.save() {
            Ok(()) => {
                crate::viewer::debug_log("config: config.toml saved");
                Ok(())
            }
            Err(e) => {
                crate::viewer::debug_log(&format!("config: config.toml save failed: {e}"));
                Err(e)
            }
        }
    }

    pub fn save_state(&mut self) -> Result<()> {
        self.config.left = panel_config_for_save(&self.left, &self.left_tabs);
        self.config.right = panel_config_for_save(&self.right, &self.right_tabs);
        self.config.dir_history = self.dir_history.iter().cloned().collect();
        self.config.palette_recent = self.palette_recent.clone();
        self.config.active_panel = match self.active {
            ActivePanel::Left => ActivePanelSide::Left,
            ActivePanel::Right => ActivePanelSide::Right,
        };
        self.config.panel_view_type = if self.quick_preview.is_some() {
            PanelViewType::QuickPreview
        } else if self.file_preview_info {
            PanelViewType::FilePreviewInfo
        } else {
            PanelViewType::Normal
        };
        // Save terminal history and output to cache (not config)
        let len = self.terminal.output.len();
        let start = len.saturating_sub(200);
        let _ = crate::terminal::save_terminal_cache(
            &self.terminal.history,
            &self.terminal.output[start..],
        );
        self.config.save_state()
    }
}

fn store_application_opener(item: &crate::plugins::StorePluginInfo) -> Option<String> {
    let bin = item.install_bin.as_deref()?.trim();
    if bin.is_empty() {
        return None;
    }
    let args = item
        .launch_args
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if args.is_empty() {
        Some(format!("{bin} %f"))
    } else {
        Some(format!("{bin} {args}"))
    }
}

fn viewer_navigable_entry(entry: &crate::panel::Entry) -> bool {
    !entry.is_dir && entry.name != ".." && !entry.cloud_only
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app_with_dirs(name: &str) -> (App, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "kkc-app-panel-sync-{}-{}",
            name,
            std::process::id()
        ));
        let left = root.join("left");
        let right = root.join("right");
        fs::create_dir_all(&left).expect("create left dir");
        fs::create_dir_all(&right).expect("create right dir");

        let mut config = Config::default();
        config.left.path = left;
        config.right.path = right;
        config.active_panel = ActivePanelSide::Left;
        (App::new(config), root)
    }

    #[test]
    fn arrow_panel_sync_opens_selected_dir_in_other_panel() {
        let (mut app, root) = test_app_with_dirs("dir");
        let target = app.left.path.join("folder");
        fs::create_dir_all(&target).expect("create target dir");
        app.left.reload().expect("reload left");
        app.left.restore_cursor_by_name("folder");
        app.file_preview_info = true;
        app.file_id_active = true;
        app.file_id_scroll = 5;

        app.send_active_entry_to_other_panel()
            .expect("sync should succeed");

        assert_eq!(app.right.path, target);
        assert_eq!(app.active, ActivePanel::Right);
        assert!(!app.file_preview_info);
        assert!(!app.file_id_active);
        assert_eq!(app.file_id_scroll, 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn arrow_panel_sync_opens_source_dir_and_selects_file_in_other_panel() {
        let (mut app, root) = test_app_with_dirs("file");
        let source_dir = app.left.path.clone();
        fs::write(source_dir.join("alpha.txt"), b"alpha").expect("write file");
        fs::write(source_dir.join("beta.txt"), b"beta").expect("write file");
        app.left.reload().expect("reload left");
        app.left.restore_cursor_by_name("beta.txt");
        app.quick_preview = Some(crate::viewer::Viewer::placeholder(
            &source_dir.join("beta.txt"),
            "Preview",
            false,
        ));
        app.quick_preview_active = true;

        app.send_active_entry_to_other_panel()
            .expect("sync should succeed");

        assert_eq!(app.right.path, source_dir);
        assert_eq!(app.active, ActivePanel::Right);
        assert_eq!(
            app.right.current_entry().map(|entry| entry.name.as_str()),
            Some("beta.txt")
        );
        assert!(app.quick_preview.is_none());
        assert!(!app.quick_preview_active);

        let _ = fs::remove_dir_all(root);
    }
}
