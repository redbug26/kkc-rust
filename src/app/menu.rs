use crate::viewer::{EncodingMode, LineFeedMode, MaskKind, ViewMode, Viewer};
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::path::PathBuf;

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
    OpenMenu,
    OpenCommandPalette,
    OpenActionPalette,
    SwitchPanel,
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
    TreeView,
    InstallPluginFromStore,
    RemoteConnect,
    FileIdPreview,
    DirBookmarks,
    ToggleFBar,
    SaveConfig,
    Shortcuts,
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
    DebugLog,
    DownloadCloudFile,
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
pub struct ViewerGotoState {
    pub cursor: usize,
    pub count: String,
}

impl ViewerGotoState {
    pub fn new() -> Self {
        Self {
            cursor: 0,
            count: String::new(),
        }
    }
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

#[derive(Debug, Clone)]
pub struct StoreInstallProgress {
    pub title: String,
    pub item_name: String,
    pub percent: u8,
    pub phase: String,
}

#[derive(Debug, Clone)]
pub enum StoreDetectChoice {
    Keep,
    Install,
    Remove,
}

#[derive(Debug, Clone)]
pub struct StoreDetectItem {
    pub app: crate::plugins::StorePluginInfo,
    pub choice: StoreDetectChoice,
}

#[derive(Debug, Clone)]
pub struct StoreDetectState {
    pub items: Vec<StoreDetectItem>,
    pub cursor: usize,
    pub detected_count: usize,
}

#[derive(Debug, Clone)]
pub struct StoreInstallPaletteState {
    pub index_path: PathBuf,
    pub index_info: crate::plugins::StoreIndexInfo,
    pub items: Vec<crate::plugins::StorePluginInfo>,
    pub installed_versions: HashMap<String, String>,
    pub installed_app_versions: HashMap<String, String>,
    pub query: String,
    pub match_pos: usize,
    pub progress: Option<StoreInstallProgress>,
    pub detect: Option<StoreDetectState>,
}

impl StoreInstallPaletteState {
    pub fn load(index_path: PathBuf) -> anyhow::Result<Self> {
        let installed_versions = crate::plugins::installed_plugin_versions_by_dir();
        let installed_app_versions = crate::plugins::load_store_applications()
            .unwrap_or_default()
            .into_iter()
            .map(|app| (app.id, app.version))
            .collect::<HashMap<_, _>>();
        let (mut items, index_info) = crate::plugins::list_store_plugins_with_info(&index_path)?;
        items.sort_by(|a, b| {
            let a_update = {
                store_item_has_update(a, &installed_versions, &installed_app_versions)
            };
            let b_update = {
                store_item_has_update(b, &installed_versions, &installed_app_versions)
            };

            b_update
                .cmp(&a_update)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.id.cmp(&b.id))
        });

        Ok(Self {
            items,
            index_path,
            index_info,
            installed_versions,
            installed_app_versions,
            query: String::new(),
            match_pos: 0,
            progress: None,
            detect: None,
        })
    }

    pub fn index_version_label(&self) -> String {
        let tag = self.index_info.tag.as_deref().unwrap_or("?");
        let count = self.index_info.plugins_count.unwrap_or_else(|| {
            self.items
                .iter()
                .filter(|item| matches!(item.item_kind, crate::plugins::StoreItemKind::Plugin))
                .count()
        });
        let app_count = self.index_info.applications_count.unwrap_or_else(|| {
            self.items
                .iter()
                .filter(|item| matches!(item.item_kind, crate::plugins::StoreItemKind::Application))
                .count()
        });
        let mut parts = vec![
            format!("Index {tag}"),
            format!("plugins {count}"),
            format!("apps {app_count}"),
        ];
        if let Some(generated_at) = self.index_info.generated_at.as_deref() {
            parts.push(format!(
                "generated {}",
                format_store_generated_at(generated_at)
            ));
        }
        if let Some(source_repo) = self.index_info.source_repo.as_deref() {
            parts.push(format!("source {source_repo}"));
        }
        parts.join("  ")
    }

