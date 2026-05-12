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
    Setup,
    Plugins,
    Associations,
    Help,
    About,
    SystemInfo,
    NewTab,
    CloseTab,
    NextTab,
    OpenTerminal,
    CaptureGif,
    MatrixScreensaver,
    OpenInOs,
    OpenFolderInOs,
    QuickPreview,
    DebugLog,
    DownloadCloudFile,
    CreateSelectionM3u,
    EnterArchivePlugin,
    ComparePanelFiles,
    ComparePanelInternal,
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
pub struct StoreInstallMethodsState {
    pub methods: Vec<crate::plugins::StoreInstallMethodCapability>,
}

#[derive(Debug, Clone)]
pub struct StoreInstallPaletteState {
    pub index_path: PathBuf,
    pub index_info: crate::plugins::StoreIndexInfo,
    pub items: Vec<crate::plugins::StorePluginInfo>,
    pub plugins_dir: PathBuf,
    pub installed_versions: HashMap<String, String>,
    pub installed_app_versions: HashMap<String, String>,
    pub installed_only: bool,
    pub query: String,
    pub match_pos: usize,
    pub scroll_offset: std::cell::Cell<usize>,
    pub progress: Option<StoreInstallProgress>,
    pub detect: Option<StoreDetectState>,
    pub methods: Option<StoreInstallMethodsState>,
}

impl StoreInstallPaletteState {
    pub fn load(index_path: PathBuf) -> anyhow::Result<Self> {
        let installed_versions = crate::plugins::installed_plugin_versions_by_dir();
        let plugins_dir = crate::plugins::plugins_dir().unwrap_or_default();
        let installed_app_versions = crate::plugins::load_store_applications()
            .unwrap_or_default()
            .into_iter()
            .map(|app| (app.id, app.version))
            .collect::<HashMap<_, _>>();
        let (mut items, index_info) = crate::plugins::list_store_plugins_with_info(&index_path)?;

        let mut known_plugin_dirs = items
            .iter()
            .filter(|item| matches!(item.item_kind, crate::plugins::StoreItemKind::Plugin))
            .map(|item| crate::plugins::store_plugin_install_dir_name(&item.id))
            .collect::<std::collections::HashSet<_>>();

        for plugin in crate::plugins::plugin_infos() {
            let Some(dir_name) = plugin
                .dir
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
            else {
                continue;
            };
            if !known_plugin_dirs.insert(dir_name.clone()) {
                continue;
            }

            let source_label = if plugins_dir.as_os_str().is_empty() {
                "external".to_string()
            } else {
                crate::plugins::plugin_source_label(&plugin.dir, &plugins_dir).to_string()
            };
            items.push(crate::plugins::StorePluginInfo {
                id: dir_name,
                name: plugin.name,
                version: plugin.version,
                plugin_type: plugin.kind,
                description: plugin.description,
                item_kind: crate::plugins::StoreItemKind::Plugin,
                source_label,
                from_store: false,
                local_dir: Some(plugin.dir),
                install_method: None,
                install_bin: None,
                uninstall_method: None,
                uninstall_package: None,
                install_methods: Vec::new(),
                mime_types: Vec::new(),
                wait_for_key_after_exit: false,
                launch_args: None,
            });
        }

        items.sort_by(|a, b| {
            item_kind_rank(a)
                .cmp(&item_kind_rank(b))
                .then_with(|| {
                    a.plugin_type
                        .to_lowercase()
                        .cmp(&b.plugin_type.to_lowercase())
                })
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.id.cmp(&b.id))
        });

