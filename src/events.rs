mod menu;
mod palette;
mod viewer;

use self::menu::handle_menu;
use self::palette::handle_command_palette;
use self::viewer::{
    handle_viewer, handle_viewer_goto_line, handle_viewer_menu, handle_viewer_plugin_palette,
    handle_viewer_searching,
};
use crate::app::{
    ActionPaletteState, App, AppMode, AssocEditorState, BookmarkListItem, CommandPaletteState,
    ConfigState, ConfirmAction, InputAction, InputDialog, MenuState, OpenerState, RemoteEditKind,
};
use crate::archive::supports_archive_navigation;
use crate::config::SortMode;
use crate::copy::CopyDialogState;
use crate::viewer::ViewMode;
use crate::remote::{
    download_to_temp, join_remote, load_profiles, make_dir as remote_make_dir,
    rename_path as remote_rename_path, upload_into_dir, RemoteKind, RemoteSource,
};
use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io;

pub fn handle_event(app: &mut App, event: Event) -> Result<bool> {
    let Event::Key(key) = event else {
        return Ok(false);
    };

    // Global shortcut available in every mode.
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
        && !key.modifiers.contains(KeyModifiers::SHIFT)
        && key.code == KeyCode::Char('b')
    {
        app.capture_gif = true;
        return Ok(false);
    }

    match &app.mode {
        AppMode::Help(_) => return handle_help(app, key),
        AppMode::Viewer(_) => return handle_viewer(app, key),
        AppMode::ViewerSearching(_) => return handle_viewer_searching(app, key),
        AppMode::ViewerGotoLine(_, _) => return handle_viewer_goto_line(app, key),
        AppMode::ViewerMenu(_, _) => return handle_viewer_menu(app, key),
        AppMode::ViewerPluginPalette(_, _) => return handle_viewer_plugin_palette(app, key),
        AppMode::Confirm(_) => return handle_confirm(app, key),
        AppMode::Input(_) => return handle_input(app, key),
        AppMode::CopyDialog(_) => return handle_copy_dialog(app, key),
        AppMode::CopyProgress(_) => return handle_copy_progress(app, key),
        AppMode::SearchPanel(_) => return handle_search(app, key),
        AppMode::DirBookmarks => return handle_dir_bookmarks(app, key),
        AppMode::QuickSearch => return handle_quicksearch(app, key),
        AppMode::Menu(_) => return handle_menu(app, key),
        AppMode::Config(_) => return handle_config(app, key),
        AppMode::Plugins(_) => return handle_plugins(app, key),
        AppMode::ActionPalette(_) => return handle_action_palette(app, key),
        AppMode::CommandPalette(_) => return handle_command_palette(app, key),
        AppMode::Opener(_) => return handle_opener(app, key),
        AppMode::AssocEditor(_) => return handle_assoc_editor(app, key),
        AppMode::RemoteConnect(_) => return handle_remote_connect(app, key),
        AppMode::RemoteEdit(_) => return handle_remote_edit(app, key),
        AppMode::RemoteAddMenu(_) => return handle_remote_add_menu(app, key),
        AppMode::RemoteConnecting(_) => return handle_remote_connecting(app, key),
        AppMode::Terminal => return crate::terminal::handle_terminal(app, key),
        AppMode::Browse => {}
    }

    handle_browse(app, key)
}

fn handle_copy_progress(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::F(10) => {
            app.cancel_copy_task();
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Browse mode
// ---------------------------------------------------------------------------

fn handle_browse(app: &mut App, key: KeyEvent) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    // Quick-preview panel focus mode: Up/Down scroll the viewer
    if app.quick_preview_active {
        match key.code {
            KeyCode::Tab | KeyCode::Esc => {
                app.quick_preview_active = false;
            }
            KeyCode::Up => {
                app.quick_preview_scroll_up();
            }
            KeyCode::Down => {
                app.quick_preview_scroll_down();
            }
            KeyCode::F(4) => {
                // Cycle forced mode: Auto → Text → Hex → Ansi → Image → Auto
                app.quick_preview_forced_mode = match app.quick_preview_forced_mode {
                    None => Some(ViewMode::Text),
                    Some(ViewMode::Text) => Some(ViewMode::Hex),
                    Some(ViewMode::Hex) => Some(ViewMode::Ansi),
                    Some(ViewMode::Ansi) => Some(ViewMode::Image),
                    Some(ViewMode::Image) => None,
                };
                if let Some(mode) = app.quick_preview_forced_mode {
                    if let Some(v) = app.quick_preview.as_mut() {
                        v.set_mode(mode);
                    }
                } else {
                    // Auto: re-open current file so it detects the mode naturally
                    app.refresh_quick_preview();
                }
            }
            _ => {}
        }
        return Ok(false);
    }

    // FileID panel focus mode: all navigation keys scroll the IDF card
    if app.file_id_active {
        match key.code {
            KeyCode::Tab | KeyCode::Esc => {
                app.file_id_active = false;
            }
            KeyCode::Up => {
                app.file_id_scroll_up();
            }
            KeyCode::Down => {
                app.file_id_scroll_down();
            }
            KeyCode::PageUp => {
                app.file_id_scroll_page_up(10);
            }
            KeyCode::PageDown => {
                app.file_id_scroll_page_down(10);
            }
            KeyCode::Home => {
                app.file_id_home();
            }
            _ => {}
        }
        return Ok(false);
    }

    // Printable char (no modifiers) → start quick-search
    if !ctrl && !alt && !shift {
        if let KeyCode::Char(ch) = key.code {
            if ch.is_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                let p = app.active_panel_mut();
                p.quicksearch.clear();
                p.qs_match_pos = 0;
                p.quicksearch_append(ch);
                let first = app.active_panel().quicksearch_matches().into_iter().next();
                if let Some(idx) = first {
                    app.active_panel_mut().cursor = idx;
                }
                app.mode = AppMode::QuickSearch;
                return Ok(false);
            }
        }
    }

    // Handle Ctrl-modified keys first
    if ctrl && !alt && !shift {
        match key.code {
            KeyCode::F(1) => {
                app.set_sort(SortMode::Name);
                return Ok(false);
            }
            KeyCode::F(2) => {
                app.set_sort(SortMode::Extension);
                return Ok(false);
            }
            KeyCode::F(3) => {
                app.set_sort(SortMode::Date);
                return Ok(false);
            }
            KeyCode::F(4) => {
                app.set_sort(SortMode::Size);
                return Ok(false);
            }
            KeyCode::F(5) => {
                app.set_sort(SortMode::Unsorted);
                return Ok(false);
            }
            KeyCode::Char('r') => {
                app.reload_panels();
                app.set_status("Reloaded");
                return Ok(false);
            }
            KeyCode::Char('h') => {
                let p = app.active_panel_mut();
                p.show_hidden = !p.show_hidden;
                let _ = p.reload();
                return Ok(false);
            }
            KeyCode::Char('d') => {
                app.open_dir_bookmarks();
                return Ok(false);
            }
            KeyCode::Char('f') => {
                app.open_remote_connect();
                return Ok(false);
            }
            KeyCode::Char('u') => {
                app.mode = AppMode::Terminal;
                return Ok(false);
            }
            KeyCode::Char('a') => {
                let cwd = app.active_panel().path.clone();
                let state = ActionPaletteState::load(cwd);
                if state.actions.is_empty() {
                    app.notify("No plugin action available");
                } else {
                    app.mode = AppMode::ActionPalette(state);
                }
                return Ok(false);
            }
            KeyCode::Char('p') => {
                app.mode = AppMode::CommandPalette(CommandPaletteState::default());
                return Ok(false);
            }
            KeyCode::Char('t') => {
                app.new_active_tab();
                return Ok(false);
            }
            KeyCode::Char('w') => {
                app.close_active_tab();
                return Ok(false);
            }
            KeyCode::Tab | KeyCode::Char('n') => {
                app.next_active_tab();
                return Ok(false);
            }
            _ => {}
        }
    }

    // Alt-modified keys
    if alt && !ctrl && !shift {
        match key.code {
            KeyCode::F(4) => {
                app.open_file_id_view();
                return Ok(false);
            }
            KeyCode::F(7) => {
                app.open_search();
                return Ok(false);
            }
            _ => {}
        }
    }

    // Shift-modified keys
    if shift && !ctrl && !alt {
        if let KeyCode::F(6) = key.code {
            start_rename(app);
            return Ok(false);
        }
    }

    // Unmodified keys
    match key.code {
        // --- Navigation ---
        KeyCode::Up => {
            app.active_panel_mut().move_up();
            app.refresh_quick_preview();
        }
        KeyCode::Down => {
            app.active_panel_mut().move_down();
            app.refresh_quick_preview();
        }
        KeyCode::PageUp => {
            app.active_panel_mut().move_page_up(20);
            app.refresh_quick_preview();
        }
        KeyCode::PageDown => {
            app.active_panel_mut().move_page_down(20);
            app.refresh_quick_preview();
        }
        KeyCode::Home => {
            app.active_panel_mut().move_home();
            app.refresh_quick_preview();
        }
        KeyCode::End => {
            app.active_panel_mut().move_end();
            app.refresh_quick_preview();
        }
        KeyCode::Tab => {
            if app.quick_preview.is_some() {
                app.quick_preview_active = true;
            } else if app.file_preview_info {
                app.file_id_active = true;
                app.file_id_scroll = 0;
            } else {
                app.switch_panel();
            }
        }
        KeyCode::Enter => {
            handle_enter(app)?;
        }
        KeyCode::Backspace => {
            app.go_parent()?;
        }

        // --- Selection ---
        KeyCode::Insert => {
            app.active_panel_mut().toggle_selected();
            if app.config.insert_moves_down {
                app.active_panel_mut().move_down();
            }
        }
        KeyCode::Char(' ') => {
            app.active_panel_mut().toggle_selected();
            app.active_panel_mut().move_down();
        }
        KeyCode::Char('+') => {
            app.mode = open_wildcard_dialog("Select pattern:", true);
        }
        KeyCode::Char('-') => {
            app.mode = open_wildcard_dialog("Deselect pattern:", false);
        }
        KeyCode::Char('*') => {
            app.active_panel_mut().invert_selection();
        }

        // --- F keys ---
        KeyCode::F(1) => {
            app.open_help();
        }
        KeyCode::F(2) => {
            let mut ms = MenuState::new();
            ms.open = false;
            app.mode = AppMode::Menu(ms);
        }
        KeyCode::F(3) => {
            app.open_viewer();
        }
        KeyCode::F(4) => {
            launch_editor(app)?;
        }
        KeyCode::F(5) => {
            app.open_copy_dialog();
        }
        KeyCode::F(6) => {
            app.cmd_move()?;
        }
        KeyCode::F(7) => {
            start_mkdir(app);
        }
        KeyCode::F(8) => {
            app.cmd_delete();
        }
        KeyCode::F(10) => {
            return confirm_quit(app);
        }
        KeyCode::Char('q') => {
            return confirm_quit(app);
        }

        KeyCode::Esc => {
            app.mode = AppMode::Terminal;
        }

        _ => {}
    }

    Ok(false)
}

