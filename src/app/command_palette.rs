//! Command palette state and data – independent from MENU_DATA so both can
//! evolve separately, and commands can easily be added, labelled, or i18n'd.

use super::App;
use super::menu::MenuAction;
use crate::config::ShortcutOverride;
use crate::lua_apps::LuaAppInfo;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// One entry in the Ctrl-P command palette.
#[derive(Debug, Clone, Copy)]
pub struct PaletteEntry {
    /// Menu category shown in the left column (e.g. "File", "Tools").
    pub category: &'static str,
    /// Human-readable label shown in the middle column.
    pub label: &'static str,
    /// Compact label for F-key bar and center buttons.
    pub shortname: &'static str,
    /// Pre-formatted keyboard shortcut shown in the right column, if any.
    pub shortcut: Option<&'static str>,
    /// Raw Rust identifier (shown in dim parens) – useful for future i18n keys.
    pub fn_name: &'static str,
    /// The action executed when this entry is selected.
    pub action: MenuAction,
}

// ---------------------------------------------------------------------------
// Palette entries – edit this table to add/remove/rename commands
// ---------------------------------------------------------------------------

pub static PALETTE_DATA: &[PaletteEntry] = &[
    // ── Interface ────────────────────────────────────────────────────────
    PaletteEntry {
        category: "Interface",
        label: "Switch panel",
        shortname: "Switch",
        shortcut: Some("Tab"),
        fn_name: "switch_panel",
        action: MenuAction::SwitchPanel,
    },
    PaletteEntry {
        category: "Interface",
        label: "Menu",
        shortname: "Menu",
        shortcut: Some("F2"),
        fn_name: "open_menu",
        action: MenuAction::OpenMenu,
    },
    PaletteEntry {
        category: "Interface",
        label: "Command palette",
        shortname: "CmdPal",
        shortcut: Some("Ctrl+P"),
        fn_name: "command_palette",
        action: MenuAction::OpenCommandPalette,
    },
    PaletteEntry {
        category: "Interface",
        label: "Plugin actions",
        shortname: "Actions",
        shortcut: Some("Ctrl+A"),
        fn_name: "plugin_actions",
        action: MenuAction::OpenActionPalette,
    },
    // ── File ─────────────────────────────────────────────────────────────
    PaletteEntry {
        category: "File",
        label: "View file",
        shortname: "View",
        shortcut: Some("F3"),
        fn_name: "view_file",
        action: MenuAction::ViewFile,
    },
    PaletteEntry {
        category: "File",
        label: "Edit file",
        shortname: "Edit",
        shortcut: Some("F4"),
        fn_name: "edit_file",
        action: MenuAction::EditFile,
    },
    PaletteEntry {
        category: "File",
        label: "Copy to…",
        shortname: "Copy",
        shortcut: Some("F5"),
        fn_name: "copy_file",
        action: MenuAction::CopyFile,
    },
    PaletteEntry {
        category: "File",
        label: "Move to…",
        shortname: "Move",
        shortcut: Some("F6"),
        fn_name: "move_file",
        action: MenuAction::MoveFile,
    },
    PaletteEntry {
        category: "File",
        label: "Create directory",
        shortname: "MDir",
        shortcut: Some("F7"),
        fn_name: "mkdir",
        action: MenuAction::MkDir,
    },
    PaletteEntry {
        category: "File",
        label: "Rename",
        shortname: "Rename",
        shortcut: Some("Shift+F6"),
        fn_name: "rename_file",
        action: MenuAction::RenameFile,
    },
    PaletteEntry {
        category: "File",
        label: "Delete",
        shortname: "Delete",
        shortcut: Some("F8"),
        fn_name: "delete_file",
        action: MenuAction::DeleteFile,
    },
    PaletteEntry {
        category: "File",
        label: "Quit",
        shortname: "Quit",
        shortcut: Some("F10"),
        fn_name: "quit",
        action: MenuAction::Quit,
    },
    // Trigger a local materialisation of cloud-only placeholder files (iCloud /
    // Dropbox / OneDrive …).  Non-cloud entries in the selection are silently
    // skipped.  Falls back to the entry under the cursor when nothing is selected.
    PaletteEntry {
        category: "File",
        label: "Download cloud file(s)",
        shortname: "Download",
        shortcut: None,
        fn_name: "download_cloud_file",
        action: MenuAction::DownloadCloudFile,
    },
    // ── Panel ────────────────────────────────────────────────────────────
    PaletteEntry {
        category: "Panel",
        label: "Swap panels",
        shortname: "Swap",
        shortcut: None,
        fn_name: "swap_panels",
        action: MenuAction::SwapPanels,
    },
    PaletteEntry {
        category: "Panel",
        label: "Sort by name",
        shortname: "Name",
        shortcut: Some("Ctrl+F1"),
        fn_name: "sort_name",
        action: MenuAction::SortName,
    },
    PaletteEntry {
        category: "Panel",
        label: "Sort by extension",
        shortname: "Ext",
        shortcut: Some("Ctrl+F2"),
        fn_name: "sort_extension",
        action: MenuAction::SortExtension,
    },
    PaletteEntry {
        category: "Panel",
        label: "Sort by date",
        shortname: "Date",
        shortcut: Some("Ctrl+F3"),
        fn_name: "sort_date",
        action: MenuAction::SortDate,
    },
    PaletteEntry {
        category: "Panel",
        label: "Sort by size",
        shortname: "Size",
        shortcut: Some("Ctrl+F4"),
        fn_name: "sort_size",
        action: MenuAction::SortSize,
    },
    PaletteEntry {
        category: "Panel",
        label: "Unsorted",
        shortname: "Unsorted",
        shortcut: Some("Ctrl+F5"),
        fn_name: "sort_unsorted",
        action: MenuAction::SortUnsorted,
    },
    PaletteEntry {
        category: "Panel",
        label: "Toggle hidden files",
        shortname: "Hidden",
        shortcut: Some("Ctrl+H"),
        fn_name: "toggle_hidden",
        action: MenuAction::ToggleHidden,
    },
    PaletteEntry {
        category: "Panel",
        label: "Reload",
        shortname: "Reload",
        shortcut: Some("Ctrl+R"),
        fn_name: "reload",
        action: MenuAction::Reload,
    },
    // ── Disk ─────────────────────────────────────────────────────────────
    PaletteEntry {
        category: "Disk",
        label: "Go to path…",
        shortname: "GoPath",
        shortcut: None,
        fn_name: "goto_path",
        action: MenuAction::GoToPath,
    },
    // ── Selection ────────────────────────────────────────────────────────
    PaletteEntry {
        category: "Select",
        label: "Select pattern…",
        shortname: "Select",
        shortcut: Some("+"),
        fn_name: "select_pattern",
        action: MenuAction::SelectPattern,
    },
    PaletteEntry {
        category: "Select",
        label: "Deselect pattern…",
        shortname: "Deselect",
        shortcut: Some("-"),
        fn_name: "deselect_pattern",
        action: MenuAction::DeselectPattern,
    },
    PaletteEntry {
        category: "Select",
        label: "Invert selection",
        shortname: "Invert",
        shortcut: Some("*"),
        fn_name: "invert_selection",
        action: MenuAction::InvertSelection,
    },
    // ── Tools ────────────────────────────────────────────────────────────
    PaletteEntry {
        category: "Tools",
        label: "Search files…",
        shortname: "Search",
        shortcut: None,
        fn_name: "search_files",
        action: MenuAction::SearchFiles,
    },
    PaletteEntry {
        category: "Tools",
        label: "Tree view…",
        shortname: "Tree",
        shortcut: None,
        fn_name: "tree_view",
        action: MenuAction::TreeView,
    },
    PaletteEntry {
        category: "Tools",
        label: "Enter archive",
        shortname: "ArcPlug",
        shortcut: None,
        fn_name: "enter_archive_plugin",
        action: MenuAction::EnterArchivePlugin,
    },
    PaletteEntry {
        category: "Tools",
        label: "Compare files (with diff)",
        shortname: "Compare",
        shortcut: None,
        fn_name: "compare_panel_diff",
        action: MenuAction::ComparePanelFiles,
    },
    PaletteEntry {
        category: "Tools",
        label: "Compare files",
        shortname: "CmpPanel",
        shortcut: None,
        fn_name: "compare_panel",
        action: MenuAction::ComparePanelInternal,
    },
    PaletteEntry {
        category: "Tools",
        label: "Store",
        shortname: "Store",
        shortcut: None,
        fn_name: "install_plugin_from_store",
        action: MenuAction::InstallPluginFromStore,
    },
    PaletteEntry {
        category: "Tools",
        label: "Remote connect…",
        shortname: "Remote",
        shortcut: Some("Ctrl+F"),
        fn_name: "remote_connect",
        action: MenuAction::RemoteConnect,
    },
    PaletteEntry {
        category: "Tools",
        label: "File Info preview",
        shortname: "Info",
        shortcut: None,
        fn_name: "file_preview_info",
        action: MenuAction::FileIdPreview,
    },
    PaletteEntry {
        category: "Tools",
        label: "Bookmarks",
        shortname: "QuickDir",
        shortcut: Some("Ctrl+D"),
        fn_name: "dir_bookmarks",
        action: MenuAction::DirBookmarks,
    },
    // ── Options ──────────────────────────────────────────────────────────
    PaletteEntry {
        category: "Options",
        label: "Setup…",
        shortname: "Setup",
        shortcut: None,
        fn_name: "setup",
        action: MenuAction::Setup,
    },
    PaletteEntry {
        category: "Options",
        label: "Plugins…",
        shortname: "Plugins",
        shortcut: None,
        fn_name: "plugins",
        action: MenuAction::Plugins,
    },
    PaletteEntry {
        category: "Options",
        label: "Associations…",
        shortname: "Assoc",
        shortcut: None,
        fn_name: "associations",
        action: MenuAction::Associations,
    },
    PaletteEntry {
        category: "Options",
        label: "Toggle F-key bar",
        shortname: "FBar",
        shortcut: None,
        fn_name: "toggle_fkey_bar",
        action: MenuAction::ToggleFBar,
    },
    PaletteEntry {
        category: "Options",
        label: "Save config",
        shortname: "SaveCfg",
        shortcut: None,
        fn_name: "save_config",
        action: MenuAction::SaveConfig,
    },
    // ── Help ─────────────────────────────────────────────────────────────
    PaletteEntry {
        category: "Help",
        label: "Help",
        shortname: "Help",
        shortcut: Some("F1"),
        fn_name: "help",
        action: MenuAction::Help,
    },
    PaletteEntry {
        category: "Help",
        label: "About KKC",
        shortname: "About",
        shortcut: None,
        fn_name: "about",
        action: MenuAction::About,
    },
    PaletteEntry {
        category: "Help",
        label: "KKC information",
        shortname: "Info",
        shortcut: None,
        fn_name: "system_info",
        action: MenuAction::SystemInfo,
    },
    // ── Tabs ─────────────────────────────────────────────────────────────
    PaletteEntry {
        category: "Tabs",
        label: "New tab",
        shortname: "NewTab",
        shortcut: Some("Ctrl+T"),
        fn_name: "new_tab",
        action: MenuAction::NewTab,
    },
    PaletteEntry {
        category: "Tools",
        label: "Run Lua app",
        shortname: "LuaApp",
        shortcut: None,
        fn_name: "run_lua_app",
        action: MenuAction::RunLuaApp,
    },
    PaletteEntry {
        category: "Tabs",
        label: "Close tab",
        shortname: "CloseTab",
        shortcut: Some("Ctrl+W"),
        fn_name: "close_tab",
        action: MenuAction::CloseTab,
    },
    PaletteEntry {
        category: "Tabs",
        label: "Next tab",
        shortname: "NextTab",
        shortcut: Some("Ctrl+N"),
        fn_name: "next_tab",
        action: MenuAction::NextTab,
    },
    PaletteEntry {
        category: "Tools",
        label: "Open terminal",
        shortname: "Terminal",
        shortcut: Some("Ctrl+U"),
        fn_name: "open_terminal",
        action: MenuAction::OpenTerminal,
    },
    PaletteEntry {
        category: "Tools",
        label: "Capture GIF frame",
        shortname: "GIF",
        shortcut: Some("Ctrl+B"),
        fn_name: "capture_gif",
        action: MenuAction::CaptureGif,
    },
    PaletteEntry {
        category: "Tools",
        label: "Matrix screensaver",
        shortname: "Matrix",
        shortcut: None,
        fn_name: "matrix_screensaver",
        action: MenuAction::MatrixScreensaver,
    },
    PaletteEntry {
        category: "Tools",
        label: "Toggle debug log",
        shortname: "DebugLog",
        shortcut: None,
        fn_name: "debug_log",
        action: MenuAction::DebugLog,
    },
    // ── OS integration ───────────────────────────────────────────────────
    PaletteEntry {
        category: "File",
        label: "Open in OS",
        shortname: "OpenOS",
        shortcut: None,
        fn_name: "open_in_os",
        action: MenuAction::OpenInOs,
    },
    PaletteEntry {
        category: "File",
        label: "Open folder in OS",
        shortname: "OpenDir",
        shortcut: None,
        fn_name: "open_folder_in_os",
        action: MenuAction::OpenFolderInOs,
    },
    PaletteEntry {
        category: "File",
        label: "Quick Preview",
        shortname: "Preview",
        shortcut: None,
        fn_name: "quick_preview",
        action: MenuAction::QuickPreview,
    },
];

