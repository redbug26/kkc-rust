use crate::archive::supports_archive_navigation;
use crate::app::{
    App, AppMode, AssocEditorState, BookmarkListItem, ConfigState, ConfirmAction, InputAction, InputDialog,
    MenuAction, MenuState, OpenerState, RemoteEditKind,
    ViewerMenuKind, ViewerMenuState,
    MENU_DATA, MENU_HEADERS,
};
use crate::copy::CopyDialogState;
use crate::config::SortMode;
use crate::remote::{
    download_to_temp, join_remote, load_profiles, make_dir as remote_make_dir, rename_path as remote_rename_path,
    upload_into_dir,
};
use crate::viewer::{EncodingMode, LineFeedMode, MaskKind, PreprocOpKind, ViewMode};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use anyhow::Result;
use std::io;

pub fn handle_event(app: &mut App, event: Event) -> Result<bool> {
    let Event::Key(key) = event else {
        return Ok(false);
    };

    match &app.mode {
        AppMode::Help(_) => return handle_help(app, key),
        AppMode::Viewer(_) => return handle_viewer(app, key),
        AppMode::ViewerSearching(_) => return handle_viewer_searching(app, key),
        AppMode::ViewerMenu(_, _) => return handle_viewer_menu(app, key),
        AppMode::Confirm(_) => return handle_confirm(app, key),
        AppMode::Input(_) => return handle_input(app, key),
        AppMode::CopyDialog(_) => return handle_copy_dialog(app, key),
        AppMode::CopyProgress(_) => return handle_copy_progress(app, key),
        AppMode::SearchPanel(_) => return handle_search(app, key),
        AppMode::DirBookmarks => return handle_dir_bookmarks(app, key),
        AppMode::QuickSearch => return handle_quicksearch(app, key),
        AppMode::Menu(_) => return handle_menu(app, key),
        AppMode::Config(_) => return handle_config(app, key),
        AppMode::Opener(_) => return handle_opener(app, key),
        AppMode::AssocEditor(_) => return handle_assoc_editor(app, key),
        AppMode::RemoteConnect(_) => return handle_remote_connect(app, key),
        AppMode::RemoteEdit(_) => return handle_remote_edit(app, key),
        AppMode::RemoteConnecting(_) => return handle_remote_connecting(app, key),
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
            KeyCode::F(1) => { app.set_sort(SortMode::Name); return Ok(false); }
            KeyCode::F(2) => { app.set_sort(SortMode::Extension); return Ok(false); }
            KeyCode::F(3) => { app.set_sort(SortMode::Date); return Ok(false); }
            KeyCode::F(4) => { app.set_sort(SortMode::Size); return Ok(false); }
            KeyCode::F(5) => { app.set_sort(SortMode::Unsorted); return Ok(false); }
            KeyCode::Char('r') => {
                app.reload_panels();
                app.status.text = "Reloaded".into();
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
        }
        KeyCode::Down => {
            app.active_panel_mut().move_down();
        }
        KeyCode::PageUp => {
            app.active_panel_mut().move_page_up(20);
        }
        KeyCode::PageDown => {
            app.active_panel_mut().move_page_down(20);
        }
        KeyCode::Home => {
            app.active_panel_mut().move_home();
        }
        KeyCode::End => {
            app.active_panel_mut().move_end();
        }
        KeyCode::Tab => {
            app.switch_panel();
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
            app.status.text.clear();
            app.active_panel_mut().deselect_all();
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
    } else if supports_archive_navigation(&entry.path) {
        if let Err(e) = app.enter_archive(entry.path.clone()) {
            app.status.text = format!("Cannot enter archive: {}", e);
        }
    } else {
        let launch_path = if app.active_panel().is_remote_view() {
            let Some(profile) = app.active_panel().remote_profile() else {
                app.status.text = "Remote profile missing".into();
                return Ok(());
            };
            match app.run_with_busy("Remote: downloading file...", |_| {
                download_to_temp(&profile, &entry.path.to_string_lossy(), false)
            }) {
                Ok(path) => path,
                Err(e) => {
                    app.status.text = format!("Remote download failed: {}", e);
                    return Ok(());
                }
            }
        } else {
            entry.path.clone()
        };
        // Check registered openers first
        let ext = entry.path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let openers = app.config.openers_for(ext).to_vec();
        match openers.len() {
            0 => {
                // No association: fall back to system default
                if let Err(e) = open::that(&launch_path) {
                    app.status.text = format!("Cannot open: {}", e);
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
        command.split_whitespace()
            .map(|t| if t == "%f" { path_str.to_string() } else { t.to_string() })
            .collect()
    } else {
        let mut v: Vec<String> = command.split_whitespace()
            .map(|t| t.to_string())
            .collect();
        v.push(path_str.to_string());
        v
    };

    if args.is_empty() {
        app.status.text = "Empty opener command".into();
        return Ok(());
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    let _ = std::process::Command::new(&args[0]).args(&args[1..]).status();

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
        app.status.text = "Editing in archive is not supported".into();
        return Ok(());
    }
    let entry = match app.active_panel().current_entry() {
        Some(e) if !e.is_dir && e.name != ".." => e.clone(),
        _ => return Ok(()),
    };

    let editor = app.config.editor.clone();
    let path = if app.active_panel().is_remote_view() {
        let Some(profile) = app.active_panel().remote_profile() else {
            app.status.text = "Remote profile missing".into();
            return Ok(());
        };
        match app.run_with_busy("Remote: downloading file...", |_| {
            download_to_temp(&profile, &entry.path.to_string_lossy(), false)
        }) {
            Ok(path) => path,
            Err(e) => {
                app.status.text = format!("Remote download failed: {}", e);
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
            app.status.text = format!("Remote upload failed: {}", e);
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
        app.status.text = "Rename in archive is not supported".into();
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
        app.status.text = "Create directory in archive is not supported".into();
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
        // Confirm: jump to highlighted match
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
            let entry_idx = app.active_panel().quicksearch_matches()
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
            let entry_idx = app.active_panel().quicksearch_matches()
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
// Viewer mode
// ---------------------------------------------------------------------------

fn handle_viewer(app: &mut App, key: KeyEvent) -> Result<bool> {
    // '/' and Esc/F3 require moving app.mode; handle them before borrowing.
    match key.code {
        KeyCode::Esc | KeyCode::F(10) => {
            if let AppMode::Viewer(ref v) = app.mode {
                v.save_position();
            }
            app.mode = AppMode::Browse;
            return Ok(false);
        }
        KeyCode::Char('/') | KeyCode::F(7) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else { return Ok(false); };
            app.mode = AppMode::ViewerSearching(v);
            return Ok(false);
        }
        KeyCode::F(3) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else { return Ok(false); };
            let menu = ViewerMenuState::new(ViewerMenuKind::LineFeed, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        KeyCode::F(4) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else { return Ok(false); };
            let menu = ViewerMenuState::new(ViewerMenuKind::Mode, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        KeyCode::F(6) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else { return Ok(false); };
            let menu = ViewerMenuState::new(ViewerMenuKind::Preproc, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        KeyCode::F(8) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else { return Ok(false); };
            let menu = ViewerMenuState::new(ViewerMenuKind::Encoding, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        KeyCode::F(9) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else { return Ok(false); };
            let menu = ViewerMenuState::new(ViewerMenuKind::Mask, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        _ => {}
    }

    let AppMode::Viewer(ref mut v) = app.mode else {
        return Ok(false);
    };

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Left => v.scroll_left(40),
            KeyCode::Right => v.scroll_right(40),
            KeyCode::Home => v.scroll_left_max(),
            _ => {
                match key.code {
                    KeyCode::Up => v.scroll_up(),
                    KeyCode::Down => v.scroll_down(),
                    KeyCode::PageUp => v.page_up(20),
                    KeyCode::PageDown => v.page_down(20),
                    KeyCode::End => v.goto_end(20),
                    KeyCode::Char('n') => v.search_next(),
                    KeyCode::Char('N') => v.search_prev(),
                    _ => {}
                }
            }
        }
    } else {
        match key.code {
            KeyCode::Up => v.scroll_up(),
            KeyCode::Down => v.scroll_down(),
            KeyCode::PageUp => v.page_up(20),
            KeyCode::PageDown => v.page_down(20),
            KeyCode::Home => v.goto_start(),
            KeyCode::End => v.goto_end(20),
            KeyCode::Left => v.scroll_left(8),
            KeyCode::Right => v.scroll_right(8),
            KeyCode::F(2) => v.toggle_wrap(),
            KeyCode::F(5) => v.toggle_zoom(),
            KeyCode::Tab => v.html_next_link(),
            KeyCode::BackTab => v.html_prev_link(),
            KeyCode::Enter => {
                let _ = v.html_follow_link();
            }
            KeyCode::Char('n') => v.search_next(),
            KeyCode::Char('N') => v.search_prev(),
            _ => {}
        }
    }
    Ok(false)
}

fn handle_viewer_menu(app: &mut App, key: KeyEvent) -> Result<bool> {
    let (viewer, mut menu) = match std::mem::replace(&mut app.mode, AppMode::Browse) {
        AppMode::ViewerMenu(viewer, menu) => (viewer, menu),
        other => {
            app.mode = other;
            return Ok(false);
        }
    };
    let visible_rows = match menu.kind {
        ViewerMenuKind::Preproc => 8usize,
        _ => 6usize,
    };

    match key.code {
        KeyCode::Esc => {
            viewer.save_position();
            app.mode = AppMode::Viewer(viewer);
            return Ok(false);
        }
        KeyCode::Up if menu.kind == ViewerMenuKind::Preproc && key.modifiers.contains(KeyModifiers::CONTROL) => {
            let mut viewer = viewer;
            if menu.cursor < viewer.preproc_len() {
                viewer.move_preproc_up(menu.cursor);
                menu.cursor = menu.cursor.saturating_sub(1);
            }
            app.mode = AppMode::ViewerMenu(viewer, menu);
            return Ok(false);
        }
        KeyCode::Down if menu.kind == ViewerMenuKind::Preproc && key.modifiers.contains(KeyModifiers::CONTROL) => {
            let mut viewer = viewer;
            if menu.cursor < viewer.preproc_len() {
                viewer.move_preproc_down(menu.cursor);
                menu.cursor = (menu.cursor + 1).min(viewer.preproc_len().saturating_sub(1));
            }
            app.mode = AppMode::ViewerMenu(viewer, menu);
            return Ok(false);
        }
        KeyCode::Up => menu.cursor = viewer_menu_prev_cursor(&viewer, menu.kind, menu.cursor),
        KeyCode::Down => menu.cursor = viewer_menu_next_cursor(&viewer, menu.kind, menu.cursor),
        KeyCode::Home => menu.cursor = viewer_menu_first_cursor(&viewer, menu.kind),
        KeyCode::End => menu.cursor = viewer_menu_last_cursor(&viewer, menu.kind),
        KeyCode::Char(ch) if menu.kind == ViewerMenuKind::Mode => {
            if let Some(cursor) = viewer_mode_shortcut(ch) {
                let mut viewer = viewer;
                let mode = match cursor {
                    0 => ViewMode::Text,
                    1 => ViewMode::Hex,
                    2 => ViewMode::Ansi,
                    3 => ViewMode::Eml,
                    4 => ViewMode::Html,
                    _ => ViewMode::Image,
                };
                viewer.set_mode(mode);
                app.mode = AppMode::Viewer(viewer);
                return Ok(false);
            }
        }
        KeyCode::Left if menu.kind == ViewerMenuKind::Preproc => {
            let mut viewer = viewer;
            if menu.cursor < viewer.preproc_len() {
                viewer.update_preproc_param(menu.cursor, -1);
            } else {
                menu.param = menu.param.saturating_sub(1);
            }
            app.mode = AppMode::ViewerMenu(viewer, menu);
            return Ok(false);
        }
        KeyCode::Right if menu.kind == ViewerMenuKind::Preproc => {
            let mut viewer = viewer;
            if menu.cursor < viewer.preproc_len() {
                viewer.update_preproc_param(menu.cursor, 1);
            } else {
                menu.param = menu.param.saturating_add(1);
            }
            app.mode = AppMode::ViewerMenu(viewer, menu);
            return Ok(false);
        }
        KeyCode::Backspace | KeyCode::Delete if menu.kind == ViewerMenuKind::Preproc => {
            let mut viewer = viewer;
            if menu.cursor < viewer.preproc_len() {
                viewer.remove_preproc(menu.cursor);
                menu.cursor = menu.cursor.min(viewer_menu_last_cursor(&viewer, menu.kind));
            } else if is_preproc_clear_item(&viewer, menu.cursor) {
                viewer.clear_preproc();
                menu.cursor = viewer_menu_first_cursor(&viewer, menu.kind);
            }
            app.mode = AppMode::ViewerMenu(viewer, menu);
            return Ok(false);
        }
        KeyCode::Enter => {
            let mut viewer = viewer;
            match menu.kind {
                ViewerMenuKind::Mode => {
                    let mode = match menu.cursor {
                        0 => ViewMode::Text,
                        1 => ViewMode::Hex,
                        2 => ViewMode::Ansi,
                        3 => ViewMode::Eml,
                        4 => ViewMode::Html,
                        _ => ViewMode::Image,
                    };
                    viewer.set_mode(mode);
                }
                ViewerMenuKind::LineFeed => {
                    let mode = match menu.cursor {
                        0 => LineFeedMode::DosCrLf,
                        1 => LineFeedMode::UnixLf,
                        2 => LineFeedMode::MacCr,
                        _ => LineFeedMode::Mixed,
                    };
                    viewer.set_line_feed(mode);
                }
                ViewerMenuKind::Preproc => {
                    if menu.cursor < viewer.preproc_len() {
                        app.mode = AppMode::ViewerMenu(viewer, menu);
                        return Ok(false);
                    }
                    if let Some(kind) = preproc_add_item_kind(&viewer, menu.cursor) {
                        viewer.push_preproc(kind, menu.param);
                        menu.cursor = viewer.preproc_len().saturating_sub(1);
                    } else if is_preproc_clear_item(&viewer, menu.cursor) {
                        viewer.clear_preproc();
                        menu.cursor = viewer_menu_first_cursor(&viewer, menu.kind);
                    }
                    app.mode = AppMode::ViewerMenu(viewer, menu);
                    return Ok(false);
                }
                ViewerMenuKind::Encoding => {
                    let mode = match menu.cursor {
                        0 => EncodingMode::Plain,
                        _ => EncodingMode::Cp437,
                    };
                    viewer.set_encoding(mode);
                }
                ViewerMenuKind::Mask => {
                    match menu.cursor {
                        0 => viewer.set_mask(Some(MaskKind::C)),
                        1 => viewer.set_mask(Some(MaskKind::Pascal)),
                        2 => viewer.set_mask(Some(MaskKind::Assembler)),
                        3 => viewer.set_mask(Some(MaskKind::Ketchup)),
                        _ => viewer.set_mask(None),
                    }
                }
            }
            app.mode = AppMode::Viewer(viewer);
            return Ok(false);
        }
        _ => {}
    }

    clamp_viewer_menu_scroll(&mut menu, &viewer, visible_rows);
    app.mode = AppMode::ViewerMenu(viewer, menu);
    Ok(false)
}

fn viewer_menu_items(kind: ViewerMenuKind) -> &'static [&'static str] {
    match kind {
        ViewerMenuKind::Mode => &["Text", "Binary", "Ansi", "EML", "Html", "Image"],
        ViewerMenuKind::LineFeed => &["DOS (CR/LF)", "Unix (LF)", "Mac (CR)", "Mixed"],
        ViewerMenuKind::Encoding => &["Plain ASCII", "DOS CP437"],
        ViewerMenuKind::Mask => &["C Style", "Pascal Style", "Assembler Style", "Ketchup Style", "Mask OFF"],
        ViewerMenuKind::Preproc => &[],
    }
}

const PREPROC_ADD_ITEMS: &[(&str, PreprocOpKind)] = &[
    ("Add XOR", PreprocOpKind::Xor),
    ("Add AND", PreprocOpKind::And),
    ("Add OR", PreprocOpKind::Or),
    ("Add NEG", PreprocOpKind::Neg),
    ("Add ROR", PreprocOpKind::Ror),
    ("Add ADD", PreprocOpKind::Add),
    ("Add Latin", PreprocOpKind::Latin),
    ("Add Elite", PreprocOpKind::Elite),
];

fn viewer_menu_len(viewer: &crate::viewer::Viewer, kind: ViewerMenuKind) -> usize {
    match kind {
        ViewerMenuKind::Preproc => {
            let existing = viewer.preproc_len();
            let separator = usize::from(existing > 0);
            existing + separator + PREPROC_ADD_ITEMS.len() + 1
        }
        _ => viewer_menu_items(kind).len(),
    }
}

fn viewer_mode_shortcut(ch: char) -> Option<usize> {
    match ch.to_ascii_lowercase() {
        't' => Some(0),
        'b' => Some(1),
        'a' => Some(2),
        'e' => Some(3),
        'h' => Some(4),
        'i' => Some(5),
        _ => None,
    }
}

fn clamp_viewer_menu_scroll(
    menu: &mut ViewerMenuState,
    viewer: &crate::viewer::Viewer,
    visible_rows: usize,
) {
    let visible_rows = visible_rows.max(1);
    let max_scroll = viewer_menu_len(viewer, menu.kind).saturating_sub(visible_rows);
    if menu.cursor < menu.scroll {
        menu.scroll = menu.cursor;
    } else if menu.cursor >= menu.scroll + visible_rows {
        menu.scroll = menu.cursor + 1 - visible_rows;
    }
    menu.scroll = menu.scroll.min(max_scroll);
}

fn viewer_menu_first_cursor(viewer: &crate::viewer::Viewer, kind: ViewerMenuKind) -> usize {
    if kind == ViewerMenuKind::Preproc && viewer.preproc_len() > 0 {
        0
    } else {
        0
    }
}

fn viewer_menu_last_cursor(viewer: &crate::viewer::Viewer, kind: ViewerMenuKind) -> usize {
    viewer_menu_len(viewer, kind).saturating_sub(1)
}

fn viewer_menu_next_cursor(viewer: &crate::viewer::Viewer, kind: ViewerMenuKind, cursor: usize) -> usize {
    let last = viewer_menu_last_cursor(viewer, kind);
    let mut next = if cursor >= last { 0 } else { cursor + 1 };
    if kind == ViewerMenuKind::Preproc && is_preproc_separator(viewer, next) {
        next = if next >= last { 0 } else { next + 1 };
    }
    next
}

fn viewer_menu_prev_cursor(viewer: &crate::viewer::Viewer, kind: ViewerMenuKind, cursor: usize) -> usize {
    let last = viewer_menu_last_cursor(viewer, kind);
    let mut prev = if cursor == 0 { last } else { cursor - 1 };
    if kind == ViewerMenuKind::Preproc && is_preproc_separator(viewer, prev) {
        prev = if prev == 0 { last } else { prev - 1 };
    }
    prev
}

fn preproc_add_base(viewer: &crate::viewer::Viewer) -> usize {
    viewer.preproc_len() + usize::from(viewer.preproc_len() > 0)
}

fn is_preproc_separator(viewer: &crate::viewer::Viewer, idx: usize) -> bool {
    viewer.preproc_len() > 0 && idx == viewer.preproc_len()
}

fn is_preproc_clear_item(viewer: &crate::viewer::Viewer, idx: usize) -> bool {
    idx == preproc_add_base(viewer) + PREPROC_ADD_ITEMS.len()
}

fn preproc_add_item_kind(viewer: &crate::viewer::Viewer, idx: usize) -> Option<PreprocOpKind> {
    let rel = idx.checked_sub(preproc_add_base(viewer))?;
    PREPROC_ADD_ITEMS.get(rel).map(|(_, kind)| *kind)
}

// ---------------------------------------------------------------------------
// Viewer search mode (live '/' search)
// ---------------------------------------------------------------------------

fn handle_viewer_searching(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => {
            // Clear search and return to normal viewer
            if let AppMode::ViewerSearching(ref mut v) = app.mode {
                v.search.clear();
                v.matches.clear();
            }
            let AppMode::ViewerSearching(v) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else { return Ok(false); };
            app.mode = AppMode::Viewer(v);
        }
        KeyCode::F(10) => {
            if let AppMode::ViewerSearching(ref v) = app.mode {
                v.save_position();
            }
            app.mode = AppMode::Browse;
        }
        KeyCode::Enter => {
            // Confirm search, stay in viewer (normal mode)
            let AppMode::ViewerSearching(v) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else { return Ok(false); };
            app.mode = AppMode::Viewer(v);
        }
        KeyCode::Backspace => {
            if let AppMode::ViewerSearching(ref mut v) = app.mode {
                v.search.pop();
                let s = v.search.clone();
                v.search_set(&s);
            }
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let AppMode::ViewerSearching(ref mut v) = app.mode {
                v.search.push(ch);
                let s = v.search.clone();
                v.search_set(&s);
            }
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Confirm dialog
// ---------------------------------------------------------------------------

fn handle_confirm(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::Confirm(ref dlg) = app.mode else {
        return Ok(false);
    };

    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            let action = dlg.action.clone();
            app.mode = AppMode::Browse;
            match action {
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
                InputAction::Rename(path) => {
                    match crate::file_ops::rename_entry(&path, &value) {
                        Ok(_) => {
                            app.status.text = format!("Renamed to '{}'", value);
                            if app.config.auto_reload {
                                app.reload_panels();
                            }
                        }
                        Err(e) => app.status.text = format!("Rename error: {}", e),
                    }
                }
                InputAction::Mkdir(parent) => {
                    match crate::file_ops::make_dir(&parent, &value) {
                        Ok(_) => {
                            app.status.text = format!("Created directory '{}'", value);
                            if app.config.auto_reload {
                                app.reload_panels();
                            }
                        }
                        Err(e) => app.status.text = format!("mkdir error: {}", e),
                    }
                }
                InputAction::RemoteRename { profile, path } => {
                    let Some(parent) = std::path::Path::new(&path).parent() else {
                        app.status.text = "Rename error: invalid remote path".into();
                        return Ok(false);
                    };
                    let dst = join_remote(&parent.to_string_lossy(), &value);
                    match app.run_with_busy("Remote: renaming...", |_| {
                        remote_rename_path(&profile, &path, &dst)
                    }) {
                        Ok(_) => {
                            app.status.text = format!("Renamed to '{}'", value);
                            if app.config.auto_reload {
                                app.reload_panels();
                            }
                        }
                        Err(e) => app.status.text = format!("Rename error: {}", e),
                    }
                }
                InputAction::RemoteMkdir { profile, parent } => {
                    let path = join_remote(&parent, &value);
                    match app.run_with_busy("Remote: creating directory...", |_| {
                        remote_make_dir(&profile, &path)
                    }) {
                        Ok(_) => {
                            app.status.text = format!("Created directory '{}'", value);
                            if app.config.auto_reload {
                                app.reload_panels();
                            }
                        }
                        Err(e) => app.status.text = format!("mkdir error: {}", e),
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
                        app.status.text = format!("Not a directory: {}", value);
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
                            action: InputAction::AssocAddOpeners { ext, edit_index: None },
                        });
                        // fix cursor to end
                        let AppMode::Input(ref mut dlg) = app.mode else { return Ok(false); };
                        dlg.cursor = dlg.value.len();
                    }
                }
                InputAction::AssocAddOpeners { ext, edit_index } => {
                    let openers: Vec<String> = value.split(',')
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
                                if let Some(existing) = app.config.file_assoc.iter_mut()
                                    .find(|a| a.ext.eq_ignore_ascii_case(&ext))
                                {
                                    existing.openers = openers;
                                } else {
                                    app.config.file_assoc.push(crate::config::FileAssoc {
                                        ext,
                                        openers,
                                    });
                                }
                            }
                        }
                    }
                    app.save_config().ok();
                    app.mode = AppMode::AssocEditor(AssocEditorState::from_config(&app.config));
                }
            }
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            let AppMode::Input(ref mut dlg) = app.mode else { return Ok(false); };
            dlg.insert_char(ch);
        }
        KeyCode::Backspace => {
            let AppMode::Input(ref mut dlg) = app.mode else { return Ok(false); };
            dlg.backspace();
        }
        KeyCode::Delete => {
            let AppMode::Input(ref mut dlg) = app.mode else { return Ok(false); };
            dlg.delete_char();
        }
        KeyCode::Left => {
            let AppMode::Input(ref mut dlg) = app.mode else { return Ok(false); };
            dlg.move_left();
        }
        KeyCode::Right => {
            let AppMode::Input(ref mut dlg) = app.mode else { return Ok(false); };
            dlg.move_right();
        }
        KeyCode::Home => {
            let AppMode::Input(ref mut dlg) = app.mode else { return Ok(false); };
            dlg.home();
        }
        KeyCode::End => {
            let AppMode::Input(ref mut dlg) = app.mode else { return Ok(false); };
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
                app.status.text = "Copy aborted".into();
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
        KeyCode::Enter => {
            match dlg.field {
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
            }
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Search panel
// ---------------------------------------------------------------------------

fn handle_search(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Tab => {
            let AppMode::SearchPanel(ref mut s) = app.mode else { return Ok(false); };
            s.input_field = 1 - s.input_field;
        }
        KeyCode::Enter => {
            app.run_search();
            // After search completes, stay in search mode to show results
        }
        KeyCode::Up => {
            let AppMode::SearchPanel(ref mut s) = app.mode else { return Ok(false); };
            if s.cursor > 0 {
                s.cursor -= 1;
            }
        }
        KeyCode::Down => {
            let AppMode::SearchPanel(ref mut s) = app.mode else { return Ok(false); };
            if s.cursor + 1 < s.results.len() {
                s.cursor += 1;
            }
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            let AppMode::SearchPanel(ref mut s) = app.mode else { return Ok(false); };
            if s.input_field == 0 {
                s.query.push(ch);
            } else {
                s.content_query.push(ch);
            }
        }
        KeyCode::Backspace => {
            let AppMode::SearchPanel(ref mut s) = app.mode else { return Ok(false); };
            if s.input_field == 0 {
                s.query.pop();
            } else {
                s.content_query.pop();
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
                            if let Some(profile) = profiles.into_iter().find(|pr| pr.name == profile_name) {
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

// ---------------------------------------------------------------------------
// Menu (F2)
// ---------------------------------------------------------------------------

fn handle_menu(app: &mut App, key: KeyEvent) -> Result<bool> {
    let (bar_pos, open, item_pos) = {
        let AppMode::Menu(ref s) = app.mode else { return Ok(false); };
        (s.bar_pos, s.open, s.item_pos)
    };

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Left => {
            let new_pos = bar_pos.saturating_sub(1);
            if let AppMode::Menu(ref mut s) = app.mode {
                s.bar_pos = new_pos;
                s.item_pos = first_selectable(MENU_DATA[new_pos]);
            }
        }
        KeyCode::Right => {
            let new_pos = (bar_pos + 1).min(MENU_HEADERS.len() - 1);
            if let AppMode::Menu(ref mut s) = app.mode {
                s.bar_pos = new_pos;
                s.item_pos = first_selectable(MENU_DATA[new_pos]);
            }
        }
        KeyCode::Down if !open => {
            if let AppMode::Menu(ref mut s) = app.mode {
                s.open = true;
                s.item_pos = first_selectable(MENU_DATA[bar_pos]);
            }
        }
        KeyCode::Enter if !open => {
            if let AppMode::Menu(ref mut s) = app.mode {
                s.open = true;
                s.item_pos = first_selectable(MENU_DATA[bar_pos]);
            }
        }
        KeyCode::Up if open => {
            let new_pos = prev_selectable(MENU_DATA[bar_pos], item_pos);
            if let AppMode::Menu(ref mut s) = app.mode {
                s.item_pos = new_pos;
            }
        }
        KeyCode::Down if open => {
            let new_pos = next_selectable(MENU_DATA[bar_pos], item_pos);
            if let AppMode::Menu(ref mut s) = app.mode {
                s.item_pos = new_pos;
            }
        }
        KeyCode::Enter if open => {
            let action = MENU_DATA[bar_pos][item_pos].2;
            app.mode = AppMode::Browse;
            return execute_menu_action(app, action);
        }
        _ => {}
    }
    Ok(false)
}

fn first_selectable(items: &[crate::app::MenuEntry]) -> usize {
    items
        .iter()
        .position(|(_, _, a)| *a != MenuAction::Separator)
        .unwrap_or(0)
}

fn next_selectable(items: &[crate::app::MenuEntry], current: usize) -> usize {
    let n = items.len();
    let mut pos = (current + 1) % n;
    let start = pos;
    loop {
        if items[pos].2 != MenuAction::Separator {
            break;
        }
        pos = (pos + 1) % n;
        if pos == start {
            break;
        }
    }
    pos
}

fn prev_selectable(items: &[crate::app::MenuEntry], current: usize) -> usize {
    let n = items.len();
    let mut pos = if current == 0 { n - 1 } else { current - 1 };
    let start = pos;
    loop {
        if items[pos].2 != MenuAction::Separator {
            break;
        }
        pos = if pos == 0 { n - 1 } else { pos - 1 };
        if pos == start {
            break;
        }
    }
    pos
}

fn execute_menu_action(app: &mut App, action: MenuAction) -> Result<bool> {
    match action {
        MenuAction::ViewFile => {
            app.open_viewer();
        }
        MenuAction::EditFile => {
            launch_editor(app)?;
        }
        MenuAction::CopyFile => {
            app.open_copy_dialog();
        }
        MenuAction::MoveFile => {
            app.cmd_move()?;
        }
        MenuAction::MkDir => {
            start_mkdir(app);
        }
        MenuAction::RenameFile => {
            start_rename(app);
        }
        MenuAction::DeleteFile => {
            app.cmd_delete();
        }
        MenuAction::Quit => {
            return confirm_quit(app);
        }
        MenuAction::SwapPanels => {
            app.swap_panels();
            app.status.text = "Panels swapped".into();
        }
        MenuAction::SortName => {
            app.set_sort(SortMode::Name);
        }
        MenuAction::SortExtension => {
            app.set_sort(SortMode::Extension);
        }
        MenuAction::SortDate => {
            app.set_sort(SortMode::Date);
        }
        MenuAction::SortSize => {
            app.set_sort(SortMode::Size);
        }
        MenuAction::SortUnsorted => {
            app.set_sort(SortMode::Unsorted);
        }
        MenuAction::ToggleHidden => {
            let p = app.active_panel_mut();
            p.show_hidden = !p.show_hidden;
            let _ = p.reload();
        }
        MenuAction::Reload => {
            app.reload_panels();
            app.status.text = "Reloaded".into();
        }
        MenuAction::GoToPath => {
            let current = app.active_panel().display_path();
            let cursor = current.len();
            app.mode = AppMode::Input(InputDialog {
                title: "Go to Path".into(),
                prompt: "Path:".into(),
                value: current,
                cursor,
                action: InputAction::GoToPath,
            });
        }
        MenuAction::SelectPattern => {
            app.mode = open_wildcard_dialog("Select pattern:", true);
        }
        MenuAction::DeselectPattern => {
            app.mode = open_wildcard_dialog("Deselect pattern:", false);
        }
        MenuAction::InvertSelection => {
            app.active_panel_mut().invert_selection();
        }
        MenuAction::SearchFiles => {
            app.open_search();
        }
        MenuAction::DirBookmarks => {
            app.open_dir_bookmarks();
        }
        MenuAction::ToggleFBar => {
            app.config.show_fkey_bar = !app.config.show_fkey_bar;
        }
        MenuAction::Setup => {
            let cs = ConfigState::from_config(&app.config);
            app.mode = AppMode::Config(cs);
        }
        MenuAction::Associations => {
            app.mode = AppMode::AssocEditor(AssocEditorState::from_config(&app.config));
        }
        MenuAction::SaveConfig => {
            match app.save_config() {
                Ok(_) => app.status.text = "Config saved".into(),
                Err(e) => app.status.text = format!("Save error: {}", e),
            }
        }
        MenuAction::Help => {
            app.open_help();
        }
        MenuAction::About => {
            app.status.text = format!(
                "KKC {} \u{2014} Rust reimplementation of KKC-DOS",
                env!("CARGO_PKG_VERSION")
            );
        }
        MenuAction::Separator => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Config screen
// ---------------------------------------------------------------------------

fn handle_config(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::Config(ref mut cs) = app.mode else { return Ok(false); };

    let total = ConfigState::NUM_TOTAL;    // 8 booleans + 3 text + OK + Cancel
    let n_bool = ConfigState::NUM_CHECKBOXES; // 8
    let n_text = 3;
    let ok_idx     = n_bool + n_text;      // 11
    let cancel_idx = n_bool + n_text + 1;  // 12

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }

        // Navigate rows
        KeyCode::Up | KeyCode::BackTab => {
            if let AppMode::Config(ref mut cs) = app.mode {
                if cs.cursor > 0 { cs.cursor -= 1; }
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            if let AppMode::Config(ref mut cs) = app.mode {
                if cs.cursor + 1 < total { cs.cursor += 1; }
            }
        }

        // Toggle checkbox or activate button
        KeyCode::Char(' ') | KeyCode::Enter => {
            let cursor = cs.cursor;
            match cursor {
                0 => cs.confirm_exit     = !cs.confirm_exit,
                1 => cs.confirm_delete   = !cs.confirm_delete,
                2 => cs.auto_reload      = !cs.auto_reload,
                3 => cs.insert_moves_down = !cs.insert_moves_down,
                4 => cs.select_dirs      = !cs.select_dirs,
                5 => cs.show_hidden      = !cs.show_hidden,
                6 => cs.color_by_type    = !cs.color_by_type,
                7 => cs.show_fkey_bar    = !cs.show_fkey_bar,
                // text fields: Enter moves focus to next
                8 | 9 | 10 => {
                    if let AppMode::Config(ref mut cs) = app.mode {
                        if cs.cursor + 1 < total { cs.cursor += 1; }
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
                    let _ = app.left.reload();
                    let _ = app.right.reload();
                    match app.save_config() {
                        Ok(_) => app.status.text = "Config saved".into(),
                        Err(e) => app.status.text = format!("Save error: {}", e),
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
                    8  => cs.editor.push(ch),
                    9  => cs.pager.push(ch),
                    10 => cs.dir_history_max.push(ch),
                    _  => {}
                }
            }
        }
        KeyCode::Backspace => {
            if let AppMode::Config(ref mut cs) = app.mode {
                match cs.cursor {
                    8  => { cs.editor.pop(); }
                    9  => { cs.pager.pop(); }
                    10 => { cs.dir_history_max.pop(); }
                    _  => {}
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
    let AppMode::Opener(ref mut s) = app.mode else { return Ok(false); };
    let total = s.items.len();

    match key.code {
        KeyCode::Esc => { app.mode = AppMode::Browse; }
        KeyCode::Up | KeyCode::BackTab => {
            if let AppMode::Opener(ref mut s) = app.mode {
                if s.cursor > 0 { s.cursor -= 1; }
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            if let AppMode::Opener(ref mut s) = app.mode {
                if s.cursor + 1 < total { s.cursor += 1; }
            }
        }
        KeyCode::Enter => {
            let (cmd, path) = if let AppMode::Opener(s) = &app.mode {
                (s.items[s.cursor].clone(), s.path.clone())
            } else { return Ok(false); };
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
    let total = if let AppMode::AssocEditor(ref s) = app.mode { s.assocs.len() } else { 0 };

    match key.code {
        KeyCode::Esc => { app.mode = AppMode::Browse; }
        KeyCode::Up => {
            if let AppMode::AssocEditor(ref mut s) = app.mode {
                if s.cursor > 0 { s.cursor -= 1; }
            }
        }
        KeyCode::Down => {
            if let AppMode::AssocEditor(ref mut s) = app.mode {
                if s.cursor + 1 < total { s.cursor += 1; }
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
                if s.assocs.is_empty() { return Ok(false); }
                let (ext, openers) = &s.assocs[s.cursor];
                (ext.clone(), openers.join(", "), s.cursor)
            } else { return Ok(false); };
            app.mode = AppMode::Input(InputDialog {
                title: "Edit association".into(),
                prompt: format!("Openers for .{} (comma-separated):", ext),
                value: openers_str.clone(),
                cursor: openers_str.len(),
                action: InputAction::AssocAddOpeners { ext, edit_index: Some(idx) },
            });
        }
        // Delete selected
        KeyCode::Delete | KeyCode::Char('d') | KeyCode::Char('D') => {
            let (idx, cursor) = if let AppMode::AssocEditor(ref s) = app.mode {
                if s.assocs.is_empty() { return Ok(false); }
                (s.cursor, s.cursor)
            } else { return Ok(false); };
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
            app.open_remote_add();
        }
        KeyCode::F(8) => {
            app.open_remote_add_imap();
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

fn handle_remote_connecting(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::F(10) => app.cancel_remote_connect(),
        _ => {}
    }
    Ok(false)
}

fn handle_remote_edit(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::RemoteEdit(ref mut s) = app.mode else { return Ok(false); };
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
                s.input_cursor = (s.input_cursor + 1).min(s.current_value().map(|v| v.len()).unwrap_or(0));
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
                if let Some(value) = s.current_value_mut() && pos < value.len() {
                    value.remove(pos);
                }
            }
        }
        KeyCode::Char(ch) => {
            if s.cursor < 6 && !key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT) {
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
                        Ok(()) => app.mode = AppMode::RemoteConnect(crate::app::RemoteConnectState::load()),
                        Err(e) => app.status.text = format!("Cannot save connection: {}", e),
                    }
                } else {
                    app.status.text = match s.kind {
                        RemoteEditKind::Sftp => "SFTP name is required".into(),
                        RemoteEditKind::Imap => "IMAP name, host and user are required".into(),
                    };
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
        HelpView::Topics { section, ref mut cursor } => match key.code {
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
                let max = state.system.sections[section].topics.len().saturating_sub(1);
                *cursor = (*cursor + 1).min(max);
            }
            KeyCode::Home => *cursor = 0,
            KeyCode::End => *cursor = state.system.sections[section].topics.len().saturating_sub(1),
            KeyCode::Enter => {
                let topic = state.system.sections[section].topics[*cursor];
                let prev = state.view;
                state.history.push(prev);
                state.view = HelpView::Page { topic, scroll: 0, selected_link: 0 };
            }
            _ => {}
        },
        HelpView::Page { topic, ref mut scroll, ref mut selected_link } => match key.code {
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
                    *selected_link = if *selected_link == 0 { count - 1 } else { *selected_link - 1 };
                }
            }
            KeyCode::Enter => {
                let target = state.system.topics[topic]
                    .selected_link_target(*selected_link)
                    .map(str::to_string);
                if let Some(target) = target {
                    if !state.open_topic_by_name(&target) {
                        app.status.text = format!("Unknown help topic: {}", target);
                    }
                }
            }
            _ => {}
        },
    }

    Ok(false)
}