fn handle_enter(app: &mut App) -> Result<()> {
    let entry = match app.active_panel().current_entry() {
        Some(e) => e.clone(),
        None => return Ok(()),
    };

    if entry.name == ".." {
        app.go_parent()?;
    } else if entry.is_dir {
        app.enter_dir(entry.path.clone())?;
    } else if crate::plugins::is_plugin_bundle(&entry.path) {
        let bundle_path = if app.active_panel().is_remote_view() {
            let Some(profile) = app.active_panel().remote_profile() else {
                app.notify("Remote profile missing");
                return Ok(());
            };
            match app.run_with_busy("Remote: downloading plugin...", |_| {
                download_to_temp(&profile, &entry.path.to_string_lossy(), false)
            }) {
                Ok(path) => path,
                Err(e) => {
                    app.notify(format!("Remote download failed: {}", e));
                    return Ok(());
                }
            }
        } else {
            entry.path.clone()
        };
        match crate::plugins::install_plugin_bundle(&bundle_path) {
            Ok(name) => {
                app.reload_panels();
                app.mode = AppMode::Confirm(crate::app::ConfirmDialog {
                    title: "Plugin installed".into(),
                    message: format!("Plugin installed: {}", name),
                    action: ConfirmAction::Message,
                });
            }
            Err(e) => app.notify(format!("Cannot install plugin: {}", e)),
        }
    } else if supports_archive_navigation(&entry.path) {
        if let Err(e) = app.enter_archive(entry.path.clone()) {
            app.notify(format!("Cannot enter archive: {}", e));
        }
    } else {
        let launch_path = if app.active_panel().is_remote_view() {
            let Some(profile) = app.active_panel().remote_profile() else {
                app.notify("Remote profile missing");
                return Ok(());
            };
            match app.run_with_busy("Remote: downloading file...", |_| {
                download_to_temp(&profile, &entry.path.to_string_lossy(), false)
            }) {
                Ok(path) => path,
                Err(e) => {
                    app.notify(format!("Remote download failed: {}", e));
                    return Ok(());
                }
            }
        } else {
            entry.path.clone()
        };
        // Check registered openers first
        let ext = entry
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let openers = app.config.openers_for(ext).to_vec();
        match openers.len() {
            0 => {
                // No association: fall back to system default
                if let Err(e) = open::that(&launch_path) {
                    app.notify(format!("Cannot open: {}", e));
                }
            }
            1 => {
                launch_external(app, &openers[0], &launch_path)?;
            }
            _ => {
                // Multiple openers: show picker
                app.mode = AppMode::Opener(OpenerState {
                    items: openers,
                    cursor: 0,
                    path: launch_path,
                });
            }
        }
    }
    Ok(())
}

