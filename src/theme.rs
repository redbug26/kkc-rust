//! Colour theme loader.
//!
//! At startup call [`init`] (or [`init_default`] for the built-in palette).
//! Everywhere else call [`theme()`] to obtain a read guard for the active theme.
//!
//! The TOML file format is documented in `assets/themes/default.toml`.
//! KKC looks for the user theme at
//!   `$XDG_CONFIG_HOME/kkc/themes/default.toml`  (Linux)
//!   `~/Library/Application Support/be.kyuran.kkc/themes/default.toml` (macOS)
//! and falls back to the compiled-in default if the file is absent or unreadable.

use anyhow::{Context, Result};
use ratatui::style::Color;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock, RwLockReadGuard};

// Compiled-in fallback — always available, even in sandboxed environments.
const DEFAULT_TOML: &str = include_str!("../assets/themes/default.toml");
const CATPPUCCIN_MOCHA_TOML: &str = include_str!("../assets/themes/catppuccin_mocha.toml");
const NC_TOML: &str = include_str!("../assets/themes/nc.toml");

static THEME: OnceLock<RwLock<Theme>> = OnceLock::new();

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the global theme, trying the configured theme name first.
/// Safe to call multiple times; subsequent calls are no-ops.
pub fn init(theme_name: &str) {
    let theme = load_named_theme(theme_name).unwrap_or_else(|_| builtin("default"));
    if let Some(cell) = THEME.get() {
        if let Ok(mut current) = cell.write() {
            *current = theme;
        }
    } else {
        let _ = THEME.set(RwLock::new(theme));
    }
}

/// Replace the active theme at runtime.
pub fn set(theme_name: &str) -> Result<()> {
    let theme = load_named_theme(theme_name)?;
    if let Some(cell) = THEME.get() {
        let mut current = cell.write().expect("theme lock poisoned");
        *current = theme;
        Ok(())
    } else {
        let _ = THEME.set(RwLock::new(theme));
        Ok(())
    }
}

/// Return a reference to the active theme.
///
/// # Panics
/// Panics if [`init`] has not been called yet.
#[inline]
pub fn theme() -> RwLockReadGuard<'static, Theme> {
    THEME
        .get()
        .expect("theme::init() must be called before theme()")
        .read()
        .expect("theme lock poisoned")
}

