use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn textarea_input_from_key_event(key: KeyEvent) -> Option<tui_textarea::Input> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let tkey = match key.code {
        KeyCode::Backspace => tui_textarea::Key::Backspace,
        KeyCode::Enter => tui_textarea::Key::Enter,
        KeyCode::Left => tui_textarea::Key::Left,
        KeyCode::Right => tui_textarea::Key::Right,
        KeyCode::Up => tui_textarea::Key::Up,
        KeyCode::Down => tui_textarea::Key::Down,
        KeyCode::Delete => tui_textarea::Key::Delete,
        KeyCode::Home => tui_textarea::Key::Home,
        KeyCode::End => tui_textarea::Key::End,
        KeyCode::PageUp => tui_textarea::Key::PageUp,
        KeyCode::PageDown => tui_textarea::Key::PageDown,
        KeyCode::Esc => tui_textarea::Key::Esc,
        KeyCode::Char(ch) => tui_textarea::Key::Char(ch),
        KeyCode::F(n) => tui_textarea::Key::F(n),
        _ => return None,
    };

    Some(tui_textarea::Input {
        key: tkey,
        ctrl,
        alt,
        shift,
    })
}