/// Spawn an external command with the given file path.
/// `%f` in command is replaced by the path; otherwise path is appended.
fn launch_external(app: &mut App, command: &str, path: &std::path::Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    let args: Vec<String> = if command.contains("%f") {
        // Split on whitespace, replace %f token
        command
            .split_whitespace()
            .map(|t| {
                if t == "%f" {
                    path_str.to_string()
                } else {
                    t.to_string()
                }
            })
            .collect()
    } else {
        let mut v: Vec<String> = command.split_whitespace().map(|t| t.to_string()).collect();
        v.push(path_str.to_string());
        v
    };

    if args.is_empty() {
        app.notify("Empty opener command");
        return Ok(());
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    let _ = std::process::Command::new(&args[0])
        .args(&args[1..])
        .status();

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    app.needs_clear = true;
    app.reload_panels();
    Ok(())
}

fn confirm_quit(app: &mut App) -> Result<bool> {
    if app.config.confirm_exit {
        app.mode = AppMode::Confirm(crate::app::ConfirmDialog {
            title: "Quit KKC".into(),
            message: "Exit KKC?".into(),
            action: ConfirmAction::Quit,
        });
        Ok(false)
    } else {
        Ok(true)
    }
}

fn launch_editor(app: &mut App) -> Result<()> {
    if app.active_panel().is_archive_view() {
        app.notify("Editing in archive is not supported");
        return Ok(());
    }
    let entry = match app.active_panel().current_entry() {
        Some(e) if !e.is_dir && e.name != ".." => e.clone(),
        _ => return Ok(()),
    };

    let editor = app.config.editor.clone();
    let path = if app.active_panel().is_remote_view() {
        let Some(profile) = app.active_panel().remote_profile() else {
            app.notify("Remote profile missing");
            return Ok(());
        };
        match app.run_with_busy("Remote: downloading file...", |_| {
            download_to_temp(&profile, &entry.path.to_string_lossy(), false)
        }) {
            Ok(path) => path,
            Err(e) => {
                app.notify(format!("Remote download failed: {}", e));
                return Ok(());
            }
        }
    } else {
        entry.path.clone()
    };

    // Restore normal terminal before handing control to an external editor.
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    use std::process::Command;
    let _ = Command::new(&editor).arg(&path).status();

    if app.active_panel().is_remote_view()
        && let Some(profile) = app.active_panel().remote_profile()
        && let Some(parent) = entry.path.parent()
    {
        let remote_dir = parent.to_string_lossy().into_owned();
        if let Err(e) = app.run_with_busy("Remote: uploading file...", |_| {
            upload_into_dir(&profile, &path, &remote_dir, false).map(|_| ())
        }) {
            app.notify(format!("Remote upload failed: {}", e));
        }
    }

    // Re-enter TUI mode once the editor exits.
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;

    // Ratatui's buffer is stale after leaving/re-entering the alternate screen;
    // signal the main loop to call terminal.clear() before the next draw.
    app.needs_clear = true;
    app.reload_panels();
    Ok(())
}

fn start_rename(app: &mut App) {
    if app.active_panel().is_archive_view() {
        app.notify("Rename in archive is not supported");
        return;
    }
    if let Some(entry) = app.active_panel().current_entry() {
        if entry.name == ".." {
            return;
        }
        let path = entry.path.clone();
        let name = entry.name.clone();
        let action = if let Some(profile) = app.active_panel().remote_profile() {
            InputAction::RemoteRename {
                profile,
                path: path.to_string_lossy().into_owned(),
            }
        } else {
            InputAction::Rename(path)
        };
        app.mode = AppMode::Input(InputDialog {
            title: "Rename".into(),
            prompt: "New name:".into(),
            value: name.clone(),
            cursor: name.len(),
            action,
        });
    }
}

fn start_mkdir(app: &mut App) {
    if app.active_panel().is_archive_view() {
        app.notify("Create directory in archive is not supported");
        return;
    }
    let action = if let Some(profile) = app.active_panel().remote_profile() {
        InputAction::RemoteMkdir {
            profile,
            parent: app.active_panel().remote_cwd().unwrap_or("/").to_string(),
        }
    } else {
        InputAction::Mkdir(app.active_panel().path.clone())
    };
    app.mode = AppMode::Input(InputDialog {
        title: "Create Directory".into(),
        prompt: "Directory name:".into(),
        value: String::new(),
        cursor: 0,
        action,
    });
}

fn open_wildcard_dialog(prompt: &str, select: bool) -> AppMode {
    AppMode::Input(InputDialog {
        title: "Wildcard".into(),
        prompt: prompt.into(),
        value: "*".into(),
        cursor: 1,
        action: if select {
            InputAction::SelectPattern
        } else {
            InputAction::DeselectPattern
        },
    })
}

// ---------------------------------------------------------------------------
// QuickSearch palette mode (VSCode-style)
// ---------------------------------------------------------------------------

fn handle_quicksearch(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        // Confirm: jump to highlighted match, or enter directory
        KeyCode::Enter => {
            let entry_idx = {
                let p = app.active_panel();
                p.quicksearch_matches().get(p.qs_match_pos).copied()
            };
            app.active_panel_mut().quicksearch_clear();
            app.active_panel_mut().qs_match_pos = 0;
            if let Some(idx) = entry_idx {
                app.active_panel_mut().cursor = idx;
            }
            app.mode = AppMode::Browse;
            // If the selected entry is a directory, navigate into it
            if let Some(entry) = app.active_panel().current_entry().cloned() {
                if entry.name == ".." {
                    app.go_parent()?;
                } else if entry.is_dir {
                    app.enter_dir(entry.path.clone())?;
                }
            }
        }
        // Cancel: restore original cursor
        KeyCode::Esc => {
            app.active_panel_mut().quicksearch_clear();
            app.active_panel_mut().qs_match_pos = 0;
            app.mode = AppMode::Browse;
        }
        // Navigate UP in the filtered list
        KeyCode::Up => {
            let p = app.active_panel_mut();
            if p.qs_match_pos > 0 {
                p.qs_match_pos -= 1;
            }
            let entry_idx = app
                .active_panel()
                .quicksearch_matches()
                .get(app.active_panel().qs_match_pos)
                .copied();
            if let Some(idx) = entry_idx {
                app.active_panel_mut().cursor = idx;
            }
        }
        // Navigate DOWN in the filtered list
        KeyCode::Down => {
            let matches_len = app.active_panel().quicksearch_matches().len();
            let p = app.active_panel_mut();
            if p.qs_match_pos + 1 < matches_len {
                p.qs_match_pos += 1;
            }
            let entry_idx = app
                .active_panel()
                .quicksearch_matches()
                .get(app.active_panel().qs_match_pos)
                .copied();
            if let Some(idx) = entry_idx {
                app.active_panel_mut().cursor = idx;
            }
        }
        // Delete last char
        KeyCode::Backspace => {
            app.active_panel_mut().quicksearch_pop();
            app.active_panel_mut().qs_match_pos = 0;
            if app.active_panel().quicksearch.is_empty() {
                app.mode = AppMode::Browse;
            } else {
                let first = app.active_panel().quicksearch_matches().into_iter().next();
                if let Some(idx) = first {
                    app.active_panel_mut().cursor = idx;
                }
            }
        }
        // Append char to query
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_panel_mut().quicksearch_append(ch);
            app.active_panel_mut().qs_match_pos = 0;
            let first = app.active_panel().quicksearch_matches().into_iter().next();
            if let Some(idx) = first {
                app.active_panel_mut().cursor = idx;
            }
        }
        // Any other key: close palette and pass through
        _ => {
            app.active_panel_mut().quicksearch_clear();
            app.active_panel_mut().qs_match_pos = 0;
            app.mode = AppMode::Browse;
            return handle_browse(app, key);
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Confirm dialog
// ---------------------------------------------------------------------------

fn handle_confirm(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::Confirm(_) = &app.mode else {
        return Ok(false);
    };

    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            let AppMode::Confirm(dlg) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            match dlg.action {
                ConfirmAction::Message => {}
                ConfirmAction::MessageThen(next) => {
                    app.mode = *next;
                }
                ConfirmAction::Quit => return Ok(true),
                ConfirmAction::Delete(paths) => {
                    app.cmd_delete_confirmed(paths)?;
                }
                ConfirmAction::DeleteRemote(targets) => {
                    app.cmd_delete_remote_confirmed(targets)?;
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Input dialog
// ---------------------------------------------------------------------------

fn handle_input(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::Input(ref mut dlg) = app.mode else {
        return Ok(false);
    };

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Enter => {
            let value = dlg.value.clone();
            let action = dlg.action.clone();
            app.mode = AppMode::Browse;

            match action {
                InputAction::Rename(path) => match crate::file_ops::rename_entry(&path, &value) {
                    Ok(_) => {
                        app.notify(format!("Renamed to '{}'", value));
                        if app.config.auto_reload {
                            app.reload_panels();
                        }
                    }
                    Err(e) => app.notify(format!("Rename error: {}", e)),
                },
                InputAction::Mkdir(parent) => match crate::file_ops::make_dir(&parent, &value) {
                    Ok(_) => {
                        app.notify(format!("Created directory '{}'", value));
                        if app.config.auto_reload {
                            app.reload_panels();
                        }
                    }
                    Err(e) => app.notify(format!("mkdir error: {}", e)),
                },
                InputAction::RemoteRename { profile, path } => {
                    let Some(parent) = std::path::Path::new(&path).parent() else {
                        app.notify("Rename error: invalid remote path");
                        return Ok(false);
                    };
                    let dst = join_remote(&parent.to_string_lossy(), &value);
                    match app.run_with_busy("Remote: renaming...", |_| {
                        remote_rename_path(&profile, &path, &dst)
                    }) {
                        Ok(_) => {
                            app.notify(format!("Renamed to '{}'", value));
                            if app.config.auto_reload {
                                app.reload_panels();
                            }
                        }
                        Err(e) => app.notify(format!("Rename error: {}", e)),
                    }
                }
                InputAction::RemoteMkdir { profile, parent } => {
                    let path = join_remote(&parent, &value);
                    match app.run_with_busy("Remote: creating directory...", |_| {
                        remote_make_dir(&profile, &path)
                    }) {
                        Ok(_) => {
                            app.notify(format!("Created directory '{}'", value));
                            if app.config.auto_reload {
                                app.reload_panels();
                            }
                        }
                        Err(e) => app.notify(format!("mkdir error: {}", e)),
                    }
                }
                InputAction::SelectPattern => {
                    app.active_panel_mut().select_pattern(&value, true);
                }
                InputAction::DeselectPattern => {
                    app.active_panel_mut().select_pattern(&value, false);
                }
                InputAction::GoToPath => {
                    let path = std::path::PathBuf::from(&value);
                    if path.is_dir() {
                        app.enter_dir(path)?;
                    } else {
                        app.notify(format!("Not a directory: {}", value));
                    }
                }
                InputAction::AssocAddExt => {
                    let ext = value.trim().trim_start_matches('.').to_ascii_lowercase();
                    if ext.is_empty() {
                        app.mode = AppMode::AssocEditor(AssocEditorState::from_config(&app.config));
                    } else {
                        // Find existing openers for pre-fill
                        let existing = app.config.openers_for(&ext).join(", ");
                        app.mode = AppMode::Input(InputDialog {
                            title: "Association".into(),
                            prompt: format!("Openers for .{} (comma-separated):", ext),
                            value: existing,
                            cursor: 0,
                            action: InputAction::AssocAddOpeners {
                                ext,
                                edit_index: None,
                            },
                        });
                        // fix cursor to end
                        let AppMode::Input(ref mut dlg) = app.mode else {
                            return Ok(false);
                        };
                        dlg.cursor = dlg.value.len();
                    }
                }
                InputAction::AssocAddOpeners { ext, edit_index } => {
                    let openers: Vec<String> = value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if openers.is_empty() {
                        // Remove entry if openers cleared
                        if let Some(idx) = edit_index {
                            if idx < app.config.file_assoc.len() {
                                app.config.file_assoc.remove(idx);
                            }
                        }
                    } else {
                        match edit_index {
                            Some(idx) if idx < app.config.file_assoc.len() => {
                                app.config.file_assoc[idx].openers = openers;
                            }
                            _ => {
                                if let Some(existing) = app
                                    .config
                                    .file_assoc
                                    .iter_mut()
                                    .find(|a| a.ext.eq_ignore_ascii_case(&ext))
                                {
                                    existing.openers = openers;
                                } else {
                                    app.config
                                        .file_assoc
                                        .push(crate::config::FileAssoc { ext, openers });
                                }
                            }
                        }
                    }
                    app.save_config().ok();
                    app.mode = AppMode::AssocEditor(AssocEditorState::from_config(&app.config));
                }
                InputAction::PluginAction { plugin, id, cwd } => {
                    match app.run_with_busy("Running plugin action...", |_| {
                        crate::plugins::run_action(&plugin, &id, &cwd, Some(&value))
                    }) {
                        Ok(message) => app.notify(if message.trim().is_empty() {
                            "Action complete".to_string()
                        } else {
                            message
                        }),
                        Err(e) => app.notify(format!("Action error: {}", e)),
                    }
                    if app.config.auto_reload {
                        app.reload_panels();
                    }
                }
            }
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            let AppMode::Input(ref mut dlg) = app.mode else {
                return Ok(false);
            };
            dlg.insert_char(ch);
        }
        KeyCode::Backspace => {
            let AppMode::Input(ref mut dlg) = app.mode else {
                return Ok(false);
            };
            dlg.backspace();
        }
        KeyCode::Delete => {
            let AppMode::Input(ref mut dlg) = app.mode else {
                return Ok(false);
            };
            dlg.delete_char();
        }
        KeyCode::Left => {
            let AppMode::Input(ref mut dlg) = app.mode else {
                return Ok(false);
            };
            dlg.move_left();
        }
        KeyCode::Right => {
            let AppMode::Input(ref mut dlg) = app.mode else {
                return Ok(false);
            };
            dlg.move_right();
        }
        KeyCode::Home => {
            let AppMode::Input(ref mut dlg) = app.mode else {
                return Ok(false);
            };
            dlg.home();
        }
        KeyCode::End => {
            let AppMode::Input(ref mut dlg) = app.mode else {
                return Ok(false);
            };
            dlg.end();
        }
        _ => {}
    }
    Ok(false)
}

fn handle_copy_dialog(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::CopyDialog(ref mut dlg) = app.mode else {
        return Ok(false);
    };

    if dlg.waiting_to_start {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                app.cancel_copy_scan();
                app.mode = AppMode::Browse;
                app.set_status("Copy aborted");
            }
            _ => {}
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => {
            app.cancel_copy_scan();
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            dlg.field = dlg.field.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Tab => {
            dlg.field = (dlg.field + 1).min(CopyDialogState::CANCEL);
        }
        KeyCode::BackTab => {
            dlg.field = dlg.field.saturating_sub(1);
        }
        KeyCode::Left => {
            if dlg.field == CopyDialogState::DESTINATION && dlg.cursor > 0 {
                dlg.cursor -= 1;
            }
        }
        KeyCode::Right => {
            if dlg.field == CopyDialogState::DESTINATION {
                dlg.cursor = (dlg.cursor + 1).min(dlg.destination.len());
            }
        }
        KeyCode::Backspace => {
            if dlg.field == CopyDialogState::DESTINATION && dlg.cursor > 0 {
                dlg.destination.remove(dlg.cursor - 1);
                dlg.cursor -= 1;
            }
        }
        KeyCode::Delete => {
            if dlg.field == CopyDialogState::DESTINATION && dlg.cursor < dlg.destination.len() {
                dlg.destination.remove(dlg.cursor);
            }
        }
        KeyCode::Char(' ') => match dlg.field {
            CopyDialogState::OVERWRITE => dlg.overwrite = !dlg.overwrite,
            CopyDialogState::NEWER_ONLY => dlg.newer_only = !dlg.newer_only,
            CopyDialogState::KEEP_ATTRIBUTES => dlg.keep_attributes = !dlg.keep_attributes,
            _ => {}
        },
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if dlg.field == CopyDialogState::DESTINATION {
                dlg.destination.insert(dlg.cursor, ch);
                dlg.cursor += ch.len_utf8();
            }
        }
        KeyCode::Enter => match dlg.field {
            CopyDialogState::OVERWRITE => dlg.overwrite = !dlg.overwrite,
            CopyDialogState::NEWER_ONLY => dlg.newer_only = !dlg.newer_only,
            CopyDialogState::KEEP_ATTRIBUTES => dlg.keep_attributes = !dlg.keep_attributes,
            CopyDialogState::CANCEL => app.mode = AppMode::Browse,
            _ => {
                if dlg.stats_pending {
                    dlg.waiting_to_start = true;
                } else {
                    let state = dlg.clone();
                    app.mode = AppMode::Browse;
                    app.execute_copy_dialog(state)?;
                }
            }
        },
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Search panel
// ---------------------------------------------------------------------------

fn handle_search(app: &mut App, key: KeyEvent) -> Result<bool> {
    let page_size = 10usize;
    match key.code {
        KeyCode::Esc | KeyCode::F(10) => {
            // If a search is running, cancel it then close the panel
            app.cancel_search();
            app.mode = AppMode::Browse;
        }
        KeyCode::Tab => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            // Cycle input fields: 0→1→2→0; ↓ from any input field enters results
            s.input_field = (s.input_field + 1) % 3;
        }
        KeyCode::BackTab => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                s.input_field = 2;
            } else {
                s.input_field = (s.input_field + 2) % 3;
            }
        }
        KeyCode::F(5) => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            s.backend = s.backend.next_available();
        }
        KeyCode::Enter => {
            let input_field = if let AppMode::SearchPanel(ref s) = app.mode {
                s.input_field
            } else {
                return Ok(false);
            };
            if input_field == 3 {
                // Navigate to selected file
                let selected = if let AppMode::SearchPanel(ref s) = app.mode {
                    s.results.get(s.cursor).map(|r| r.path.clone())
                } else {
                    None
                };
                if let Some(path) = selected {
                    app.mode = AppMode::Browse;
                    if let Some(dir) = path.parent() {
                        app.enter_dir(dir.to_path_buf())?;
                        let file_name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        app.active_panel_mut().cursor = app
                            .active_panel()
                            .entries
                            .iter()
                            .position(|e| e.name == file_name)
                            .unwrap_or(0);
                    }
                }
            } else {
                app.run_search();
                // Auto-focus results if any
                if let AppMode::SearchPanel(ref mut s) = app.mode {
                    if !s.results.is_empty() {
                        s.input_field = 3;
                    }
                }
            }
        }
        KeyCode::Up => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                if s.cursor > 0 {
                    s.cursor -= 1;
                    if s.cursor < s.scroll {
                        s.scroll = s.cursor;
                    }
                } else {
                    // Leave results focus back to inputs
                    s.input_field = 2;
                }
            } else {
                s.input_field = (s.input_field + 2) % 3;
            }
        }
        KeyCode::Down => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                if s.cursor + 1 < s.results.len() {
                    s.cursor += 1;
                }
            } else if !s.results.is_empty() {
                s.input_field = 3;
                s.cursor = 0;
                s.scroll = 0;
            } else {
                s.input_field = (s.input_field + 1) % 3;
            }
        }
        KeyCode::PageUp => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                s.cursor = s.cursor.saturating_sub(page_size);
                if s.cursor < s.scroll {
                    s.scroll = s.cursor;
                }
            }
        }
        KeyCode::PageDown => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                let max = s.results.len().saturating_sub(1);
                s.cursor = (s.cursor + page_size).min(max);
            }
        }
        KeyCode::Home => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                s.cursor = 0;
                s.scroll = 0;
            }
        }
        KeyCode::End => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                s.cursor = s.results.len().saturating_sub(1);
            }
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            match s.input_field {
                0 => s.query.push(ch),
                1 => s.content_query.push(ch),
                2 => s.dir_query.push(ch),
                _ => {}
            }
        }
        KeyCode::Backspace => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            match s.input_field {
                0 => {
                    s.query.pop();
                }
                1 => {
                    s.content_query.pop();
                }
                2 => {
                    s.dir_query.pop();
                }
                _ => {}
            }
        }
        KeyCode::Delete => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            // Clear the active input field
            match s.input_field {
                0 => {
                    s.query.clear();
                    s.query.push('*');
                }
                1 => s.content_query.clear(),
                2 => s.dir_query = s.start_dir.to_string_lossy().into_owned(),
                _ => {}
            }
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Dir history
// ---------------------------------------------------------------------------

