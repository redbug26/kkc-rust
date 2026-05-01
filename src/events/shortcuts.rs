use crate::app::{App, AppMode, PALETTE_DATA, shortcut_from_key_event};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_shortcut_panel(app: &mut App, key: KeyEvent) -> Result<bool> {
    let mut state = match std::mem::replace(&mut app.mode, AppMode::Browse) {
        AppMode::ShortcutPanel(state) => state,
        other => {
            app.mode = other;
            return Ok(false);
        }
    };

    if state.capture {
        match key.code {
            KeyCode::Esc => {
                state.capture = false;
                app.mode = AppMode::ShortcutPanel(state);
            }
            KeyCode::Char('r') | KeyCode::Char('R') if key.modifiers.is_empty() => {
                if let Some(entry) = state
                    .selected_palette_index()
                    .and_then(|idx| PALETTE_DATA.get(idx))
                {
                    app.reset_shortcut_for_fn(entry.fn_name);
                    match app.save_config() {
                        Ok(_) => app.set_status(format!("Shortcut reset: {}", entry.label)),
                        Err(e) => app.set_status(format!("Save error: {}", e)),
                    }
                }
                state.capture = false;
                app.mode = AppMode::ShortcutPanel(state);
            }
            KeyCode::Backspace | KeyCode::Delete => {
                if let Some(entry) = state
                    .selected_palette_index()
                    .and_then(|idx| PALETTE_DATA.get(idx))
                {
                    app.set_shortcut_for_fn(entry.fn_name, None);
                    match app.save_config() {
                        Ok(_) => app.set_status(format!("Shortcut removed: {}", entry.label)),
                        Err(e) => app.set_status(format!("Save error: {}", e)),
                    }
                }
                state.capture = false;
                app.mode = AppMode::ShortcutPanel(state);
            }
            _ => {
                if let Some(shortcut) = shortcut_from_key_event(key) {
                    if let Some(entry) = state
                        .selected_palette_index()
                        .and_then(|idx| PALETTE_DATA.get(idx))
                    {
                        app.set_shortcut_for_fn(entry.fn_name, Some(shortcut.clone()));
                        match app.save_config() {
                            Ok(_) => app.set_status(format!(
                                "Shortcut saved: {} -> {}",
                                entry.label, shortcut
                            )),
                            Err(e) => app.set_status(format!("Save error: {}", e)),
                        }
                    }
                    state.capture = false;
                }
                app.mode = AppMode::ShortcutPanel(state);
            }
        }
        return Ok(false);
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
            return Ok(false);
        }
        KeyCode::F(10) => {
            app.mode = AppMode::Browse;
            return Ok(false);
        }
        KeyCode::Up => state.move_prev(),
        KeyCode::Down => state.move_next(),
        KeyCode::Home => state.cursor = 0,
        KeyCode::End => state.cursor = state.filtered_indices().len().saturating_sub(1),
        KeyCode::Enter | KeyCode::Char(' ') => {
            state.capture = true;
        }
        KeyCode::Backspace => {
            state.query.pop();
            state.cursor = 0;
        }
        KeyCode::Delete => {
            if let Some(entry) = state
                .selected_palette_index()
                .and_then(|idx| PALETTE_DATA.get(idx))
            {
                app.set_shortcut_for_fn(entry.fn_name, None);
                match app.save_config() {
                    Ok(_) => app.set_status(format!("Shortcut removed: {}", entry.label)),
                    Err(e) => app.set_status(format!("Save error: {}", e)),
                }
            }
        }
        KeyCode::Char(ch) if !ctrl && !alt && !ch.is_control() => {
            state.query.push(ch);
            state.cursor = 0;
        }
        _ => {}
    }

    state.clamp_cursor();
    app.mode = AppMode::ShortcutPanel(state);
    Ok(false)
}