    pub fn install_dir_name_for(&self, item: &crate::plugins::StorePluginInfo) -> String {
        crate::plugins::store_plugin_install_dir_name(&item.id)
    }

    pub fn installed_version_for(&self, item: &crate::plugins::StorePluginInfo) -> Option<&str> {
        if matches!(item.item_kind, crate::plugins::StoreItemKind::Application) {
            return self.installed_app_versions.get(&item.id).map(|s| s.as_str());
        }
        let dir = self.install_dir_name_for(item);
        self.installed_versions.get(&dir).map(|s| s.as_str())
    }

    pub fn is_installed(&self, item: &crate::plugins::StorePluginInfo) -> bool {
        self.installed_version_for(item).is_some()
    }

    pub fn has_update(&self, item: &crate::plugins::StorePluginInfo) -> bool {
        self.installed_version_for(item)
            .map(|v| v != item.version)
            .unwrap_or(false)
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
                "{} {} {} {} {} {} {}",
                item.id,
                item.name,
                item.plugin_type,
                item.version,
                item.description,
                match item.item_kind {
                    crate::plugins::StoreItemKind::Plugin => "plugin",
                    crate::plugins::StoreItemKind::Application => "application app",
                },
                item.install_method.as_deref().unwrap_or_default(),
            );
            let lowered = searchable.to_lowercase();
            if !rest.iter().all(|token| lowered.contains(token.as_str())) {
                continue;
            }
            if item.id.to_lowercase().starts_with(first.as_str())
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
        self.clamp_match();
    }

    pub fn move_next(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.match_pos = 0;
        } else {
            self.match_pos = (self.match_pos + 1) % len;
        }
        self.clamp_match();
    }

    pub(crate) fn clamp_match(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.match_pos = 0;
        } else {
            self.match_pos = self.match_pos.min(len.saturating_sub(1));
        }
    }
}

fn store_item_has_update(
    item: &crate::plugins::StorePluginInfo,
    installed_versions: &HashMap<String, String>,
    installed_app_versions: &HashMap<String, String>,
) -> bool {
    if matches!(item.item_kind, crate::plugins::StoreItemKind::Application) {
        return installed_app_versions
            .get(&item.id)
            .map(|installed| installed != &item.version)
            .unwrap_or(false);
    }
    let dir = crate::plugins::store_plugin_install_dir_name(&item.id);
    installed_versions
        .get(&dir)
        .map(|installed| installed != &item.version)
        .unwrap_or(false)
}

fn format_store_generated_at(value: &str) -> String {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S %Z")
                .to_string()
        })
        .unwrap_or_else(|_| value.to_string())
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

pub type MenuEntry = MenuAction;

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
        MenuAction::ViewFile,
        MenuAction::EditFile,
        MenuAction::Separator,
        MenuAction::CopyFile,
        MenuAction::MoveFile,
        MenuAction::MkDir,
        MenuAction::RenameFile,
        MenuAction::DeleteFile,
        MenuAction::Separator,
        MenuAction::Quit,
    ],
    &[
        MenuAction::SwapPanels,
        MenuAction::Separator,
        MenuAction::SortName,
        MenuAction::SortExtension,
        MenuAction::SortDate,
        MenuAction::SortSize,
        MenuAction::SortUnsorted,
        MenuAction::Separator,
        MenuAction::ToggleHidden,
        MenuAction::Reload,
    ],
    &[MenuAction::GoToPath],
    &[
        MenuAction::SelectPattern,
        MenuAction::DeselectPattern,
        MenuAction::InvertSelection,
    ],
    &[
        MenuAction::SearchFiles,
        MenuAction::TreeView,
        MenuAction::InstallPluginFromStore,
        MenuAction::RemoteConnect,
        MenuAction::FileIdPreview,
        MenuAction::DirBookmarks,
    ],
    &[
        MenuAction::Setup,
        MenuAction::Shortcuts,
        MenuAction::Plugins,
        MenuAction::Associations,
        MenuAction::ToggleFBar,
        MenuAction::SaveConfig,
    ],
    &[MenuAction::Help, MenuAction::About],
];
