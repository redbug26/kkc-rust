use crate::screen_transition::ScreenTransitionEffect;
use anyhow::{Context, Result, anyhow};
use chrono::Local;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::ffi::CStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static RUNTIME_SESSION_ID: OnceLock<String> = OnceLock::new();
static RUNTIME_RESUME_SOURCE_SESSION_ID: OnceLock<String> = OnceLock::new();

#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionTabMap {
    #[serde(default)]
    tabs: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PaletteRecentEntry {
    Name(String),
    Index(usize),
}

fn deserialize_palette_recent<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let entries = Vec::<PaletteRecentEntry>::deserialize(deserializer)?;
    Ok(entries
        .into_iter()
        .map(|entry| match entry {
            PaletteRecentEntry::Name(name) => name,
            PaletteRecentEntry::Index(i) => i.to_string(),
        })
        .collect())
}

/// Returns the ProjectDirs handle for KKC.
pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("be", "kyuran", "kkc").context("Could not determine project directories")
}

/// Returns the path to the config file, creating parent dirs if needed.
pub fn config_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.preference_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("config.toml"))
}

/// Returns the path to the persisted runtime state file, creating parent dirs if needed.
pub fn state_path() -> Result<PathBuf> {
    let session_id = init_runtime_session(None)?;
    session_state_path(&session_id)
}

/// Returns the path to the data directory, creating it if needed.
#[allow(dead_code)]
pub fn data_dir() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.data_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.to_path_buf())
}

/// Returns the path to the terminal cache file, creating parent dirs if needed.
pub fn terminal_cache_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.cache_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("terminal.toml"))
}

// ---------------------------------------------------------------------------
// Sort modes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortMode {
    #[default]
    Name,
    Extension,
    Date,
    Size,
    Unsorted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PanelViewType {
    #[default]
    Normal,
    FilePreviewInfo,
    QuickPreview,
    TextEditorPanel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActivePanelSide {
    #[default]
    Left,
    Right,
}

// ---------------------------------------------------------------------------
// Panel config (saved per side)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelTabConfig {
    /// Last visited local fallback path for this tab.
    #[serde(default = "dirs_home")]
    pub path: PathBuf,
    /// Remote profile name if this tab was on a remote location.
    #[serde(default)]
    pub remote_name: Option<String>,
    /// Remote current directory for persisted remote tabs.
    #[serde(default)]
    pub remote_path: Option<String>,
    /// How files are sorted.
    #[serde(default)]
    pub sort: SortMode,
    /// Show hidden files.
    #[serde(default)]
    pub show_hidden: bool,
    /// Last highlighted entry name in this tab.
    #[serde(default)]
    pub cursor_name: Option<String>,
    /// Selected entry names in this tab.
    #[serde(default)]
    pub selected_names: Vec<String>,
}

