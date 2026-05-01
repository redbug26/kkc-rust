use super::{App, MenuAction, PALETTE_DATA};
use crate::config::ShortcutOverride;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct ShortcutPanelState {
    pub query: String,
    pub cursor: usize,
    pub capture: bool,
}

impl ShortcutPanelState {
    pub fn filtered_indices(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        if q.is_empty() {
            return (0..PALETTE_DATA.len()).collect();
        }
        PALETTE_DATA
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                format!(
                    "{} {} {} {}",
                    entry.category,
                    entry.label,
                    entry.fn_name,
                    entry.shortcut.unwrap_or("")
                )
                .to_lowercase()
                .contains(&q)
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn clamp_cursor(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.cursor = 0;
        } else {
            self.cursor = self.cursor.min(len.saturating_sub(1));
        }
    }

    pub fn move_prev(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor == 0 {
            self.cursor = len - 1;
        } else {
            self.cursor -= 1;
        }
    }

    pub fn move_next(&mut self) {
        let len = self.filtered_indices().len();
        if len == 0 {
            self.cursor = 0;
        } else {
            self.cursor = (self.cursor + 1) % len;
        }
    }

    pub fn selected_palette_index(&self) -> Option<usize> {
        self.filtered_indices().get(self.cursor).copied()
    }
}

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
}
