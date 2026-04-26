use crate::app::{App, AppMode, ViewerMenuKind, ViewerMenuState, ViewerPluginPaletteState};
use crate::viewer::{EncodingMode, LineFeedMode, MaskKind, PreprocOpKind, ViewMode};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Convert a viewer key event to a plugin-facing key string.
/// Returns `None` for Ctrl-modified keys and unrecognised key codes.
fn keyevent_to_plugin_key(key: KeyEvent) -> Option<String> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return None;
    }
    Some(match key.code {
        KeyCode::Char(c) => format!("char:{c}"),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pgup".into(),
        KeyCode::PageDown => "pgdown".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "backtab".into(),
        KeyCode::F(n) => format!("f{n}"),
        _ => return None,
    })
}

pub(super) fn handle_viewer(app: &mut App, key: KeyEvent) -> Result<bool> {
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
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            app.mode = AppMode::ViewerSearching(v);
            return Ok(false);
        }
        KeyCode::F(3) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            let menu = ViewerMenuState::new(ViewerMenuKind::LineFeed, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        KeyCode::F(4) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            let menu = ViewerMenuState::new(ViewerMenuKind::Mode, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        KeyCode::F(6) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            let menu = ViewerMenuState::new(ViewerMenuKind::Preproc, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        KeyCode::F(8) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            let menu = ViewerMenuState::new(ViewerMenuKind::Encoding, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        KeyCode::F(9) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
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
            _ => match key.code {
                KeyCode::Up => v.scroll_up(),
                KeyCode::Down => v.scroll_down(),
                KeyCode::PageUp => v.page_up(20),
                KeyCode::PageDown => v.page_down(20),
                KeyCode::End => v.goto_end(20),
                KeyCode::Char('n') => v.search_next(),
                KeyCode::Char('N') => v.search_prev(),
                _ => {}
            },
        }
    } else {
        // Let the active viewer plugin intercept the key first.
        if let Some(key_str) = keyevent_to_plugin_key(key)
            && v.handle_plugin_key(&key_str)
        {
            return Ok(false);
        }
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
            KeyCode::Char('n') => v.search_next(),
            KeyCode::Char('N') => v.search_prev(),
            _ => {}
        }
    }
    Ok(false)
}

pub(super) fn handle_viewer_menu(app: &mut App, key: KeyEvent) -> Result<bool> {
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
        KeyCode::Up
            if menu.kind == ViewerMenuKind::Preproc
                && key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            let mut viewer = viewer;
            if menu.cursor < viewer.preproc_len() {
                viewer.move_preproc_up(menu.cursor);
                menu.cursor = menu.cursor.saturating_sub(1);
            }
            app.mode = AppMode::ViewerMenu(viewer, menu);
            return Ok(false);
        }
        KeyCode::Down
            if menu.kind == ViewerMenuKind::Preproc
                && key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
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
                if cursor == VIEWER_PLUGIN_MENU_INDEX {
                    let state = ViewerPluginPaletteState::load(&viewer);
                    app.mode = AppMode::ViewerPluginPalette(viewer, state);
                } else {
                    set_viewer_mode(&mut viewer, cursor);
                    app.mode = AppMode::Viewer(viewer);
                }
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
                    if menu.cursor == VIEWER_PLUGIN_MENU_INDEX {
                        let state = ViewerPluginPaletteState::load(&viewer);
                        app.mode = AppMode::ViewerPluginPalette(viewer, state);
                        return Ok(false);
                    }
                    set_viewer_mode(&mut viewer, menu.cursor);
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
                ViewerMenuKind::Mask => match menu.cursor {
                    0 => viewer.set_mask(Some(MaskKind::C)),
                    1 => viewer.set_mask(Some(MaskKind::Pascal)),
                    2 => viewer.set_mask(Some(MaskKind::Assembler)),
                    3 => viewer.set_mask(Some(MaskKind::Ketchup)),
                    _ => viewer.set_mask(None),
                },
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
        ViewerMenuKind::Mode => &["Text", "Binary", "Ansi", "Image", "Plugins viewer"],
        ViewerMenuKind::LineFeed => &["DOS (CR/LF)", "Unix (LF)", "Mac (CR)", "Mixed"],
        ViewerMenuKind::Encoding => &["Plain ASCII", "DOS CP437"],
        ViewerMenuKind::Mask => &[
            "C Style",
            "Pascal Style",
            "Assembler Style",
            "Ketchup Style",
            "Mask OFF",
        ],
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
const VIEWER_PLUGIN_MENU_INDEX: usize = 4;

