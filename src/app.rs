use crate::config::{Config, SortMode};
use crate::file_ops;
use crate::help::HelpState;
use crate::idf::render_idf_card;
use crate::panel::Panel;
use crate::remote::{
    SftpProfile, delete_path as remote_delete_path, download_into_dir, download_to_temp,
    join_remote, load_profiles, rename_path as remote_rename_path, save_profile,
    upload_into_dir,
};
use crate::search::{search, SearchQuery, SearchResult};
use crate::viewer::{EncodingMode, LineFeedMode, MaskKind, ViewMode, Viewer};
use anyhow::Result;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

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
    /// Viewer with a popup choice menu.
    ViewerMenu(Viewer, ViewerMenuState),
    /// Search panel (Alt-F7).
    SearchPanel(SearchState),
    /// Confirmation dialog.
    Confirm(ConfirmDialog),
    /// Single-line text input dialog.
    Input(InputDialog),
    /// Directory history popup.
    DirHistory,
    /// Help overlay.
    Help(HelpState),
    /// Menu bar / dropdown (F2).
    Menu(MenuState),
    /// Configuration screen (Options > Setup).
    Config(ConfigState),
    /// Choose from multiple registered openers.
    Opener(OpenerState),
    /// File-type association editor (Options > Associations).
    AssocEditor(AssocEditorState),
    /// Remote connection picker (Ctrl-F).
    RemoteConnect(RemoteConnectState),
    /// Add a new remote connection.
    RemoteEdit(RemoteEditState),
}

// ---------------------------------------------------------------------------
// Config screen
// ---------------------------------------------------------------------------

/// State for the full configuration screen.
#[derive(Debug, Clone)]
pub struct ConfigState {
    // checkboxes
    pub confirm_exit:     bool,
    pub confirm_delete:   bool,
    pub auto_reload:      bool,
    pub insert_moves_down: bool,
    pub select_dirs:      bool,
    pub show_hidden:      bool,
    pub color_by_type:    bool,
    pub show_fkey_bar:    bool,
    // text fields
    pub editor:           String,
    pub pager:            String,
    pub dir_history_max:  String,
    // cursor inside the form (0-based, covers checkboxes then text fields)
    pub cursor:           usize,
}

impl ConfigState {
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self {
            confirm_exit:     cfg.confirm_exit,
            confirm_delete:   cfg.confirm_delete,
            auto_reload:      cfg.auto_reload,
            insert_moves_down: cfg.insert_moves_down,
            select_dirs:      cfg.select_dirs,
            show_hidden:      cfg.left.show_hidden,
            color_by_type:    cfg.color_by_type,
            show_fkey_bar:    cfg.show_fkey_bar,
            editor:           cfg.editor.clone(),
            pager:            cfg.pager.clone(),
            dir_history_max:  cfg.dir_history_max.to_string(),
            cursor:           0,
        }
    }

    /// Apply the form values back into a Config.
    pub fn apply_to(&self, cfg: &mut crate::config::Config) {
        cfg.confirm_exit     = self.confirm_exit;
        cfg.confirm_delete   = self.confirm_delete;
        cfg.auto_reload      = self.auto_reload;
        cfg.insert_moves_down = self.insert_moves_down;
        cfg.select_dirs      = self.select_dirs;
        cfg.left.show_hidden = self.show_hidden;
        cfg.right.show_hidden = self.show_hidden;
        cfg.color_by_type    = self.color_by_type;
        cfg.show_fkey_bar    = self.show_fkey_bar;
        if !self.editor.trim().is_empty() {
            cfg.editor = self.editor.trim().to_owned();
        }
        if !self.pager.trim().is_empty() {
            cfg.pager = self.pager.trim().to_owned();
        }
        if let Ok(n) = self.dir_history_max.trim().parse::<usize>() {
            if n > 0 { cfg.dir_history_max = n; }
        }
    }

    pub const NUM_CHECKBOXES: usize = 8;
    pub const NUM_TOTAL: usize = 13; // 8 + 3 + OK + Cancel
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
    pub items: Vec<SftpProfile>,
    pub cursor: usize,
}

