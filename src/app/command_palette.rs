//! Command palette state and data – independent from MENU_DATA so both can
//! evolve separately, and commands can easily be added, labelled, or i18n'd.

use super::menu::MenuAction;

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
    // ── File ─────────────────────────────────────────────────────────────
    PaletteEntry { category: "File", label: "View file",          shortcut: Some("F3"),       fn_name: "view_file",          action: MenuAction::ViewFile },
    PaletteEntry { category: "File", label: "Edit file",          shortcut: Some("F4"),       fn_name: "edit_file",          action: MenuAction::EditFile },
    PaletteEntry { category: "File", label: "Copy to…",           shortcut: Some("F5"),       fn_name: "copy_file",          action: MenuAction::CopyFile },
    PaletteEntry { category: "File", label: "Move to…",           shortcut: Some("F6"),       fn_name: "move_file",          action: MenuAction::MoveFile },
    PaletteEntry { category: "File", label: "Create directory",   shortcut: Some("F7"),       fn_name: "mkdir",              action: MenuAction::MkDir },
    PaletteEntry { category: "File", label: "Rename",             shortcut: Some("Shift+F6"), fn_name: "rename_file",        action: MenuAction::RenameFile },
    PaletteEntry { category: "File", label: "Delete",             shortcut: Some("F8"),       fn_name: "delete_file",        action: MenuAction::DeleteFile },
    PaletteEntry { category: "File", label: "Quit",               shortcut: Some("F10"),      fn_name: "quit",               action: MenuAction::Quit },
    // ── Panel ────────────────────────────────────────────────────────────
    PaletteEntry { category: "Panel", label: "Swap panels",           shortcut: None,             fn_name: "swap_panels",        action: MenuAction::SwapPanels },
    PaletteEntry { category: "Panel", label: "Sort by name",          shortcut: Some("Ctrl+F1"),  fn_name: "sort_name",          action: MenuAction::SortName },
    PaletteEntry { category: "Panel", label: "Sort by extension",     shortcut: Some("Ctrl+F2"),  fn_name: "sort_extension",     action: MenuAction::SortExtension },
    PaletteEntry { category: "Panel", label: "Sort by date",          shortcut: Some("Ctrl+F3"),  fn_name: "sort_date",          action: MenuAction::SortDate },
    PaletteEntry { category: "Panel", label: "Sort by size",          shortcut: Some("Ctrl+F4"),  fn_name: "sort_size",          action: MenuAction::SortSize },
    PaletteEntry { category: "Panel", label: "Unsorted",              shortcut: Some("Ctrl+F5"),  fn_name: "sort_unsorted",      action: MenuAction::SortUnsorted },
    PaletteEntry { category: "Panel", label: "Toggle hidden files",   shortcut: Some("Ctrl+H"),   fn_name: "toggle_hidden",      action: MenuAction::ToggleHidden },
    PaletteEntry { category: "Panel", label: "Reload",                shortcut: Some("Ctrl+R"),   fn_name: "reload",             action: MenuAction::Reload },
    // ── Disk ─────────────────────────────────────────────────────────────
    PaletteEntry { category: "Disk", label: "Go to path…",        shortcut: None,             fn_name: "goto_path",          action: MenuAction::GoToPath },
    // ── Selection ────────────────────────────────────────────────────────
    PaletteEntry { category: "Select", label: "Select pattern…",   shortcut: Some("+"),        fn_name: "select_pattern",     action: MenuAction::SelectPattern },
    PaletteEntry { category: "Select", label: "Deselect pattern…", shortcut: Some("-"),        fn_name: "deselect_pattern",   action: MenuAction::DeselectPattern },
    PaletteEntry { category: "Select", label: "Invert selection",  shortcut: Some("*"),        fn_name: "invert_selection",   action: MenuAction::InvertSelection },
    // ── Tools ────────────────────────────────────────────────────────────
    PaletteEntry { category: "Tools", label: "Search files…",      shortcut: Some("Alt+F7"),   fn_name: "search_files",       action: MenuAction::SearchFiles },
    PaletteEntry { category: "Tools", label: "Remote connect…",    shortcut: Some("Ctrl+F"),   fn_name: "remote_connect",     action: MenuAction::RemoteConnect },
    PaletteEntry { category: "Tools", label: "File ID preview",    shortcut: Some("Alt+F4"),   fn_name: "file_id_preview",    action: MenuAction::FileIdPreview },
    PaletteEntry { category: "Tools", label: "Bookmarks",          shortcut: Some("Ctrl+D"),   fn_name: "dir_bookmarks",      action: MenuAction::DirBookmarks },
    // ── Options ──────────────────────────────────────────────────────────
    PaletteEntry { category: "Options", label: "Setup…",           shortcut: None,             fn_name: "setup",              action: MenuAction::Setup },
    PaletteEntry { category: "Options", label: "Plugins…",         shortcut: None,             fn_name: "plugins",            action: MenuAction::Plugins },
    PaletteEntry { category: "Options", label: "Associations…",    shortcut: None,             fn_name: "associations",       action: MenuAction::Associations },
    PaletteEntry { category: "Options", label: "Toggle F-key bar", shortcut: None,             fn_name: "toggle_fkey_bar",    action: MenuAction::ToggleFBar },
    PaletteEntry { category: "Options", label: "Save config",      shortcut: None,             fn_name: "save_config",        action: MenuAction::SaveConfig },
    // ── Help ─────────────────────────────────────────────────────────────
    PaletteEntry { category: "Help", label: "Help",                shortcut: Some("F1"),       fn_name: "help",               action: MenuAction::Help },
    PaletteEntry { category: "Help", label: "About KKC",           shortcut: None,             fn_name: "about",              action: MenuAction::About },
    // ── Tabs ─────────────────────────────────────────────────────────────
    PaletteEntry { category: "Tabs", label: "New tab",             shortcut: Some("Ctrl+T"),   fn_name: "new_tab",            action: MenuAction::NewTab },
    PaletteEntry { category: "Tabs", label: "Close tab",           shortcut: Some("Ctrl+W"),   fn_name: "close_tab",          action: MenuAction::CloseTab },
    PaletteEntry { category: "Tabs", label: "Next tab",            shortcut: Some("Ctrl+N"),   fn_name: "next_tab",           action: MenuAction::NextTab },
    PaletteEntry { category: "Tools", label: "Open terminal",      shortcut: Some("Ctrl+U"),   fn_name: "open_terminal",      action: MenuAction::OpenTerminal },
    PaletteEntry { category: "Tools", label: "Capture GIF frame",  shortcut: Some("Ctrl+G"),   fn_name: "capture_gif",        action: MenuAction::CaptureGif },
    // ── OS integration ───────────────────────────────────────────────────
    PaletteEntry { category: "File",  label: "Open in OS",           shortcut: None,             fn_name: "open_in_os",         action: MenuAction::OpenInOs },
    PaletteEntry { category: "File",  label: "Open folder in OS",    shortcut: None,             fn_name: "open_folder_in_os",  action: MenuAction::OpenFolderInOs },
    PaletteEntry { category: "File",  label: "Quick Preview",        shortcut: None,             fn_name: "quick_preview",      action: MenuAction::QuickPreview },
];

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct CommandPaletteState {
    pub query: String,
    pub match_pos: usize,
}

impl CommandPaletteState {
    /// Returns the indices into PALETTE_DATA that match the current query.
    pub fn filtered_indices(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        if q.is_empty() {
            return (0..PALETTE_DATA.len()).collect();
        }
        PALETTE_DATA
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                format!(
                    "{} {} {} {}",
                    e.category,
                    e.label,
                    e.fn_name,
                    e.shortcut.unwrap_or("")
                )
                .to_lowercase()
                .contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    }
}