pub fn palette_label_for_action(action: MenuAction) -> &'static str {
    PALETTE_DATA
        .iter()
        .find(|entry| entry.action == action)
        .map(|entry| entry.label)
        .unwrap_or("")
}

pub fn palette_shortname_for_action(action: MenuAction) -> &'static str {
    PALETTE_DATA
        .iter()
        .find(|entry| entry.action == action)
        .map(|entry| entry.shortname)
        .unwrap_or("")
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct CommandPaletteState {
    pub query: String,
    pub match_pos: usize,
    pub capture: bool,
    /// Snapshot of recently-used commands (fn_name values), most-recent first.
    /// Populated from `App::palette_recent` when the palette is opened.
    pub recent: Vec<String>,
    /// Dynamic Lua app entries appended after the static PALETTE_DATA.
    /// Indices into this vec are encoded as `PALETTE_DATA.len() + lua_app_idx`.
    pub lua_apps: Vec<LuaAppInfo>,
}

/// Sentinel value used in `filtered_indices()` to represent the visual separator between
/// the "recent" section and the full command list.
pub const PALETTE_SEP: usize = usize::MAX;

pub fn normalize_shortcut(value: &str) -> String {
    value
        .replace("Ctrl+", "Ctrl+")
        .replace("CTRL+", "Ctrl+")
        .replace("Control+", "Ctrl+")
        .replace("^", "Ctrl+")
        .replace("Shift+", "Shift+")
        .replace("SHIFT+", "Shift+")
        .replace("S-", "Shift+")
        .replace("Alt+", "Alt+")
        .replace("ALT+", "Alt+")
        .replace("A-", "Alt+")
}