impl Default for PanelTabConfig {
    fn default() -> Self {
        Self {
            path: dirs_home(),
            remote_name: None,
            remote_path: None,
            sort: SortMode::Name,
            show_hidden: false,
            cursor_name: None,
            selected_names: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelConfig {
    /// Last visited path for this panel.
    #[serde(default = "dirs_home")]
    pub path: PathBuf,
    /// Remote profile name if this panel was on a remote location.
    #[serde(default)]
    pub remote_name: Option<String>,
    /// Remote current directory for persisted remote panels.
    #[serde(default)]
    pub remote_path: Option<String>,
    /// How files are sorted.
    #[serde(default)]
    pub sort: SortMode,
    /// Show hidden files.
    #[serde(default)]
    pub show_hidden: bool,
    /// Persisted tabs for this panel, including the active tab.
    #[serde(default)]
    pub tabs: Vec<PanelTabConfig>,
    /// Index of the active tab inside `tabs`.
    #[serde(default)]
    pub active_tab: usize,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            path: dirs_home(),
            remote_name: None,
            remote_path: None,
            sort: SortMode::Name,
            show_hidden: false,
            tabs: Vec::new(),
            active_tab: 0,
        }
    }
}

impl PanelConfig {
    pub fn active_tab_config(&self) -> PanelTabConfig {
        PanelTabConfig {
            path: self.path.clone(),
            remote_name: self.remote_name.clone(),
            remote_path: self.remote_path.clone(),
            sort: self.sort,
            show_hidden: self.show_hidden,
            cursor_name: None,
            selected_names: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedState {
    #[serde(default)]
    left: PanelConfig,
    #[serde(default)]
    right: PanelConfig,
    #[serde(default, deserialize_with = "deserialize_palette_recent")]
    palette_recent: Vec<String>,
    #[serde(default)]
    panel_view_type: PanelViewType,
    #[serde(default)]
    active_panel: ActivePanelSide,
    #[serde(default)]
    panel_text_editor_path: Option<PathBuf>,
    #[serde(default)]
    panel_text_editor_side: ActivePanelSide,
}

impl Default for PersistedState {
    fn default() -> Self {
        let defaults = Config::default();
        Self {
            left: defaults.left,
            right: defaults.right,
            palette_recent: defaults.palette_recent,
            panel_view_type: defaults.panel_view_type,
            active_panel: defaults.active_panel,
            panel_text_editor_path: defaults.panel_text_editor_path,
            panel_text_editor_side: defaults.panel_text_editor_side,
        }
    }
}

// ---------------------------------------------------------------------------
// Viewer config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewerConfig {
    /// Wrap long lines in text mode.
    #[serde(default = "t")]
    pub word_wrap: bool,
    /// Tab width (in spaces).
    #[serde(default = "tab_default")]
    pub tab_width: usize,
    /// Open the viewer in zoomed (full-screen) mode by default.
    #[serde(default = "t")]
    pub default_zoom: bool,
    /// Delay before autoplay advances to next non-audio file.
    #[serde(default = "viewer_autoplay_delay_secs_default")]
    pub autoplay_delay_secs: u64,
}

impl Default for ViewerConfig {
    fn default() -> Self {
        Self {
            word_wrap: true,
            tab_width: 4,
            default_zoom: true,
            autoplay_delay_secs: viewer_autoplay_delay_secs_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// File-type associations
// ---------------------------------------------------------------------------

/// Maps a MIME type to one or more opener commands.
/// `mime_type` is stored lowercase (e.g. "audio/mpeg").
/// Commands may contain `%f` as a placeholder for the file path;
/// if absent the path is appended as the last argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAssoc {
    #[serde(alias = "ext")]
    pub mime_type: String,
    #[serde(default)]
    pub openers: Vec<String>,
}

/// Maps a MIME type to the preferred native audio plugin id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioPlayerAssoc {
    #[serde(alias = "ext")]
    pub mime_type: String,
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutOverride {
    pub fn_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionConfig {
    /// Enable screen transitions for the screensaver and application exit.
    #[serde(default = "t")]
    pub enabled: bool,
    /// Number of frames used by screen transitions.
    #[serde(default = "transition_frames_default")]
    pub frames: u16,
    /// Transition effect used when entering/leaving the screensaver.
    #[serde(default = "transition_effect_default")]
    pub screensaver_effect: String,
    /// Transition effect used when quitting the application.
    #[serde(default = "transition_effect_default")]
    pub quit_effect: String,
}

impl Default for TransitionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            frames: transition_frames_default(),
            screensaver_effect: transition_effect_default(),
            quit_effect: transition_quit_effect_default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Main config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // --- Panels ---
    /// Left panel settings.
    #[serde(default)]
    pub left: PanelConfig,
    /// Right panel settings.
    #[serde(default)]
    pub right: PanelConfig,

    // --- UI ---
    /// Show the function-key bar at the bottom.
    #[serde(default = "t")]
    pub show_fkey_bar: bool,
    /// Confirm before quit.
    #[serde(default = "t")]
    pub confirm_exit: bool,
    /// Confirm before delete.
    #[serde(default = "t")]
    pub confirm_delete: bool,
    /// Auto-reload both panels after a file operation.
    #[serde(default = "t")]
    pub auto_reload: bool,
    /// Cursor moves down after Insert-select.
    #[serde(default = "t")]
    pub insert_moves_down: bool,
    /// '+' key also selects directories.
    #[serde(default)]
    pub select_dirs: bool,
    /// Idle timeout in minutes before launching the Matrix screensaver.
    /// Set to 0 to disable auto screensaver.
    #[serde(default = "screensaver_idle_minutes_default")]
    pub screensaver_idle_minutes: u64,
    #[serde(default)]
    pub transition: TransitionConfig,
    /// Color-code files by type category.
    #[serde(default = "t")]
    pub color_by_type: bool,
    /// Show a cloud icon for files that are only available online.
    #[serde(default = "t")]
    pub show_cloud_icons: bool,
    /// Show file-type icons in panel listings.
    #[serde(default = "t")]
    pub show_file_icons: bool,

    // --- External programs ---
    /// External editor command (defaults to internal editor).
    #[serde(default = "default_editor")]
    pub editor: String,
    /// External pager/viewer command.
    #[serde(default = "default_pager")]
    pub pager: String,
    /// Plugin store index source (URL or local file path).
    #[serde(default = "default_store_index_path")]
    pub store_index_path: String,

    // --- Viewer ---
    #[serde(default)]
    pub viewer: ViewerConfig,

    // --- History ---
    /// Maximum number of directory history entries.
    #[serde(default = "history_max")]
    pub dir_history_max: usize,

    // --- Bookmarks ---
    /// User-defined directory bookmarks.
    #[serde(default = "default_bookmarks")]
    pub bookmarks: Vec<PathBuf>,

    /// File-type associations (MIME type → opener commands).
    #[serde(default)]
    pub file_assoc: Vec<FileAssoc>,

    /// Preferred audio plugin (MIME type → native audio plugin id).
    #[serde(default)]
    pub audio_player_assoc: Vec<AudioPlayerAssoc>,

    // --- Command palette ---
    /// Recently-used command palette entries (fn_name values), most-recent first.
    #[serde(default, deserialize_with = "deserialize_palette_recent")]
    pub palette_recent: Vec<String>,

    /// User shortcut overrides. Only entries that differ from palette defaults
    /// are written, including `shortcut = None` for removed default shortcuts.
    #[serde(default)]
    pub shortcut_overrides: Vec<ShortcutOverride>,

    // --- Debug ---
    /// Write debug messages to a log file (disabled by default).
    #[serde(default)]
    pub debug_log: bool,

    /// Last panel rendering mode in browse view.
    #[serde(default)]
    pub panel_view_type: PanelViewType,

    /// Active colour theme name.
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Active panel when the app was closed.
    #[serde(default)]
    pub active_panel: ActivePanelSide,

    /// Path of the file open in the side panel text editor when the app was closed.
    #[serde(default)]
    pub panel_text_editor_path: Option<PathBuf>,

    /// Which panel side the text editor was anchored to.
    #[serde(default)]
    pub panel_text_editor_side: ActivePanelSide,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            left: PanelConfig::default(),
            right: PanelConfig {
                path: dirs_home(),
                remote_name: None,
                remote_path: None,
                sort: SortMode::Name,
                show_hidden: false,
                tabs: Vec::new(),
                active_tab: 0,
            },
            show_fkey_bar: true,
            confirm_exit: true,
            confirm_delete: true,
            auto_reload: true,
            insert_moves_down: true,
            select_dirs: false,
            screensaver_idle_minutes: screensaver_idle_minutes_default(),
            transition: TransitionConfig::default(),
            color_by_type: true,
            show_cloud_icons: true,
            show_file_icons: false,
            editor: "".to_string(),
            pager: default_pager(),
            store_index_path: default_store_index_path(),
            viewer: ViewerConfig::default(),
            dir_history_max: 32,
            bookmarks: default_bookmarks(),
            file_assoc: Vec::new(),
            audio_player_assoc: Vec::new(),
            palette_recent: Vec::new(),
            shortcut_overrides: Vec::new(),
            debug_log: false,
            panel_view_type: PanelViewType::Normal,
            theme: default_theme(),
            active_panel: ActivePanelSide::Left,
            panel_text_editor_path: None,
            panel_text_editor_side: ActivePanelSide::Left,
        }
    }
}

impl Config {
    /// Load config from disk, or return defaults if file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if path.exists() {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("Reading config: {}", path.display()))?;
            let mut cfg: Self = match toml::from_str(&text) {
                Ok(cfg) => cfg,
                Err(parse_err) => {
                    let backup = backup_invalid_config(&path).with_context(|| {
                        format!("Parsing config and backing it up: {}", path.display())
                    })?;
                    return Err(anyhow::anyhow!(
                        "Parsing config: {} ({parse_err}). Invalid file moved to {}",
                        path.display(),
                        backup.display()
                    ));
                }
            };

            // Migration path for configs written during the sectioned-save
            // refactor where root keys were accidentally emitted under [viewer].
            if let Ok(raw) = toml::from_str::<toml::Value>(&text) {
                if raw.get("transition").and_then(|v| v.as_table()).is_none() {}

                if let Some(viewer) = raw.get("viewer").and_then(|v| v.as_table()) {
                    if cfg.bookmarks == default_bookmarks() {
                        if let Some(arr) = viewer.get("bookmarks").and_then(|v| v.as_array()) {
                            let restored: Vec<PathBuf> = arr
                                .iter()
                                .filter_map(|v| v.as_str().map(PathBuf::from))
                                .collect();
                            if !restored.is_empty() {
                                cfg.bookmarks = restored;
                            }
                        }
                    }

                    if cfg.palette_recent.is_empty() {
                        if let Some(arr) = viewer.get("palette_recent").and_then(|v| v.as_array()) {
                            let restored: Vec<String> = arr
                                .iter()
                                .filter_map(|v| {
                                    if let Some(s) = v.as_str() {
                                        Some(s.to_string())
                                    } else {
                                        v.as_integer().filter(|&n| n >= 0).map(|n| n.to_string())
                                    }
                                })
                                .collect();
                            if !restored.is_empty() {
                                cfg.palette_recent = restored;
                            }
                        }
                    }

                    if cfg.editor == default_editor() {
                        if let Some(v) = viewer.get("editor").and_then(|v| v.as_str()) {
                            cfg.editor = v.to_string();
                        }
                    }
                    if cfg.pager == default_pager() {
                        if let Some(v) = viewer.get("pager").and_then(|v| v.as_str()) {
                            cfg.pager = v.to_string();
                        }
                    }
                    if cfg.dir_history_max == history_max() {
                        if let Some(v) = viewer.get("dir_history_max").and_then(|v| v.as_integer())
                        {
                            if v >= 1 {
                                cfg.dir_history_max = v as usize;
                            }
                        }
                    }
                    if let Some(v) = viewer.get("debug_log").and_then(|v| v.as_bool()) {
                        cfg.debug_log = v;
                    }
                }
            }

            // Runtime state now lives in state.toml. If it exists, it wins.
            // If it does not exist, values parsed from config.toml are kept
            // (backward compatibility with older single-file configs).
            if let Some(state) = load_state_file()? {
                apply_state_to_config(&mut cfg, state);
            }

            Ok(cfg)
        } else {
            Ok(Self::default())
        }
    }

