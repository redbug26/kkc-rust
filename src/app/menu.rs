use crate::viewer::{EncodingMode, LineFeedMode, MaskKind, ViewMode, Viewer};

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
        Self {
            bar_pos: 0,
            open: false,
            item_pos: 0,
        }
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
    RemoteConnect,
    FileIdPreview,
    DirBookmarks,
    ToggleFBar,
    SaveConfig,
    Setup,
    Plugins,
    Associations,
    Help,
    About,
    NewTab,
    CloseTab,
    NextTab,
    OpenTerminal,
    CaptureGif,
    OpenInOs,
    OpenFolderInOs,
    QuickPreview,
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
    pub scroll: usize,
    pub param: u8,
}

#[derive(Debug, Clone)]
pub struct ViewerPluginPaletteState {
    pub items: Vec<crate::plugins::PluginInfo>,
    pub query: String,
    pub match_pos: usize,
}

impl ViewerPluginPaletteState {
    pub fn load(viewer: &Viewer) -> Self {
        let mut state = Self {
            items: crate::plugins::viewer_plugin_infos(),
            query: String::new(),
            match_pos: 0,
        };
        if let Some(plugin_name) = &viewer.viewer_plugin
            && let Some(pos) = state
                .items
                .iter()
                .position(|plugin| &plugin.name == plugin_name)
        {
            state.match_pos = pos;
        }
        state
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.query.trim().is_empty() {
            return (0..self.items.len()).collect();
        }

        let tokens: Vec<String> = self
            .query
            .split_whitespace()
            .map(|token| token.to_lowercase())
            .filter(|token| !token.is_empty())
            .collect();
        if tokens.is_empty() {
            return (0..self.items.len()).collect();
        }

        let first = &tokens[0];
        let rest = &tokens[1..];
        let mut starts = Vec::new();
        let mut contains = Vec::new();

        for (idx, item) in self.items.iter().enumerate() {
            let searchable = format!(
                "{} {} {}",
                item.name,
                item.description,
                item.extensions.join(" ")
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
        self.match_pos = 0;
        self.clamp_match();
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.match_pos = 0;
        self.clamp_match();
    }

    pub fn move_prev(&mut self) {
        self.match_pos = self.match_pos.saturating_sub(1);
        self.clamp_match();
    }

    pub fn move_next(&mut self) {
        let len = self.filtered_indices().len();
        if self.match_pos + 1 < len {
            self.match_pos += 1;
        }
        self.clamp_match();
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

impl ViewerMenuState {
    pub fn new(kind: ViewerMenuKind, viewer: &Viewer) -> Self {
        let cursor = match kind {
            ViewerMenuKind::Mode => {
                if viewer.viewer_plugin.is_some() {
                    4
                } else {
                    match viewer.mode {
                        ViewMode::Text => 0,
                        ViewMode::Hex => 1,
                        ViewMode::Ansi => 2,
                        ViewMode::Image => 3,
                    }
                }
            }
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
                    13 // "Syntax OFF" is the last item
                } else {
                    match viewer.mask {
                        MaskKind::Auto => 0,
                        MaskKind::C => 1,
                        MaskKind::Rust => 2,
                        MaskKind::JavaScript => 3,
                        MaskKind::Python => 4,
                        MaskKind::Php => 5,
                        MaskKind::Html => 6,
                        MaskKind::Css => 7,
                        MaskKind::Sql => 8,
                        MaskKind::Shell => 9,
                        MaskKind::Pascal => 10,
                        MaskKind::Assembler => 11,
                        MaskKind::Ketchup => 12,
                    }
                }
            }
        };
        let param = viewer.preproc_last_param().unwrap_or(0);
        Self {
            kind,
            cursor,
            scroll: 0,
            param,
        }
    }
}

pub type MenuEntry = (&'static str, Option<&'static str>, MenuAction);

pub const MENU_HEADERS: &[&str] = &[
    "File",
    "Panel",
    "Disk",
    "Selection",
    "Tools",
    "Options",
    "Help",
];

pub static MENU_DATA: &[&[MenuEntry]] = &[
    &[
        ("View", Some("F3"), MenuAction::ViewFile),
        ("Edit", Some("F4"), MenuAction::EditFile),
        ("", None, MenuAction::Separator),
        ("Copy to..", Some("F5"), MenuAction::CopyFile),
        ("Move to..", Some("F6"), MenuAction::MoveFile),
        ("Create Dir", Some("F7"), MenuAction::MkDir),
        ("Rename", Some("S-F6"), MenuAction::RenameFile),
        ("Delete", Some("F8"), MenuAction::DeleteFile),
        ("", None, MenuAction::Separator),
        ("Quit", Some("F10"), MenuAction::Quit),
    ],
    &[
        ("Swap Panels", None, MenuAction::SwapPanels),
        ("", None, MenuAction::Separator),
        ("Sort by Name", Some("^F1"), MenuAction::SortName),
        ("Sort by Ext", Some("^F2"), MenuAction::SortExtension),
        ("Sort by Date", Some("^F3"), MenuAction::SortDate),
        ("Sort by Size", Some("^F4"), MenuAction::SortSize),
        ("Unsorted", Some("^F5"), MenuAction::SortUnsorted),
        ("", None, MenuAction::Separator),
        ("Tgl. Hidden", Some("^H"), MenuAction::ToggleHidden),
        ("Reload", Some("^R"), MenuAction::Reload),
    ],
    &[("Go to Path..", None, MenuAction::GoToPath)],
    &[
        ("Select..", Some("+"), MenuAction::SelectPattern),
        ("Deselect..", Some("-"), MenuAction::DeselectPattern),
        ("Invert", Some("*"), MenuAction::InvertSelection),
    ],
    &[
        ("Search..", Some("A-F7"), MenuAction::SearchFiles),
        ("Remote Connect..", Some("^F"), MenuAction::RemoteConnect),
        ("File ID Preview", Some("A-F4"), MenuAction::FileIdPreview),
        ("Bookmarks", Some("^D"), MenuAction::DirBookmarks),
    ],
    &[
        ("Setup..", None, MenuAction::Setup),
        ("Plugins..", None, MenuAction::Plugins),
        ("Associations..", None, MenuAction::Associations),
        ("Tgl. F-Key Bar", None, MenuAction::ToggleFBar),
        ("Save Config", None, MenuAction::SaveConfig),
    ],
    &[
        ("Help", Some("F1"), MenuAction::Help),
        ("About KKC", None, MenuAction::About),
    ],
];
