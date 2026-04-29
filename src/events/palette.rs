//! Command palette event handler (Ctrl-P).

use super::fx_shortcut;
use super::menu::execute_menu_action;
use crate::app::{App, AppMode, PALETTE_DATA, PALETTE_SEP};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_command_palette(app: &mut App, key: KeyEvent) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let fn_key = fx_shortcut(key);

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        _ if fn_key == Some(10) => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            if let AppMode::CommandPalette(ref mut s) = app.mode {
                let indices = s.filtered_indices();
                let len = indices.len();
                if len > 0 {
                    let mut pos = if s.match_pos == 0 {
                        len - 1
                    } else {
                        s.match_pos - 1
                    };
                    // Skip over any separator sentinels.
                    let mut guard = 0;
                    while indices.get(pos).copied() == Some(PALETTE_SEP) && guard < len {
                        pos = if pos == 0 { len - 1 } else { pos - 1 };
                        guard += 1;
                    }
                    s.match_pos = pos;
                }
            }
        }
        KeyCode::Down => {
            if let AppMode::CommandPalette(ref mut s) = app.mode {
                let indices = s.filtered_indices();
                let len = indices.len();
                if len > 0 {
                    let mut pos = (s.match_pos + 1) % len;
                    // Skip over any separator sentinels.
                    let mut guard = 0;
                    while indices.get(pos).copied() == Some(PALETTE_SEP) && guard < len {
                        pos = (pos + 1) % len;
                        guard += 1;
                    }
                    s.match_pos = pos;
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
            let (action, data_idx) = if let AppMode::CommandPalette(ref s) = app.mode {
                let indices = s.filtered_indices();
                if let Some(&i) = indices.get(s.match_pos) {
                    if i != PALETTE_SEP {
                        (PALETTE_DATA.get(i).map(|e| e.action), Some(i))
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            };

            // Record in recents: deduplicate then prepend, cap at 5.
            if let Some(idx) = data_idx {
                let fn_name = PALETTE_DATA[idx].fn_name.to_string();
                app.palette_recent.retain(|x| x != &fn_name);
                app.palette_recent.insert(0, fn_name);
                app.palette_recent.truncate(5);
            }

            app.mode = AppMode::Browse;
            if let Some(action) = action {
                return execute_menu_action(app, action);
            }
        }
        _ => {}
    }
    Ok(false)
}