    /// Persist preferences to config.toml.
    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        let out = self.to_toml_string()?;

        fs::write(&path, out).with_context(|| format!("Writing config: {}", path.display()))?;

        Ok(())
    }

    /// Persist runtime state to state.toml.
    pub fn save_state(&self) -> Result<()> {
        let state = state_from_config(self);
        write_state_file(&state)?;
        Ok(())
    }

    fn to_toml_string(&self) -> Result<String> {
        let mut out = String::new();

        // ─── Behaviour ────────────────────────────────────────────────────
        out.push_str("# \u{2500}\u{2500}\u{2500} Behaviour \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");
        out.push_str(&format!("confirm_exit = {}\n", self.confirm_exit));
        out.push_str(&format!("confirm_delete = {}\n", self.confirm_delete));
        out.push_str(&format!("auto_reload = {}\n", self.auto_reload));
        out.push_str(&format!("insert_moves_down = {}\n", self.insert_moves_down));
        out.push_str(&format!("select_dirs = {}\n", self.select_dirs));
        out.push_str(&format!(
            "screensaver_idle_minutes = {}\n",
            self.screensaver_idle_minutes
        ));
        out.push('\n');

        // ─── Display ──────────────────────────────────────────────────────
        out.push_str("# \u{2500}\u{2500}\u{2500} Display \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");
        out.push_str(&format!("show_fkey_bar = {}\n", self.show_fkey_bar));
        out.push_str(&format!("color_by_type = {}\n", self.color_by_type));
        out.push_str(&format!("show_cloud_icons = {}\n", self.show_cloud_icons));
        out.push_str(&format!("show_file_icons = {}\n", self.show_file_icons));
        out.push_str(&format!(
            "theme = {}\n",
            toml::Value::String(self.theme.clone())
        ));
        out.push('\n');

        // ─── Viewer ───────────────────────────────────────────────────────
        out.push_str("# \u{2500}\u{2500}\u{2500} Viewer \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");
        out.push_str(&format!("viewer.word_wrap = {}\n", self.viewer.word_wrap));
        out.push_str(&format!("viewer.tab_width = {}\n", self.viewer.tab_width));
        out.push_str(&format!(
            "viewer.default_zoom = {}\n",
            self.viewer.default_zoom
        ));
        out.push_str(&format!(
            "viewer.autoplay_delay_secs = {}\n",
            self.viewer.autoplay_delay_secs
        ));
        out.push('\n');

        // ─── External ─────────────────────────────────────────────────────
        out.push_str("# \u{2500}\u{2500}\u{2500} External \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");
        out.push_str(&format!(
            "editor = {}\n",
            toml::Value::String(self.editor.clone())
        ));
        out.push_str(&format!(
            "pager = {}\n",
            toml::Value::String(self.pager.clone())
        ));
        out.push_str(&format!(
            "store_index_path = {}\n",
            toml::Value::String(self.store_index_path.clone())
        ));
        out.push_str(&format!("dir_history_max = {}\n", self.dir_history_max));
        out.push('\n');

        // ─── Debug ────────────────────────────────────────────────────────
        out.push_str("# \u{2500}\u{2500}\u{2500} Debug \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");
        out.push_str(&format!("debug_log = {}\n", self.debug_log));
        out.push('\n');

        // Preferences tail (runtime state is written to state.toml)
        #[derive(serde::Serialize)]
        struct ConfigTail<'a> {
            bookmarks: &'a Vec<PathBuf>,
            file_assoc: &'a Vec<FileAssoc>,
            audio_player_assoc: &'a Vec<AudioPlayerAssoc>,
            shortcut_overrides: &'a Vec<ShortcutOverride>,
        }
        let tail = toml::to_string_pretty(&ConfigTail {
            bookmarks: &self.bookmarks,
            file_assoc: &self.file_assoc,
            audio_player_assoc: &self.audio_player_assoc,
            shortcut_overrides: &self.shortcut_overrides,
        })
        .context("Serialising panels config")?;
        out.push_str(&tail);