pub fn shortcut_from_key_event(key: KeyEvent) -> Option<String> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    if alt && !ctrl && !shift {
        if let KeyCode::Char(c) = key.code {
            return match c {
                '1'..='9' => Some(format!("F{}", (c as u8) - b'0')),
                '0' => Some("F10".to_string()),
                _ => None,
            };
        }
    }

    let mut parts: Vec<&str> = Vec::new();
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }

    let key_name = match key.code {
        KeyCode::F(n) => format!("F{}", n),
        KeyCode::Char(c) => c.to_ascii_uppercase().to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "Shift+Tab".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        _ => return None,
    };

    if key_name == "Shift+Tab" {
        return Some(key_name);
    }
    if parts.is_empty() {
        Some(key_name)
    } else {
        Some(format!("{}+{}", parts.join("+"), key_name))
    }
}

fn entry_matches(e: &PaletteEntry, q: &str) -> bool {
    format!(
        "{} {} {} {} {}",
        e.category,
        e.label,
        e.shortname,
        e.fn_name,
        e.shortcut.unwrap_or("")
    )
    .to_lowercase()
    .contains(q)
}

fn lua_app_entry_matches(info: &LuaAppInfo, q: &str) -> bool {
    format!("apps {} lua_app_{} {}", info.name, info.id, info.description)
        .to_lowercase()
        .contains(q)
}

