mod command_palette;
mod menu;
mod panel_tabs;

pub use self::command_palette::{CommandPaletteState, PALETTE_DATA, PALETTE_SEP};
pub use self::menu::{
    MENU_DATA, MENU_HEADERS, MenuAction, MenuEntry, MenuState, StoreInstallPaletteState,
    ViewerMenuKind, ViewerMenuState, ViewerPluginPaletteState,
};
use self::panel_tabs::{PanelTabs, panel_config_for_save, restore_panel_side};
use crate::about::AboutState;
use crate::config::{ActivePanelSide, Config, PanelConfig, PanelViewType, SortMode};
use crate::copy::{
    CopyDestination, CopyDialogState, CopyJob, CopyProgressState, CopyScanTask, CopySource,
    CopyTask, CopyTaskMessage, count_local_files, spawn_copy_scan, spawn_copy_task,
};
use crate::file_ops::{self, CopyOptions};
use crate::help::HelpState;
use crate::idf::render_idf_card;
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
    /// Viewer with a popup choice menu.
    ViewerMenu(Viewer, ViewerMenuState),
    /// Viewer plugin picker with a quick-palette filter.
    ViewerPluginPalette(Viewer, ViewerPluginPaletteState),
    /// Search panel (Alt-F7).
    SearchPanel(SearchState),
    /// Confirmation dialog.
    Confirm(ConfirmDialog),
    /// Single-line text input dialog.
    Input(InputDialog),
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
    pub const NUM_TOTAL: usize = 18; // 13 + 3 text + OK + Cancel

    pub fn ok_cursor() -> usize {
        Self::NUM_CHECKBOXES + 3
    }

    pub fn cancel_cursor() -> usize {
        Self::NUM_CHECKBOXES + 4
    }

    pub fn tab_range(tab: usize) -> std::ops::RangeInclusive<usize> {
        match tab {
            Self::TAB_BEHAVIOUR => 0..=4,
            Self::TAB_DISPLAY => 5..=9,
            Self::TAB_VIEWER => 10..=12,
            Self::TAB_EXTERNAL => 13..=15,
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
            13..=15 => Self::TAB_EXTERNAL,
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
pub struct OpenerState {
    pub items: Vec<String>,
    pub cursor: usize,
    pub path: std::path::PathBuf,
}

// ---------------------------------------------------------------------------
// Association editor state
// ---------------------------------------------------------------------------

/// State for the full-screen association editor.
#[derive(Debug, Clone)]
pub struct AssocEditorState {
    /// (extension, openers) pairs – mirrors config.file_assoc.
    pub assocs: Vec<(String, Vec<String>)>,
    pub cursor: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteEditKind {
    Sftp,
    Imap,
    Smb,
}

impl RemoteEditKind {
    /// All protocol choices in menu order.
    pub fn all() -> &'static [Self] {
        &[Self::Sftp, Self::Imap, Self::Smb]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Sftp => "SFTP",
            Self::Imap => "IMAP",
            Self::Smb => "SMB",
        }
    }

    /// UI accent colour (R, G, B).
    pub fn color_rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Sftp => (121, 214, 255),
            Self::Imap => (181, 238, 170),
            Self::Smb => (255, 165, 80),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Sftp => " Add SFTP Server ",
            Self::Imap => " Add IMAP Server ",
            Self::Smb => " Add SMB Server ",
        }
    }

    pub fn field_labels(self) -> [&'static str; 6] {
        match self {
            Self::Sftp => ["Name", "Host", "User", "Port", "Path", "Identity"],
            Self::Imap => ["Name", "Host", "User", "Port", "Mailbox", "Password"],
            Self::Smb => ["Name", "Host", "User", "Workgroup", "Share", "Password"],
        }
    }

    pub fn validation_message(self) -> &'static str {
        match self {
            Self::Sftp => "SFTP name is required",
            Self::Imap => "IMAP name, host and user are required",
            Self::Smb => "SMB name and host are required",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteEditState {
    pub kind: RemoteEditKind,
    pub fields: [String; 6],
    pub cursor: usize,
    pub input_cursor: usize,
    /// Original name when editing an existing profile (for rename support).
    pub edit_original_name: Option<String>,
    /// Fetched share list for SMB connections (populated on F5), with cursor.
    pub share_picker: Option<(Vec<String>, usize)>,
}

impl RemoteEditState {
    pub const NAME: usize = 0;
    pub const HOST: usize = 1;
    pub const USER: usize = 2;
    pub const PORT: usize = 3;
    pub const PATH: usize = 4;
    pub const SECRET: usize = 5;
    pub const SAVE: usize = 6;
    pub const CANCEL: usize = 7;

    pub fn new(kind: RemoteEditKind) -> Self {
        let fields = match kind {
            RemoteEditKind::Sftp => [
                String::new(),
                String::new(),
                String::new(),
                "22".into(),
                "~".into(),
                String::new(),
            ],
            _ => Default::default(),
        };
        let input_cursor = fields[Self::NAME].len();
        Self {
            kind,
            fields,
            cursor: 0,
            input_cursor,
            edit_original_name: None,
            share_picker: None,
        }
    }

    pub fn from_profile(profile: &RemoteProfile) -> Self {
        let (kind, fields) = match &profile.kind {
            RemoteKind::Sftp(sftp) => (
                RemoteEditKind::Sftp,
                [
                    profile.name.clone(),
                    sftp.host.clone().unwrap_or_default(),
                    sftp.user.clone().unwrap_or_default(),
                    sftp.port.map(|p| p.to_string()).unwrap_or_default(),
                    sftp.path.clone().unwrap_or_default(),
                    sftp.identity_file.clone().unwrap_or_default(),
                ],
            ),
            RemoteKind::Imap(imap) => (
                RemoteEditKind::Imap,
                [
                    profile.name.clone(),
                    imap.host.clone(),
                    imap.user.clone(),
                    imap.port.map(|p| p.to_string()).unwrap_or_default(),
                    imap.path.clone().unwrap_or_default(),
                    imap.password.clone().unwrap_or_default(),
                ],
            ),
            RemoteKind::Smb(smb) => (
                RemoteEditKind::Smb,
                [
                    profile.name.clone(),
                    smb.host.clone(),
                    smb.user.clone().unwrap_or_default(),
                    smb.workgroup.clone().unwrap_or_default(),
                    smb.share.clone().unwrap_or_default(),
                    smb.password.clone().unwrap_or_default(),
                ],
            ),
        };
        Self {
            kind,
            input_cursor: fields[Self::NAME].len(),
            fields,
            cursor: 0,
            edit_original_name: Some(profile.name.clone()),
            share_picker: None,
        }
    }

    pub fn current_value(&self) -> Option<&String> {
        self.fields.get(self.cursor)
    }

    pub fn current_value_mut(&mut self) -> Option<&mut String> {
        self.fields.get_mut(self.cursor)
    }

    pub fn sync_cursor(&mut self) {
        self.input_cursor = self.current_value().map(|s| s.len()).unwrap_or(0);
    }

    pub fn build_profile(&self) -> Option<RemoteProfile> {
        let name = self.fields[Self::NAME].trim();
        if name.is_empty() {
            return None;
        }
        let port = if self.fields[Self::PORT].trim().is_empty() {
            None
        } else {
            self.fields[Self::PORT].trim().parse::<u16>().ok()
        };
        Some(match self.kind {
            RemoteEditKind::Sftp => RemoteProfile {
                name: name.to_string(),
                source: RemoteSource::UserToml,
                kind: RemoteKind::Sftp(crate::remote::SftpProfile {
                    host: trim_opt(&self.fields[Self::HOST]),
                    user: trim_opt(&self.fields[Self::USER]),
                    port,
                    path: trim_opt(&self.fields[Self::PATH]),
                    identity_file: trim_opt(&self.fields[Self::SECRET]),
                }),
            },
            RemoteEditKind::Imap => {
                let host = self.fields[Self::HOST].trim();
                let user = self.fields[Self::USER].trim();
                if host.is_empty() || user.is_empty() {
                    return None;
                }
                RemoteProfile {
                    name: name.to_string(),
                    source: RemoteSource::UserToml,
                    kind: RemoteKind::Imap(crate::remote::ImapProfile {
                        host: host.to_string(),
                        user: user.to_string(),
                        port,
                        path: trim_opt(&self.fields[Self::PATH]),
                        password: trim_opt(&self.fields[Self::SECRET]),
                    }),
                }
            }
            RemoteEditKind::Smb => {
                let host = self.fields[Self::HOST].trim();
                if host.is_empty() {
                    return None;
                }
                RemoteProfile {
                    name: name.to_string(),
                    source: RemoteSource::UserToml,
                    kind: RemoteKind::Smb(crate::remote::SmbProfile {
                        host: host.to_string(),
                        user: trim_opt(&self.fields[Self::USER]),
                        workgroup: trim_opt(&self.fields[Self::PORT]),
                        share: trim_opt(&self.fields[Self::PATH]),
                        password: trim_opt(&self.fields[Self::SECRET]),
                        path: None,
                    }),
                }
            }
        })
    }
}

impl AssocEditorState {
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self {
            assocs: cfg
                .file_assoc
                .iter()
                .map(|a| (a.ext.clone(), a.openers.clone()))
                .collect(),
            cursor: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Debug)]
pub enum ConfirmAction {
    Message,
    /// Show message, then switch to this mode on dismiss.
    MessageThen(Box<AppMode>),
    Quit,
    Delete(Vec<PathBuf>),
    DeleteRemote(Vec<RemoteDeleteTarget>),
}

#[derive(Debug, Clone)]
pub struct RemoteDeleteTarget {
    pub profile: RemoteProfile,
    pub path: String,
    pub is_dir: bool,
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
    RemoteRename {
        profile: RemoteProfile,
        path: String,
    },
    RemoteMkdir {
        profile: RemoteProfile,
        parent: String,
    },
    /// Wildcard select (+)
    SelectPattern,
    /// Wildcard deselect (-)
    DeselectPattern,
    /// Navigate active panel to typed path
    GoToPath,
    /// Step 1 of adding an association: user typed the extension
    AssocAddExt,
    /// Step 2 of adding/editing: user typed the openers (comma-separated)
    AssocAddOpeners {
        ext: String,
        /// Some(idx) = editing existing row, None = new
        edit_index: Option<usize>,
    },
    PluginAction {
        plugin: String,
        id: String,
        cwd: PathBuf,
    },
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

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.value.len();
    }
}