fn viewer_menu_len(viewer: &crate::viewer::Viewer, kind: ViewerMenuKind) -> usize {
    match kind {
        ViewerMenuKind::Mode => viewer_menu_items(kind).len(),
        ViewerMenuKind::Preproc => {
            let existing = viewer.preproc_len();
            let separator = usize::from(existing > 0);
            existing + separator + PREPROC_ADD_ITEMS.len() + 1
        }
        _ => viewer_menu_items(kind).len(),
    }
}

fn set_viewer_mode(viewer: &mut crate::viewer::Viewer, cursor: usize) {
    match cursor {
        0 => viewer.set_mode(ViewMode::Text),
        1 => viewer.set_mode(ViewMode::Hex),
        2 => viewer.set_mode(ViewMode::Ansi),
        3 => viewer.set_mode(ViewMode::Image),
        _ => {}
    }
}

fn viewer_mode_shortcut(ch: char) -> Option<usize> {
    if let Some(digit) = ch.to_digit(10)
        && (1..=9).contains(&digit)
    {
        return Some(digit as usize - 1);
    }

    match ch.to_ascii_lowercase() {
        't' => Some(0),
        'b' => Some(1),
        'a' => Some(2),
        'i' => Some(3),
        'p' => Some(VIEWER_PLUGIN_MENU_INDEX),
        _ => None,
    }
}

pub(super) fn handle_viewer_plugin_palette(app: &mut App, key: KeyEvent) -> Result<bool> {
    let (viewer, mut state) = match std::mem::replace(&mut app.mode, AppMode::Browse) {
        AppMode::ViewerPluginPalette(viewer, state) => (viewer, state),
        other => {
            app.mode = other;
            return Ok(false);
        }
    };

    match key.code {
        KeyCode::Esc => {
            let mut menu = ViewerMenuState::new(ViewerMenuKind::Mode, &viewer);
            menu.cursor = VIEWER_PLUGIN_MENU_INDEX;
            app.mode = AppMode::ViewerMenu(viewer, menu);
            return Ok(false);
        }
        KeyCode::Up => state.move_prev(),
        KeyCode::Down => state.move_next(),
        KeyCode::Home => state.match_pos = 0,
        KeyCode::End => {
            state.match_pos = state.filtered_indices().len().saturating_sub(1);
        }
        KeyCode::Backspace => state.pop_query(),
        KeyCode::Enter => {
            let selected = state
                .filtered_indices()
                .get(state.match_pos)
                .and_then(|idx| state.items.get(*idx))
                .cloned();
            let mut viewer = viewer;
            if let Some(plugin) = selected {
                viewer.set_viewer_plugin(plugin.name);
                app.mode = AppMode::Viewer(viewer);
            } else {
                app.mode = AppMode::ViewerPluginPalette(viewer, state);
            }
            return Ok(false);
        }
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !ch.is_control() =>
        {
            state.append_query(ch);
        }
        _ => {}
    }

    app.mode = AppMode::ViewerPluginPalette(viewer, state);
    Ok(false)
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

fn viewer_menu_next_cursor(
    viewer: &crate::viewer::Viewer,
    kind: ViewerMenuKind,
    cursor: usize,
) -> usize {
    let last = viewer_menu_last_cursor(viewer, kind);
    let mut next = if cursor >= last { 0 } else { cursor + 1 };
    if kind == ViewerMenuKind::Preproc && is_preproc_separator(viewer, next) {
        next = if next >= last { 0 } else { next + 1 };
    }
    next
}

fn viewer_menu_prev_cursor(
    viewer: &crate::viewer::Viewer,
    kind: ViewerMenuKind,
    cursor: usize,
) -> usize {
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

pub(super) fn handle_viewer_searching(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => {
            // Clear search and return to normal viewer
            if let AppMode::ViewerSearching(ref mut v) = app.mode {
                v.search.clear();
                v.matches.clear();
            }
            let AppMode::ViewerSearching(v) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else {
                return Ok(false);
            };
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
            else {
                return Ok(false);
            };
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
