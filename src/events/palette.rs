//! Command palette event handler (Ctrl-P).

use super::menu::execute_menu_action;
use crate::app::{App, AppMode, PALETTE_DATA};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_command_palette(app: &mut App, key: KeyEvent) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Esc | KeyCode::F(10) => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            if let AppMode::CommandPalette(ref mut s) = app.mode {
                let len = s.filtered_indices().len();
                if len > 0 {
                    // Wrap-around: first → last
                    s.match_pos = if s.match_pos == 0 { len - 1 } else { s.match_pos - 1 };
                }
            }
        }
        KeyCode::Down => {
            if let AppMode::CommandPalette(ref mut s) = app.mode {
                let len = s.filtered_indices().len();
                if len > 0 {
                    // Wrap-around: last → first
                    s.match_pos = (s.match_pos + 1) % len;
                }
            }
        }
        KeyCode::Backspace => {
            if let AppMode::CommandPalette(ref mut s) = app.mode {
                s.query.pop();
                s.match_pos = 0;
            }
        }
        KeyCode::Char(ch) if !ctrl && !alt => {
            if let AppMode::CommandPalette(ref mut s) = app.mode {
                s.query.push(ch);
                s.match_pos = 0;
            }
        }
        KeyCode::Enter => {
            let action = if let AppMode::CommandPalette(ref s) = app.mode {
                let indices = s.filtered_indices();
                indices
                    .get(s.match_pos)
                    .and_then(|&i| PALETTE_DATA.get(i))
                    .map(|e| e.action)
            } else {
                None
            };
            app.mode = AppMode::Browse;
            if let Some(action) = action {
                return execute_menu_action(app, action);
            }
        }
        _ => {}
    }
    Ok(false)
}