        // ─── Transition ───────────────────────────────────────────────────
        out.push_str("\n# \u{2500}\u{2500}\u{2500} Transition \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");
        out.push_str("[transition]\n");
        out.push_str(&format!("enabled = {}\n", self.transition.enabled));
        out.push_str(&format!("frames = {}\n", self.transition.frames));
        out.push_str(&format!(
            "screensaver_effect = \"{}\"\n",
            self.transition.screensaver_effect
        ));
        out.push_str(&format!(
            "quit_effect = \"{}\"\n",
            self.transition.quit_effect
        ));

        Ok(out)
    }

    /// Return the registered openers for the given MIME type.
    /// Comparison is case-insensitive.
    pub fn openers_for_mime(&self, mime_type: &str) -> &[String] {
        self.file_assoc
            .iter()
            .find(|a| a.mime_type.eq_ignore_ascii_case(mime_type))
            .map(|a| a.openers.as_slice())
            .unwrap_or(&[])
    }

    pub fn add_opener_for_mime(&mut self, mime_type: &str, opener: String) {
        let mime_type = mime_type.trim().to_ascii_lowercase();
        if mime_type.is_empty() || opener.trim().is_empty() {
            return;
        }

        if let Some(existing) = self
            .file_assoc
            .iter_mut()
            .find(|a| a.mime_type.eq_ignore_ascii_case(&mime_type))
        {
            if !existing.openers.iter().any(|cmd| cmd == &opener) {
                existing.openers.push(opener);
            }
        } else {
            self.file_assoc.push(FileAssoc {
                mime_type,
                openers: vec![opener],
            });
        }
    }

    pub fn remove_opener_for_mime(&mut self, mime_type: &str, opener: &str) -> bool {
        let mime_type = mime_type.trim().to_ascii_lowercase();
        if mime_type.is_empty() || opener.trim().is_empty() {
            return false;
        }

        let mut changed = false;
        for assoc in &mut self.file_assoc {
            if assoc.mime_type.eq_ignore_ascii_case(&mime_type) {
                let before = assoc.openers.len();
                assoc.openers.retain(|cmd| cmd != opener);
                changed |= assoc.openers.len() != before;
            }
        }
        let before = self.file_assoc.len();
        self.file_assoc.retain(|assoc| !assoc.openers.is_empty());
        changed || self.file_assoc.len() != before
    }

    /// Return the preferred native audio plugin id for the given MIME type.
    pub fn audio_player_for_mime(&self, mime_type: &str) -> Option<&str> {
        self.audio_player_assoc
            .iter()
            .find(|assoc| assoc.mime_type.eq_ignore_ascii_case(mime_type))
            .map(|assoc| assoc.plugin_id.as_str())
    }

    pub fn set_audio_player_for_mime(&mut self, mime_type: &str, plugin_id: String) {
        let mime_type = mime_type.trim().to_ascii_lowercase();
        let plugin_id = plugin_id.trim().to_string();
        if mime_type.is_empty() || plugin_id.is_empty() {
            return;
        }

        if let Some(existing) = self
            .audio_player_assoc
            .iter_mut()
            .find(|assoc| assoc.mime_type.eq_ignore_ascii_case(&mime_type))
        {
            existing.plugin_id = plugin_id;
        } else {
            self.audio_player_assoc.push(AudioPlayerAssoc {
                mime_type,
                plugin_id,
            });
        }
    }
}

