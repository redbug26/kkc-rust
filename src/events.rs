use crate::app::{
    App, AppMode, ConfirmAction, InputAction, InputDialog, MenuAction, MenuState,
    ViewerMenuKind, ViewerMenuState,
    MENU_DATA, MENU_HEADERS,
};
use crate::config::SortMode;
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
        AppMode::SearchPanel(_) => return handle_search(app, key),
        AppMode::DirHistory => return handle_dir_history(app, key),
        AppMode::QuickSearch => return handle_quicksearch(app, key),
        AppMode::Menu(_) => return handle_menu(app, key),
        AppMode::Browse => {}
    }

    handle_browse(app, key)
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
                app.active_panel_mut().quicksearch_append(ch);
                if let Some(idx) = app.active_panel().quicksearch_index() {
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
                app.history_cursor = 0;
                app.mode = AppMode::DirHistory;
                return Ok(false);
            }
            _ => {}
        }
    }

    // Alt-modified keys
    if alt && !ctrl && !shift {
        match key.code {
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
            app.cmd_copy()?;
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
    } else {
        // Open with system default
        if let Err(e) = open::that(&entry.path) {
            app.status.text = format!("Cannot open: {}", e);
        }
    }
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
    let entry = match app.active_panel().current_entry() {
        Some(e) if !e.is_dir && e.name != ".." => e.clone(),
        _ => return Ok(()),
    };

    let editor = app.config.editor.clone();
    let path = entry.path.clone();

    // Restore normal terminal before handing control to an external editor.
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    use std::process::Command;
    let _ = Command::new(&editor).arg(&path).status();

    // Re-enter TUI mode once the editor exits.
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;

    app.reload_panels();
    Ok(())
}

fn start_rename(app: &mut App) {
    if let Some(entry) = app.active_panel().current_entry() {
        if entry.name == ".." {
            return;
        }
        let path = entry.path.clone();
        let name = entry.name.clone();
        app.mode = AppMode::Input(InputDialog {
            title: "Rename".into(),
            prompt: "New name:".into(),
            value: name.clone(),
            cursor: name.len(),
            action: InputAction::Rename(path),
        });
    }
}

fn start_mkdir(app: &mut App) {
    let current = app.active_panel().path.clone();
    app.mode = AppMode::Input(InputDialog {
        title: "Create Directory".into(),
        prompt: "Directory name:".into(),
        value: String::new(),
        cursor: 0,
        action: InputAction::Mkdir(current),
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
// QuickSearch mode
// ---------------------------------------------------------------------------

fn handle_quicksearch(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.active_panel_mut().quicksearch_clear();
            app.mode = AppMode::Browse;
        }
        KeyCode::Backspace => {
            app.active_panel_mut().quicksearch_pop();
            if app.active_panel().quicksearch.is_empty() {
                app.mode = AppMode::Browse;
            } else if let Some(idx) = app.active_panel().quicksearch_index() {
                app.active_panel_mut().cursor = idx;
            }
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_panel_mut().quicksearch_append(ch);
            if let Some(idx) = app.active_panel().quicksearch_index() {
                app.active_panel_mut().cursor = idx;
            }
        }
        _ => {
            // Pass through navigation keys
            app.active_panel_mut().quicksearch_clear();
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
                        _ => ViewMode::Html,
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

    app.mode = AppMode::ViewerMenu(viewer, menu);
    Ok(false)
}

fn viewer_menu_items(kind: ViewerMenuKind) -> &'static [&'static str] {
    match kind {
        ViewerMenuKind::Mode => &["Text", "Binary", "Ansi", "Html"],
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
    let mut next = (cursor + 1).min(viewer_menu_last_cursor(viewer, kind));
    if kind == ViewerMenuKind::Preproc && is_preproc_separator(viewer, next) {
        next = (next + 1).min(viewer_menu_last_cursor(viewer, kind));
    }
    next
}

fn viewer_menu_prev_cursor(viewer: &crate::viewer::Viewer, kind: ViewerMenuKind, cursor: usize) -> usize {
    let mut prev = cursor.saturating_sub(1);
    if kind == ViewerMenuKind::Preproc && is_preproc_separator(viewer, prev) {
        prev = prev.saturating_sub(1);
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

fn handle_dir_history(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            if app.history_cursor > 0 {
                app.history_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.history_cursor + 1 < app.dir_history.len() {
                app.history_cursor += 1;
            }
        }
        KeyCode::Enter => {
            let path = app.dir_history.get(app.history_cursor).cloned();
            app.mode = AppMode::Browse;
            if let Some(p) = path {
                if p.is_dir() {
                    app.enter_dir(p)?;
                }
            }
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
            app.cmd_copy()?;
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
            let current = app.active_panel().path.to_string_lossy().into_owned();
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
        MenuAction::DirHistory => {
            app.history_cursor = 0;
            app.mode = AppMode::DirHistory;
        }
        MenuAction::ToggleFBar => {
            app.config.show_fkey_bar = !app.config.show_fkey_bar;
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