impl CommandPaletteState {
    /// Returns indices into PALETTE_DATA that match the current query.
    ///
    /// When there are recent commands, the list is structured as:
    ///   `[recent…, PALETTE_SEP, rest…]`
    /// where `PALETTE_SEP` (`usize::MAX`) is a non-selectable visual separator row.
    pub fn filtered_indices(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();

        // Resolve persisted fn_name entries to palette indices.
        // Also accept legacy numeric strings and "lua_app:<id>" tokens from older configs.
        let lua_base = PALETTE_DATA.len();
        let mut recent_seen = std::collections::HashSet::new();
        let recent_valid: Vec<usize> = self
            .recent
            .iter()
            .filter_map(|name| {
                // "lua_app:<id>" token → encoded index into lua_apps
                if let Some(id) = name.strip_prefix("lua_app:") {
                    let idx = self.lua_apps.iter().position(|info| info.id == id)?;
                    return Some(lua_base + idx);
                }
                if let Ok(i) = name.parse::<usize>() {
                    if i < PALETTE_DATA.len() {
                        return Some(i);
                    }
                }
                PALETTE_DATA.iter().position(|e| e.fn_name == name)
            })
            .filter(|i| recent_seen.insert(*i))
            .collect();

        // Lua app indices: PALETTE_DATA.len() + lua_app_idx
        let lua_matching: Vec<usize> = self
            .lua_apps
            .iter()
            .enumerate()
            .filter(|(_, info)| q.is_empty() || lua_app_entry_matches(info, &q))
            .map(|(i, _)| lua_base + i)
            .collect();

        if recent_valid.is_empty() {
            // No recents — original behaviour.
            let mut result: Vec<usize> = if q.is_empty() {
                (0..PALETTE_DATA.len()).collect()
            } else {
                PALETTE_DATA
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| entry_matches(e, &q))
                    .map(|(i, _)| i)
                    .collect()
            };
            if !lua_matching.is_empty() {
                if !result.is_empty() {
                    result.push(PALETTE_SEP);
                }
                result.extend_from_slice(&lua_matching);
            }
            return result;
        }