        Ok(Self {
            items,
            index_path,
            index_info,
            plugins_dir,
            installed_versions,
            installed_app_versions,
            installed_only: false,
            query: String::new(),
            match_pos: 0,
            scroll_offset: std::cell::Cell::new(0),
            progress: None,
            detect: None,
            methods: None,
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
        if let Some(dir_name) = item
            .local_dir
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(|s| s.to_string())
        {
            return dir_name;
        }
        crate::plugins::store_plugin_install_dir_name(&item.id)
    }

    pub fn plugin_install_dir_for(
        &self,
        item: &crate::plugins::StorePluginInfo,
    ) -> Option<PathBuf> {
        if !matches!(item.item_kind, crate::plugins::StoreItemKind::Plugin) {
            return None;
        }
        if let Some(dir) = item.local_dir.clone() {
            return Some(dir);
        }
        Some(self.plugins_dir.join(self.install_dir_name_for(item)))
    }

    pub fn can_install(&self, item: &crate::plugins::StorePluginInfo) -> bool {
        item.from_store
    }

    pub fn installed_version_for(&self, item: &crate::plugins::StorePluginInfo) -> Option<&str> {
        if matches!(item.item_kind, crate::plugins::StoreItemKind::Application) {
            return self
                .installed_app_versions
                .get(&item.id)
                .map(|s| s.as_str());
        }
        let dir = self.install_dir_name_for(item);
        self.installed_versions.get(&dir).map(|s| s.as_str())
    }

    pub fn is_installed(&self, item: &crate::plugins::StorePluginInfo) -> bool {
        self.installed_version_for(item).is_some()
    }

    pub fn has_update(&self, item: &crate::plugins::StorePluginInfo) -> bool {
        if matches!(item.item_kind, crate::plugins::StoreItemKind::Application)
            && item.version == "?"
        {
            return false;
        }
        self.installed_version_for(item)
            .map(|v| v != item.version)
            .unwrap_or(false)
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let matches_installed = |item: &crate::plugins::StorePluginInfo| {
            !self.installed_only || self.is_installed(item)
        };

        if self.query.trim().is_empty() {
            return self
                .items
                .iter()
                .enumerate()
                .filter_map(|(idx, item)| matches_installed(item).then_some(idx))
                .collect();
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
            if !matches_installed(item) {
                continue;
            }
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
                format!(
                    "{} {}",
                    item.install_method.as_deref().unwrap_or_default(),
                    item.source_label
                ),
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
        self.scroll_offset.set(0);
        self.clamp_match();
    }

    pub fn toggle_installed_only(&mut self) {
        self.installed_only = !self.installed_only;
        self.match_pos = 0;
        self.scroll_offset.set(0);
        self.clamp_match();
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.match_pos = 0;
        self.scroll_offset.set(0);
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

fn item_kind_rank(item: &crate::plugins::StorePluginInfo) -> u8 {
    match item.item_kind {
        crate::plugins::StoreItemKind::Plugin => 0,
        crate::plugins::StoreItemKind::Application => 1,
    }
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

    fn clamp_match(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.match_pos = 0;
        } else {
            self.match_pos = self.match_pos.min(len.saturating_sub(1));
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioPlayerPaletteState {
    pub items: Vec<crate::audio_plugins::AudioRustPluginInfo>,
    pub query: String,
    pub match_pos: usize,
}

impl AudioPlayerPaletteState {
    pub fn load(viewer: &Viewer) -> Self {
        let plugins_dir = crate::plugins::plugins_dir().unwrap_or_default();
        let items = crate::audio_plugins::discover_audio_rust_plugins(&plugins_dir)
            .unwrap_or_default();
        let mut state = Self {
            items,
            query: String::new(),
            match_pos: 0,
        };

        if let Some(plugin_id) = crate::tracker_audio::preferred_audio_plugin_ids_for_path(&viewer.path)
            .first()
            .cloned()
            && let Some(pos) = state.items.iter().position(|plugin| plugin.id == plugin_id)
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
                "{} {} {} {} {}",
                item.id,
                item.name,
                item.description,
                item.mime_types.join(" "),
                item.extensions.join(" ")
            );
            let lowered = searchable.to_lowercase();
            if !rest.iter().all(|token| lowered.contains(token.as_str())) {
                continue;
            }
            if lowered.starts_with(first.as_str()) || item.name.to_lowercase().starts_with(first.as_str()) {
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
    }

    pub fn move_next(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.match_pos = 0;
        } else {
            self.match_pos = (self.match_pos + 1) % len;
        }
    }

    pub fn selected_item(&self) -> Option<&crate::audio_plugins::AudioRustPluginInfo> {
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

impl ViewerMenuState {
    pub fn new(kind: ViewerMenuKind, viewer: &Viewer) -> Self {
        let cursor = match kind {
            ViewerMenuKind::Mode => {
                if viewer.viewer_plugin.is_some() {
                    5
                } else {
                    match viewer.mode {
                        ViewMode::Text => 0,
                        ViewMode::Hex => 1,
                        ViewMode::Ansi => 2,
                        ViewMode::Image => 3,
                        ViewMode::Module => 4,
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
                    15 // "Syntax OFF" is the last item
                } else {
                    match viewer.mask {
                        MaskKind::Auto => 0,
                        MaskKind::Markdown => 1,
                        MaskKind::C => 2,
                        MaskKind::Rust => 3,
                        MaskKind::JavaScript => 4,
                        MaskKind::Python => 5,
                        MaskKind::Php => 6,
                        MaskKind::Html => 7,
                        MaskKind::Css => 8,
                        MaskKind::Toml => 9,
                        MaskKind::Sql => 10,
                        MaskKind::Shell => 11,
                        MaskKind::Pascal => 12,
                        MaskKind::Assembler => 13,
                        MaskKind::Ketchup => 14,
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
        MenuAction::Plugins,
        MenuAction::Associations,
        MenuAction::ToggleFBar,
        MenuAction::SaveConfig,
    ],
    &[MenuAction::Help, MenuAction::About],
];