pub fn available_theme_names() -> Vec<String> {
    let mut names = vec![
        "catppuccin_mocha".to_string(),
        "default".to_string(),
        "nc".to_string(),
    ];
    if let Ok(dir) = user_theme_dir()
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            if let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
                && !names.iter().any(|known| known == name)
            {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names
}

// ---------------------------------------------------------------------------
// Theme struct
// ---------------------------------------------------------------------------

/// All configurable colours grouped by UI area.
#[derive(Debug)]
pub struct Theme {
    pub app: AppColors,
    pub panel: PanelColors,
    pub header: HeaderColors,
    pub cursor: CursorColors,
    pub selection: SelectionColors,
    pub files: FileColors,
    pub status: StatusColors,
    pub fkeys: FkeyColors,
    pub buttons: ButtonColors,
    pub menu: MenuColors,
    pub palette: PaletteColors,
    pub dialog: DialogColors,
    pub search: SearchColors,
    pub syntax: SyntaxColors,
}

#[derive(Debug)]
pub struct AppColors {
    pub background: Color,
}

#[derive(Debug)]
pub struct PanelColors {
    pub background: Color,
    pub border: Color,
    pub border_dim: Color,
    pub title: Color,
    pub tree_connector: Color,
}

#[derive(Debug)]
pub struct HeaderColors {
    pub background: Color,
    pub foreground: Color,
}

#[derive(Debug)]
pub struct CursorColors {
    pub background: Color,
    pub foreground: Color,
}

#[derive(Debug)]
pub struct SelectionColors {
    pub foreground: Color,
}

#[derive(Debug)]
pub struct FileColors {
    pub directory: Color,
    pub executable: Color,
    pub archive: Color,
    pub audio: Color,
    pub image: Color,
    pub video: Color,
    pub document: Color,
    pub source: Color,
    pub data: Color,
    pub text: Color,
    pub unknown: Color,
}

#[derive(Debug)]
pub struct StatusColors {
    pub background: Color,
    pub foreground: Color,
}

#[derive(Debug)]
pub struct FkeyColors {
    pub background: Color,
    pub number_foreground: Color,
    pub number_background: Color,
    pub label: Color,
}

#[derive(Debug)]
pub struct ButtonColors {
    pub background: Color,
    pub foreground: Color,
}

#[derive(Debug)]
pub struct MenuColors {
    pub bar_background: Color,
    pub bar_foreground: Color,
    pub selected_background: Color,
    pub selected_foreground: Color,
    pub dropdown_background: Color,
    pub dropdown_foreground: Color,
    pub dropdown_separator: Color,
    pub border: Color,
    pub hotkey: Color,
    pub danger_button_inactive_background: Color,
    pub danger_button_inactive_foreground: Color,
}

#[derive(Debug)]
pub struct PaletteColors {
    pub background: Color,
    pub border: Color,
    pub input_background: Color,
    pub input_foreground: Color,
    pub separator: Color,
    pub list_foreground: Color,
    pub selected_background: Color,
    pub selected_foreground: Color,
    pub match_highlight: Color,
    pub match_highlight_selected: Color,
    pub no_match: Color,
    pub directory_foreground: Color,
    pub title: Color,
    pub footer_background: Color,
    pub footer_foreground: Color,
}

#[derive(Debug)]
pub struct DialogColors {
    pub background: Color,
    pub foreground: Color,
    pub border: Color,
    pub title: Color,
    pub selected_background: Color,
    pub selected_foreground: Color,
    pub hint: Color,
    pub inactive_button_background: Color,
    pub inactive_button_foreground: Color,
}

#[derive(Debug)]
pub struct SearchColors {
    pub background: Color,
    pub label_foreground: Color,
    pub active_label_foreground: Color,
    pub input_foreground: Color,
    pub input_background_active: Color,
    pub input_background_idle: Color,
    pub placeholder: Color,
    pub header: Color,
    pub separator: Color,
    pub running: Color,
}

#[derive(Debug)]
pub struct SyntaxColors {
    pub keyword: Color,
    pub type_name: Color,
    pub string: Color,
    pub comment: Color,
    pub number: Color,
    pub preprocessor: Color,
    pub function: Color,
    pub operator: Color,
    pub plain: Color,
    pub variable_language: Color,
    pub ketchup: Color,
}

// ---------------------------------------------------------------------------
// TOML deserialisation helpers
// ---------------------------------------------------------------------------

fn user_theme_dir() -> Result<PathBuf> {
    Ok(directories::ProjectDirs::from("be", "kyuran", "kkc")
        .context("cannot locate project dirs")?
        .config_dir()
        .join("themes"))
}

fn load_named_theme(name: &str) -> Result<Theme> {
    let name = sanitize_theme_name(name);
    if let Ok(dir) = user_theme_dir() {
        let path = dir.join(format!("{name}.toml"));
        if path.exists() {
            return load_theme_file(&path);
        }
    }

    builtin_named(&name).with_context(|| format!("unknown theme '{name}'"))
}

fn load_theme_file(path: &Path) -> Result<Theme> {
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading theme file {}", path.display()))?;
    parse_toml(&text).with_context(|| format!("parsing theme file {}", path.display()))
}

fn builtin_named(name: &str) -> Result<Theme> {
    match name {
        "catppuccin_mocha" => Ok(parse_toml(CATPPUCCIN_MOCHA_TOML)
            .expect("built-in Catppuccin Mocha theme TOML is always valid")),
        "default" => Ok(builtin("default")),
        "nc" => Ok(parse_toml(NC_TOML).expect("built-in NC theme TOML is always valid")),
        _ => anyhow::bail!("unknown built-in theme"),
    }
}

fn builtin(name: &str) -> Theme {
    match name {
        "catppuccin_mocha" => parse_toml(CATPPUCCIN_MOCHA_TOML)
            .expect("built-in Catppuccin Mocha theme TOML is always valid"),
        "nc" => parse_toml(NC_TOML).expect("built-in NC theme TOML is always valid"),
        _ => parse_toml(DEFAULT_TOML).expect("built-in default theme TOML is always valid"),
    }
}

fn sanitize_theme_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
    {
        "default".to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_toml(text: &str) -> Result<Theme> {
    let raw: RawTheme = toml::from_str(text).context("TOML parse error")?;
    Ok(raw.into())
}

// ---------------------------------------------------------------------------
// Raw serde types (string-based colours)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawTheme {
    app: RawApp,
    panel: RawPanel,
    header: RawHeader,
    cursor: RawCursor,
    selection: RawSelection,
    files: RawFiles,
    status: RawStatus,
    fkeys: RawFkeys,
    buttons: RawButtons,
    menu: RawMenu,
    palette: RawPalette,
    dialog: RawDialog,
    search: RawSearch,
    syntax: RawSyntax,
}

#[derive(Debug, Deserialize)]
struct RawApp {
    background: String,
}

#[derive(Debug, Deserialize)]
struct RawPanel {
    background: String,
    border: String,
    border_dim: String,
    title: String,
    tree_connector: String,
}

#[derive(Debug, Deserialize)]
struct RawHeader {
    background: String,
    foreground: String,
}

#[derive(Debug, Deserialize)]
struct RawCursor {
    background: String,
    foreground: String,
}

#[derive(Debug, Deserialize)]
struct RawSelection {
    foreground: String,
}

#[derive(Debug, Deserialize)]
struct RawFiles {
    directory: String,
    executable: String,
    archive: String,
    audio: String,
    image: String,
    video: String,
    document: String,
    source: String,
    data: String,
    text: String,
    unknown: String,
}

#[derive(Debug, Deserialize)]
struct RawStatus {
    background: String,
    foreground: String,
}

#[derive(Debug, Deserialize)]
struct RawFkeys {
    background: String,
    number_foreground: String,
    number_background: String,
    label: String,
}

#[derive(Debug, Deserialize)]
struct RawButtons {
    background: String,
    foreground: String,
}

#[derive(Debug, Deserialize)]
struct RawMenu {
    bar_background: String,
    bar_foreground: String,
    selected_background: String,
    selected_foreground: String,
    dropdown_background: String,
    dropdown_foreground: String,
    dropdown_separator: String,
    border: String,
    hotkey: String,
    #[serde(default)]
    danger_button_inactive_background: String,
    #[serde(default)]
    danger_button_inactive_foreground: String,
}

#[derive(Debug, Deserialize)]
struct RawPalette {
    background: String,
    border: String,
    input_background: String,
    input_foreground: String,
    separator: String,
    list_foreground: String,
    selected_background: String,
    selected_foreground: String,
    match_highlight: String,
    match_highlight_selected: String,
    no_match: String,
    directory_foreground: String,
    title: String,
    footer_background: String,
    footer_foreground: String,
}

#[derive(Debug, Deserialize)]
struct RawDialog {
    background: String,
    foreground: String,
    border: String,
    title: String,
    selected_background: String,
    selected_foreground: String,
    hint: String,
    #[serde(default)]
    inactive_button_background: String,
    #[serde(default)]
    inactive_button_foreground: String,
}

#[derive(Debug, Deserialize)]
struct RawSearch {
    background: String,
    label_foreground: String,
    active_label_foreground: String,
    input_foreground: String,
    input_background_active: String,
    input_background_idle: String,
    placeholder: String,
    header: String,
    separator: String,
    running: String,
}

#[derive(Debug, Deserialize)]
struct RawSyntax {
    keyword: String,
    type_name: String,
    string: String,
    comment: String,
    number: String,
    preprocessor: String,
    function: String,
    operator: String,
    plain: String,
    variable_language: String,
    ketchup: String,
}

// ---------------------------------------------------------------------------
// Conversion: Raw -> Theme
// ---------------------------------------------------------------------------

impl From<RawTheme> for Theme {
    fn from(r: RawTheme) -> Self {
        Theme {
            app: AppColors {
                background: parse_color(&r.app.background),
            },
            panel: PanelColors {
                background: parse_color(&r.panel.background),
                border: parse_color(&r.panel.border),
                border_dim: parse_color(&r.panel.border_dim),
                title: parse_color(&r.panel.title),
                tree_connector: parse_color(&r.panel.tree_connector),
            },
            header: HeaderColors {
                background: parse_color(&r.header.background),
                foreground: parse_color(&r.header.foreground),
            },
            cursor: CursorColors {
                background: parse_color(&r.cursor.background),
                foreground: parse_color(&r.cursor.foreground),
            },
            selection: SelectionColors {
                foreground: parse_color(&r.selection.foreground),
            },
            files: FileColors {
                directory: parse_color(&r.files.directory),
                executable: parse_color(&r.files.executable),
                archive: parse_color(&r.files.archive),
                audio: parse_color(&r.files.audio),
                image: parse_color(&r.files.image),
                video: parse_color(&r.files.video),
                document: parse_color(&r.files.document),
                source: parse_color(&r.files.source),
                data: parse_color(&r.files.data),
                text: parse_color(&r.files.text),
                unknown: parse_color(&r.files.unknown),
            },
            status: StatusColors {
                background: parse_color(&r.status.background),
                foreground: parse_color(&r.status.foreground),
            },
            fkeys: FkeyColors {
                background: parse_color(&r.fkeys.background),
                number_foreground: parse_color(&r.fkeys.number_foreground),
                number_background: parse_color(&r.fkeys.number_background),
                label: parse_color(&r.fkeys.label),
            },
            buttons: ButtonColors {
                background: parse_color(&r.buttons.background),
                foreground: parse_color(&r.buttons.foreground),
            },
            menu: MenuColors {
                bar_background: parse_color(&r.menu.bar_background),
                bar_foreground: parse_color(&r.menu.bar_foreground),
                selected_background: parse_color(&r.menu.selected_background),
                selected_foreground: parse_color(&r.menu.selected_foreground),
                dropdown_background: parse_color(&r.menu.dropdown_background),
                dropdown_foreground: parse_color(&r.menu.dropdown_foreground),
                dropdown_separator: parse_color(&r.menu.dropdown_separator),
                border: parse_color(&r.menu.border),
                hotkey: parse_color(&r.menu.hotkey),
                danger_button_inactive_background: if r
                    .menu
                    .danger_button_inactive_background
                    .is_empty()
                {
                    parse_color(&r.menu.dropdown_background)
                } else {
                    parse_color(&r.menu.danger_button_inactive_background)
                },
                danger_button_inactive_foreground: if r
                    .menu
                    .danger_button_inactive_foreground
                    .is_empty()
                {
                    parse_color(&r.menu.dropdown_foreground)
                } else {
                    parse_color(&r.menu.danger_button_inactive_foreground)
                },
            },
            palette: PaletteColors {
                background: parse_color(&r.palette.background),
                border: parse_color(&r.palette.border),
                input_background: parse_color(&r.palette.input_background),
                input_foreground: parse_color(&r.palette.input_foreground),
                separator: parse_color(&r.palette.separator),
                list_foreground: parse_color(&r.palette.list_foreground),
                selected_background: parse_color(&r.palette.selected_background),
                selected_foreground: parse_color(&r.palette.selected_foreground),
                match_highlight: parse_color(&r.palette.match_highlight),
                match_highlight_selected: parse_color(&r.palette.match_highlight_selected),
                no_match: parse_color(&r.palette.no_match),
                directory_foreground: parse_color(&r.palette.directory_foreground),
                title: parse_color(&r.palette.title),
                footer_background: parse_color(&r.palette.footer_background),
                footer_foreground: parse_color(&r.palette.footer_foreground),
            },
            dialog: DialogColors {
                background: parse_color(&r.dialog.background),
                foreground: parse_color(&r.dialog.foreground),
                border: parse_color(&r.dialog.border),
                title: parse_color(&r.dialog.title),
                selected_background: parse_color(&r.dialog.selected_background),
                selected_foreground: parse_color(&r.dialog.selected_foreground),
                hint: parse_color(&r.dialog.hint),
                inactive_button_background: if r.dialog.inactive_button_background.is_empty() {
                    parse_color(&r.dialog.foreground)
                } else {
                    parse_color(&r.dialog.inactive_button_background)
                },
                inactive_button_foreground: if r.dialog.inactive_button_foreground.is_empty() {
                    parse_color(&r.dialog.hint)
                } else {
                    parse_color(&r.dialog.inactive_button_foreground)
                },
            },
            search: SearchColors {
                background: parse_color(&r.search.background),
                label_foreground: parse_color(&r.search.label_foreground),
                active_label_foreground: parse_color(&r.search.active_label_foreground),
                input_foreground: parse_color(&r.search.input_foreground),
                input_background_active: parse_color(&r.search.input_background_active),
                input_background_idle: parse_color(&r.search.input_background_idle),
                placeholder: parse_color(&r.search.placeholder),
                header: parse_color(&r.search.header),
                separator: parse_color(&r.search.separator),
                running: parse_color(&r.search.running),
            },
            syntax: SyntaxColors {
                keyword: parse_color(&r.syntax.keyword),
                type_name: parse_color(&r.syntax.type_name),
                string: parse_color(&r.syntax.string),
                comment: parse_color(&r.syntax.comment),
                number: parse_color(&r.syntax.number),
                preprocessor: parse_color(&r.syntax.preprocessor),
                function: parse_color(&r.syntax.function),
                operator: parse_color(&r.syntax.operator),
                plain: parse_color(&r.syntax.plain),
                variable_language: parse_color(&r.syntax.variable_language),
                ketchup: parse_color(&r.syntax.ketchup),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Colour string parser
// ---------------------------------------------------------------------------

/// Parse a colour string from the TOML file into a [`Color`].
///
/// Supported formats:
/// - `"#RRGGBB"` — 24-bit hex
/// - ANSI names: `"black"`, `"red"`, `"green"`, `"yellow"`, `"blue"`,
///   `"magenta"`, `"cyan"`, `"gray"`, `"darkgray"`, `"lightred"`,
///   `"lightgreen"`, `"lightyellow"`, `"lightblue"`, `"lightmagenta"`,
///   `"lightcyan"`, `"white"`
///
/// Unknown values fall back to `Color::Reset`.
fn parse_color(s: &str) -> Color {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        }
    }
    match s.to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        "reset" => Color::Reset,
        _ => {
            // Not a recognised colour — use a visible fallback so theming
            // mistakes are obvious rather than silently invisible.
            Color::Reset
        }
    }
}
