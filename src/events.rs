use crate::app::{App, AppMode, ConfirmAction, InputAction, InputDialog};
use crate::config::SortMode;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use anyhow::Result;

pub fn handle_event(app: &mut App, event: Event) -> Result<bool> {
    let Event::Key(key) = event else {
        return Ok(false);
    };

    match &app.mode {
        AppMode::Help => {
            app.mode = AppMode::Browse;
            return Ok(false);
        }
        AppMode::Viewer(_) => return handle_viewer(app, key),
        AppMode::ViewerSearching(_) => return handle_viewer_searching(app, key),
        AppMode::Confirm(_) => return handle_confirm(app, key),
        AppMode::Input(_) => return handle_input(app, key),
        AppMode::SearchPanel(_) => return handle_search(app, key),
        AppMode::DirHistory => return handle_dir_history(app, key),
        AppMode::QuickSearch => return handle_quicksearch(app, key),
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
            app.mode = AppMode::Help;
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

    if entry.is_dir || entry.name == ".." {
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

    use std::process::Command;
    let _ = Command::new(&editor).arg(&path).spawn().and_then(|mut c| c.wait());
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
        KeyCode::Esc | KeyCode::F(3) => {
            app.mode = AppMode::Browse;
            return Ok(false);
        }
        KeyCode::Char('/') => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else { return Ok(false); };
            app.mode = AppMode::ViewerSearching(v);
            return Ok(false);
        }
        _ => {}
    }

    let AppMode::Viewer(ref mut v) = app.mode else {
        return Ok(false);
    };

    match key.code {
        KeyCode::Up => v.scroll_up(),
        KeyCode::Down => v.scroll_down(),
        KeyCode::PageUp => v.page_up(20),
        KeyCode::PageDown => v.page_down(20),
        KeyCode::Home => v.goto_start(),
        KeyCode::End => v.goto_end(20),
        KeyCode::Tab => v.switch_mode(),
        KeyCode::Char('n') => v.search_next(),
        KeyCode::Char('N') => v.search_prev(),
        _ => {}
    }
    Ok(false)
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