fn state_from_config(cfg: &Config) -> PersistedState {
    PersistedState {
        left: cfg.left.clone(),
        right: cfg.right.clone(),
        palette_recent: cfg.palette_recent.clone(),
        panel_view_type: cfg.panel_view_type,
        active_panel: cfg.active_panel,
        panel_text_editor_path: cfg.panel_text_editor_path.clone(),
        panel_text_editor_side: cfg.panel_text_editor_side,
    }
}

fn apply_state_to_config(cfg: &mut Config, state: PersistedState) {
    cfg.left = state.left;
    cfg.right = state.right;
    cfg.palette_recent = state.palette_recent;
    cfg.panel_view_type = state.panel_view_type;
    cfg.active_panel = state.active_panel;
    cfg.panel_text_editor_path = state.panel_text_editor_path;
    cfg.panel_text_editor_side = state.panel_text_editor_side;
}

fn load_state_file() -> Result<Option<PersistedState>> {
    let path = state_path()?;
    if path.exists() {
        return read_state_file(&path).map(Some);
    }

    // When starting with `kkc resume <id>`, load from that source session,
    // but persist future state updates under the newly allocated session id.
    if let Some(source_id) = RUNTIME_RESUME_SOURCE_SESSION_ID.get() {
        let source_path = session_state_path(source_id)?;
        if source_path.exists() {
            return read_state_file(&source_path).map(Some);
        }
    }

    // Backward compatibility: older versions stored state.toml in preference_dir.
    let legacy_path = legacy_state_path()?;
    if legacy_path.exists() {
        return read_state_file(&legacy_path).map(Some);
    }

    Ok(None)
}