        // Build a set for fast membership tests.
        let recent_set: std::collections::HashSet<usize> = recent_valid.iter().copied().collect();

        let recent_items: Vec<usize> = recent_valid
            .iter()
            .copied()
            .filter(|&i| q.is_empty() || entry_matches(&PALETTE_DATA[i], &q))
            .collect();

        let rest_items: Vec<usize> = PALETTE_DATA
            .iter()
            .enumerate()
            .filter(|(i, e)| !recent_set.contains(i) && (q.is_empty() || entry_matches(e, &q)))
            .map(|(i, _)| i)
            .collect();

        let mut result = Vec::new();
        if !recent_items.is_empty() {
            result.extend_from_slice(&recent_items);
            result.push(PALETTE_SEP); // visual separator
        }
        result.extend_from_slice(&rest_items);
        if !lua_matching.is_empty() {
            if !result.iter().all(|&i| i == PALETTE_SEP) || !result.is_empty() {
                result.push(PALETTE_SEP);
            }
            result.extend_from_slice(&lua_matching);
        }
        result
    }

    /// Returns a Lua app info when the selected index encodes a Lua app.
    pub fn selected_lua_app(&self) -> Option<&LuaAppInfo> {
        let idx = self.selected_palette_index()?;
        let lua_base = PALETTE_DATA.len();
        if idx >= lua_base {
            self.lua_apps.get(idx - lua_base)
        } else {
            None
        }
    }

    pub fn selected_palette_index(&self) -> Option<usize> {
        self.filtered_indices()
            .get(self.match_pos)
            .copied()
            .filter(|idx| *idx != PALETTE_SEP)
    }

    pub fn clamp_match(&mut self) {
        let indices = self.filtered_indices();
        let len = indices.len();
        if len == 0 {
            self.match_pos = 0;
            return;
        }

        self.match_pos = self.match_pos.min(len.saturating_sub(1));
        if indices[self.match_pos] == PALETTE_SEP {
            if let Some(pos) = indices.iter().position(|idx| *idx != PALETTE_SEP) {
                self.match_pos = pos;
            } else {
                self.match_pos = 0;
            }
        }
    }
}

impl App {
    pub fn effective_shortcut_for(&self, fn_name: &str, default: Option<&str>) -> Option<String> {
        self.config
            .shortcut_overrides
            .iter()
            .find(|item| item.fn_name == fn_name)
            .map(|item| item.shortcut.clone())
            .unwrap_or_else(|| default.map(normalize_shortcut))
    }

