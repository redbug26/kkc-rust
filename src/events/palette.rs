//! Command palette event handler (Ctrl-P).

use super::fx_shortcut;
use super::menu::execute_menu_action;
use crate::app::{
    App, AppMode, PALETTE_DATA, PALETTE_SEP, StoreDetectChoice, shortcut_from_key_event,
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_command_palette(app: &mut App, key: KeyEvent) -> Result<bool> {
    let mut state = match std::mem::replace(&mut app.mode, AppMode::Browse) {
        AppMode::CommandPalette(state) => state,
        other => {
            app.mode = other;
            return Ok(false);
        }
    };

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let fn_key = fx_shortcut(key);

    if state.capture {
        // Determine the fn_name of the selected entry (static or Lua app).
        let selected_fn_name = state
            .selected_lua_app()
            .map(|info| format!("lua_app_{}", info.id))
            .or_else(|| {
                state
                    .selected_palette_index()
                    .and_then(|idx| PALETTE_DATA.get(idx))
                    .map(|e| e.fn_name.to_string())
            });

        match key.code {
            KeyCode::Esc => {
                state.capture = false;
            }
            KeyCode::Char('r') | KeyCode::Char('R') if key.modifiers.is_empty() => {
                if let Some(fn_name) = selected_fn_name {
                    app.reset_shortcut_for_fn(&fn_name);
                    match app.save_config() {
                        Ok(_) => app.set_status(format!("Shortcut reset: {}", fn_name)),
                        Err(e) => app.set_status(format!("Save error: {}", e)),
                    }
                }
                state.capture = false;
            }
            KeyCode::Backspace | KeyCode::Delete => {
                if let Some(fn_name) = selected_fn_name {
                    app.set_shortcut_for_fn(&fn_name, None);
                    match app.save_config() {
                        Ok(_) => app.set_status(format!("Shortcut removed: {}", fn_name)),
                        Err(e) => app.set_status(format!("Save error: {}", e)),
                    }
                }
                state.capture = false;
            }
            _ => {
                if let Some(shortcut) = shortcut_from_key_event(key) {
                    if let Some(fn_name) = selected_fn_name {
                        app.set_shortcut_for_fn(&fn_name, Some(shortcut.clone()));
                        match app.save_config() {
                            Ok(_) => app
                                .set_status(format!("Shortcut saved: {} -> {}", fn_name, shortcut)),
                            Err(e) => app.set_status(format!("Save error: {}", e)),
                        }
                        state.capture = false;
                    }
                }
            }
        }

        app.mode = AppMode::CommandPalette(state);
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        _ if fn_key == Some(10) => {
            app.mode = AppMode::Browse;
        }
        KeyCode::F(4) => {
            if state.selected_palette_index().is_some() {
                state.capture = true;
                app.mode = AppMode::CommandPalette(state);
            } else {
                app.mode = AppMode::CommandPalette(state);
            }
        }

        KeyCode::Up => {
            let indices = state.filtered_indices();
            let len = indices.len();
            if len > 0 {
                let mut pos = if state.match_pos == 0 {
                    len - 1
                } else {
                    state.match_pos - 1
                };
                // Skip over any separator sentinels.
                let mut guard = 0;
                while indices.get(pos).copied() == Some(PALETTE_SEP) && guard < len {
                    pos = if pos == 0 { len - 1 } else { pos - 1 };
                    guard += 1;
                }
                state.match_pos = pos;
            }
            app.mode = AppMode::CommandPalette(state);
        }
        KeyCode::Down => {
            let indices = state.filtered_indices();
            let len = indices.len();
            if len > 0 {
                let mut pos = (state.match_pos + 1) % len;
                // Skip over any separator sentinels.
                let mut guard = 0;
                while indices.get(pos).copied() == Some(PALETTE_SEP) && guard < len {
                    pos = (pos + 1) % len;
                    guard += 1;
                }
                state.match_pos = pos;
            }
            app.mode = AppMode::CommandPalette(state);
        }
        KeyCode::Backspace => {
            state.query.pop();
            state.match_pos = 0;
            state.clamp_match();
            app.mode = AppMode::CommandPalette(state);
        }
        KeyCode::Char(ch) if !ctrl && !alt => {
            state.query.push(ch);
            state.match_pos = 0;
            state.clamp_match();
            app.mode = AppMode::CommandPalette(state);
        }
        KeyCode::Enter => {
            let data_idx = state.selected_palette_index();

            // Check if the selected entry is a Lua app (encoded index >= PALETTE_DATA.len()).
            let lua_app_id = state.selected_lua_app().map(|info| info.id.clone());

            let action = data_idx.and_then(|i| {
                if i < PALETTE_DATA.len() {
                    PALETTE_DATA.get(i).map(|e| e.action)
                } else {
                    None
                }
            });

            // Record in recents: deduplicate then prepend, cap at 5.
            if let Some(idx) = data_idx {
                if idx < PALETTE_DATA.len() {
                    let fn_name = PALETTE_DATA[idx].fn_name.to_string();
                    app.palette_recent.retain(|x| x != &fn_name);
                    app.palette_recent.insert(0, fn_name);
                    app.palette_recent.truncate(5);
                }
            }

            app.mode = AppMode::Browse;

            if let Some(app_id) = lua_app_id {
                // Record Lua app in recents with a distinct prefix.
                let recent_token = format!("lua_app:{}", app_id);
                app.palette_recent.retain(|x| x != &recent_token);
                app.palette_recent.insert(0, recent_token);
                app.palette_recent.truncate(5);
                let panel_cwd = app.active_panel().path.clone();
                crate::lua_apps::launch_lua_application_with_cwd(&app_id, &[], Some(&panel_cwd))?;
                app.needs_full_redraw = true;
                return Ok(false);
            }
            if let Some(action) = action {
                return execute_menu_action(app, action);
            }
        }
        _ => {
            app.mode = AppMode::CommandPalette(state);
        }
    }
    Ok(false)
}

pub(super) fn handle_store_install_palette(app: &mut App, key: KeyEvent) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let fn_key = fx_shortcut(key);

    let mut state = match std::mem::replace(&mut app.mode, AppMode::Browse) {
        AppMode::StoreInstallPalette(state) => state,
        other => {
            app.mode = other;
            return Ok(false);
        }
    };

    if state.progress.is_some() {
        app.mode = AppMode::StoreInstallPalette(state);
        return Ok(false);
    }

    if state.methods.is_some() {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                state.methods = None;
            }
            _ => {}
        }
        app.mode = AppMode::StoreInstallPalette(state);
        return Ok(false);
    }

    if state.detect.is_some() {
        match key.code {
            KeyCode::Esc => {
                state.detect = None;
                app.mode = AppMode::StoreInstallPalette(state);
            }
            KeyCode::Up => {
                if let Some(detect) = &mut state.detect
                    && detect.cursor > 0
                {
                    detect.cursor -= 1;
                }
                app.mode = AppMode::StoreInstallPalette(state);
            }
            KeyCode::Down => {
                if let Some(detect) = &mut state.detect
                    && detect.cursor + 1 < detect.items.len()
                {
                    detect.cursor += 1;
                }
                app.mode = AppMode::StoreInstallPalette(state);
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                if let Some(detect) = &mut state.detect
                    && let Some(item) = detect.items.get_mut(detect.cursor)
                {
                    item.choice = match item.choice {
                        StoreDetectChoice::Keep => StoreDetectChoice::Install,
                        StoreDetectChoice::Install => StoreDetectChoice::Remove,
                        StoreDetectChoice::Remove => StoreDetectChoice::Keep,
                    };
                }
                app.mode = AppMode::StoreInstallPalette(state);
            }
            KeyCode::Enter => app.apply_store_detection_choices(state),
            _ => app.mode = AppMode::StoreInstallPalette(state),
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
            return Ok(false);
        }
        _ if fn_key == Some(10) => {
            app.mode = AppMode::Browse;
            return Ok(false);
        }
        KeyCode::Up => state.move_prev(),
        KeyCode::Down => state.move_next(),
        KeyCode::Home => state.match_pos = 0,
        KeyCode::End => {
            state.match_pos = state.filtered_indices().len().saturating_sub(1);
        }
        KeyCode::Backspace => state.pop_query(),
        KeyCode::Char('r') | KeyCode::Char('R') if ctrl && !alt => {
            let index_path = state.index_path.clone();
            let query = state.query.clone();
            let selected_id = state
                .filtered_indices()
                .get(state.match_pos)
                .and_then(|idx| state.items.get(*idx))
                .map(|item| item.id.clone());
            let installed_only = state.installed_only;
            match crate::app::StoreInstallPaletteState::load(index_path.clone()) {
                Ok(mut refreshed) => {
                    refreshed.query = query;
                    refreshed.installed_only = installed_only;
                    if let Some(selected_id) = selected_id
                        && let Some(pos) = refreshed
                            .filtered_indices()
                            .iter()
                            .position(|idx| refreshed.items[*idx].id == selected_id)
                    {
                        refreshed.match_pos = pos;
                    }
                    refreshed.clamp_match();
                    app.notify(format!("Store index refreshed: {}", index_path.display()));
                    app.mode = AppMode::StoreInstallPalette(refreshed);
                }
                Err(e) => {
                    app.notify(format!(
                        "Cannot refresh plugin store index {}: {}",
                        index_path.display(),
                        e
                    ));
                    app.mode = AppMode::StoreInstallPalette(state);
                }
            }
            return Ok(false);
        }
        KeyCode::Char('d') | KeyCode::Char('D') if ctrl && !alt => {
            app.open_store_detection_dialog(state);
            return Ok(false);
        }
        KeyCode::Char('y') | KeyCode::Char('Y') if ctrl && !alt => {
            app.open_store_install_methods_dialog(state);
            return Ok(false);
        }
        KeyCode::Char('i') | KeyCode::Char('I') if ctrl && !alt => {
            state.toggle_installed_only();
            app.mode = AppMode::StoreInstallPalette(state);
            return Ok(false);
        }
        KeyCode::Char('u') | KeyCode::Char('U') if ctrl && !alt => {
            let selected = state
                .filtered_indices()
                .get(state.match_pos)
                .and_then(|idx| state.items.get(*idx))
                .cloned();
            if let Some(item) = selected {
                if matches!(item.item_kind, crate::plugins::StoreItemKind::Application) {
                    app.notify("Application updates are handled by the system package manager");
                    app.mode = AppMode::StoreInstallPalette(state);
                    return Ok(false);
                }
                if !item.from_store {
                    app.notify("Selected item is local-only and cannot be updated from store");
                    app.mode = AppMode::StoreInstallPalette(state);
                    return Ok(false);
                }
                if !state.has_update(&item) {
                    app.notify("Selected plugin is already up to date");
                    app.mode = AppMode::StoreInstallPalette(state);
                    return Ok(false);
                }
                let index_path = state.index_path.clone();
                state.index_path = index_path;
                app.start_store_install(state, item, "Updating plugin from store".to_string());
                return Ok(false);
            }
        }
        KeyCode::Char('o') | KeyCode::Char('O') if ctrl && !alt => {
            let selected = state
                .filtered_indices()
                .get(state.match_pos)
                .and_then(|idx| state.items.get(*idx))
                .cloned();
            if let Some(item) = selected {
                if let Some(dir) = state.plugin_install_dir_for(&item) {
                    if dir.is_dir() {
                        app.mode = AppMode::Browse;
                        if let Err(e) = app.enter_dir(dir.clone()) {
                            app.set_status(format!("Cannot enter install directory: {}", e));
                        } else {
                            app.set_status(format!("Install directory: {}", dir.display()));
                        }
                    } else {
                        app.notify("Install directory is not available");
                        app.mode = AppMode::StoreInstallPalette(state);
                    }
                } else {
                    app.notify("Open directory is only available for plugins");
                    app.mode = AppMode::StoreInstallPalette(state);
                }
                return Ok(false);
            }
        }
        KeyCode::Delete => {
            let selected = state
                .filtered_indices()
                .get(state.match_pos)
                .and_then(|idx| state.items.get(*idx))
                .cloned();

            if let Some(item) = selected {
                let query = state.query.clone();
                let selected_id = item.id.clone();
                let installed_only = state.installed_only;
                let index_path = state.index_path.clone();

                let result = match item.item_kind {
                    crate::plugins::StoreItemKind::Plugin => {
                        if let Some(dir) = state.plugin_install_dir_for(&item) {
                            crate::plugins::remove_plugin(&dir)
                        } else {
                            Err(anyhow::anyhow!("Plugin install directory is unavailable"))
                        }
                    }
                    crate::plugins::StoreItemKind::Application => {
                        crate::plugins::uninstall_store_application(&item)
                    }
                };

                match result {
                    Ok(()) => {
                        if matches!(item.item_kind, crate::plugins::StoreItemKind::Application) {
                            let _ = crate::plugins::remove_store_application(&item.id);
                            let _ = app.save_config();
                        }
                        app.reload_panels();
                        match crate::app::StoreInstallPaletteState::load(index_path) {
                            Ok(mut refreshed) => {
                                refreshed.query = query;
                                refreshed.installed_only = installed_only;
                                if let Some(pos) = refreshed
                                    .filtered_indices()
                                    .iter()
                                    .position(|idx| refreshed.items[*idx].id == selected_id)
                                {
                                    refreshed.match_pos = pos;
                                }
                                refreshed.clamp_match();
                                app.notify(format!("Removed: {}", item.name));
                                app.mode = AppMode::StoreInstallPalette(refreshed);
                            }
                            Err(e) => {
                                app.notify(format!("Refresh after uninstall failed: {}", e));
                                app.mode = AppMode::Browse;
                            }
                        }
                    }
                    Err(e) => {
                        app.notify(format!("Cannot remove item: {}", e));
                        app.mode = AppMode::StoreInstallPalette(state);
                    }
                }
                return Ok(false);
            }
        }
        KeyCode::Char(ch) if !ctrl && !alt && !ch.is_control() => state.append_query(ch),
        KeyCode::Enter => {
            let selected = state
                .filtered_indices()
                .get(state.match_pos)
                .and_then(|idx| state.items.get(*idx))
                .cloned();
            if let Some(item) = selected {
                if !state.can_install(&item) {
                    app.notify("Selected item is local-only; no store install action available");
                    app.mode = AppMode::StoreInstallPalette(state);
                    return Ok(false);
                }
                let index_path = state.index_path.clone();
                let is_application =
                    matches!(item.item_kind, crate::plugins::StoreItemKind::Application);
                let title = if is_application {
                    "Installing application from store"
                } else {
                    "Installing plugin from store"
                };
                state.index_path = index_path;
                app.start_store_install(state, item, title.to_string());
                return Ok(false);
            }
        }
        _ => {}
    }

    app.mode = AppMode::StoreInstallPalette(state);
    Ok(false)
}