fn read_state_file(path: &Path) -> Result<PersistedState> {
    let text =
        fs::read_to_string(&path).with_context(|| format!("Reading state: {}", path.display()))?;
    let state: PersistedState = match toml::from_str(&text) {
        Ok(state) => state,
        Err(parse_err) => {
            let backup = backup_invalid_config(&path)
                .with_context(|| format!("Parsing state and backing it up: {}", path.display()))?;
            return Err(anyhow::anyhow!(
                "Parsing state: {} ({parse_err}). Invalid file moved to {}",
                path.display(),
                backup.display()
            ));
        }
    };
    Ok(state)
}

fn write_state_file(state: &PersistedState) -> Result<()> {
    let path = state_path()?;
    let state_toml = toml::to_string_pretty(state).context("Serialising state config")?;
    fs::write(&path, state_toml).with_context(|| format!("Writing state: {}", path.display()))?;
    Ok(())
}

pub fn init_runtime_session(resume_id: Option<&str>) -> Result<String> {
    if let Some(id) = RUNTIME_SESSION_ID.get() {
        return Ok(id.clone());
    }

    let tab_key = session_tab_key();
    let mut map = load_session_tab_map()?;

    let resolved_id = if let Some(id) = resume_id {
        let _ = RUNTIME_RESUME_SOURCE_SESSION_ID.set(id.to_string());
        uuid::Uuid::now_v7().to_string()
    } else if let Some(existing) = map.tabs.get(&tab_key).cloned() {
        if session_state_path(&existing)?.exists() {
            existing
        } else {
            uuid::Uuid::now_v7().to_string()
        }
    } else {
        uuid::Uuid::now_v7().to_string()
    };

    map.tabs.insert(tab_key, resolved_id.clone());
    save_session_tab_map(&map)?;

    RUNTIME_SESSION_ID
        .set(resolved_id.clone())
        .map_err(|_| anyhow!("Runtime session id already initialised"))?;

    Ok(resolved_id)
}

