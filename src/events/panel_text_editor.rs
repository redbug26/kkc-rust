use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn textarea_input_from_key_event(key: KeyEvent) -> Option<ratatui_textarea::Input> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let tkey = match key.code {
        KeyCode::Backspace => ratatui_textarea::Key::Backspace,
        KeyCode::Enter => ratatui_textarea::Key::Enter,
        KeyCode::Left => ratatui_textarea::Key::Left,
        KeyCode::Right => ratatui_textarea::Key::Right,
        KeyCode::Up => ratatui_textarea::Key::Up,
        KeyCode::Down => ratatui_textarea::Key::Down,
        KeyCode::Delete => ratatui_textarea::Key::Delete,
        KeyCode::Home => ratatui_textarea::Key::Home,
        KeyCode::End => ratatui_textarea::Key::End,
        KeyCode::PageUp => ratatui_textarea::Key::PageUp,
        KeyCode::PageDown => ratatui_textarea::Key::PageDown,
        KeyCode::Esc => ratatui_textarea::Key::Esc,
        KeyCode::Char(ch) => ratatui_textarea::Key::Char(ch),
        KeyCode::F(n) => ratatui_textarea::Key::F(n),
        _ => return None,
    };

    Some(ratatui_textarea::Input {
        key: tkey,
        ctrl,
        alt,
        shift,
    })
}
