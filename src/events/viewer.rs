use super::fx_shortcut;
use crate::app::{
    App, AppMode, ViewerGotoState, ViewerMenuKind, ViewerMenuState, ViewerPluginPaletteState,
};
use crate::viewer::{EncodingMode, LineFeedMode, MaskKind, PreprocOpKind, ViewMode, Viewer};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

/// Number of display rows available for viewer content in full-screen mode.
/// Full-screen viewer: terminal height − 1 (footer) − 2 (border) = height − 3.
fn viewer_display_rows() -> usize {
    crossterm::terminal::size()
        .map(|(_, h)| (h as usize).saturating_sub(3).max(1))
        .unwrap_or(20)
}

/// Full-screen viewer text-area width (term_width − 2 borders).
fn viewer_text_width(v: &Viewer) -> usize {
    crossterm::terminal::size()
        .map(|(w, _)| {
            let ln = v.line_number_width();
            (w as usize).saturating_sub(2 + ln).max(1)
        })
        .unwrap_or(78)
}

/// Logical lines per page, accounting for word-wrap in text/ansi modes.
fn viewer_page_size(v: &Viewer) -> usize {
    let rows = viewer_display_rows();
    v.page_lines_for(rows, viewer_text_width(v))
}

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
    let fn_key = fx_shortcut(key);

    // '/' and Esc/F3 require moving app.mode; handle them before borrowing.
    match key.code {
        KeyCode::Esc => {
            if let AppMode::Viewer(ref v) = app.mode {
                v.save_position();
            }
            app.mode = AppMode::Browse;
            return Ok(false);
        }
        KeyCode::Char('/') => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            app.mode = AppMode::ViewerSearching(v);
            return Ok(false);
        }
        KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            app.mode = AppMode::ViewerGotoLine(v, String::new());
            return Ok(false);
        }
        KeyCode::Char('g')
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            app.mode = AppMode::ViewerGoto(v, ViewerGotoState::new());
            return Ok(false);
        }
        _ => {}
    }

    if let AppMode::Viewer(ref mut v) = app.mode {
        v.clear_mouse_selection();
    }

    match fn_key {
        Some(10) => {
            if let AppMode::Viewer(ref v) = app.mode {
                v.save_position();
            }
            app.mode = AppMode::Browse;
            return Ok(false);
        }
        Some(7) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            app.mode = AppMode::ViewerSearching(v);
            return Ok(false);
        }
        Some(3) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            let menu = ViewerMenuState::new(ViewerMenuKind::LineFeed, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        Some(4) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            let menu = ViewerMenuState::new(ViewerMenuKind::Mode, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        Some(6) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            let menu = ViewerMenuState::new(ViewerMenuKind::Preproc, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        Some(8) => {
            let AppMode::Viewer(v) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
                return Ok(false);
            };
            let menu = ViewerMenuState::new(ViewerMenuKind::Encoding, &v);
            app.mode = AppMode::ViewerMenu(v, menu);
            return Ok(false);
        }
        Some(9) => {
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

    let page_size = viewer_page_size(v);

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Left => v.scroll_left(40),
            KeyCode::Right => v.scroll_right(40),
            KeyCode::Home => v.scroll_left_max(),
            _ => match key.code {
                KeyCode::Up => v.scroll_up(),
                KeyCode::Down => v.scroll_down(),
                KeyCode::PageUp => v.page_up(page_size),
                KeyCode::PageDown => v.page_down(page_size),
                KeyCode::End => v.goto_end(page_size),
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
            KeyCode::PageUp => v.page_up(page_size),
            KeyCode::PageDown | KeyCode::Char(' ') => v.page_down(page_size),
            KeyCode::Home => v.goto_start(),
            KeyCode::End => v.goto_end(page_size),
            KeyCode::Left => v.scroll_left(8),
            KeyCode::Right => v.scroll_right(8),
            KeyCode::Char('n') => v.search_next(),
            KeyCode::Char('N') => v.search_prev(),
            _ => {}
        }
        match fn_key {
            Some(2) => v.toggle_wrap(),
            Some(5) => v.toggle_zoom(),
            _ => {}
        }
    }
    Ok(false)
}

pub(super) fn handle_mouse_viewer(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    if let Some(key) = viewer_footer_click_to_key(app, mouse) {
        return handle_viewer(app, key);
    }

    let AppMode::Viewer(ref mut viewer) = app.mode else {
        return Ok(false);
    };

    let Some((inner, text_width, visible_rows)) = viewer_mouse_text_layout(viewer) else {
        return Ok(false);
    };

    match mouse.kind {
        MouseEventKind::ScrollUp => {
            viewer.clear_mouse_selection();
            viewer.scroll_up();
        }
        MouseEventKind::ScrollDown => {
            viewer.clear_mouse_selection();
            viewer.scroll_down();
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in_rect(mouse.column, mouse.row, inner)
                && viewer.supports_mouse_text_selection()
            {
                let row = mouse.row.saturating_sub(inner.y) as usize;
                let col = mouse
                    .column
                    .saturating_sub(inner.x + viewer.line_number_width() as u16)
                    as usize;
                viewer.start_mouse_selection(row, col, text_width, visible_rows);
            } else {
                viewer.clear_mouse_selection();
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if viewer.supports_mouse_text_selection() {
                let row = mouse.row.clamp(inner.y, inner.bottom().saturating_sub(1)) - inner.y;
                let col = mouse
                    .column
                    .clamp(inner.x + viewer.line_number_width() as u16, inner.right())
                    .saturating_sub(inner.x + viewer.line_number_width() as u16)
                    as usize;
                viewer.update_mouse_selection(row as usize, col);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if let Some(text) = viewer.selected_visible_text(text_width, visible_rows)
                && !text.is_empty()
            {
                match super::copy_text_to_clipboard(&text) {
                    Ok(()) => app.set_status("Selection copied"),
                    Err(err) => app.set_status(format!("Clipboard error: {}", err)),
                }
            }
        }
        _ => {}
    }

    Ok(false)
}

fn viewer_footer_click_to_key(app: &App, mouse: MouseEvent) -> Option<KeyEvent> {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return None;
    }
    let AppMode::Viewer(viewer) = &app.mode else {
        return None;
    };
    let (_, height) = crossterm::terminal::size().ok()?;
    if mouse.row != height.saturating_sub(1) {
        return None;
    }
    let shortcuts = crate::ui::viewer_footer_shortcuts(viewer);
    crate::ui::footer_shortcut_key_at_column(&shortcuts, 0, mouse.column).map(KeyEvent::from)
}

fn viewer_mouse_text_layout(viewer: &Viewer) -> Option<(Rect, usize, usize)> {
    let (width, height) = crossterm::terminal::size().ok()?;
    if height == 0 {
        return None;
    }
    let term_area = Rect {
        x: 0,
        y: 0,
        width,
        height,
    };
    let viewer_host = Rect {
        x: term_area.x,
        y: term_area.y,
        width: term_area.width,
        height: term_area.height.saturating_sub(1),
    };
    let area = crate::ui::viewer_area(viewer, viewer_host);
    let inner = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let ln_width = viewer.line_number_width();
    let text_width = inner.width.saturating_sub(ln_width as u16) as usize;
    let visible_rows = inner.height as usize;
    (inner.width > 0 && inner.height > 0 && text_width > 0).then_some((
        inner,
        text_width,
        visible_rows,
    ))
}

fn point_in_rect(column: u16, row: u16, rect: Rect) -> bool {
    column >= rect.x && column < rect.right() && row >= rect.y && row < rect.bottom()
}

const VIEWER_GOTO_ITEMS: &[(&str, &str)] = &[
    ("g", "Goto line number <n> else file start"),
    ("e", "Goto last line"),
    ("s", "Goto first non-blank"),
    ("n", "Goto next page"),
    ("p", "Goto previous page"),
];

pub(super) fn handle_viewer_goto(app: &mut App, key: KeyEvent) -> Result<bool> {
    let fn_key = fx_shortcut(key);
    let (viewer, mut state) = match std::mem::replace(&mut app.mode, AppMode::Browse) {
        AppMode::ViewerGoto(viewer, state) => (viewer, state),
        other => {
            app.mode = other;
            return Ok(false);
        }
    };

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Viewer(viewer);
            return Ok(false);
        }
        _ if fn_key == Some(10) => {
            viewer.save_position();
            app.mode = AppMode::Browse;
            return Ok(false);
        }
        KeyCode::Up => state.cursor = viewer_goto_prev_cursor(state.cursor),
        KeyCode::Down => state.cursor = viewer_goto_next_cursor(state.cursor),
        KeyCode::Home => state.cursor = 0,
        KeyCode::End => state.cursor = VIEWER_GOTO_ITEMS.len().saturating_sub(1),
        KeyCode::Backspace | KeyCode::Delete => {
            state.count.pop();
        }
        KeyCode::Enter => {
            apply_viewer_goto_selection(app, viewer, state);
            return Ok(false);
        }
        KeyCode::Char(ch)
            if ch.is_ascii_digit()
                && !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            if state.count.len() < 9 {
                state.count.push(ch);
            }
        }
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            let ch = ch.to_ascii_lowercase();
            if let Some(idx) = VIEWER_GOTO_ITEMS
                .iter()
                .position(|(shortcut, _)| shortcut.starts_with(ch))
            {
                state.cursor = idx;
                apply_viewer_goto_selection(app, viewer, state);
                return Ok(false);
            }
        }
        _ => {}
    }

    app.mode = AppMode::ViewerGoto(viewer, state);
    Ok(false)
}