fn session_state_path(session_id: &str) -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.cache_dir().join("sessions");
    fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{session_id}.toml")))
}

fn session_map_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.cache_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("session-tabs.toml"))
}

fn legacy_state_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.preference_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("state.toml"))
}

fn load_session_tab_map() -> Result<SessionTabMap> {
    let path = session_map_path()?;
    if !path.exists() {
        return Ok(SessionTabMap::default());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("Reading session tab map: {}", path.display()))?;
    Ok(toml::from_str(&text).unwrap_or_default())
}

fn save_session_tab_map(map: &SessionTabMap) -> Result<()> {
    let path = session_map_path()?;
    let text = toml::to_string_pretty(map).context("Serialising session tab map")?;
    fs::write(&path, text).with_context(|| format!("Writing session tab map: {}", path.display()))
}

fn session_tab_key() -> String {
    if let Ok(ssh_tty) = env::var("SSH_TTY")
        && !ssh_tty.trim().is_empty()
    {
        return format!("ssh:{ssh_tty}");
    }
    if let Some(tty) = current_tty() {
        return format!("tty:{tty}");
    }
    if let Ok(term_session_id) = env::var("TERM_SESSION_ID")
        && !term_session_id.trim().is_empty()
    {
        return format!("term:{term_session_id}");
    }
    format!("pid:{}", std::process::id())
}