impl RemoteConnectState {
    pub fn load() -> Self {
        Self {
            items: load_profiles().unwrap_or_default(),
            cursor: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemoteEditState {
    pub fields: [String; 6],
    pub cursor: usize,
    pub input_cursor: usize,
}

impl RemoteEditState {
    pub const NAME: usize = 0;
    pub const HOST: usize = 1;
    pub const USER: usize = 2;
    pub const PORT: usize = 3;
    pub const PATH: usize = 4;
    pub const IDENTITY: usize = 5;
    pub const SAVE: usize = 6;
    pub const CANCEL: usize = 7;

    pub fn new() -> Self {
        Self {
            fields: Default::default(),
            cursor: 0,
            input_cursor: 0,
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

    pub fn build_profile(&self) -> Option<SftpProfile> {
        let name = self.fields[Self::NAME].trim();
        if name.is_empty() {
            return None;
        }
        let port = if self.fields[Self::PORT].trim().is_empty() {
            None
        } else {
            self.fields[Self::PORT].trim().parse::<u16>().ok()
        };
        Some(SftpProfile {
            name: name.to_string(),
            host: trim_opt(&self.fields[Self::HOST]),
            user: trim_opt(&self.fields[Self::USER]),
            port,
            path: trim_opt(&self.fields[Self::PATH]),
            identity_file: trim_opt(&self.fields[Self::IDENTITY]),
            source: crate::remote::RemoteSource::UserToml,
        })
    }
}

impl AssocEditorState {
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self {
            assocs: cfg.file_assoc.iter()
                .map(|a| (a.ext.clone(), a.openers.clone()))
                .collect(),
            cursor: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MenuState {
    /// Which top-level header is highlighted (0 = File … 6 = Help).
    pub bar_pos: usize,
    /// Whether the dropdown is open.
    pub open: bool,
    /// Cursor inside the dropdown (index into MENU_DATA[bar_pos]).
    pub item_pos: usize,
}

impl MenuState {
    pub fn new() -> Self {
        Self { bar_pos: 0, open: false, item_pos: 0 }
    }
}

/// Action executed when a menu item is chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuAction {
    Separator,
    ViewFile,
    EditFile,
    CopyFile,
    MoveFile,
    MkDir,
    RenameFile,
    DeleteFile,
    Quit,
    SwapPanels,
    SortName,
    SortExtension,
    SortDate,
    SortSize,
    SortUnsorted,
    ToggleHidden,
    Reload,
    GoToPath,
    SelectPattern,
    DeselectPattern,
    InvertSelection,
    SearchFiles,
    DirHistory,
    ToggleFBar,
    SaveConfig,
    Setup,
    Associations,
    Help,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerMenuKind {
    Mode,
    LineFeed,
    Preproc,
    Encoding,
    Mask,
}

#[derive(Debug, Clone)]
pub struct ViewerMenuState {
    pub kind: ViewerMenuKind,
    pub cursor: usize,
    pub param: u8,
}

impl ViewerMenuState {
    pub fn new(kind: ViewerMenuKind, viewer: &Viewer) -> Self {
        let cursor = match kind {
            ViewerMenuKind::Mode => match viewer.mode {
                ViewMode::Text => 0,
                ViewMode::Hex => 1,
                ViewMode::Ansi => 2,
                ViewMode::Html => 3,
            },
            ViewerMenuKind::LineFeed => match viewer.line_feed {
                LineFeedMode::DosCrLf => 0,
                LineFeedMode::UnixLf => 1,
                LineFeedMode::MacCr => 2,
                LineFeedMode::Mixed => 3,
            },
            ViewerMenuKind::Preproc => 0,
            ViewerMenuKind::Encoding => match viewer.encoding {
                EncodingMode::Plain => 0,
                EncodingMode::Cp437 => 1,
            },
            ViewerMenuKind::Mask => {
                if !viewer.mask_enabled {
                    4
                } else {
                    match viewer.mask {
                        MaskKind::C => 0,
                        MaskKind::Pascal => 1,
                        MaskKind::Assembler => 2,
                        MaskKind::Ketchup => 3,
                    }
                }
            }
        };
        let param = viewer.preproc_last_param().unwrap_or(0);
        Self { kind, cursor, param }
    }
}

pub type MenuEntry = (&'static str, Option<&'static str>, MenuAction);

pub const MENU_HEADERS: &[&str] = &[
    "File", "Panel", "Disk", "Selection", "Tools", "Options", "Help",
];

pub static MENU_DATA: &[&[MenuEntry]] = &[
    // 0 – File
    &[
        ("View",        Some("F3"),   MenuAction::ViewFile),
        ("Edit",        Some("F4"),   MenuAction::EditFile),
        ("",            None,         MenuAction::Separator),
        ("Copy to..",   Some("F5"),   MenuAction::CopyFile),
        ("Move to..",   Some("F6"),   MenuAction::MoveFile),
        ("Create Dir",  Some("F7"),   MenuAction::MkDir),
        ("Rename",      Some("S-F6"), MenuAction::RenameFile),
        ("Delete",      Some("F8"),   MenuAction::DeleteFile),
        ("",            None,         MenuAction::Separator),
        ("Quit",        Some("F10"),  MenuAction::Quit),
    ],
    // 1 – Panel
    &[
        ("Swap Panels",   None,         MenuAction::SwapPanels),
        ("",              None,         MenuAction::Separator),
        ("Sort by Name",  Some("^F1"),  MenuAction::SortName),
        ("Sort by Ext",   Some("^F2"),  MenuAction::SortExtension),
        ("Sort by Date",  Some("^F3"),  MenuAction::SortDate),
        ("Sort by Size",  Some("^F4"),  MenuAction::SortSize),
        ("Unsorted",      Some("^F5"),  MenuAction::SortUnsorted),
        ("",              None,         MenuAction::Separator),
        ("Tgl. Hidden",   Some("^H"),   MenuAction::ToggleHidden),
        ("Reload",        Some("^R"),   MenuAction::Reload),
    ],
    // 2 – Disk
    &[
        ("Go to Path..",  None,         MenuAction::GoToPath),
    ],
    // 3 – Selection
    &[
        ("Select..",      Some("+"),    MenuAction::SelectPattern),
        ("Deselect..",    Some("-"),    MenuAction::DeselectPattern),
        ("Invert",        Some("*"),    MenuAction::InvertSelection),
    ],
    // 4 – Tools
    &[
        ("Search..",      Some("A-F7"), MenuAction::SearchFiles),
        ("Dir History",   Some("^D"),   MenuAction::DirHistory),
    ],
    // 5 – Options
    &[
        ("Setup..",          None,        MenuAction::Setup),
        ("Associations..",   None,        MenuAction::Associations),
        ("Tgl. F-Key Bar",   None,        MenuAction::ToggleFBar),
        ("Save Config",      None,        MenuAction::SaveConfig),
    ],
    // 6 – Help
    &[
        ("Help",           Some("F1"),  MenuAction::Help),
        ("About KKC",      None,        MenuAction::About),
    ],
];

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    #[allow(dead_code)]
    pub title: String,
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    Quit,
    Delete(Vec<PathBuf>),
    DeleteRemote(Vec<RemoteDeleteTarget>),
}

#[derive(Debug, Clone)]
pub struct RemoteDeleteTarget {
    pub profile: SftpProfile,
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
    RemoteRename { profile: SftpProfile, path: String },
    RemoteMkdir { profile: SftpProfile, parent: String },
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
    pub file_id_preview: bool,
    pub mode: AppMode,
    pub status: StatusMessage,
    pub dir_history: VecDeque<PathBuf>,
    pub history_cursor: usize,
    /// Set to true after spawning an external program so the main loop can
    /// call terminal.clear() before the next draw.
    pub needs_clear: bool,
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
        let max = config.dir_history_max;
        let mut history: VecDeque<PathBuf> =
            config.dir_history.iter().cloned().take(max).collect();
        // Always seed with the left panel path if history is empty
        if history.is_empty() {
            history.push_front(config.left.path.clone());
        }

        App {
            config,
            left,
            right,
            active: ActivePanel::Left,
            file_id_preview: false,
            mode: AppMode::Browse,
            status: StatusMessage::default(),
            dir_history: history,
            history_cursor: 0,
            needs_clear: false,
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

    pub fn swap_panels(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
    }

    pub fn open_remote_connect(&mut self) {
        self.mode = AppMode::RemoteConnect(RemoteConnectState::load());
    }

    pub fn open_remote_add(&mut self) {
        self.mode = AppMode::RemoteEdit(RemoteEditState::new());
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

    pub fn enter_archive(&mut self, path: PathBuf) -> Result<()> {
        self.push_dir_history(path.clone());
        self.active_panel_mut().enter_archive(path)
    }

    pub fn connect_sftp(&mut self, profile: SftpProfile) -> Result<()> {
        self.file_id_preview = false;
        self.active_panel_mut().enter_remote(profile)
    }

    pub fn save_remote_profile(&mut self, profile: SftpProfile) -> Result<()> {
        save_profile(&profile)?;
        Ok(())
    }

    pub fn go_parent(&mut self) -> Result<()> {
        if self.active_panel().is_remote_view() {
            let current = self.active_panel().path.clone();
            let parent = current.parent().unwrap_or(std::path::Path::new("/")).to_path_buf();
            self.active_panel_mut().enter_dir(parent)?;
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

    // -----------------------------------------------------------------------
    // File operations
    // -----------------------------------------------------------------------

    pub fn cmd_copy(&mut self) -> Result<()> {
        if self.active_panel().is_archive_view() || self.other_panel().is_archive_view() {
            self.status.text = "Copy in archive is not supported".into();
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
        for entry in &sources {
            if let Err(e) = self.copy_between_panels(entry) {
                errors.push(format!("{}: {}", entry.name, e));
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
        if self.active_panel().is_archive_view() || self.other_panel().is_archive_view() {
            self.status.text = "Move in archive is not supported".into();
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
        for entry in &sources {
            if let Err(e) = self.move_between_panels(entry) {
                errors.push(format!("{}: {}", entry.name, e));
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
        if self.active_panel().is_archive_view() {
            self.status.text = "Delete in archive is not supported".into();
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
            self.status.text = format!("Deleted {} item(s)", paths.len());
        } else {
            self.status.text = format!("Errors: {}", errors.join("; "));
        }
        Ok(())
    }

    pub fn cmd_delete_remote_confirmed(&mut self, targets: Vec<RemoteDeleteTarget>) -> Result<()> {
        let mut errors = Vec::new();
        for target in &targets {
            if let Err(e) = remote_delete_path(&target.profile, &target.path, target.is_dir) {
                errors.push(format!("{}: {}", target.path, e));
            }
        }
        if self.config.auto_reload {
            self.reload_panels();
        }
        if errors.is_empty() {
            self.status.text = format!("Deleted {} item(s)", targets.len());
        } else {
            self.status.text = format!("Errors: {}", errors.join("; "));
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
        if let Some(entry) = self.active_panel().current_entry() {
            if entry.is_dir || entry.name == ".." {
                self.status.text = "Cannot view a directory".into();
                return;
            }
            let view_path = if self.active_panel().is_remote_view() {
                let Some(profile) = self.active_panel().remote_profile() else {
                    self.status.text = "Remote profile missing".into();
                    return;
                };
                match download_to_temp(&profile, &entry.path.to_string_lossy(), false) {
                    Ok(path) => path,
                    Err(e) => {
                        self.status.text = format!("Remote download failed: {}", e);
                        return;
                    }
                }
            } else {
                entry.path.clone()
            };
            match Viewer::open(&view_path, self.config.viewer.word_wrap) {
                Ok(v) => self.mode = AppMode::Viewer(v),
                Err(e) => self.status.text = format!("Cannot open viewer: {}", e),
            }
        }
    }

    pub fn open_file_id_view(&mut self) {
        self.file_id_preview = !self.file_id_preview;
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

        render_idf_card(&entry.path).unwrap_or_else(|| "No FILE_ID.DIZ.".into())
    }

    fn copy_between_panels(&self, entry: &crate::panel::Entry) -> Result<()> {
        let src_remote = self.active_panel().remote_profile();
        let dst_remote = self.other_panel().remote_profile();

        match (src_remote, dst_remote) {
            (None, None) => file_ops::copy_entry(&entry.path, &self.other_panel().path, None),
            (None, Some(profile)) => {
                upload_into_dir(
                    &profile,
                    &entry.path,
                    self.other_panel().remote_cwd().unwrap_or("/"),
                    entry.is_dir,
                )?;
                Ok(())
            }
            (Some(profile), None) => {
                download_into_dir(
                    &profile,
                    &entry.path.to_string_lossy(),
                    &self.other_panel().path,
                    entry.is_dir,
                )?;
                Ok(())
            }
            (Some(src_profile), Some(dst_profile)) => {
                let tmp = download_to_temp(&src_profile, &entry.path.to_string_lossy(), entry.is_dir)?;
                upload_into_dir(
                    &dst_profile,
                    &tmp,
                    self.other_panel().remote_cwd().unwrap_or("/"),
                    entry.is_dir,
                )?;
                cleanup_temp_download(&tmp);
                Ok(())
            }
        }
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
            (Some(src_profile), Some(dst_profile)) if same_remote_target(&src_profile, &dst_profile) => {
                let dst_path = join_remote(
                    self.other_panel().remote_cwd().unwrap_or("/"),
                    &entry.name,
                );
                remote_rename_path(&src_profile, &entry.path.to_string_lossy(), &dst_path)
            }
            (Some(src_profile), Some(dst_profile)) => {
                let tmp = download_to_temp(&src_profile, &entry.path.to_string_lossy(), entry.is_dir)?;
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

    pub fn open_help(&mut self) {
        self.mode = AppMode::Help(HelpState::load());
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
        self.config.left.path = self.left.persisted_path();
        self.config.left.sort = self.left.sort;
        self.config.left.show_hidden = self.left.show_hidden;
        self.config.right.path = self.right.persisted_path();
        self.config.right.sort = self.right.sort;
        self.config.right.show_hidden = self.right.show_hidden;
        self.config.dir_history = self.dir_history.iter().cloned().collect();
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

fn same_remote_target(a: &SftpProfile, b: &SftpProfile) -> bool {
    a.name.eq_ignore_ascii_case(&b.name)
        && a.host == b.host
        && a.user == b.user
        && a.port == b.port
}