fn handle_dir_bookmarks(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            app.move_prev_bookmark();
        }
        KeyCode::Down => {
            app.move_next_bookmark();
        }
        KeyCode::Backspace => {
            app.pop_bookmark_query();
        }
        KeyCode::Enter => {
            let selected = app
                .filtered_bookmark_items()
                .get(app.bookmark_match_pos)
                .cloned();
            match selected {
                Some(BookmarkListItem::AddCurrentDir(_)) => {
                    app.add_current_dir_bookmark();
                }
                Some(BookmarkListItem::Existing(idx)) => {
                    let path = app.bookmarks.get(idx).cloned();
                    app.mode = AppMode::Browse;
                    if let Some(p) = path {
                        let s = p.to_string_lossy();
                        if let Some(rest) = s.strip_prefix("remote://") {
                            let (profile_name, cwd) = rest.split_once('/').unwrap_or((rest, ""));
                            let target_cwd = format!("/{}", cwd);
                            let profiles = load_profiles().unwrap_or_default();
                            if let Some(profile) =
                                profiles.into_iter().find(|pr| pr.name == profile_name)
                            {
                                app.start_remote_connect_with_cwd(profile, target_cwd);
                            }
                        } else if p.is_dir() {
                            app.enter_dir(p)?;
                        }
                    }
                }
                None => app.mode = AppMode::Browse,
            }
        }
        KeyCode::Delete => {
            if let Some(BookmarkListItem::Existing(idx)) = app
                .filtered_bookmark_items()
                .get(app.bookmark_match_pos)
                .cloned()
            {
                if idx < app.bookmarks.len() {
                    app.bookmarks.remove(idx);
                    if app.bookmark_cursor >= app.bookmarks.len() && app.bookmark_cursor > 0 {
                        app.bookmark_cursor -= 1;
                    }
                    app.sync_bookmark_cursor();
                }
            }
        }
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !ch.is_control() =>
        {
            app.append_bookmark_query(ch);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_plugins(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::F(10) => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            if let AppMode::Plugins(ref mut s) = app.mode {
                s.cursor = s.cursor.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if let AppMode::Plugins(ref mut s) = app.mode {
                let max = s.plugins.len().saturating_sub(1);
                s.cursor = (s.cursor + 1).min(max);
            }
        }
        KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('O') => {
            let dir = if let AppMode::Plugins(ref s) = app.mode {
                // Use the selected plugin's own directory; fall back to the global plugins_dir
                s.plugins
                    .get(s.cursor)
                    .map(|p| p.dir.clone())
                    .filter(|d| !d.as_os_str().is_empty())
                    .unwrap_or_else(|| s.plugins_dir.clone())
            } else {
                return Ok(false);
            };
            if dir.as_os_str().is_empty() {
                app.set_status("Plugin directory unavailable");
            } else {
                app.mode = AppMode::Browse;
                if let Err(e) = app.enter_dir(dir.clone()) {
                    app.set_status(format!("Cannot enter plugin directory: {}", e));
                } else {
                    app.set_status(format!("Plugin directory: {}", dir.display()));
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_action_palette(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::F(10) => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            if let AppMode::ActionPalette(ref mut state) = app.mode {
                state.cursor = state.cursor.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if let AppMode::ActionPalette(ref mut state) = app.mode {
                let max = state.actions.len().saturating_sub(1);
                state.cursor = (state.cursor + 1).min(max);
            }
        }
        KeyCode::Enter => {
            let (action, cwd) = if let AppMode::ActionPalette(ref state) = app.mode {
                let Some(action) = state.actions.get(state.cursor).cloned() else {
                    app.mode = AppMode::Browse;
                    return Ok(false);
                };
                (action, state.cwd.clone())
            } else {
                return Ok(false);
            };

            app.mode = AppMode::Browse;
            if let Some(prompt) = action.prompt.clone() {
                app.mode = AppMode::Input(InputDialog {
                    title: action.title,
                    prompt,
                    value: String::new(),
                    cursor: 0,
                    action: InputAction::PluginAction {
                        plugin: action.plugin,
                        id: action.id,
                        cwd,
                    },
                });
            } else {
                match app.run_with_busy("Running plugin action...", |_| {
                    crate::plugins::run_action(&action.plugin, &action.id, &cwd, None)
                }) {
                    Ok(message) => app.notify(if message.trim().is_empty() {
                        "Action complete".to_string()
                    } else {
                        message
                    }),
                    Err(e) => app.notify(format!("Action error: {}", e)),
                }
                if app.config.auto_reload {
                    app.reload_panels();
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Config screen
// ---------------------------------------------------------------------------

fn handle_config(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::Config(ref mut cs) = app.mode else {
        return Ok(false);
    };

    let total = ConfigState::NUM_TOTAL; // 9 booleans + 3 text + OK + Cancel
    let n_bool = ConfigState::NUM_CHECKBOXES; // 9
    let n_text = 3;
    let ok_idx = n_bool + n_text; // 11
    let cancel_idx = n_bool + n_text + 1; // 12

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }

        // Navigate rows
        KeyCode::Up | KeyCode::BackTab => {
            if let AppMode::Config(ref mut cs) = app.mode {
                if cs.cursor > 0 {
                    cs.cursor -= 1;
                }
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            if let AppMode::Config(ref mut cs) = app.mode {
                if cs.cursor + 1 < total {
                    cs.cursor += 1;
                }
            }
        }

        // Toggle checkbox or activate button
        KeyCode::Char(' ') | KeyCode::Enter => {
            let cursor = cs.cursor;
            match cursor {
                0 => cs.confirm_exit = !cs.confirm_exit,
                1 => cs.confirm_delete = !cs.confirm_delete,
                2 => cs.auto_reload = !cs.auto_reload,
                3 => cs.insert_moves_down = !cs.insert_moves_down,
                4 => cs.select_dirs = !cs.select_dirs,
                5 => cs.show_hidden = !cs.show_hidden,
                6 => cs.color_by_type = !cs.color_by_type,
                7 => cs.show_fkey_bar = !cs.show_fkey_bar,
                8 => cs.debug_log = !cs.debug_log,
                // text fields: Enter moves focus to next
                9 | 10 | 11 => {
                    if let AppMode::Config(ref mut cs) = app.mode {
                        if cs.cursor + 1 < total {
                            cs.cursor += 1;
                        }
                    }
                }
                c if c == ok_idx => {
                    // Apply & save
                    let cs_clone = cs.clone();
                    app.mode = AppMode::Browse;
                    cs_clone.apply_to(&mut app.config);
                    // Sync hidden flag on live panels
                    app.left.show_hidden = app.config.left.show_hidden;
                    app.right.show_hidden = app.config.right.show_hidden;
                    // Apply debug-log toggle immediately
                    crate::viewer::set_debug_log_enabled(app.config.debug_log);
                    let _ = app.left.reload();
                    let _ = app.right.reload();
                    match app.save_config() {
                        Ok(_) => {
                            if app.config.debug_log {
                                let log_path = crate::viewer::debug_log_path()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_else(|| "?".into());
                                app.set_status(format!("Config saved — debug log: {}", log_path));
                            } else {
                                app.set_status("Config saved");
                            }
                        }
                        Err(e) => app.set_status(format!("Save error: {}", e)),
                    }
                }
                c if c == cancel_idx => {
                    app.mode = AppMode::Browse;
                }
                _ => {}
            }
        }

        // Text field editing
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let AppMode::Config(ref mut cs) = app.mode {
                match cs.cursor {
                    9 => cs.editor.push(ch),
                    10 => cs.pager.push(ch),
                    11 => cs.dir_history_max.push(ch),
                    _ => {}
                }
            }
        }
        KeyCode::Backspace => {
            if let AppMode::Config(ref mut cs) = app.mode {
                match cs.cursor {
                    9 => {
                        cs.editor.pop();
                    }
                    10 => {
                        cs.pager.pop();
                    }
                    11 => {
                        cs.dir_history_max.pop();
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Opener picker
// ---------------------------------------------------------------------------

fn handle_opener(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::Opener(ref mut s) = app.mode else {
        return Ok(false);
    };
    let total = s.items.len();

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up | KeyCode::BackTab => {
            if let AppMode::Opener(ref mut s) = app.mode {
                if s.cursor > 0 {
                    s.cursor -= 1;
                }
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            if let AppMode::Opener(ref mut s) = app.mode {
                if s.cursor + 1 < total {
                    s.cursor += 1;
                }
            }
        }
        KeyCode::Enter => {
            let (cmd, path) = if let AppMode::Opener(s) = &app.mode {
                (s.items[s.cursor].clone(), s.path.clone())
            } else {
                return Ok(false);
            };
            app.mode = AppMode::Browse;
            launch_external(app, &cmd, &path)?;
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Association editor
// ---------------------------------------------------------------------------

fn handle_assoc_editor(app: &mut App, key: KeyEvent) -> Result<bool> {
    let total = if let AppMode::AssocEditor(ref s) = app.mode {
        s.assocs.len()
    } else {
        0
    };

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            if let AppMode::AssocEditor(ref mut s) = app.mode {
                if s.cursor > 0 {
                    s.cursor -= 1;
                }
            }
        }
        KeyCode::Down => {
            if let AppMode::AssocEditor(ref mut s) = app.mode {
                if s.cursor + 1 < total {
                    s.cursor += 1;
                }
            }
        }
        // Add new association
        KeyCode::Char('a') | KeyCode::Char('A') | KeyCode::Char('+') | KeyCode::F(1) => {
            app.mode = AppMode::Input(InputDialog {
                title: "New association".into(),
                prompt: "Extension (without dot):".into(),
                value: String::new(),
                cursor: 0,
                action: InputAction::AssocAddExt,
            });
        }
        // Edit selected
        KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char('E') => {
            let (ext, openers_str, idx) = if let AppMode::AssocEditor(ref s) = app.mode {
                if s.assocs.is_empty() {
                    return Ok(false);
                }
                let (ext, openers) = &s.assocs[s.cursor];
                (ext.clone(), openers.join(", "), s.cursor)
            } else {
                return Ok(false);
            };
            app.mode = AppMode::Input(InputDialog {
                title: "Edit association".into(),
                prompt: format!("Openers for .{} (comma-separated):", ext),
                value: openers_str.clone(),
                cursor: openers_str.len(),
                action: InputAction::AssocAddOpeners {
                    ext,
                    edit_index: Some(idx),
                },
            });
        }
        // Delete selected
        KeyCode::Delete | KeyCode::Char('d') | KeyCode::Char('D') => {
            let (idx, cursor) = if let AppMode::AssocEditor(ref s) = app.mode {
                if s.assocs.is_empty() {
                    return Ok(false);
                }
                (s.cursor, s.cursor)
            } else {
                return Ok(false);
            };
            if idx < app.config.file_assoc.len() {
                app.config.file_assoc.remove(idx);
            }
            app.save_config().ok();
            let new_cursor = if cursor > 0 && cursor >= app.config.file_assoc.len() {
                app.config.file_assoc.len().saturating_sub(1)
            } else {
                cursor
            };
            let mut new_s = AssocEditorState::from_config(&app.config);
            new_s.cursor = new_cursor;
            app.mode = AppMode::AssocEditor(new_s);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_remote_connect(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => app.mode = AppMode::Browse,
        KeyCode::Tab => {
            launch_ssh_for_profile(app)?;
        }
        KeyCode::Up => {
            if let AppMode::RemoteConnect(ref mut s) = app.mode {
                s.move_prev();
            }
        }
        KeyCode::Down => {
            if let AppMode::RemoteConnect(ref mut s) = app.mode {
                s.move_next();
            }
        }
        KeyCode::Backspace => {
            if let AppMode::RemoteConnect(ref mut s) = app.mode {
                s.pop_query();
            }
        }
        KeyCode::F(6) => {
            app.open_remote_edit();
        }
        KeyCode::F(7) => {
            app.open_remote_add_menu();
        }
        KeyCode::Enter => {
            let profile = if let AppMode::RemoteConnect(ref s) = app.mode {
                s.filtered_indices()
                    .get(s.match_pos)
                    .and_then(|idx| s.items.get(*idx))
                    .cloned()
            } else {
                None
            };
            if let Some(profile) = profile {
                let return_state = if let AppMode::RemoteConnect(ref s) = app.mode {
                    s.clone()
                } else {
                    crate::app::RemoteConnectState::load()
                };
                app.start_remote_connect(profile, return_state);
            }
        }
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !ch.is_control() =>
        {
            if let AppMode::RemoteConnect(ref mut s) = app.mode {
                s.append_query(ch);
            }
        }
        _ => {}
    }
    Ok(false)
}

fn launch_ssh_for_profile(app: &mut App) -> Result<()> {
    let profile = if let AppMode::RemoteConnect(ref s) = app.mode {
        s.filtered_indices()
            .get(s.match_pos)
            .and_then(|idx| s.items.get(*idx))
            .cloned()
    } else {
        None
    };
    let Some(profile) = profile else {
        return Ok(());
    };
    let sftp = match &profile.kind {
        RemoteKind::Sftp(sftp) => sftp.clone(),
        _ => return Ok(()),
    };
    let mut args: Vec<String> = vec!["ssh".to_string()];
    match profile.source {
        RemoteSource::SshConfig => {
            args.push(profile.name.clone());
        }
        RemoteSource::UserToml => {
            if let Some(ref identity) = sftp.identity_file {
                args.push("-i".to_string());
                args.push(identity.clone());
            }
            if let Some(port) = sftp.port {
                args.push("-p".to_string());
                args.push(port.to_string());
            }
            let host = sftp.host.clone().unwrap_or_else(|| profile.name.clone());
            let target = if let Some(ref user) = sftp.user {
                format!("{}@{}", user, host)
            } else {
                host
            };
            args.push(target);
        }
    }
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    let _ = std::process::Command::new(&args[0]).args(&args[1..]).status();
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    app.needs_clear = true;
    Ok(())
}

fn handle_remote_add_menu(app: &mut App, key: KeyEvent) -> Result<bool> {
    let choices = RemoteEditKind::all();
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::RemoteConnect(crate::app::RemoteConnectState::load());
        }
        KeyCode::Up => {
            if let AppMode::RemoteAddMenu(ref mut c) = app.mode {
                if *c > 0 {
                    *c -= 1;
                }
            }
        }
        KeyCode::Down => {
            if let AppMode::RemoteAddMenu(ref mut c) = app.mode {
                if *c + 1 < choices.len() {
                    *c += 1;
                }
            }
        }
        KeyCode::Enter => {
            if let AppMode::RemoteAddMenu(cursor) = app.mode {
                if let Some(&kind) = choices.get(cursor) {
                    app.mode = AppMode::RemoteEdit(crate::app::RemoteEditState::new(kind));
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_remote_connecting(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::F(10) => app.cancel_remote_connect(),
        _ => {}
    }
    Ok(false)
}

fn handle_remote_edit(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::RemoteEdit(ref mut s) = app.mode else {
        return Ok(false);
    };

    // ── Share picker navigation (intercepts all keys when open) ──────────
    if s.share_picker.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::F(5) => {
                s.share_picker = None;
            }
            KeyCode::Up => {
                if let Some((_, ref mut cur)) = s.share_picker {
                    *cur = cur.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some((ref shares, ref mut cur)) = s.share_picker {
                    *cur = (*cur + 1).min(shares.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                if let Some((ref shares, cur)) = s.share_picker {
                    s.fields[crate::app::RemoteEditState::PATH] = shares[cur].clone();
                    s.input_cursor = s.fields[crate::app::RemoteEditState::PATH].len();
                }
                s.share_picker = None;
                // Move cursor to Password field after selecting share
                s.cursor = crate::app::RemoteEditState::SECRET;
                s.sync_cursor();
            }
            _ => {}
        }
        return Ok(false);
    }

    // ── F5: fetch SMB share list ──────────────────────────────────────────
    if key.code == KeyCode::F(5)
        && matches!(s.kind, crate::app::RemoteEditKind::Smb)
        && s.cursor == crate::app::RemoteEditState::PATH
    {
        let host = s.fields[crate::app::RemoteEditState::HOST].trim().to_string();
        if host.is_empty() {
            app.set_status("Enter host first");
            return Ok(false);
        }
        let user = s.fields[crate::app::RemoteEditState::USER].trim();
        let workgroup = s.fields[crate::app::RemoteEditState::PORT].trim();
        let password = s.fields[crate::app::RemoteEditState::SECRET].trim();
        let smb = crate::remote::SmbProfile {
            host: host.clone(),
            user: if user.is_empty() { None } else { Some(user.to_string()) },
            workgroup: if workgroup.is_empty() { None } else { Some(workgroup.to_string()) },
            password: if password.is_empty() { None } else { Some(password.to_string()) },
            share: None,
            path: None,
        };
        let profile = crate::remote::RemoteProfile {
            name: "tmp".into(),
            source: crate::remote::RemoteSource::UserToml,
            kind: crate::remote::RemoteKind::Smb(smb),
        };
        match crate::remote::list_smb_shares(&profile) {
            Ok(shares) => {
                let current = s.fields[crate::app::RemoteEditState::PATH].trim().to_lowercase();
                let cur = shares
                    .iter()
                    .position(|sh| sh.to_lowercase() == current)
                    .unwrap_or(0);
                s.share_picker = Some((shares, cur));
            }
            Err(e) => {
                app.set_status(format!("Share list: {}", e));
            }
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => app.mode = AppMode::RemoteConnect(crate::app::RemoteConnectState::load()),
        KeyCode::Tab | KeyCode::Down => {
            s.cursor = (s.cursor + 1).min(crate::app::RemoteEditState::CANCEL);
            s.sync_cursor();
        }
        KeyCode::BackTab | KeyCode::Up => {
            s.cursor = s.cursor.saturating_sub(1);
            s.sync_cursor();
        }
        KeyCode::Left => {
            if s.cursor < 6 && s.input_cursor > 0 {
                s.input_cursor -= 1;
            }
        }
        KeyCode::Right => {
            if s.cursor < 6 {
                s.input_cursor =
                    (s.input_cursor + 1).min(s.current_value().map(|v| v.len()).unwrap_or(0));
            }
        }
        KeyCode::Backspace => {
            let pos = s.input_cursor;
            if s.cursor < 6 && pos > 0 {
                if let Some(value) = s.current_value_mut() {
                    value.remove(pos - 1);
                }
                s.input_cursor -= 1;
            }
        }
        KeyCode::Delete => {
            if s.cursor < 6 {
                let pos = s.input_cursor;
                if let Some(value) = s.current_value_mut()
                    && pos < value.len()
                {
                    value.remove(pos);
                }
            }
        }
        KeyCode::Char(ch) => {
            if s.cursor < 6
                && !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
            {
                let pos = s.input_cursor;
                if let Some(value) = s.current_value_mut() {
                    value.insert(pos, ch);
                    s.input_cursor += ch.len_utf8();
                }
            }
        }
        KeyCode::Enter => {
            if s.cursor == crate::app::RemoteEditState::CANCEL {
                app.mode = AppMode::RemoteConnect(crate::app::RemoteConnectState::load());
            } else if s.cursor == crate::app::RemoteEditState::SAVE {
                if let Some(profile) = s.build_profile() {
                    let old_name = s.edit_original_name.clone();
                    match app.save_remote_profile(profile, old_name) {
                        Ok(()) => {
                            app.mode =
                                AppMode::RemoteConnect(crate::app::RemoteConnectState::load())
                        }
                        Err(e) => app.set_status(format!("Cannot save connection: {}", e)),
                    }
                } else {
                    let msg = s.kind.validation_message().to_string();
                    app.set_status(msg);
                }
            } else {
                s.cursor = (s.cursor + 1).min(crate::app::RemoteEditState::CANCEL);
                s.sync_cursor();
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_help(app: &mut App, key: KeyEvent) -> Result<bool> {
    use crate::help::HelpView;

    let Some(state) = (match &mut app.mode {
        AppMode::Help(state) => Some(state),
        _ => None,
    }) else {
        return Ok(false);
    };

    match state.view {
        HelpView::Index { ref mut cursor } => match key.code {
            KeyCode::Esc | KeyCode::F(10) => app.mode = AppMode::Browse,
            KeyCode::Up => *cursor = cursor.saturating_sub(1),
            KeyCode::Down => {
                let max = state.system.sections.len().saturating_sub(1);
                *cursor = (*cursor + 1).min(max);
            }
            KeyCode::Home => *cursor = 0,
            KeyCode::End => *cursor = state.system.sections.len().saturating_sub(1),
            KeyCode::Enter => {
                let section = *cursor;
                let prev = state.view;
                state.history.push(prev);
                state.view = HelpView::Topics { section, cursor: 0 };
            }
            _ => {}
        },
        HelpView::Topics {
            section,
            ref mut cursor,
        } => match key.code {
            KeyCode::Esc => {
                if !state.back() {
                    app.mode = AppMode::Browse;
                }
            }
            KeyCode::Backspace => {
                let _ = state.back();
            }
            KeyCode::Up => *cursor = cursor.saturating_sub(1),
            KeyCode::Down => {
                let max = state.system.sections[section]
                    .topics
                    .len()
                    .saturating_sub(1);
                *cursor = (*cursor + 1).min(max);
            }
            KeyCode::Home => *cursor = 0,
            KeyCode::End => {
                *cursor = state.system.sections[section]
                    .topics
                    .len()
                    .saturating_sub(1)
            }
            KeyCode::Enter => {
                let topic = state.system.sections[section].topics[*cursor];
                let prev = state.view;
                state.history.push(prev);
                state.view = HelpView::Page {
                    topic,
                    scroll: 0,
                    selected_link: 0,
                };
            }
            _ => {}
        },
        HelpView::Page {
            topic,
            ref mut scroll,
            ref mut selected_link,
        } => match key.code {
            KeyCode::Esc => {
                if !state.back() {
                    app.mode = AppMode::Browse;
                }
            }
            KeyCode::Backspace => {
                let _ = state.back();
            }
            KeyCode::Up => *scroll = scroll.saturating_sub(1),
            KeyCode::Down => *scroll = scroll.saturating_add(1),
            KeyCode::PageUp => *scroll = scroll.saturating_sub(12),
            KeyCode::PageDown => *scroll = scroll.saturating_add(12),
            KeyCode::Home => *scroll = 0,
            KeyCode::Tab => {
                let count = state.system.topics[topic].link_count();
                if count > 0 {
                    *selected_link = (*selected_link + 1) % count;
                }
            }
            KeyCode::BackTab => {
                let count = state.system.topics[topic].link_count();
                if count > 0 {
                    *selected_link = if *selected_link == 0 {
                        count - 1
                    } else {
                        *selected_link - 1
                    };
                }
            }
            KeyCode::Enter => {
                let target = state.system.topics[topic]
                    .selected_link_target(*selected_link)
                    .map(str::to_string);
                if let Some(target) = target {
                    if !state.open_topic_by_name(&target) {
                        app.set_status(format!("Unknown help topic: {}", target));
                    }
                }
            }
            _ => {}
        },
    }

    Ok(false)
}