fn current_tty() -> Option<String> {
    let mut buf = [0 as libc::c_char; 512];
    let rc = unsafe { libc::ttyname_r(libc::STDIN_FILENO, buf.as_mut_ptr(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let cstr = unsafe { CStr::from_ptr(buf.as_ptr()) };
    cstr.to_str().ok().map(|s| s.to_string())
}

fn backup_invalid_config(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.toml");
    let ts = Local::now().format("%Y%m%d-%H%M%S").to_string();

    let mut candidate = parent.join(format!("{file_name}.{ts}"));
    let mut idx = 1usize;
    while candidate.exists() {
        candidate = parent.join(format!("{file_name}.{ts}.{idx}"));
        idx += 1;
    }

    fs::rename(path, &candidate).with_context(|| {
        format!(
            "Renaming invalid config {} -> {}",
            path.display(),
            candidate.display()
        )
    })?;
    Ok(candidate)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn dirs_home() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| Some(d.home_dir().to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn default_editor() -> String {
    "".to_string()
}

fn default_pager() -> String {
    std::env::var("PAGER").unwrap_or_else(|_| "less".into())
}

fn default_theme() -> String {
    "default".to_string()
}

pub fn default_store_index_path() -> String {
    "https://raw.githubusercontent.com/redbug26/kkc-store/main/dist/store-index.json".to_string()
}

const fn t() -> bool {
    true
}
const fn tab_default() -> usize {
    4
}
const fn history_max() -> usize {
    32
}

fn default_bookmarks() -> Vec<PathBuf> {
    vec![dirs_home()]
}

const fn screensaver_idle_minutes_default() -> u64 {
    15
}

const fn transition_frames_default() -> u16 {
    48
}

fn transition_effect_default() -> String {
    ScreenTransitionEffect::Plasma.as_config_name().to_string()
}

fn transition_quit_effect_default() -> String {
    ScreenTransitionEffect::Melt.as_config_name().to_string()
}

fn viewer_autoplay_delay_secs_default() -> u64 {
    15
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_config_toml_roundtrips_preferences() {
        let mut cfg = Config::default();
        cfg.confirm_exit = false;
        cfg.confirm_delete = false;
        cfg.auto_reload = false;
        cfg.insert_moves_down = false;
        cfg.select_dirs = true;
        cfg.screensaver_idle_minutes = 42;
        cfg.transition.enabled = false;
        cfg.transition.frames = 96;
        cfg.transition.screensaver_effect = "tunnel".into();
        cfg.transition.quit_effect = "melt".into();
        cfg.show_fkey_bar = false;
        cfg.color_by_type = false;
        cfg.show_cloud_icons = false;
        cfg.show_file_icons = false;
        cfg.viewer.word_wrap = false;
        cfg.viewer.tab_width = 8;
        cfg.viewer.default_zoom = false;
        cfg.viewer.autoplay_delay_secs = 9;
        cfg.editor = "vim".into();
        cfg.pager = "less -R".into();
        cfg.store_index_path = "/tmp/store-index.json".into();
        cfg.dir_history_max = 64;
        cfg.debug_log = true;
        cfg.bookmarks = vec![PathBuf::from("/Users/test")];
        cfg.file_assoc = vec![FileAssoc {
            mime_type: "text/plain".into(),
            openers: vec!["vim %f".into()],
        }];
        cfg.audio_player_assoc = vec![AudioPlayerAssoc {
            mime_type: "audio/x-ay".into(),
            plugin_id: "gme".into(),
        }];

        let text = cfg.to_toml_string().expect("serialize config");
        assert!(text.contains("[transition]\n"));
        assert!(text.contains("enabled = false\n"));
        assert!(text.contains("frames = 96\n"));
        assert!(text.contains("screensaver_effect = \"tunnel\"\n"));
        assert!(text.contains("quit_effect = \"melt\"\n"));

        let parsed: Config = toml::from_str(&text).expect("parse saved config");

        assert!(!parsed.confirm_exit);
        assert!(parsed.select_dirs);
        assert!(!parsed.show_cloud_icons);
        assert!(!parsed.show_file_icons);
        assert_eq!(parsed.screensaver_idle_minutes, 42);
        assert!(!parsed.transition.enabled);
        assert_eq!(parsed.transition.frames, 96);
        assert_eq!(parsed.transition.screensaver_effect, "tunnel");
        assert_eq!(parsed.transition.quit_effect, "melt");
        assert!(!parsed.viewer.word_wrap);
        assert_eq!(parsed.viewer.tab_width, 8);
        assert_eq!(parsed.viewer.autoplay_delay_secs, 9);
        assert_eq!(parsed.editor, "vim");
        assert_eq!(parsed.store_index_path, "/tmp/store-index.json");
        assert_eq!(parsed.dir_history_max, 64);
        assert_eq!(parsed.file_assoc[0].mime_type, "text/plain");
        assert_eq!(parsed.file_assoc[0].openers, vec!["vim %f"]);
        assert_eq!(parsed.audio_player_assoc[0].mime_type, "audio/x-ay");
        assert_eq!(parsed.audio_player_assoc[0].plugin_id, "gme");
    }

    #[test]
    fn state_toml_roundtrips_runtime_state() {
        let state: PersistedState = toml::from_str(
            r#"
    palette_recent = ["copy", "save_config"]
    panel_view_type = "quick_preview"
    active_panel = "right"

[left]
sort = "size"

[[left.tabs]]
sort = "date"
"#,
        )
        .expect("state should parse");

        assert_eq!(state.left.sort, SortMode::Size);
        assert_eq!(state.left.path, dirs_home());
        assert_eq!(state.left.tabs.len(), 1);
        assert_eq!(state.left.tabs[0].path, dirs_home());
        assert_eq!(state.left.tabs[0].sort, SortMode::Date);
        assert_eq!(state.palette_recent, vec!["copy", "save_config"]);
        assert_eq!(state.panel_view_type, PanelViewType::QuickPreview);
        assert_eq!(state.active_panel, ActivePanelSide::Right);
    }
}