    pub fn action_for_key(&self, key: KeyEvent) -> Option<MenuAction> {
        let shortcut = shortcut_from_key_event(key)?;
        PALETTE_DATA.iter().find_map(|entry| {
            (self
                .effective_shortcut_for(entry.fn_name, entry.shortcut)
                .as_deref()
                == Some(shortcut.as_str()))
            .then_some(entry.action)
        })
    }

    pub fn shortcut_key_is_managed(&self, key: KeyEvent) -> bool {
        let Some(shortcut) = shortcut_from_key_event(key) else {
            return false;
        };
        PALETTE_DATA
            .iter()
            .filter_map(|entry| entry.shortcut.map(normalize_shortcut))
            .chain(
                self.config
                    .shortcut_overrides
                    .iter()
                    .filter_map(|item| item.shortcut.clone()),
            )
            .any(|known| known == shortcut)
    }

    pub fn set_shortcut_for_fn(&mut self, fn_name: &str, shortcut: Option<String>) {
        let shortcut = shortcut.map(|value| normalize_shortcut(&value));

        if let Some(ref value) = shortcut {
            self.config.shortcut_overrides.retain(|item| {
                item.fn_name == fn_name || item.shortcut.as_deref() != Some(value.as_str())
            });

            for entry in PALETTE_DATA {
                if entry.fn_name == fn_name {
                    continue;
                }
                if entry.shortcut.map(normalize_shortcut).as_deref() == Some(value.as_str()) {
                    self.config
                        .shortcut_overrides
                        .retain(|item| item.fn_name != entry.fn_name);
                    self.config.shortcut_overrides.push(ShortcutOverride {
                        fn_name: entry.fn_name.to_string(),
                        shortcut: None,
                    });
                }
            }
        }

        let default = PALETTE_DATA
            .iter()
            .find(|entry| entry.fn_name == fn_name)
            .and_then(|entry| entry.shortcut)
            .map(normalize_shortcut);

        self.config
            .shortcut_overrides
            .retain(|item| item.fn_name != fn_name);

        if shortcut != default {
            self.config.shortcut_overrides.push(ShortcutOverride {
                fn_name: fn_name.to_string(),
                shortcut,
            });
        }

        self.normalize_shortcut_overrides();
    }

    pub fn reset_shortcut_for_fn(&mut self, fn_name: &str) {
        let default = PALETTE_DATA
            .iter()
            .find(|entry| entry.fn_name == fn_name)
            .and_then(|entry| entry.shortcut)
            .map(normalize_shortcut);

        if let Some(default) = default {
            self.set_shortcut_for_fn(fn_name, Some(default));
        } else {
            self.config
                .shortcut_overrides
                .retain(|item| item.fn_name != fn_name);
            self.normalize_shortcut_overrides();
        }
    }

    pub(crate) fn normalize_shortcut_overrides(&mut self) {
        let valid_names: HashSet<&str> = PALETTE_DATA.iter().map(|entry| entry.fn_name).collect();
        self.config.shortcut_overrides.retain(|item| {
            // Always preserve Lua app shortcut entries (fn_name = "lua_app_<id>").
            if item.fn_name.starts_with("lua_app_") {
                return item.shortcut.is_some();
            }
            if !valid_names.contains(item.fn_name.as_str()) {
                return false;
            }
            let default = PALETTE_DATA
                .iter()
                .find(|entry| entry.fn_name == item.fn_name)
                .and_then(|entry| entry.shortcut)
                .map(normalize_shortcut);
            item.shortcut != default
        });
    }

    /// Returns the app_id if `key` matches a shortcut override for a Lua app (`lua_app_<id>`).
    pub fn lua_app_id_for_key(&self, key: crossterm::event::KeyEvent) -> Option<String> {
        let shortcut = shortcut_from_key_event(key)?;
        self.config
            .shortcut_overrides
            .iter()
            .find(|item| {
                item.fn_name.starts_with("lua_app_")
                    && item.shortcut.as_deref() == Some(shortcut.as_str())
            })
            .and_then(|item| item.fn_name.strip_prefix("lua_app_").map(str::to_string))
    }
}