fn viewer_goto_next_cursor(cursor: usize) -> usize {
    if cursor + 1 >= VIEWER_GOTO_ITEMS.len() {
        0
    } else {
        cursor + 1
    }
}

fn viewer_goto_prev_cursor(cursor: usize) -> usize {
    if cursor == 0 {
        VIEWER_GOTO_ITEMS.len().saturating_sub(1)
    } else {
        cursor - 1
    }
}

fn apply_viewer_goto_selection(app: &mut App, mut viewer: Viewer, state: ViewerGotoState) {
    let page_size = viewer_page_size(&viewer);
    match state.cursor {
        0 => {
            if let Ok(line) = state.count.parse::<usize>() {
                if line > 0 {
                    viewer.goto_line(line - 1);
                } else {
                    viewer.goto_start();
                }
            } else {
                viewer.goto_start();
            }
        }
        1 => viewer.goto_end(viewer_page_size(&viewer)),
        2 => viewer.goto_first_non_blank(),
        3 => viewer.page_down(page_size),
        4 => viewer.page_up(page_size),
        _ => {}
    }
    app.mode = AppMode::Viewer(viewer);
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
        ViewerMenuKind::Mask => 10usize,
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
        KeyCode::Char(ch) => {
            if let Some(cursor) = viewer_menu_shortcut(&viewer, menu.kind, menu.cursor, ch) {
                menu.cursor = cursor;
                apply_viewer_menu_selection(app, viewer, menu);
                return Ok(false);
            }
        }
        KeyCode::Enter => {
            apply_viewer_menu_selection(app, viewer, menu);
            return Ok(false);
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
            "Auto detect",
            "C / C++",
            "Rust",
            "JavaScript / TS",
            "Python",
            "PHP",
            "HTML / XML",
            "CSS / SCSS",
            "SQL",
            "Shell / Bash",
            "Pascal",
            "Assembler",
            "Ketchup",
            "Syntax OFF",
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

fn apply_viewer_menu_selection(
    app: &mut App,
    mut viewer: crate::viewer::Viewer,
    mut menu: ViewerMenuState,
) {
    match menu.kind {
        ViewerMenuKind::Mode => {
            if menu.cursor == VIEWER_PLUGIN_MENU_INDEX {
                let state = ViewerPluginPaletteState::load(&viewer);
                app.mode = AppMode::ViewerPluginPalette(viewer, state);
                return;
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
                return;
            }
            if let Some(kind) = preproc_add_item_kind(&viewer, menu.cursor) {
                viewer.push_preproc(kind, menu.param);
                menu.cursor = viewer.preproc_len().saturating_sub(1);
            } else if is_preproc_clear_item(&viewer, menu.cursor) {
                viewer.clear_preproc();
                menu.cursor = viewer_menu_first_cursor(&viewer, menu.kind);
            }
            app.mode = AppMode::ViewerMenu(viewer, menu);
            return;
        }
        ViewerMenuKind::Encoding => {
            let mode = match menu.cursor {
                0 => EncodingMode::Plain,
                _ => EncodingMode::Cp437,
            };
            viewer.set_encoding(mode);
        }
        ViewerMenuKind::Mask => match menu.cursor {
            0 => viewer.set_mask(Some(MaskKind::Auto)),
            1 => viewer.set_mask(Some(MaskKind::C)),
            2 => viewer.set_mask(Some(MaskKind::Rust)),
            3 => viewer.set_mask(Some(MaskKind::JavaScript)),
            4 => viewer.set_mask(Some(MaskKind::Python)),
            5 => viewer.set_mask(Some(MaskKind::Php)),
            6 => viewer.set_mask(Some(MaskKind::Html)),
            7 => viewer.set_mask(Some(MaskKind::Css)),
            8 => viewer.set_mask(Some(MaskKind::Sql)),
            9 => viewer.set_mask(Some(MaskKind::Shell)),
            10 => viewer.set_mask(Some(MaskKind::Pascal)),
            11 => viewer.set_mask(Some(MaskKind::Assembler)),
            12 => viewer.set_mask(Some(MaskKind::Ketchup)),
            _ => viewer.set_mask(None),
        },
    }
    app.mode = AppMode::Viewer(viewer);
}

fn viewer_menu_shortcut(
    viewer: &crate::viewer::Viewer,
    kind: ViewerMenuKind,
    current: usize,
    ch: char,
) -> Option<usize> {
    if let Some(digit) = ch.to_digit(10)
        && (1..=9).contains(&digit)
    {
        let cursor = digit as usize - 1;
        return (cursor < viewer_menu_len(viewer, kind)
            && !(kind == ViewerMenuKind::Preproc && is_preproc_separator(viewer, cursor)))
        .then_some(cursor);
    }

    let ch = ch.to_ascii_lowercase();
    if kind == ViewerMenuKind::Preproc {
        return viewer_preproc_shortcut(viewer, current, ch);
    }

    let labels = viewer_menu_labels(viewer, kind);
    let mnemonics = mnemonics_for_labels(&labels);
    let len = labels.len();
    (1..=len)
        .map(|offset| (current + offset) % len)
        .find(|&idx| mnemonics.get(idx).copied().flatten() == Some(ch))
}

fn viewer_preproc_shortcut(
    viewer: &crate::viewer::Viewer,
    current: usize,
    ch: char,
) -> Option<usize> {
    let labels = viewer_menu_labels(viewer, ViewerMenuKind::Preproc);
    let mnemonics = mnemonics_for_labels(&labels);
    let len = labels.len();
    (1..=len)
        .map(|offset| (current + offset) % len)
        .find(|&idx| {
            !is_preproc_separator(viewer, idx) && mnemonics.get(idx).copied().flatten() == Some(ch)
        })
}

fn viewer_menu_labels(viewer: &crate::viewer::Viewer, kind: ViewerMenuKind) -> Vec<String> {
    match kind {
        ViewerMenuKind::Preproc => {
            let mut labels = Vec::new();
            for idx in 0..viewer.preproc_len() {
                if let Some(label) = viewer.preproc_item_label(idx) {
                    labels.push(label);
                }
            }
            if viewer.preproc_len() > 0 {
                labels.push(String::new());
            }
            labels.extend(
                PREPROC_ADD_ITEMS
                    .iter()
                    .map(|(label, _)| (*label).to_string()),
            );
            labels.push("Clear All".into());
            labels
        }
        _ => viewer_menu_items(kind)
            .iter()
            .map(|label| (*label).to_string())
            .collect(),
    }
}

fn mnemonics_for_labels(labels: &[String]) -> Vec<Option<char>> {
    let mut used = Vec::new();
    labels
        .iter()
        .map(|label| {
            let candidates = label
                .chars()
                .filter(|ch| ch.is_alphanumeric())
                .map(|ch| ch.to_ascii_lowercase())
                .collect::<Vec<_>>();
            let chosen = candidates
                .iter()
                .copied()
                .find(|candidate| !used.contains(candidate))
                .or_else(|| candidates.first().copied());
            if let Some(ch) = chosen {
                used.push(ch);
            }
            chosen
        })
        .collect()
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
    let fn_key = fx_shortcut(key);
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
        _ if fn_key == Some(10) => {
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

pub(super) fn handle_viewer_goto_line(app: &mut App, key: KeyEvent) -> Result<bool> {
    let fn_key = fx_shortcut(key);
    match key.code {
        KeyCode::Esc => {
            let AppMode::ViewerGotoLine(v, _) = std::mem::replace(&mut app.mode, AppMode::Browse)
            else {
                return Ok(false);
            };
            app.mode = AppMode::Viewer(v);
        }
        _ if fn_key == Some(10) => {
            if let AppMode::ViewerGotoLine(ref v, _) = app.mode {
                v.save_position();
            }
            app.mode = AppMode::Browse;
        }
        KeyCode::Enter => {
            let AppMode::ViewerGotoLine(mut v, input) =
                std::mem::replace(&mut app.mode, AppMode::Browse)
            else {
                return Ok(false);
            };
            if matches!(v.mode, ViewMode::Hex) {
                // Hex mode: input is a byte offset in hex
                if let Ok(offset) = usize::from_str_radix(&input, 16) {
                    let bpr = v.hex_bytes_per_row.get().max(1);
                    v.goto_line(offset / bpr);
                }
            } else if let Ok(n) = input.parse::<usize>() {
                if n > 0 {
                    v.goto_line(n - 1);
                }
            }
            app.mode = AppMode::Viewer(v);
        }
        KeyCode::Backspace => {
            if let AppMode::ViewerGotoLine(_, ref mut input) = app.mode {
                input.pop();
            }
        }
        KeyCode::Char(ch) if ch.is_ascii_hexdigit() => {
            if let AppMode::ViewerGotoLine(ref v, ref mut input) = app.mode {
                // In text mode restrict to decimal digits
                if matches!(v.mode, ViewMode::Hex) || ch.is_ascii_digit() {
                    input.push(ch);
                }
            }
        }
        _ => {}
    }
    Ok(false)
}