// ---------------------------------------------------------------------------
// Search state
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct SearchState {
    pub query: String,
    pub content_query: String,
    pub dir_query: String,
    pub input_field: usize, // 0=name 1=content 2=dir 3=results
    pub results: Vec<SearchResult>,
    pub cursor: usize,
    pub scroll: usize,
    pub running: bool,
    pub start_dir: PathBuf,
    pub backend: SearchBackend,
    pub follow_links: bool,
    /// Background search thread sends results here.
    pub search_rx: Option<std::sync::mpsc::Receiver<SearchResult>>,
    /// Set to `true` to ask the background thread to stop early.
    pub cancel_flag: Option<Arc<AtomicBool>>,
    /// Number of directories visited so far (for progress display).
    pub dirs_visited: usize,
}

// ---------------------------------------------------------------------------
// Status-bar message
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct StatusMessage {
    pub text: String,
    /// When the current text was last set (used for auto-clear after 30 s).
    pub set_at: Option<std::time::Instant>,
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
            mode: if let Some(msg) = plugin_status {
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
        };

        match app.config.panel_view_type {
            PanelViewType::Normal => {}
            PanelViewType::FilePreviewInfo => {
                app.file_preview_info = true;
            }
            PanelViewType::QuickPreview => {
                let preview_start = std::time::Instant::now();
                if let Some(entry) = app.active_panel().current_entry().cloned() {
                    if entry.is_dir || entry.name == ".." {
                        let mut v =
                            Viewer::placeholder(&entry.path, "Folder", app.config.viewer.word_wrap);
                        v.zoomed = true;
                        app.quick_preview = Some(v);
                    } else if entry.cloud_only {
                        let mut v = Viewer::placeholder(
                            &entry.path,
                            "Cloud-only file\nPreview disabled to avoid downloading it.",
                            app.config.viewer.word_wrap,
                        );
                        v.zoomed = true;
                        app.quick_preview = Some(v);
                    } else if let Ok(mut v) =
                        Viewer::open_preview(&entry.path, app.config.viewer.word_wrap)
                    {
                        v.zoomed = true;
                        if let Some(mode) = app.quick_preview_forced_mode {
                            v.set_mode(mode);
                        }
                        app.quick_preview = Some(v);
                    }
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

    pub fn add_current_dir_bookmark(&mut self) {
        let cur = self.current_bookmark_candidate();
        if !self.bookmarks.contains(&cur) {
            self.bookmarks.push(cur);
            self.bookmark_cursor = self.bookmarks.len() - 1;
            self.bookmark_match_pos = 0;
            self.sync_bookmark_cursor();
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
        if self.bookmark_match_pos > 0 {
            self.bookmark_match_pos -= 1;
        }
        self.sync_bookmark_cursor();
    }

    pub fn move_next_bookmark(&mut self) {
        let len = self.filtered_bookmark_items().len();
        if self.bookmark_match_pos + 1 < len {
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
        self.push_dir_history(path.clone());
        if self.active_panel().is_remote_view() && path.is_dir() {
            // Local path selected while on a remote panel — disconnect first.
            self.active_panel_mut().disconnect();
            return self.active_panel_mut().enter_dir(path);
        }
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
                .filter(|p| p.source == RemoteSource::UserToml)
                .cloned()
        } else {
            None
        };
        if let Some(profile) = profile {
            self.mode = AppMode::RemoteEdit(RemoteEditState::from_profile(&profile));
        } else {
            self.notify("Only user-defined (toml) connections can be edited");
        }
    }

    pub fn go_parent(&mut self) -> Result<()> {
        if self.active_panel().is_remote_view() {
            let current = self.active_panel().path.clone();
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
        // Auto-clear status bar text after 30 seconds.
        if let Some(set_at) = self.status.set_at {
            if set_at.elapsed() >= std::time::Duration::from_secs(30) {
                self.status.text.clear();
                self.status.set_at = None;
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
                    // Image mode always zooms; for other modes, honour the config default.
                    if !matches!(v.mode, ViewMode::Image) {
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

    pub fn close_quick_preview(&mut self) {
        self.quick_preview = None;
        self.quick_preview_active = false;
        self.quick_preview_forced_mode = None;
    }

    pub fn toggle_quick_preview(&mut self) -> Result<()> {
        if self.quick_preview.is_some() {
            self.close_quick_preview();
            return Ok(());
        }

        let Some(entry) = self.active_panel().current_entry().cloned() else {
            return Ok(());
        };
        self.file_preview_info = false;
        self.file_id_active = false;
        self.file_id_scroll = 0;

        let mut v = if entry.is_dir || entry.name == ".." {
            Viewer::placeholder(&entry.path, "Folder", self.config.viewer.word_wrap)
        } else if entry.cloud_only {
            Viewer::placeholder(
                &entry.path,
                "Cloud-only file\nPreview disabled to avoid downloading it.",
                self.config.viewer.word_wrap,
            )
        } else {
            Viewer::open_preview(&entry.path, self.config.viewer.word_wrap)?
        };
        v.zoomed = true;
        if let Some(mode) = self.quick_preview_forced_mode
            && !(entry.is_dir || entry.name == "..")
        {
            v.set_mode(mode);
        }
        self.quick_preview = Some(v);
        Ok(())
    }

    /// Refresh the quick-preview viewer when the cursor moves to a new file.
    /// Does nothing if quick_preview is None. Clears the preview for dirs.
    pub fn refresh_quick_preview(&mut self) {
        if self.quick_preview.is_none() {
            return;
        }
        let wrap = self.config.viewer.word_wrap;
        match self.active_panel().current_entry().cloned() {
            Some(entry) if entry.is_dir || entry.name == ".." => {
                let mut v = Viewer::placeholder(&entry.path, "Folder", wrap);
                v.zoomed = true;
                self.quick_preview = Some(v);
            }
            Some(entry) if entry.cloud_only => {
                let mut v = Viewer::placeholder(
                    &entry.path,
                    "Cloud-only file\nPreview disabled to avoid downloading it.",
                    wrap,
                );
                v.zoomed = true;
                self.quick_preview = Some(v);
            }
            Some(entry) => {
                if let Ok(mut v) = Viewer::open_preview(&entry.path, wrap) {
                    v.zoomed = true;
                    if let Some(mode) = self.quick_preview_forced_mode {
                        v.set_mode(mode);
                    }
                    self.quick_preview = Some(v);
                }
                // On failure keep the previous preview (better than blank)
            }
            None => {
                self.quick_preview = None;
                self.quick_preview_active = false;
            }
        }
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

    pub fn run_with_progress<T, F>(&mut self, title: &str, op: F) -> Result<T>
    where
        F: FnOnce(&mut dyn FnMut(u8, &str)) -> Result<T>,
    {
        let previous_status = self.status.text.clone();
        let has_fkey_bar = self.config.show_fkey_bar;

        let mut report = |percent: u8, phase: &str| {
            let pct = percent.min(100);
            let bar = progress_bar(pct, 24);
            let msg = format!("{} {} {:>3}% {}", title, bar, pct, phase);
            self.status.text = msg.clone();
            let _ = draw_busy_status(&msg, has_fkey_bar);
        };

        report(0, "Starting...");
        let result = op(&mut report);
        if self.status.text.starts_with(title) {
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

        let query = SearchQuery {
            pattern: if state.query.is_empty() || state.query == "*" {
                "*".into()
            } else {
                state.query.clone()
            },
            content: if state.content_query.is_empty() {
                None
            } else {
                Some(state.content_query.clone())
            },
            start,
            follow_links: state.follow_links,
        };

        let backend = state.backend;
        let (tx, rx) = std::sync::mpsc::channel::<SearchResult>();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel);

        match backend {
            SearchBackend::Walk => {
                std::thread::spawn(move || {
                    let _ = search(&query, |r| {
                        if cancel_clone.load(Ordering::Relaxed) {
                            return false;
                        }
                        tx.send(r.clone()).is_ok()
                    });
                    // tx drops here → Disconnected signals completion to poller
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

        state.search_rx = Some(rx);
        state.cancel_flag = Some(cancel);
    }

    // -----------------------------------------------------------------------
    // Config persistence
    // -----------------------------------------------------------------------

    pub fn save_config(&mut self) -> Result<()> {
        self.config.left = panel_config_for_save(&self.left, &self.left_tabs);
        self.config.right = panel_config_for_save(&self.right, &self.right_tabs);
        self.config.dir_history = self.dir_history.iter().cloned().collect();
        self.config.bookmarks = self.bookmarks.clone();
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
        self.config.save()
    }
}

fn trim_opt(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn panel_config_needs_profiles(cfg: &PanelConfig) -> bool {
    cfg.remote_name.is_some() || cfg.tabs.iter().any(|tab| tab.remote_name.is_some())
}

fn cleanup_temp_download(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
        if let Some(parent) = path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

fn same_remote_target(a: &RemoteProfile, b: &RemoteProfile) -> bool {
    match (&a.kind, &b.kind) {
        (RemoteKind::Sftp(a), RemoteKind::Sftp(b)) => {
            a.host == b.host
                && a.user == b.user
                && a.port == b.port
                && a.identity_file == b.identity_file
        }
        _ => false,
    }
}

fn draw_busy_status(message: &str, has_fkey_bar: bool) -> Result<()> {
    let (_, rows) = size()?;
    if rows == 0 {
        return Ok(());
    }
    let status_row = if has_fkey_bar {
        rows.saturating_sub(2)
    } else {
        rows.saturating_sub(1)
    };
    let mut stdout = io::stdout();
    let line = format!(" {} ", message);
    queue!(
        stdout,
        MoveTo(0, status_row),
        SetForegroundColor(crossterm::style::Color::Rgb {
            r: 244,
            g: 235,
            b: 208
        }),
        SetBackgroundColor(crossterm::style::Color::Rgb {
            r: 125,
            g: 107,
            b: 92
        }),
        Clear(ClearType::CurrentLine),
        Print(line),
        ResetColor,
    )?;
    stdout.flush()?;
    Ok(())
}

fn progress_bar(percent: u8, width: usize) -> String {
    let p = percent.min(100) as usize;
    let filled = (p * width) / 100;
    let mut bar = String::with_capacity(width + 2);
    bar.push('[');
    bar.extend(std::iter::repeat('#').take(filled));
    bar.extend(std::iter::repeat('-').take(width.saturating_sub(filled)));
    bar.push(']');
    bar
}

fn spawn_remote_connect_task(profile: RemoteProfile, show_hidden: bool) -> RemoteConnectTask {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_bg = cancel.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(String, Vec<RemoteEntry>)> {
            if cancel_bg.load(Ordering::Relaxed) {
                anyhow::bail!("Aborted");
            }
            let mut progress = |phase: String| {
                let _ = tx.send(RemoteConnectMessage::Progress(phase));
            };
            prepare_connection(&profile, show_hidden, &mut progress, &cancel_bg)
        })();
        match result {
            Ok((cwd, entries)) => {
                let _ = tx.send(RemoteConnectMessage::Connected {
                    profile,
                    cwd,
                    entries,
                });
            }
            Err(err) => {
                if !cancel_bg.load(Ordering::Relaxed) {
                    let _ = tx.send(RemoteConnectMessage::Failed(err.to_string()));
                }
            }
        }
    });
    RemoteConnectTask { rx, cancel }
}
