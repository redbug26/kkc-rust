use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Returns the ProjectDirs handle for KKC.
pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("be", "kyuran", "kkc")
        .context("Could not determine project directories")
}

/// Returns the path to the config file, creating parent dirs if needed.
pub fn config_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.config_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("config.toml"))
}

/// Returns the path to the data directory, creating it if needed.
#[allow(dead_code)]
pub fn data_dir() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.data_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.to_path_buf())
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

// ---------------------------------------------------------------------------
// Panel config (saved per side)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelConfig {
    /// Last visited path for this panel.
    pub path: PathBuf,
    /// How files are sorted.
    #[serde(default)]
    pub sort: SortMode,
    /// Show hidden files.
    #[serde(default)]
    pub show_hidden: bool,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            path: dirs_home(),
            sort: SortMode::Name,
            show_hidden: false,
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
}

impl Default for ViewerConfig {
    fn default() -> Self {
        Self { word_wrap: true, tab_width: 4 }
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
    /// Color-code files by type category.
    #[serde(default = "t")]
    pub color_by_type: bool,

    // --- External programs ---
    /// External editor command (defaults to $EDITOR or nano).
    #[serde(default = "default_editor")]
    pub editor: String,
    /// External pager/viewer command.
    #[serde(default = "default_pager")]
    pub pager: String,

    // --- Viewer ---
    #[serde(default)]
    pub viewer: ViewerConfig,

    // --- History ---
    /// Maximum number of directory history entries.
    #[serde(default = "history_max")]
    pub dir_history_max: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            left: PanelConfig::default(),
            right: PanelConfig { path: dirs_home(), sort: SortMode::Name, show_hidden: false },
            show_fkey_bar: true,
            confirm_exit: true,
            confirm_delete: true,
            auto_reload: true,
            insert_moves_down: true,
            select_dirs: false,
            color_by_type: true,
            editor: default_editor(),
            pager: default_pager(),
            viewer: ViewerConfig::default(),
            dir_history_max: 32,
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
            let cfg: Self = toml::from_str(&text)
                .with_context(|| format!("Parsing config: {}", path.display()))?;
            Ok(cfg)
        } else {
            Ok(Self::default())
        }
    }

    /// Persist config to disk.
    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        let text = toml::to_string_pretty(self).context("Serialising config")?;
        fs::write(&path, text)
            .with_context(|| format!("Writing config: {}", path.display()))?;
        Ok(())
    }
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
    std::env::var("EDITOR").unwrap_or_else(|_| "nano".into())
}

fn default_pager() -> String {
    std::env::var("PAGER").unwrap_or_else(|_| "less".into())
}

const fn t() -> bool { true }
const fn tab_default() -> usize { 4 }
const fn history_max() -> usize { 32 }
