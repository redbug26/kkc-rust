mod menu;
mod palette;
mod panel_text_editor;
mod viewer;

use self::menu::handle_menu;
use self::palette::{handle_command_palette, handle_store_install_palette};
use self::panel_text_editor::textarea_input_from_key_event;
use self::viewer::{
    handle_audio_player_palette, handle_mouse_viewer, handle_viewer, handle_viewer_goto,
    handle_viewer_goto_line, handle_viewer_menu, handle_viewer_plugin_palette,
    handle_viewer_searching,
};
use crate::app::{
    ActivePanel, App, AppMode, AssocEditorState, AssocInputAction, AssocInputDialog,
    BookmarkListItem, ConfigState, ConfirmAction, ConfirmButton, ConfirmDialog, InputAction,
    InputDialog, MENU_DATA, MENU_HEADERS, MenuAction, MenuState, OpenerActionItem,
    OpenerActionKind, OpenerState, RemoteEditKind, TextInputState,
};
use crate::archive::supports_archive_navigation;
use crate::compare::{jump_to_compare_search_match, rebuild_compare_panel_state};
use crate::copy::CopyDialogState;
use crate::remote::{
    RemoteKind, RemoteSource, download_to_temp, join_remote, load_profiles,
    make_dir as remote_make_dir, rename_path as remote_rename_path, upload_into_dir,
};
use crate::viewer::ViewMode;
use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use std::io::{self, Write};

pub(crate) fn fx_shortcut(key: KeyEvent) -> Option<u8> {
    match key.code {
        KeyCode::F(n) => Some(n),
        KeyCode::Char(c)
            if key.modifiers.contains(KeyModifiers::ALT)
                && !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            match c {
                '1'..='9' => Some((c as u8) - b'0'),
                '0' => Some(10),
                _ => None,
            }
        }
        _ => None,
    }
}

fn remote_plugin_auth_start_feedback(auth_session: &str) -> (String, Vec<String>) {
    let fallback = "Plugin auth started. Complete authentication, then press F6".to_string();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(auth_session) else {
        return (fallback, Vec::new());
    };
    let field = |name: &str| {
        value
            .get(name)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let auth_type = field("type");
    let instructions = field("instructions");
    let message = field("message");
    let auth_url = field("verification_uri_complete")
        .or_else(|| field("verification_uri"))
        .or_else(|| field("auth_url"));
    let user_code = field("user_code");

    let mut details = Vec::new();
    if let Some(line) = instructions {
        details.push(line.to_string());
    }
    if let Some(line) = message
        && details.iter().all(|existing| existing != line)
    {
        details.push(line.to_string());
    }
    if let Some(url) = auth_url {
        details.push(format!("Open: {url}"));
    }
    if let Some(code) = user_code {
        details.push(format!("Code: {code}"));
    }

    let status = match (auth_type, auth_url, user_code) {
        (Some("device_code"), Some(url), Some(code)) => {
            format!("Open {url}, enter code {code}, finish sign-in, then press F6")
        }
        (Some("device_code"), Some(url), None) => {
            format!("Open {url}, finish sign-in, then press F6")
        }
        (Some("authorization_code_pkce"), Some(url), _) => {
            format!("Open {url}, paste the returned code/value here, then press F6")
        }
        (_, Some(url), Some(code)) => {
            format!("Open {url}, use code {code}, then press F6")
        }
        (_, Some(url), None) => format!("Open {url}, then press F6"),
        _ => instructions
            .or(message)
            .map(ToOwned::to_owned)
            .unwrap_or(fallback),
    };

    (status, details)
}

pub fn handle_event(app: &mut App, event: Event) -> Result<bool> {
    match event {
        Event::Key(key) => {
            if app.action_for_key(key) == Some(MenuAction::CaptureGif) {
                app.capture_gif = true;
                return Ok(false);
            }

            if app.action_for_key(key) == Some(MenuAction::Quit) {
                return menu::execute_menu_action(app, MenuAction::Quit);
            }

            if let Some(result) = handle_key_mode(app, key) {
                return result;
            }

            handle_browse(app, key)
        }
        Event::Mouse(mouse) => handle_mouse(app, mouse),
        _ => Ok(false),
    }
}

fn handle_key_mode(app: &mut App, key: KeyEvent) -> Option<Result<bool>> {
    match &app.mode {
        AppMode::MatrixScreensaver(_) => {
            app.mode = AppMode::Browse;
            Some(Ok(false))
        }
        AppMode::Help(_) => Some(handle_help(app, key)),
        AppMode::Viewer(_) => Some(handle_viewer(app, key)),
        AppMode::ViewerSearching(_) => Some(handle_viewer_searching(app, key)),
        AppMode::ViewerGotoLine(_, _) => Some(handle_viewer_goto_line(app, key)),
        AppMode::ViewerGoto(_, _) => Some(handle_viewer_goto(app, key)),
        AppMode::ViewerMenu(_, _) => Some(handle_viewer_menu(app, key)),
        AppMode::ViewerPluginPalette(_, _) => Some(handle_viewer_plugin_palette(app, key)),
        AppMode::AudioPlayerPalette(_, _) => Some(handle_audio_player_palette(app, key)),
        AppMode::Confirm(_) => Some(handle_confirm(app, key)),
        AppMode::Input(_) => Some(handle_input(app, key)),
        AppMode::AssocInput(_) => Some(handle_assoc_input(app, key)),
        AppMode::CopyDialog(_) => Some(handle_copy_dialog(app, key)),
        AppMode::CopyProgress(_) => Some(handle_copy_progress(app, key)),
        AppMode::SearchPanel(_) => Some(handle_search(app, key)),
        AppMode::ComparePanel(_) => Some(handle_compare_panel(app, key)),
        AppMode::TreeView(_) => Some(handle_tree_view(app, key)),
        AppMode::DirBookmarks => Some(handle_dir_bookmarks(app, key)),
        AppMode::QuickSearch => Some(handle_quicksearch(app, key)),
        AppMode::Menu(_) => Some(handle_menu(app, key)),
        AppMode::Config(_) => Some(handle_config(app, key)),
        AppMode::Plugins(_) => Some(handle_plugins(app, key)),
        AppMode::ActionPalette(_) => Some(handle_action_palette(app, key)),
        AppMode::CommandPalette(_) => Some(handle_command_palette(app, key)),
        AppMode::StoreInstallPalette(_) => Some(handle_store_install_palette(app, key)),
        AppMode::Opener(_) => Some(handle_opener(app, key)),
        AppMode::AssocEditor(_) => Some(handle_assoc_editor(app, key)),
        AppMode::RemoteConnect(_) => Some(handle_remote_connect(app, key)),
        AppMode::RemoteEdit(_) => Some(handle_remote_edit(app, key)),
        AppMode::RemoteAddMenu(_) => Some(handle_remote_add_menu(app, key)),
        AppMode::RemoteConnecting(_) => Some(handle_remote_connecting(app, key)),
        AppMode::Terminal => Some(crate::terminal::handle_terminal(app, key)),
        AppMode::About(_) => Some(handle_about(app, key)),
        AppMode::Browse => None,
    }
}

#[derive(Clone, Copy)]
struct MainMouseLayout {
    left_panel: Rect,
    center: Rect,
    right_panel: Rect,
    status: Rect,
    fkey: Option<Rect>,
}

fn handle_mouse(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    match &app.mode {
        AppMode::Browse => handle_mouse_browse(app, mouse),
        AppMode::MatrixScreensaver(_) => {
            app.mode = AppMode::Browse;
            Ok(false)
        }
        AppMode::Viewer(_) => handle_mouse_viewer(app, mouse),
        AppMode::RemoteConnect(_) => handle_mouse_remote_connect(app, mouse),
        AppMode::Menu(_) => handle_mouse_menu(app, mouse),
        AppMode::Confirm(_) => handle_mouse_confirm(app, mouse),
        AppMode::Input(_) => handle_mouse_input(app, mouse),
        AppMode::AssocInput(_) => handle_mouse_assoc_input(app, mouse),
        AppMode::CopyDialog(_) => handle_mouse_copy_dialog(app, mouse),
        AppMode::AssocEditor(_) => handle_mouse_assoc_editor(app, mouse),
        AppMode::RemoteAddMenu(_) => handle_mouse_remote_add_menu(app, mouse),
        AppMode::RemoteEdit(_) => handle_mouse_remote_edit(app, mouse),
        AppMode::RemoteConnecting(_) => handle_mouse_remote_connecting(app, mouse),
        AppMode::DirBookmarks => handle_mouse_dir_bookmarks(app, mouse),
        AppMode::Plugins(_) => handle_mouse_plugins(app, mouse),
        AppMode::CommandPalette(_) => handle_mouse_command_palette(app, mouse),
        AppMode::Help(_) => handle_mouse_help(app, mouse),
        AppMode::SearchPanel(_) => handle_mouse_search_panel(app, mouse),
        AppMode::StoreInstallPalette(_) => handle_mouse_store_install_palette(app, mouse),
        _ => Ok(false),
    }
}

fn handle_mouse_remote_connect(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    if handle_status_copy_click(app, mouse, area) {
        return Ok(false);
    }
    let (_popup, _inner, list_area, hint_area) = remote_connect_rect(area);

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in_rect(mouse.column, mouse.row, list_area) {
                let row = (mouse.row - list_area.y) as usize;
                let (clicked_match_idx, profile_opt) =
                    if let AppMode::RemoteConnect(ref s) = app.mode {
                        let rows = list_area.height as usize;
                        let scroll = if s.match_pos >= rows && rows > 0 {
                            s.match_pos - rows + 1
                        } else {
                            0
                        };
                        let clicked = scroll + row;
                        let profile = s
                            .filtered_indices()
                            .get(clicked)
                            .and_then(|idx| s.items.get(*idx))
                            .cloned();
                        (Some(clicked), profile)
                    } else {
                        (None, None)
                    };

                if let Some(clicked) = clicked_match_idx
                    && let AppMode::RemoteConnect(ref mut s) = app.mode
                {
                    let same = s.match_pos == clicked;
                    s.match_pos = clicked.min(s.filtered_indices().len().saturating_sub(1));
                    if same && let Some(profile) = profile_opt {
                        let return_state = s.clone();
                        app.start_remote_connect(profile, return_state);
                    }
                }
                return Ok(false);
            }

            if point_in_rect(mouse.column, mouse.row, hint_area)
                && let Some(key) = crate::ui::footer_shortcut_key_at_column(
                    &crate::ui::remote_connect_shortcuts(),
                    hint_area.x,
                    mouse.column,
                )
            {
                return handle_remote_connect(app, KeyEvent::from(key));
            }
        }
        MouseEventKind::ScrollUp => {
            if point_in_rect(mouse.column, mouse.row, list_area)
                && let AppMode::RemoteConnect(ref mut s) = app.mode
            {
                s.move_prev();
            }
        }
        MouseEventKind::ScrollDown => {
            if point_in_rect(mouse.column, mouse.row, list_area)
                && let AppMode::RemoteConnect(ref mut s) = app.mode
            {
                s.move_next();
            }
        }
        _ => {}
    }

    Ok(false)
}

fn handle_mouse_remote_add_menu(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    if handle_status_copy_click(app, mouse, area) {
        return Ok(false);
    }
    let choices = RemoteEditKind::all();
    let (popup, inner) = remote_add_menu_rect(area, choices.len());
    let hint_row = Rect {
        x: inner.x,
        y: inner.y + choices.len() as u16,
        width: inner.width,
        height: 1,
    };

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if !point_in_rect(mouse.column, mouse.row, popup) {
                return Ok(false);
            }
            if point_in_rect(mouse.column, mouse.row, hint_row)
                && let Some(key) = crate::ui::footer_shortcut_key_at_column(
                    &crate::ui::remote_add_menu_shortcuts(),
                    hint_row.x,
                    mouse.column,
                )
            {
                return handle_remote_add_menu(app, KeyEvent::from(key));
            }
            if mouse.row >= inner.y && mouse.row < inner.y + choices.len() as u16 {
                let idx = (mouse.row - inner.y) as usize;
                if idx < choices.len() {
                    app.mode =
                        AppMode::RemoteEdit(crate::app::RemoteEditState::new(choices[idx].clone()));
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if point_in_rect(mouse.column, mouse.row, popup)
                && let AppMode::RemoteAddMenu(ref mut cursor) = app.mode
            {
                *cursor = cursor.saturating_sub(1);
            }
        }
        MouseEventKind::ScrollDown => {
            if point_in_rect(mouse.column, mouse.row, popup)
                && let AppMode::RemoteAddMenu(ref mut cursor) = app.mode
            {
                let max = choices.len().saturating_sub(1);
                *cursor = (*cursor + 1).min(max);
            }
        }
        _ => {}
    }

    Ok(false)
}

fn handle_mouse_remote_edit(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    if handle_status_copy_click(app, mouse, area) {
        return Ok(false);
    }
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return Ok(false);
    }
    let AppMode::RemoteEdit(ref state) = app.mode else {
        return Ok(false);
    };
    let (popup, inner) = remote_edit_rect(area, state);
    if !point_in_rect(mouse.column, mouse.row, popup) {
        return Ok(false);
    }

    enum ClickAction {
        None,
        Submit,
        Cancel,
    }

    let mut action = ClickAction::None;

    if let AppMode::RemoteEdit(ref mut s) = app.mode {
        if let Some((ref shares, picker_cur)) = s.share_picker {
            if let Some((dd_area, dd_inner, scroll)) = remote_edit_share_picker_rect(
                area,
                inner,
                s.path_field_index() as u16,
                s.field_value_offset(s.path_field_index()),
                shares.len(),
                picker_cur,
            ) {
                if point_in_rect(mouse.column, mouse.row, dd_area)
                    && point_in_rect(mouse.column, mouse.row, dd_inner)
                {
                    let rel = (mouse.row - dd_inner.y) as usize;
                    let idx = scroll + rel;
                    if idx < shares.len() {
                        let path_idx = s.path_field_index();
                        s.set_field_text(path_idx, shares[idx].clone());
                        s.share_picker = None;
                        s.focus_field_at_end(crate::app::RemoteEditState::SECRET);
                    }
                } else {
                    s.share_picker = None;
                }
            }
            return Ok(false);
        }

        let labels = s.kind.field_labels();
        if mouse.row >= inner.y && mouse.row < inner.y + labels.len() as u16 {
            let idx = (mouse.row - inner.y) as usize;
            let label = labels.get(idx).map(String::as_str).unwrap_or_default();
            if idx < s.input_count() && !label.is_empty() {
                let value_x = inner.x + s.field_value_offset(idx);
                let col = mouse.column.saturating_sub(value_x) as usize;
                s.focus_field_at_column(idx, col);
                return Ok(false);
            }
        }

        let button_y = inner.y + labels.len() as u16 + 1;
        let hint_y = inner.y + labels.len() as u16 + 3;
        if mouse.row == button_y {
            let save_rect = Rect {
                x: inner.x,
                y: button_y,
                width: 10,
                height: 1,
            };
            let cancel_rect = Rect {
                x: inner.x + 12,
                y: button_y,
                width: 10,
                height: 1,
            };
            if point_in_rect(mouse.column, mouse.row, save_rect) {
                s.cursor = s.save_index();
                action = ClickAction::Submit;
            } else if point_in_rect(mouse.column, mouse.row, cancel_rect) {
                s.cursor = s.cancel_index();
                action = ClickAction::Cancel;
            }
        }

        if mouse.row == hint_y {
            let shortcuts = crate::ui::remote_edit_shortcuts(s);
            if let Some(key) =
                crate::ui::footer_shortcut_key_at_column(&shortcuts, inner.x, mouse.column)
            {
                return handle_remote_edit(app, KeyEvent::from(key));
            }
        }
    }

    match action {
        ClickAction::Submit => handle_remote_edit(app, KeyEvent::from(KeyCode::Enter)),
        ClickAction::Cancel => handle_remote_edit(app, KeyEvent::from(KeyCode::Enter)),
        ClickAction::None => Ok(false),
    }
}

fn handle_mouse_dir_bookmarks(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    if handle_status_copy_click(app, mouse, area) {
        return Ok(false);
    }

    let (_popup, _inner, list_area, hint_area) = dir_bookmarks_rect(area, app.bookmarks.len());

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in_rect(mouse.column, mouse.row, hint_area)
                && let Some(key) = crate::ui::footer_shortcut_key_at_column(
                    &crate::ui::dir_bookmarks_shortcuts(),
                    hint_area.x,
                    mouse.column,
                )
                && key != KeyCode::Null
            {
                return handle_dir_bookmarks(app, KeyEvent::from(key));
            }

            if point_in_rect(mouse.column, mouse.row, list_area) {
                let row = (mouse.row - list_area.y) as usize;
                let rows = list_area.height as usize;
                let scroll = if app.bookmark_match_pos >= rows && rows > 0 {
                    app.bookmark_match_pos - rows + 1
                } else {
                    0
                };
                let clicked = scroll + row;
                let total = app.filtered_bookmark_items().len();
                if total > 0 {
                    let same = app.bookmark_match_pos == clicked;
                    app.bookmark_match_pos = clicked.min(total.saturating_sub(1));
                    if same {
                        return handle_dir_bookmarks(app, KeyEvent::from(KeyCode::Enter));
                    }
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if point_in_rect(mouse.column, mouse.row, list_area) {
                app.move_prev_bookmark();
            }
        }
        MouseEventKind::ScrollDown => {
            if point_in_rect(mouse.column, mouse.row, list_area) {
                app.move_next_bookmark();
            }
        }
        _ => {}
    }

    Ok(false)
}

fn handle_mouse_plugins(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    if handle_status_copy_click(app, mouse, area) {
        return Ok(false);
    }

    let AppMode::Plugins(state) = &app.mode else {
        return Ok(false);
    };
    let (_popup, _inner, left_list, hint_area) = plugins_rect(area, state);

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in_rect(mouse.column, mouse.row, hint_area)
                && let Some(key) = crate::ui::footer_shortcut_key_at_column(
                    &crate::ui::plugins_shortcuts(),
                    hint_area.x,
                    mouse.column,
                )
            {
                if key == KeyCode::Char('s') {
                    return handle_plugins(
                        app,
                        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
                    );
                }
                return handle_plugins(app, KeyEvent::from(key));
            }

            if point_in_rect(mouse.column, mouse.row, left_list) {
                let row = (mouse.row - left_list.y) as usize;
                let list_h = left_list.height as usize;
                let current = if let AppMode::Plugins(s) = &app.mode {
                    s.cursor
                } else {
                    0
                };
                let scroll = if current < list_h {
                    0
                } else {
                    current.saturating_sub(list_h.saturating_sub(1))
                };
                let clicked = scroll + row;
                if let AppMode::Plugins(ref mut s) = app.mode {
                    let total = s.filtered_indices().len();
                    if total > 0 {
                        let same = s.cursor == clicked;
                        s.cursor = clicked.min(total.saturating_sub(1));
                        if same {
                            return handle_plugins(app, KeyEvent::from(KeyCode::Enter));
                        }
                    }
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if point_in_rect(mouse.column, mouse.row, left_list) {
                return handle_plugins(app, KeyEvent::from(KeyCode::Up));
            }
        }
        MouseEventKind::ScrollDown => {
            if point_in_rect(mouse.column, mouse.row, left_list) {
                return handle_plugins(app, KeyEvent::from(KeyCode::Down));
            }
        }
        _ => {}
    }

    Ok(false)
}

fn handle_mouse_command_palette(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    if handle_status_copy_click(app, mouse, area) {
        return Ok(false);
    }

    let item_count = if let AppMode::CommandPalette(s) = &app.mode {
        s.filtered_indices().len()
    } else {
        0
    };
    let (_popup, _inner, list_area, hint_area) = command_palette_rect(area, item_count);

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in_rect(mouse.column, mouse.row, hint_area)
                && let Some(key) = crate::ui::footer_shortcut_key_at_column(
                    &crate::ui::command_palette_shortcuts(
                        matches!(&app.mode, AppMode::CommandPalette(s) if s.capture),
                    ),
                    hint_area.x,
                    mouse.column,
                )
                && key != KeyCode::Null
            {
                return handle_command_palette(app, KeyEvent::from(key));
            }

            if point_in_rect(mouse.column, mouse.row, list_area)
                && let AppMode::CommandPalette(ref mut s) = app.mode
            {
                let row = (mouse.row - list_area.y) as usize;
                let list_h = list_area.height as usize;
                let indices = s.filtered_indices();
                let scroll = s.visible_start(list_h, indices.len());
                let clicked = scroll + row;
                if clicked < indices.len() && indices[clicked] != crate::app::PALETTE_SEP {
                    let same = s.match_pos == clicked;
                    s.match_pos = clicked;
                    s.scroll_match_into_view(list_h);
                    if same {
                        return handle_command_palette(app, KeyEvent::from(KeyCode::Enter));
                    }
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if point_in_rect(mouse.column, mouse.row, list_area) {
                return handle_command_palette(app, KeyEvent::from(KeyCode::Up));
            }
        }
        MouseEventKind::ScrollDown => {
            if point_in_rect(mouse.column, mouse.row, list_area) {
                return handle_command_palette(app, KeyEvent::from(KeyCode::Down));
            }
        }
        _ => {}
    }

    Ok(false)
}

fn handle_mouse_help(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    if handle_status_copy_click(app, mouse, area) {
        return Ok(false);
    }
    let (_popup, _inner, body, footer) = help_rect(area);

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in_rect(mouse.column, mouse.row, footer)
                && let AppMode::Help(state) = &app.mode
                && let Some(key) = crate::ui::footer_shortcut_key_at_column(
                    &crate::ui::help_shortcuts(&state.view),
                    footer.x,
                    mouse.column,
                )
                && key != KeyCode::Null
            {
                return handle_help(app, KeyEvent::from(key));
            }

            if point_in_rect(mouse.column, mouse.row, body)
                && let AppMode::Help(ref mut state) = app.mode
            {
                match state.view {
                    crate::help::HelpView::Index { ref mut cursor } => {
                        let row = (mouse.row - body.y) as usize;
                        if row < state.system.sections.len() {
                            let same = *cursor == row;
                            *cursor = row;
                            if same {
                                return handle_help(app, KeyEvent::from(KeyCode::Enter));
                            }
                        }
                    }
                    crate::help::HelpView::Topics {
                        section,
                        ref mut cursor,
                    } => {
                        let row = (mouse.row - body.y) as usize;
                        let len = state.system.sections[section].topics.len();
                        if row < len {
                            let same = *cursor == row;
                            *cursor = row;
                            if same {
                                return handle_help(app, KeyEvent::from(KeyCode::Enter));
                            }
                        }
                    }
                    crate::help::HelpView::Page { .. } => {}
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if point_in_rect(mouse.column, mouse.row, body) {
                return handle_help(app, KeyEvent::from(KeyCode::Up));
            }
        }
        MouseEventKind::ScrollDown => {
            if point_in_rect(mouse.column, mouse.row, body) {
                return handle_help(app, KeyEvent::from(KeyCode::Down));
            }
        }
        _ => {}
    }

    Ok(false)
}

fn handle_mouse_search_panel(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    if handle_status_copy_click(app, mouse, area) {
        return Ok(false);
    }

    let (_popup, inner, results_list, hint_area) = search_panel_rect(area);

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in_rect(mouse.column, mouse.row, hint_area)
                && let AppMode::SearchPanel(state) = &app.mode
                && let Some(key) = crate::ui::footer_shortcut_key_at_column(
                    &crate::ui::search_panel_shortcuts(state),
                    hint_area.x,
                    mouse.column,
                )
                && key != KeyCode::Null
            {
                return handle_search(app, KeyEvent::from(key));
            }

            if mouse.row > inner.y
                && mouse.row <= inner.y + 3
                && let AppMode::SearchPanel(ref mut s) = app.mode
            {
                s.input_field = (mouse.row - (inner.y + 1)) as usize;
                return Ok(false);
            }

            if point_in_rect(mouse.column, mouse.row, results_list)
                && let AppMode::SearchPanel(ref mut s) = app.mode
                && !s.results.is_empty()
            {
                let row = (mouse.row - results_list.y) as usize;
                let clicked = s.scroll + row;
                let same = s.input_field == 3 && s.cursor == clicked;
                s.input_field = 3;
                s.cursor = clicked.min(s.results.len().saturating_sub(1));
                if same {
                    return handle_search(app, KeyEvent::from(KeyCode::Enter));
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if point_in_rect(mouse.column, mouse.row, results_list) {
                return handle_search(app, KeyEvent::from(KeyCode::Up));
            }
        }
        MouseEventKind::ScrollDown => {
            if point_in_rect(mouse.column, mouse.row, results_list) {
                return handle_search(app, KeyEvent::from(KeyCode::Down));
            }
        }
        _ => {}
    }

    Ok(false)
}

fn handle_mouse_store_install_palette(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    if handle_status_copy_click(app, mouse, area) {
        return Ok(false);
    }

    let AppMode::StoreInstallPalette(state) = &app.mode else {
        return Ok(false);
    };

    if state.detect.is_some() {
        let (_popup, _inner, list_area, hint_area) = store_detect_rect(area);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if point_in_rect(mouse.column, mouse.row, hint_area)
                    && let Some(key) = crate::ui::footer_shortcut_key_at_column(
                        &crate::ui::store_detect_shortcuts(),
                        hint_area.x,
                        mouse.column,
                    )
                {
                    return handle_store_install_palette(app, KeyEvent::from(key));
                }

                if point_in_rect(mouse.column, mouse.row, list_area)
                    && let AppMode::StoreInstallPalette(ref mut s) = app.mode
                    && let Some(detect) = &mut s.detect
                {
                    let row = (mouse.row - list_area.y) as usize;
                    let list_h = list_area.height as usize;
                    let start = if detect.cursor >= list_h {
                        detect.cursor.saturating_sub(list_h.saturating_sub(1))
                    } else {
                        0
                    };
                    let clicked = start + row;
                    if clicked < detect.items.len() {
                        detect.cursor = clicked;
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if point_in_rect(mouse.column, mouse.row, list_area) {
                    return handle_store_install_palette(app, KeyEvent::from(KeyCode::Up));
                }
            }
            MouseEventKind::ScrollDown => {
                if point_in_rect(mouse.column, mouse.row, list_area) {
                    return handle_store_install_palette(app, KeyEvent::from(KeyCode::Down));
                }
            }
            _ => {}
        }
        return Ok(false);
    }

    let (_popup, _inner, left_list, hint_area) = store_install_rect(area, state);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in_rect(mouse.column, mouse.row, hint_area)
                && let AppMode::StoreInstallPalette(s) = &app.mode
                && let Some(key) = crate::ui::footer_shortcut_key_at_column(
                    &crate::ui::store_install_shortcuts(s),
                    hint_area.x,
                    mouse.column,
                )
            {
                if matches!(
                    key,
                    KeyCode::Char('d')
                        | KeyCode::Char('u')
                        | KeyCode::Char('y')
                        | KeyCode::Char('r')
                ) {
                    return handle_store_install_palette(
                        app,
                        KeyEvent::new(key, KeyModifiers::CONTROL),
                    );
                }
                return handle_store_install_palette(app, KeyEvent::from(key));
            }

            if point_in_rect(mouse.column, mouse.row, left_list)
                && let AppMode::StoreInstallPalette(ref mut s) = app.mode
            {
                let row = (mouse.row - left_list.y) as usize;
                let list_h = left_list.height as usize;
                let total = s.filtered_indices().len();
                if total > 0 {
                    let scroll = if s.match_pos < list_h {
                        0
                    } else {
                        s.match_pos.saturating_sub(list_h.saturating_sub(1))
                    };
                    let clicked = scroll + row;
                    let same = s.match_pos == clicked;
                    s.match_pos = clicked.min(total.saturating_sub(1));
                    if same {
                        return handle_store_install_palette(app, KeyEvent::from(KeyCode::Enter));
                    }
                }
            }
        }
        MouseEventKind::ScrollUp => {
            if point_in_rect(mouse.column, mouse.row, left_list) {
                return handle_store_install_palette(app, KeyEvent::from(KeyCode::Up));
            }
        }
        MouseEventKind::ScrollDown => {
            if point_in_rect(mouse.column, mouse.row, left_list) {
                return handle_store_install_palette(app, KeyEvent::from(KeyCode::Down));
            }
        }
        _ => {}
    }

    Ok(false)
}

fn handle_mouse_browse(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    let layout = main_mouse_layout(app, area);

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if point_in_rect(mouse.column, mouse.row, layout.status) {
                let line = crate::ui::status_line_for_copy(app);
                match copy_text_to_clipboard(&line) {
                    Ok(()) => app.trigger_status_copy_icon(),
                    Err(err) => app.set_status(format!("Clipboard error: {}", err)),
                }
                return Ok(false);
            }

            if let Some(fkey_area) = layout.fkey
                && point_in_rect(mouse.column, mouse.row, fkey_area)
                && let Some(fnum) = crate::ui::fkey_number_at_column(app, fkey_area.x, mouse.column)
                && let Some(action) = fkey_action_for_number(app, fnum)
            {
                app.set_status(format!(
                    "F{}: {}",
                    fnum,
                    crate::app::palette_label_for_action(action)
                ));
                return menu::execute_menu_action(app, action);
            }

            if let Some((side, index, list_height)) =
                panel_hit(app, mouse.column, mouse.row, layout)
            {
                let was_active = app.active == side;
                let was_current = match side {
                    ActivePanel::Left => app.left.cursor == index,
                    ActivePanel::Right => app.right.cursor == index,
                };
                app.active = side;
                match side {
                    ActivePanel::Left => {
                        app.left.cursor = index;
                        app.left.clamp_scroll(list_height);
                    }
                    ActivePanel::Right => {
                        app.right.cursor = index;
                        app.right.clamp_scroll(list_height);
                    }
                }
                app.refresh_quick_preview();
                if was_active && was_current {
                    handle_enter(app)?;
                }
                return Ok(false);
            }

            if point_in_rect(mouse.column, mouse.row, layout.center)
                && let Some(action) = center_button_hit(app, layout.center, mouse.column, mouse.row)
            {
                return menu::execute_menu_action(app, action);
            }
        }
        MouseEventKind::Down(MouseButton::Right) => {
            if let Some((side, index, list_height)) =
                panel_hit(app, mouse.column, mouse.row, layout)
            {
                app.active = side;
                let panel = match side {
                    ActivePanel::Left => &mut app.left,
                    ActivePanel::Right => &mut app.right,
                };
                panel.cursor = index;
                panel.clamp_scroll(list_height);
                panel.toggle_selected();
                app.refresh_quick_preview();
            }
        }
        MouseEventKind::ScrollUp => {
            if let Some((side, _, list_height)) = panel_hit(app, mouse.column, mouse.row, layout) {
                app.active = side;
                app.active_panel_mut().move_up();
                app.active_panel_mut().clamp_scroll(list_height);
                app.refresh_quick_preview();
            }
        }
        MouseEventKind::ScrollDown => {
            if let Some((side, _, list_height)) = panel_hit(app, mouse.column, mouse.row, layout) {
                app.active = side;
                app.active_panel_mut().move_down();
                app.active_panel_mut().clamp_scroll(list_height);
                app.refresh_quick_preview();
            }
        }
        _ => {}
    }

    Ok(false)
}

fn handle_mouse_menu(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return Ok(false);
    }

    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    let AppMode::Menu(state) = &app.mode else {
        return Ok(false);
    };

    let bar_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    if let Some(header_idx) = menu_header_hit(bar_area, mouse.column, mouse.row) {
        if let AppMode::Menu(ref mut state) = app.mode {
            state.bar_pos = header_idx;
            state.open = true;
            state.item_pos = first_menu_selectable(MENU_DATA[header_idx]);
        }
        return Ok(false);
    }

    if state.open
        && let Some((dd_area, inner)) = menu_dropdown_rect(app, state, area)
        && point_in_rect(mouse.column, mouse.row, dd_area)
        && mouse.row >= inner.y
    {
        let idx = (mouse.row - inner.y) as usize;
        if idx < MENU_DATA[state.bar_pos].len()
            && MENU_DATA[state.bar_pos][idx] != MenuAction::Separator
        {
            if let AppMode::Menu(ref mut state) = app.mode {
                state.item_pos = idx;
            }
            return handle_menu(app, KeyEvent::from(KeyCode::Enter));
        }
        return Ok(false);
    }

    app.mode = AppMode::Browse;
    Ok(false)
}

fn handle_mouse_confirm(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return Ok(false);
    }

    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    let AppMode::Confirm(dlg) = &app.mode else {
        return Ok(false);
    };
    let (accept, reject) = confirm_button_rects(dlg, area);
    if point_in_rect(mouse.column, mouse.row, accept) {
        return handle_confirm(app, KeyEvent::from(KeyCode::Enter));
    }
    if let Some(reject) = reject
        && point_in_rect(mouse.column, mouse.row, reject)
    {
        return handle_confirm(app, KeyEvent::from(KeyCode::Char('n')));
    }
    Ok(false)
}

fn handle_mouse_input(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return Ok(false);
    }

    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    let (popup, inner) = input_popup_rect(area);
    if !point_in_rect(mouse.column, mouse.row, popup) {
        return Ok(false);
    }

    let input_area = Rect {
        x: inner.x + 1,
        y: inner.y + 3,
        width: inner.width.saturating_sub(2),
        height: 1,
    };
    if point_in_rect(mouse.column, mouse.row, input_area)
        && let AppMode::Input(ref mut dlg) = app.mode
    {
        let offset = mouse.column.saturating_sub(input_area.x);
        dlg.textarea
            .move_cursor(ratatui_textarea::CursorMove::Jump(0, offset));
    }
    Ok(false)
}

fn handle_mouse_assoc_input(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return Ok(false);
    }

    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    let (popup, inner) = assoc_input_popup_rect(area, &app.mode);
    if !point_in_rect(mouse.column, mouse.row, popup) {
        return Ok(false);
    }

    let is_openers = matches!(
        app.mode,
        AppMode::AssocInput(AssocInputDialog {
            action: AssocInputAction::Openers { .. },
            ..
        })
    );
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    if is_openers
        && point_in_rect(mouse.column, mouse.row, hint_area)
        && let Some(key) = crate::ui::footer_shortcut_key_at_column(
            &crate::ui::assoc_input_shortcuts(),
            hint_area.x,
            mouse.column,
        )
    {
        return handle_assoc_input(app, KeyEvent::from(key));
    }

    let input_area = if is_openers {
        Rect {
            x: inner.x + 1,
            y: inner.y + 1,
            width: inner.width.saturating_sub(2),
            height: inner.height.saturating_sub(3),
        }
    } else {
        Rect {
            x: inner.x + 1,
            y: inner.y + 2,
            width: inner.width.saturating_sub(2),
            height: 1,
        }
    };

    if point_in_rect(mouse.column, mouse.row, input_area)
        && let AppMode::AssocInput(ref mut dlg) = app.mode
    {
        let col = mouse.column.saturating_sub(input_area.x);
        if is_openers {
            let row = mouse.row.saturating_sub(input_area.y);
            dlg.textarea
                .move_cursor(ratatui_textarea::CursorMove::Jump(row, col));
        } else {
            dlg.textarea
                .move_cursor(ratatui_textarea::CursorMove::Jump(0, col));
        }
    }
    Ok(false)
}

fn handle_mouse_assoc_editor(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return Ok(false);
    }

    let Some(area) = terminal_rect() else {
        return Ok(false);
    };

    const W: u16 = 88;
    const H: u16 = 24;
    let popup_w = W.min(area.width).max(1);
    let popup_h = H.min(area.height).max(1);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(popup_w)) / 2,
        y: area.y + (area.height.saturating_sub(popup_h)) / 2,
        width: popup_w,
        height: popup_h,
    };
    if !point_in_rect(mouse.column, mouse.row, popup) {
        return Ok(false);
    }

    let inner = Rect {
        x: popup.x.saturating_add(1),
        y: popup.y.saturating_add(1),
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };

    if point_in_rect(mouse.column, mouse.row, hint_area)
        && let Some(key) = crate::ui::footer_shortcut_key_at_column(
            &crate::ui::assoc_editor_shortcuts(),
            hint_area.x,
            mouse.column,
        )
    {
        return handle_assoc_editor(app, KeyEvent::from(key));
    }

    Ok(false)
}

fn handle_mouse_copy_dialog(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return Ok(false);
    }

    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    let (_popup, inner) = copy_dialog_rect(area);
    let AppMode::CopyDialog(dlg) = &app.mode else {
        return Ok(false);
    };
    let waiting_to_start = dlg.waiting_to_start;
    let stats_pending = dlg.stats_pending;

    let destination = Rect {
        x: inner.x,
        y: inner.y + 4,
        width: inner.width,
        height: 1,
    };
    if point_in_rect(mouse.column, mouse.row, destination) && !stats_pending && !waiting_to_start {
        if let AppMode::CopyDialog(ref mut dlg) = app.mode {
            dlg.field = CopyDialogState::DESTINATION;
            let offset = mouse.column.saturating_sub(inner.x + 1) as usize;
            dlg.cursor = byte_index_for_display_column(&dlg.destination, offset);
        }
        return Ok(false);
    }

    let toggle_rows = [
        (CopyDialogState::KEEP_ATTRIBUTES, inner.y + 5),
        (CopyDialogState::OVERWRITE, inner.y + 6),
        (CopyDialogState::NEWER_ONLY, inner.y + 7),
    ];
    for (field, y) in toggle_rows {
        let row = Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: 1,
        };
        if point_in_rect(mouse.column, mouse.row, row) {
            if let AppMode::CopyDialog(ref mut dlg) = app.mode {
                dlg.field = field;
                match field {
                    CopyDialogState::KEEP_ATTRIBUTES => dlg.keep_attributes = !dlg.keep_attributes,
                    CopyDialogState::OVERWRITE => dlg.overwrite = !dlg.overwrite,
                    CopyDialogState::NEWER_ONLY => dlg.newer_only = !dlg.newer_only,
                    _ => {}
                }
            }
            return Ok(false);
        }
    }

    let start_width = if waiting_to_start { 11 } else { 16 };
    let start_rect = Rect {
        x: inner.x,
        y: inner.y + 9,
        width: start_width,
        height: 1,
    };
    if point_in_rect(mouse.column, mouse.row, start_rect) {
        if let AppMode::CopyDialog(ref mut dlg) = app.mode {
            dlg.field = CopyDialogState::START;
        }
        return handle_copy_dialog(app, KeyEvent::from(KeyCode::Enter));
    }

    if !waiting_to_start {
        let cancel_rect = Rect {
            x: inner.x + 18,
            y: inner.y + 9,
            width: 12,
            height: 1,
        };
        if point_in_rect(mouse.column, mouse.row, cancel_rect) {
            if let AppMode::CopyDialog(ref mut dlg) = app.mode {
                dlg.field = CopyDialogState::CANCEL;
            }
            return handle_copy_dialog(app, KeyEvent::from(KeyCode::Esc));
        }
    }

    Ok(false)
}

fn terminal_rect() -> Option<Rect> {
    crossterm::terminal::size()
        .ok()
        .map(|(width, height)| Rect {
            x: 0,
            y: 0,
            width,
            height,
        })
}

fn main_mouse_layout(app: &App, area: Rect) -> MainMouseLayout {
    let main_vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if app.config.show_fkey_bar {
            vec![
                Constraint::Min(5),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
        } else {
            vec![Constraint::Min(5), Constraint::Length(1)]
        })
        .split(area);
    let panel_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(28),
            Constraint::Length(13),
            Constraint::Min(28),
        ])
        .split(main_vert[0]);

    MainMouseLayout {
        left_panel: panel_chunks[0],
        center: panel_chunks[1],
        right_panel: panel_chunks[2],
        status: main_vert[1],
        fkey: if app.config.show_fkey_bar {
            Some(main_vert[2])
        } else {
            None
        },
    }
}

fn fkey_action_for_number(app: &App, n: u8) -> Option<MenuAction> {
    let shortcut = format!("F{}", n);
    crate::app::PALETTE_DATA
        .iter()
        .find(|entry| {
            app.effective_shortcut_for(entry.fn_name, entry.shortcut)
                .as_deref()
                == Some(shortcut.as_str())
        })
        .map(|entry| entry.action)
}

fn inset_rect(area: Rect, dx: u16, dy: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(dx),
        y: area.y.saturating_add(dy),
        width: area.width.saturating_sub(dx.saturating_mul(2)),
        height: area.height.saturating_sub(dy.saturating_mul(2)),
    }
}

fn point_in_rect(column: u16, row: u16, rect: Rect) -> bool {
    column >= rect.x && column < rect.right() && row >= rect.y && row < rect.bottom()
}

fn panel_hit(
    app: &App,
    column: u16,
    row: u16,
    layout: MainMouseLayout,
) -> Option<(ActivePanel, usize, usize)> {
    panel_list_hit(&app.left, layout.left_panel, column, row)
        .map(|(idx, height)| (ActivePanel::Left, idx, height))
        .or_else(|| {
            panel_list_hit(&app.right, layout.right_panel, column, row)
                .map(|(idx, height)| (ActivePanel::Right, idx, height))
        })
}

fn panel_list_hit(
    panel: &crate::panel::Panel,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<(usize, usize)> {
    let inner = inset_rect(area, 1, 1);
    if inner.height < 4 {
        return None;
    }
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };
    if !point_in_rect(column, row, list_area) {
        return None;
    }
    let rel = (row - list_area.y) as usize;
    let idx = panel.scroll + rel;
    (idx < panel.entries.len()).then_some((idx, list_area.height as usize))
}

fn center_button_hit(app: &App, area: Rect, column: u16, row: u16) -> Option<MenuAction> {
    if area.height == 0 || area.width < 9 {
        return None;
    }

    let mut actions = app.center_buttons.clone();
    let clock_index = actions.len();
    actions.push(MenuAction::Separator);

    let button_count = actions.len() as u16;
    let button_h = if area.height >= button_count * 3 {
        3
    } else if area.height >= button_count * 2 {
        2
    } else {
        1
    };
    let total_button_h = button_count * button_h;
    let mut skipped = 0usize;
    if total_button_h > area.height {
        let skip = (total_button_h - area.height) as usize;
        if skip >= actions.len() {
            return None;
        }
        skipped = skip;
        actions.drain(0..skip);
    }

    let button_count = actions.len() as u16;
    let total_button_h = button_count * button_h;
    let gaps = button_count.saturating_add(1);
    let free = area.height.saturating_sub(total_button_h);
    let base_gap = free / gaps.max(1);
    let extra_gap = free % gaps.max(1);

    let mut y = area.y + base_gap;
    for (idx, action) in actions.into_iter().enumerate() {
        let is_clock = skipped + idx == clock_index;
        if is_clock {
            y += extra_gap;
        }
        let slot = Rect {
            x: area.x,
            y,
            width: area.width,
            height: button_h.min(area.bottom().saturating_sub(y)),
        };
        if point_in_rect(column, row, slot) {
            let original_idx = skipped + idx;
            return (original_idx != clock_index).then_some(action);
        }
        y = y.saturating_add(button_h);
        if idx + 1 < button_count as usize {
            y = y.saturating_add(base_gap);
        }
    }
    None
}

fn menu_header_hit(bar_area: Rect, column: u16, row: u16) -> Option<usize> {
    if !point_in_rect(column, row, bar_area) {
        return None;
    }
    let mut x = bar_area.x + 1;
    for (idx, header) in MENU_HEADERS.iter().enumerate() {
        x = x.saturating_add(1);
        let rect = Rect {
            x,
            y: bar_area.y,
            width: header.len() as u16 + 1,
            height: 1,
        };
        if point_in_rect(column, row, rect) {
            return Some(idx);
        }
        x = x.saturating_add(header.len() as u16 + 3);
    }
    None
}

fn menu_dropdown_rect(app: &App, state: &MenuState, area: Rect) -> Option<(Rect, Rect)> {
    if !state.open {
        return None;
    }
    let items = MENU_DATA[state.bar_pos];
    let mut dd_x = 1u16;
    for header in MENU_HEADERS.iter().take(state.bar_pos) {
        dd_x += header.len() as u16 + 4;
    }
    let max_label = items
        .iter()
        .map(|action| {
            unicode_width::UnicodeWidthStr::width(crate::app::palette_label_for_action(*action))
        })
        .max()
        .unwrap_or(6);
    let max_key = items
        .iter()
        .filter_map(|action| {
            crate::app::PALETTE_DATA
                .iter()
                .find(|entry| entry.action == *action)
                .and_then(|entry| app.effective_shortcut_for(entry.fn_name, entry.shortcut))
        })
        .map(|shortcut| shortcut.len())
        .max()
        .unwrap_or(0);
    let inner_w = (max_label + max_key + 4).max(18) as u16;
    let dd_width = inner_w + 2;
    let dd_height = items.len() as u16 + 2;
    let dd_area = Rect {
        x: area.x + dd_x.min(area.width.saturating_sub(dd_width)),
        y: area.y + 1,
        width: dd_width.min(area.width),
        height: dd_height.min(area.height.saturating_sub(1)),
    };
    Some((dd_area, inset_rect(dd_area, 1, 1)))
}

fn first_menu_selectable(items: &[crate::app::MenuEntry]) -> usize {
    items
        .iter()
        .position(|action| *action != MenuAction::Separator)
        .unwrap_or(0)
}

fn confirm_button_rects(dlg: &ConfirmDialog, area: Rect) -> (Rect, Option<Rect>) {
    if dlg.macro_name.is_some() {
        let Some(spec) = crate::lua_dialog::confirm_render_spec(dlg) else {
            return (
                Rect {
                    x: area.x,
                    y: area.y,
                    width: 0,
                    height: 0,
                },
                None,
            );
        };
        let (accept, reject) = crate::lua_dialog::confirm_dialog_button_rect_pair(&spec, area);
        return (accept, Some(reject));
    }

    match &dlg.action {
        ConfirmAction::Message | ConfirmAction::MessageThen(_) => {
            let width = 72u16.min(area.width.saturating_sub(4)).max(40);
            let height = 8;
            let popup = Rect {
                x: area.x + area.width.saturating_sub(width) / 2,
                y: area.y + area.height.saturating_sub(height) / 2,
                width,
                height,
            };
            let inner = inset_rect(popup, 1, 1);
            (
                Rect {
                    x: inner.x + inner.width.saturating_sub(8) / 2,
                    y: inner.y + inner.height.saturating_sub(2),
                    width: 8,
                    height: 1,
                },
                None,
            )
        }
        ConfirmAction::Quit | ConfirmAction::Delete(_) | ConfirmAction::DeleteRemote(_) => (
            Rect {
                x: area.x,
                y: area.y,
                width: 0,
                height: 0,
            },
            None,
        ),
        ConfirmAction::CloseTextEditorUnsaved | ConfirmAction::SaveEditorBeforeQuit => {
            let popup = Rect {
                x: area.x + area.width.saturating_sub(58) / 2,
                y: area.y + area.height.saturating_sub(9) / 2,
                width: 58,
                height: 9,
            };
            let inner = inset_rect(popup, 1, 1);
            let btn_y = inner.y + 4;
            let btn_x = inner.x + inner.width.saturating_sub(28) / 2;
            (
                Rect {
                    x: btn_x,
                    y: btn_y,
                    width: 11,
                    height: 1,
                },
                Some(Rect {
                    x: btn_x + 15,
                    y: btn_y,
                    width: 13,
                    height: 1,
                }),
            )
        }
    }
}

fn input_popup_rect(area: Rect) -> (Rect, Rect) {
    let width = 60u16.min(area.width.saturating_sub(4));
    let height = 7u16;
    let popup = Rect {
        x: (area.width.saturating_sub(width)) / 2 + area.x,
        y: (area.height.saturating_sub(height)) / 2 + area.y,
        width,
        height,
    };
    (popup, inset_rect(popup, 1, 1))
}

fn assoc_input_popup_rect(area: Rect, mode: &AppMode) -> (Rect, Rect) {
    let is_multiline = matches!(
        mode,
        AppMode::AssocInput(AssocInputDialog {
            action: AssocInputAction::Openers { .. },
            ..
        })
    );
    let width = if is_multiline {
        84u16.min(area.width.saturating_sub(4)).max(56)
    } else {
        60u16.min(area.width.saturating_sub(4)).max(42)
    };
    let height = if is_multiline {
        12u16.min(area.height.saturating_sub(4)).max(8)
    } else {
        7u16
    };
    let popup = Rect {
        x: (area.width.saturating_sub(width)) / 2 + area.x,
        y: (area.height.saturating_sub(height)) / 2 + area.y,
        width,
        height,
    };
    (popup, inset_rect(popup, 1, 1))
}

fn copy_dialog_rect(area: Rect) -> (Rect, Rect) {
    let width = 66u16.min(area.width.saturating_sub(4));
    let height = 14u16.min(area.height.saturating_sub(2));
    let popup = Rect {
        x: (area.width.saturating_sub(width)) / 2 + area.x,
        y: (area.height.saturating_sub(height)) / 2 + area.y,
        width,
        height,
    };
    (popup, inset_rect(popup, 1, 1))
}

fn remote_add_menu_rect(area: Rect, choices_len: usize) -> (Rect, Rect) {
    let width: u16 = 22;
    let height: u16 = (choices_len as u16) + 3;
    let popup = clamp_rect_local(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    (popup, inset_rect(popup, 1, 1))
}

fn remote_edit_rect(area: Rect, state: &crate::app::RemoteEditState) -> (Rect, Rect) {
    let width = 72u16.min(area.width.saturating_sub(4));
    let labels = state.kind.field_labels();
    let body_height = labels.len() as u16 + 5;
    let height = body_height.min(area.height.saturating_sub(2)).max(10);
    let popup = clamp_rect_local(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    (popup, inset_rect(popup, 1, 1))
}

fn remote_edit_share_picker_rect(
    area: Rect,
    inner: Rect,
    path_row: u16,
    value_offset: u16,
    shares_len: usize,
    picker_cur: usize,
) -> Option<(Rect, Rect, usize)> {
    let dd_x = inner.x + value_offset;
    let dd_y = inner.y + path_row + 1;
    let dd_w = inner.width.saturating_sub(value_offset).clamp(16, 40);
    let max_visible: usize = 8;
    let visible = shares_len.min(max_visible);
    let dd_h = (visible as u16 + 2).min(area.height.saturating_sub(dd_y));
    let dd_area = clamp_rect_local(
        area,
        Rect {
            x: dd_x,
            y: dd_y,
            width: dd_w,
            height: dd_h,
        },
    );
    let dd_inner = inset_rect(dd_area, 1, 1);
    let scroll = if picker_cur >= max_visible {
        picker_cur - max_visible + 1
    } else {
        0
    };
    Some((dd_area, dd_inner, scroll))
}

fn remote_connect_rect(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let width = 76u16.min(area.width.saturating_sub(4));
    let height = 20u16.min(area.height.saturating_sub(2)).max(10);
    let popup = clamp_rect_local(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    let inner = inset_rect(popup, 1, 1);
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };
    let hint_area = clamp_rect_local(
        area,
        Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        },
    );
    (popup, inner, list_area, hint_area)
}

fn help_rect(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let popup = clamp_rect_local(
        area,
        Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        },
    );
    let inner = inset_rect(popup, 1, 1);
    let body = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };
    let footer = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    (popup, inner, body, footer)
}

fn search_panel_rect(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let width = 100u16.min(area.width.saturating_sub(2));
    let height = (area.height * 4 / 5).clamp(18, area.height.saturating_sub(2));
    let popup = clamp_rect_local(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    let inner = inset_rect(popup, 1, 1);
    let input_h = 5u16.min(inner.height);
    let results_area = Rect {
        x: inner.x,
        y: inner.y + input_h,
        width: inner.width,
        height: inner.height.saturating_sub(input_h + 1),
    };
    let results_body = Rect {
        x: results_area.x,
        y: results_area.y + 1,
        width: results_area.width,
        height: results_area.height.saturating_sub(1),
    };
    let result_inner = Rect {
        x: results_body.x,
        y: results_body.y + 1,
        width: results_body.width,
        height: results_body.height.saturating_sub(1),
    };
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    (popup, inner, result_inner, hint_area)
}

fn command_palette_rect(area: Rect, item_count: usize) -> (Rect, Rect, Rect, Rect) {
    let w = area.width.saturating_sub(4).clamp(54, 72);
    let visible = (item_count as u16).clamp(3, 18);
    let h = (visible + 5).min(area.height.saturating_sub(3)).max(8);
    let popup = clamp_rect_local(
        area,
        Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + 2,
            width: w,
            height: h,
        },
    );
    let inner = inset_rect(popup, 1, 1);
    let list_h = inner.height.saturating_sub(3);
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: list_h,
    };
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    (popup, inner, list_area, hint_area)
}

fn store_install_rect(
    area: Rect,
    state: &crate::app::StoreInstallPaletteState,
) -> (Rect, Rect, Rect, Rect) {
    let w: u16 = area.width.saturating_sub(4).min(140).max(90);
    let h: u16 = area.height.saturating_sub(4).min(30).max(22);
    let popup = clamp_rect_local(
        area,
        Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + (area.height.saturating_sub(h)) / 2,
            width: w,
            height: h,
        },
    );
    let inner = inset_rect(popup, 1, 1);
    let button_y = inner.y + inner.height.saturating_sub(1);
    let footer_sep_y = button_y.saturating_sub(1);
    let body_y = inner.y + 3;
    let body_h = footer_sep_y.saturating_sub(body_y);
    let body = Rect {
        x: inner.x,
        y: body_y,
        width: inner.width,
        height: body_h,
    };
    let max_name = state
        .items
        .iter()
        .map(|p| p.name.len() + 18)
        .max()
        .unwrap_or(8);
    let left_w = ((max_name + 4) as u16)
        .clamp(36, 64)
        .min(body.width.saturating_sub(30));
    let left_area = Rect {
        x: body.x,
        y: body.y,
        width: left_w,
        height: body.height,
    };
    let left_list = Rect {
        x: left_area.x,
        y: left_area.y + 1,
        width: left_area.width,
        height: left_area.height.saturating_sub(1),
    };
    let hint_area = Rect {
        x: inner.x,
        y: button_y,
        width: inner.width,
        height: 1,
    };
    (popup, inner, left_list, hint_area)
}

fn store_detect_rect(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let w: u16 = area.width.saturating_sub(4).min(140).max(90);
    let h: u16 = area.height.saturating_sub(4).min(30).max(22);
    let store_popup = clamp_rect_local(
        area,
        Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + (area.height.saturating_sub(h)) / 2,
            width: w,
            height: h,
        },
    );
    let store_inner = inset_rect(store_popup, 1, 1);
    let width = store_inner.width.saturating_sub(4).min(104).max(56);
    let height = store_inner.height.saturating_sub(4).min(18).max(10);
    let popup = Rect {
        x: store_inner.x + store_inner.width.saturating_sub(width) / 2,
        y: store_inner.y + store_inner.height.saturating_sub(height) / 2,
        width,
        height,
    };
    let inner = inset_rect(popup, 1, 1);
    let list = Rect {
        x: inner.x,
        y: inner.y + 3,
        width: inner.width,
        height: inner.height.saturating_sub(5),
    };
    let hint = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    (popup, inner, list, hint)
}

fn dir_bookmarks_rect(area: Rect, bookmark_count: usize) -> (Rect, Rect, Rect, Rect) {
    let list_h = bookmark_count.max(3) as u16;
    let height = (list_h + 5).min(area.height.saturating_sub(4)).max(8);
    let width = 64u16.min(area.width.saturating_sub(4));
    let popup = clamp_rect_local(
        area,
        Rect {
            x: (area.width.saturating_sub(width)) / 2 + area.x,
            y: (area.height.saturating_sub(height)) / 2 + area.y,
            width,
            height,
        },
    );
    let inner = inset_rect(popup, 1, 1);
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    (popup, inner, list_area, hint_area)
}

fn plugins_rect(area: Rect, s: &crate::app::PluginsState) -> (Rect, Rect, Rect, Rect) {
    let w: u16 = area.width.saturating_sub(4).min(130).max(80);
    let h: u16 = area.height.saturating_sub(4).min(28).max(20);
    let popup = clamp_rect_local(
        area,
        Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + (area.height.saturating_sub(h)) / 2,
            width: w,
            height: h,
        },
    );
    let inner = inset_rect(popup, 1, 1);
    let button_y = inner.y + inner.height.saturating_sub(1);
    let footer_sep_y = button_y.saturating_sub(1);
    let body_y = inner.y + 4;
    let body_h = footer_sep_y.saturating_sub(body_y);
    let body = Rect {
        x: inner.x,
        y: body_y,
        width: inner.width,
        height: body_h,
    };
    let max_name = s
        .plugins
        .iter()
        .map(|p| {
            let src = crate::plugins::plugin_source_label(&p.dir, &s.plugins_dir);
            p.name.len() + src.len() + 6
        })
        .max()
        .unwrap_or(8);
    let left_w = ((max_name + 4) as u16)
        .clamp(32, 56)
        .min(body.width.saturating_sub(28));
    let left_area = Rect {
        x: body.x,
        y: body.y,
        width: left_w,
        height: body.height,
    };
    let left_list = Rect {
        x: left_area.x,
        y: left_area.y + 1,
        width: left_area.width,
        height: left_area.height.saturating_sub(1),
    };
    let hint_area = Rect {
        x: inner.x,
        y: button_y,
        width: inner.width,
        height: 1,
    };
    (popup, inner, left_list, hint_area)
}

fn remote_connecting_rect(area: Rect) -> (Rect, Rect, Rect) {
    let width = 46u16.min(area.width.saturating_sub(4)).max(30);
    let height = 7u16.min(area.height.saturating_sub(2)).max(6);
    let popup = clamp_rect_local(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    let inner = inset_rect(popup, 1, 1);
    let hint = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    (popup, inner, hint)
}

fn handle_mouse_remote_connecting(app: &mut App, mouse: MouseEvent) -> Result<bool> {
    let Some(area) = terminal_rect() else {
        return Ok(false);
    };
    if handle_status_copy_click(app, mouse, area) {
        return Ok(false);
    }
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return Ok(false);
    }
    let (_popup, _inner, hint) = remote_connecting_rect(area);
    if point_in_rect(mouse.column, mouse.row, hint)
        && let Some(key) = crate::ui::footer_shortcut_key_at_column(
            &crate::ui::remote_connecting_shortcuts(),
            hint.x,
            mouse.column,
        )
    {
        return handle_remote_connecting(app, KeyEvent::from(key));
    }
    Ok(false)
}

fn handle_status_copy_click(app: &mut App, mouse: MouseEvent, area: Rect) -> bool {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return false;
    }
    let layout = main_mouse_layout(app, area);
    if !point_in_rect(mouse.column, mouse.row, layout.status) {
        return false;
    }
    let line = crate::ui::status_line_for_copy(app);
    match copy_text_to_clipboard(&line) {
        Ok(()) => app.trigger_status_copy_icon(),
        Err(err) => app.set_status(format!("Clipboard error: {}", err)),
    }
    true
}

fn clamp_rect_local(area: Rect, rect: Rect) -> Rect {
    let x1 = rect.x.max(area.x);
    let y1 = rect.y.max(area.y);
    let x2 = rect.right().min(area.right());
    let y2 = rect.bottom().min(area.bottom());
    Rect {
        x: x1,
        y: y1,
        width: x2.saturating_sub(x1),
        height: y2.saturating_sub(y1),
    }
}

fn byte_index_for_display_column(s: &str, column: usize) -> usize {
    if column == 0 {
        return 0;
    }
    let mut display = 0usize;
    let mut last = 0usize;
    for (idx, ch) in s.char_indices() {
        let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if display + width > column {
            break;
        }
        display += width;
        last = idx + ch.len_utf8();
    }
    last
}

fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(text.to_string())?;
    Ok(())
}

fn paste_text_from_clipboard() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    clipboard.get_text().ok()
}

fn handle_about(app: &mut App, _key: KeyEvent) -> Result<bool> {
    app.mode = AppMode::Browse;
    Ok(false)
}

fn handle_copy_progress(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.cancel_copy_task();
        }
        _ => {}
    }
    if fx_shortcut(key) == Some(10) {
        app.cancel_copy_task();
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Browse mode
// ---------------------------------------------------------------------------

fn handle_browse(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Check if the key matches a Lua app shortcut before any other handling.
    if let Some(app_id) = app.lua_app_id_for_key(key) {
        let panel_cwd = app.active_panel().path.clone();
        crate::lua_apps::launch_lua_application_with_cwd(&app_id, &[], Some(&panel_cwd))?;
        app.needs_full_redraw = true;
        return Ok(false);
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let fn_key = fx_shortcut(key);

    // Side-panel text editor focus mode.
    if app.panel_text_editor_active {
        match key.code {
            KeyCode::Tab => {
                app.panel_text_editor_active = false;
                // Move focus to the panel opposite the editor, regardless of
                // what app.active currently is (important after a restore).
                if let Some(editor_side) = app.panel_text_editor_side() {
                    app.active = editor_side.other();
                } else {
                    app.switch_panel();
                }
            }
            KeyCode::Esc => {
                app.request_close_panel_text_editor()?;
            }
            _ if ctrl && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S')) => {
                if let Err(e) = app.save_panel_text_editor() {
                    app.notify(format!("Cannot save text editor file: {}", e));
                }
            }
            _ if alt && matches!(key.code, KeyCode::Char('z') | KeyCode::Char('Z')) => {
                app.toggle_panel_text_editor_wrap();
            }
            _ => {
                if let Some(editor) = app.panel_text_editor.as_mut() {
                    if let Some(input) = textarea_input_from_key_event(key) {
                        editor.textarea.input(input);
                    }
                }
            }
        }
        return Ok(false);
    }

    // Quick-preview panel focus mode: Up/Down scroll the viewer
    if app.quick_preview_active {
        match key.code {
            KeyCode::Tab => {
                app.quick_preview_active = false;
            }
            KeyCode::Esc => {
                app.close_quick_preview();
            }
            KeyCode::Up => {
                app.quick_preview_scroll_up();
            }
            KeyCode::Down => {
                app.quick_preview_scroll_down();
            }
            _ if fn_key == Some(4) => {
                // Cycle forced mode: Auto → Text → Markdown → Hex → Ansi → Image → Audio → Auto
                app.quick_preview_forced_mode = match app.quick_preview_forced_mode {
                    None => Some(ViewMode::Text),
                    Some(ViewMode::Text) => Some(ViewMode::Markdown),
                    Some(ViewMode::Markdown) => Some(ViewMode::Hex),
                    Some(ViewMode::Hex) => Some(ViewMode::Ansi),
                    Some(ViewMode::Ansi) => Some(ViewMode::Image),
                    Some(ViewMode::Image) => Some(ViewMode::Module),
                    Some(ViewMode::Module) => None,
                };
                if let Some(mode) = app.quick_preview_forced_mode {
                    if let Some(v) = app.quick_preview.as_mut() {
                        v.set_mode(mode);
                    }
                } else {
                    // Auto: re-open current file so it detects the mode naturally
                    app.refresh_quick_preview();
                }
            }
            _ => {}
        }
        return Ok(false);
    }

    // FileID panel focus mode: all navigation keys scroll the IDF card
    if app.file_id_active {
        match key.code {
            KeyCode::Tab => {
                app.file_id_active = false;
            }
            KeyCode::Esc => {
                app.close_file_id_view();
            }
            KeyCode::Up => {
                app.file_id_scroll_up();
            }
            KeyCode::Down => {
                app.file_id_scroll_down();
            }
            KeyCode::PageUp => {
                app.file_id_scroll_page_up(10);
            }
            KeyCode::PageDown => {
                app.file_id_scroll_page_down(10);
            }
            KeyCode::Home => {
                app.file_id_home();
            }
            _ => {}
        }
        return Ok(false);
    }

    if let Some(action) = app.action_for_key(key) {
        return menu::execute_menu_action(app, action);
    }
    if app.shortcut_key_is_managed(key) {
        return Ok(false);
    }

    // Printable char (no modifiers) → start quick-search
    if !ctrl && !alt && !shift {
        if let KeyCode::Char(ch) = key.code {
            if ch.is_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                let p = app.active_panel_mut();
                p.quicksearch.clear();
                p.qs_match_pos = 0;
                p.quicksearch_append(ch);
                let first = app.active_panel().quicksearch_matches().into_iter().next();
                if let Some(idx) = first {
                    app.active_panel_mut().cursor = idx;
                }
                app.mode = AppMode::QuickSearch;
                return Ok(false);
            }
        }
    }

    // Handle Ctrl-modified keys first
    if ctrl && !alt && !shift {
        match fn_key {
            Some(1) => return menu::execute_menu_action(app, MenuAction::SortName),
            Some(2) => return menu::execute_menu_action(app, MenuAction::SortExtension),
            Some(3) => return menu::execute_menu_action(app, MenuAction::SortDate),
            Some(4) => return menu::execute_menu_action(app, MenuAction::SortSize),
            Some(5) => return menu::execute_menu_action(app, MenuAction::SortUnsorted),
            _ => match key.code {
                KeyCode::Char('r') => return menu::execute_menu_action(app, MenuAction::Reload),
                KeyCode::Char('h') => {
                    return menu::execute_menu_action(app, MenuAction::ToggleHidden);
                }
                KeyCode::Char('d') => {
                    return menu::execute_menu_action(app, MenuAction::DirBookmarks);
                }
                KeyCode::Char('f') => {
                    return menu::execute_menu_action(app, MenuAction::RemoteConnect);
                }
                KeyCode::Char('u') => {
                    return menu::execute_menu_action(app, MenuAction::OpenTerminal);
                }
                KeyCode::Char('a') => {
                    return menu::execute_menu_action(app, MenuAction::OpenActionPalette);
                }
                KeyCode::Char('p') => {
                    return menu::execute_menu_action(app, MenuAction::OpenCommandPalette);
                }
                KeyCode::Char('t') => return menu::execute_menu_action(app, MenuAction::NewTab),
                KeyCode::Char('w') => return menu::execute_menu_action(app, MenuAction::CloseTab),
                KeyCode::Tab | KeyCode::Char('n') => {
                    return menu::execute_menu_action(app, MenuAction::NextTab);
                }
                _ => {}
            },
        }
    }

    // Shift-modified keys
    if shift && !ctrl && !alt {
        if fn_key == Some(6) {
            return menu::execute_menu_action(app, MenuAction::RenameFile);
        }
    }

    // Unmodified keys
    match key.code {
        // --- Navigation ---
        KeyCode::Up => {
            app.active_panel_mut().move_up();
            app.refresh_quick_preview();
        }
        KeyCode::Down => {
            app.active_panel_mut().move_down();
            app.refresh_quick_preview();
        }
        KeyCode::PageUp => {
            app.active_panel_mut().move_page_up(20);
            app.refresh_quick_preview();
        }
        KeyCode::PageDown => {
            app.active_panel_mut().move_page_down(20);
            app.refresh_quick_preview();
        }
        KeyCode::Home => {
            app.active_panel_mut().move_home();
            app.refresh_quick_preview();
        }
        KeyCode::End => {
            app.active_panel_mut().move_end();
            app.refresh_quick_preview();
        }
        KeyCode::Right if app.active == crate::app::ActivePanel::Left => {
            app.send_active_entry_to_other_panel()?;
        }
        KeyCode::Left if app.active == crate::app::ActivePanel::Right => {
            app.send_active_entry_to_other_panel()?;
        }
        KeyCode::Tab => {
            return menu::execute_menu_action(app, MenuAction::SwitchPanel);
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
        KeyCode::Char(' ') => {
            app.active_panel_mut().toggle_selected();
            app.active_panel_mut().move_down();
        }
        KeyCode::Char('+') => {
            return menu::execute_menu_action(app, MenuAction::SelectPattern);
        }
        KeyCode::Char('-') => {
            return menu::execute_menu_action(app, MenuAction::DeselectPattern);
        }
        KeyCode::Char('*') => {
            return menu::execute_menu_action(app, MenuAction::InvertSelection);
        }

        KeyCode::Esc => {
            if app.panel_text_editor.is_some() {
                app.request_close_panel_text_editor()?;
            } else if app.file_preview_info {
                app.close_file_id_view();
            } else {
                app.mode = AppMode::Terminal;
            }
        }

        _ => {}
    }

    match fn_key {
        Some(1) if !ctrl && !shift => {
            return menu::execute_menu_action(app, MenuAction::Help);
        }
        Some(3) if !ctrl && !shift => {
            return menu::execute_menu_action(app, MenuAction::ViewFile);
        }
        Some(4) if !ctrl && !shift => {
            return menu::execute_menu_action(app, MenuAction::EditFile);
        }
        Some(5) if !ctrl && !shift => {
            return menu::execute_menu_action(app, MenuAction::CopyFile);
        }
        Some(6) if !ctrl && !shift => {
            return menu::execute_menu_action(app, MenuAction::MoveFile);
        }
        Some(7) if !ctrl && !shift => {
            return menu::execute_menu_action(app, MenuAction::MkDir);
        }
        Some(8) if !ctrl && !shift => {
            return menu::execute_menu_action(app, MenuAction::DeleteFile);
        }
        Some(9) if !ctrl && !shift => {
            return menu::execute_menu_action(app, MenuAction::OpenMenu);
        }
        Some(10) if !ctrl && !shift => {
            return menu::execute_menu_action(app, MenuAction::Quit);
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

    if entry.name == "[disconnect]" && app.active_panel().is_remote_view() {
        app.active_panel_mut().disconnect();
        // app.active_panel_mut().goto_root()?;
    } else if entry.name == ".." {
        app.go_parent()?;
    } else if entry.is_dir {
        app.enter_dir(entry.path.clone())?;
    } else if crate::plugins::is_plugin_bundle(&entry.path) {
        let bundle_path = if app.active_panel().is_remote_view() {
            let Some(profile) = app.active_panel().remote_profile() else {
                app.notify("Remote profile missing");
                return Ok(());
            };
            match app.run_with_busy("Remote: downloading plugin...", |_| {
                download_to_temp(&profile, &entry.path.to_string_lossy(), false)
            }) {
                Ok(path) => path,
                Err(e) => {
                    app.notify(format!("Remote download failed: {}", e));
                    return Ok(());
                }
            }
        } else {
            entry.path.clone()
        };
        match crate::plugins::install_plugin_bundle(&bundle_path) {
            Ok(name) => {
                app.reload_panels();
                app.mode = AppMode::Confirm(crate::app::ConfirmDialog {
                    title: None,
                    message: Some(format!("Plugin installed: {}", name)),
                    action: ConfirmAction::Message,
                    macro_name: Some("confirm_notify"),
                    active_button: crate::app::ConfirmButton::Primary,
                });
            }
            Err(e) => app.notify(format!("Cannot install plugin: {}", e)),
        }
    } else {
        let launch_path = if app.active_panel().is_remote_view() {
            let Some(profile) = app.active_panel().remote_profile() else {
                app.notify("Remote profile missing");
                return Ok(());
            };
            match app.run_with_busy("Remote: downloading file...", |_| {
                download_to_temp(&profile, &entry.path.to_string_lossy(), false)
            }) {
                Ok(path) => path,
                Err(e) => {
                    app.notify(format!("Remote download failed: {}", e));
                    return Ok(());
                }
            }
        } else {
            entry.path.clone()
        };
        // Check registered openers first, using FileID MIME types.
        let mime_types = crate::idf::probe_path(&launch_path)
            .map(|info| info.mime_types)
            .filter(|mime_types| !mime_types.is_empty())
            .unwrap_or_else(|| vec!["application/octet-stream".to_string()]);
        let mut actions = Vec::new();
        let mut seen_openers: Vec<String> = Vec::new();
        for mime_type in &mime_types {
            for opener in app.config.openers_for_mime(mime_type) {
                if seen_openers.iter().any(|existing| existing == opener) {
                    continue;
                }
                seen_openers.push(opener.clone());
                let display_label =
                    match crate::plugins::store_application_launch_args_for_command(opener) {
                        Some(Some(args)) => {
                            let program = split_command_args(opener)
                                .first()
                                .cloned()
                                .unwrap_or_else(|| opener.clone());
                            if args.trim().is_empty() {
                                program
                            } else {
                                format!("{} {}", program, args)
                            }
                        }
                        _ => opener.clone(),
                    };
                actions.push(OpenerActionItem {
                    category: "Associations",
                    label: display_label,
                    detail: mime_type.clone(),
                    kind: OpenerActionKind::Association {
                        command: opener.clone(),
                    },
                });
            }
        }
        if supports_archive_navigation(&launch_path) {
            actions.push(OpenerActionItem {
                category: "Archive",
                label: "Enter archive".to_string(),
                detail: "Browse archive contents".to_string(),
                kind: OpenerActionKind::Archive,
            });
        }
        actions.push(OpenerActionItem {
            category: "System",
            label: "Open with system".to_string(),
            detail: "Default application".to_string(),
            kind: OpenerActionKind::System,
        });

        if actions.len() == 1 {
            execute_opener_action(app, actions.remove(0), &launch_path)?;
        } else {
            app.mode = AppMode::Opener(OpenerState {
                items: actions,
                query: String::new(),
                match_pos: 0,
                path: launch_path,
            });
        }
    }
    Ok(())
}

fn execute_opener_action(
    app: &mut App,
    action: OpenerActionItem,
    path: &std::path::Path,
) -> Result<()> {
    match action.kind {
        OpenerActionKind::System => {
            if let Err(e) = open::that(path) {
                app.notify(format!("Cannot open: {}", e));
            }
        }
        OpenerActionKind::Association { command } => {
            launch_external(app, &command, path)?;
        }
        OpenerActionKind::Archive => {
            if let Err(e) = app.enter_archive(path.to_path_buf()) {
                app.notify(format!("Cannot enter archive: {}", e));
            }
        }
    }
    Ok(())
}

/// Spawn an external command with the selected file context.
/// Supports placeholders in arguments (`%f`, `%n`, `%d`, `%e`, `%b`, `%%`).
fn launch_external(app: &mut App, command: &str, path: &std::path::Path) -> Result<()> {
    let parsed = split_command_args(command);
    if parsed.is_empty() {
        app.notify("Empty opener command");
        return Ok(());
    }

    if let Some(app_id) = crate::lua_apps::app_id_from_command_token(&parsed[0]) {
        let mut app_args = Vec::new();
        match crate::plugins::store_application_launch_args_for_command(command) {
            Some(Some(store_args)) => {
                for token in split_command_args(&store_args) {
                    app_args.push(expand_opener_placeholders(&token, path));
                }
            }
            _ => {
                for token in parsed.iter().skip(1) {
                    app_args.push(expand_opener_placeholders(token, path));
                }
            }
        }
        if app_args.is_empty() {
            app_args.push(path.to_string_lossy().into_owned());
        }

        let launch_cwd = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| app.active_panel().path.clone())
        };

        match crate::lua_apps::launch_lua_application_with_cwd(
            &app_id,
            &app_args,
            Some(&launch_cwd),
        ) {
            Ok(_) => {
                app.needs_clear = true;
                app.reload_panels();
            }
            Err(e) => {
                app.notify(format!("Cannot launch Lua app '{}': {}", app_id, e));
            }
        }
        return Ok(());
    }

    let wait_for_key = crate::plugins::store_application_waits_after_command(command);

    let mut args = Vec::with_capacity(parsed.len() + 4);
    args.push(parsed[0].clone());

    match crate::plugins::store_application_launch_args_for_command(command) {
        // Store app with explicit launch args: these replace the historical auto `%f` behavior.
        Some(Some(store_args)) => {
            for token in split_command_args(&store_args) {
                args.push(expand_opener_placeholders(&token, path));
            }
        }
        _ => {
            let mut has_file_placeholder = false;
            for token in parsed.into_iter().skip(1) {
                if token.contains("%f") {
                    has_file_placeholder = true;
                }
                args.push(expand_opener_placeholders(&token, path));
            }
            if !has_file_placeholder {
                args.push(path.to_string_lossy().into_owned());
            }
        }
    }

    if args.is_empty() {
        app.notify("Empty opener command");
        return Ok(());
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    let _ = std::process::Command::new(&args[0])
        .args(&args[1..])
        .status();

    let wait_error = if wait_for_key {
        wait_for_key_after_external().err()
    } else {
        None
    };

    if let Some(e) = wait_error {
        app.notify(format!("Wait for key failed: {}", e));
    }

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    app.needs_clear = true;
    app.reload_panels();
    Ok(())
}

fn split_command_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_double => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        args.push(current);
    }
    args
}

fn expand_opener_placeholders(token: &str, path: &std::path::Path) -> String {
    let full = path.to_string_lossy().into_owned();
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let dir = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let base = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let mut out = String::with_capacity(token.len() + 16);
    let mut chars = token.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('f') => out.push_str(&full),
            Some('n') => out.push_str(&name),
            Some('d') => out.push_str(&dir),
            Some('e') => out.push_str(&ext),
            Some('b') => out.push_str(&base),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

fn wait_for_key_after_external() -> Result<()> {
    let mut stdout = io::stdout();
    write!(stdout, "\nPress a key to continue...")?;
    stdout.flush()?;
    enable_raw_mode()?;
    let read_result = crossterm::event::read();
    disable_raw_mode()?;
    read_result?;
    writeln!(stdout)?;
    stdout.flush()?;
    Ok(())
}

fn confirm_quit(app: &mut App) -> Result<bool> {
    // If the text editor has unsaved changes, ask about them first.
    if app.panel_text_editor_modified() {
        app.mode = AppMode::Confirm(crate::app::ConfirmDialog {
            title: None,
            message: None,
            action: ConfirmAction::SaveEditorBeforeQuit,
            macro_name: Some("confirm_save_editor_before_quit"),
            active_button: crate::app::ConfirmButton::Primary,
        });
        return Ok(false);
    }
    if app.config.confirm_exit {
        app.mode = AppMode::Confirm(crate::app::ConfirmDialog {
            title: None,
            message: None,
            action: ConfirmAction::Quit,
            macro_name: Some("confirm_quit"),
            active_button: crate::app::ConfirmButton::Primary,
        });
        Ok(false)
    } else {
        Ok(true)
    }
}

fn launch_editor(app: &mut App) -> Result<()> {
    if app.active_panel().is_archive_view() {
        app.notify("Editing in archive is not supported");
        return Ok(());
    }
    let entry = match app.active_panel().current_entry() {
        Some(e) if !e.is_dir && e.name != ".." => e.clone(),
        _ => return Ok(()),
    };

    let editor = app.config.editor.clone();
    if editor.trim().is_empty() {
        if app.active_panel().is_remote_view() {
            app.notify("Internal editor is only available for local files");
            return Ok(());
        }
        app.open_panel_text_editor_with_path(entry.path.clone(), app.active)?;
        return Ok(());
    }

    let path = if app.active_panel().is_remote_view() {
        let Some(profile) = app.active_panel().remote_profile() else {
            app.notify("Remote profile missing");
            return Ok(());
        };
        match app.run_with_busy("Remote: downloading file...", |_| {
            download_to_temp(&profile, &entry.path.to_string_lossy(), false)
        }) {
            Ok(path) => path,
            Err(e) => {
                app.notify(format!("Remote download failed: {}", e));
                return Ok(());
            }
        }
    } else {
        entry.path.clone()
    };

    let editing_config = is_config_path(&path);
    let config_modified_before = if editing_config {
        file_modified_time(&path)
    } else {
        None
    };

    // Restore normal terminal before handing control to an external editor.
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    use std::process::Command;
    let _ = Command::new(&editor).arg(&path).status();

    if app.active_panel().is_remote_view()
        && let Some(profile) = app.active_panel().remote_profile()
        && let Some(parent) = entry.path.parent()
    {
        let remote_dir = parent.to_string_lossy().into_owned();
        if let Err(e) = app.run_with_busy("Remote: uploading file...", |_| {
            upload_into_dir(&profile, &path, &remote_dir, false).map(|_| ())
        }) {
            app.notify(format!("Remote upload failed: {}", e));
        }
    }

    // Re-enter TUI mode once the editor exits.
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;

    // Ratatui's buffer is stale after leaving/re-entering the alternate screen;
    // signal the main loop to call terminal.clear() before the next draw.
    app.needs_clear = true;
    if editing_config && config_modified_before != file_modified_time(&path) {
        if let Err(e) = app.reload_config_from_disk() {
            app.notify(format!("Config reload failed: {}", e));
        }
    } else {
        app.reload_panels();
    }
    Ok(())
}

fn file_modified_time(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn is_config_path(path: &std::path::Path) -> bool {
    let Ok(config_path) = crate::config::config_path() else {
        return false;
    };
    same_path(path, &config_path)
}

fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn start_rename(app: &mut App) {
    if app.active_panel().is_archive_view() {
        app.notify("Rename in archive is not supported");
        return;
    }
    if let Some(entry) = app.active_panel().current_entry() {
        if entry.name == ".." {
            return;
        }
        let path = entry.path.clone();
        let name = entry.name.clone();
        let action = if let Some(profile) = app.active_panel().remote_profile() {
            InputAction::RemoteRename {
                profile,
                path: path.to_string_lossy().into_owned(),
            }
        } else {
            InputAction::Rename(path)
        };
        app.mode = AppMode::Input(InputDialog {
            title: Some("Rename".into()),
            prompt: Some("New name:".into()),
            textarea: InputDialog::make_textarea(name.clone()),
            action,
            macro_name: Some("input_rename"),
            focused_button: Some(0),
        });
    }
}

fn start_mkdir(app: &mut App) {
    if app.active_panel().is_archive_view() {
        app.notify("Create directory in archive is not supported");
        return;
    }
    let action = if let Some(profile) = app.active_panel().remote_profile() {
        InputAction::RemoteMkdir {
            profile,
            parent: app.active_panel().remote_cwd().unwrap_or("/").to_string(),
        }
    } else {
        InputAction::Mkdir(app.active_panel().path.clone())
    };
    app.mode = AppMode::Input(InputDialog {
        title: Some("Create Directory".into()),
        prompt: Some("Directory name:".into()),
        textarea: InputDialog::make_textarea(""),
        action,
        macro_name: Some("input_mkdir"),
        focused_button: Some(0),
    });
}

fn open_wildcard_dialog(prompt: &str, select: bool) -> AppMode {
    AppMode::Input(InputDialog {
        title: Some("Wildcard".into()),
        prompt: Some(prompt.into()),
        textarea: InputDialog::make_textarea("*"),
        action: if select {
            InputAction::SelectPattern
        } else {
            InputAction::DeselectPattern
        },
        macro_name: Some("input_wildcard"),
        focused_button: Some(0),
    })
}

// ---------------------------------------------------------------------------
// QuickSearch palette mode (VSCode-style)
// ---------------------------------------------------------------------------

fn handle_quicksearch(app: &mut App, key: KeyEvent) -> Result<bool> {
    let mut refresh_preview = false;
    match key.code {
        // Confirm: jump to highlighted match, or enter directory
        KeyCode::Enter => {
            let entry_idx = {
                let p = app.active_panel();
                p.quicksearch_matches().get(p.qs_match_pos).copied()
            };
            app.active_panel_mut().quicksearch_clear();
            app.active_panel_mut().qs_match_pos = 0;
            if let Some(idx) = entry_idx {
                app.active_panel_mut().cursor = idx;
                refresh_preview = true;
            }
            app.mode = AppMode::Browse;
            // If the selected entry is a directory, navigate into it
            if let Some(entry) = app.active_panel().current_entry().cloned() {
                if entry.name == ".." {
                    app.go_parent()?;
                } else if entry.is_dir {
                    app.enter_dir(entry.path.clone())?;
                }
            }
        }
        // Cancel: restore original cursor
        KeyCode::Esc => {
            app.active_panel_mut().quicksearch_clear();
            app.active_panel_mut().qs_match_pos = 0;
            app.mode = AppMode::Browse;
        }
        // Navigate UP in the filtered list (with wrap-around)
        KeyCode::Up => {
            let matches_len = app.active_panel().quicksearch_matches().len();
            let p = app.active_panel_mut();
            if matches_len > 0 {
                if p.qs_match_pos > 0 {
                    p.qs_match_pos -= 1;
                } else {
                    p.qs_match_pos = matches_len - 1;
                }
            }
            let entry_idx = app
                .active_panel()
                .quicksearch_matches()
                .get(app.active_panel().qs_match_pos)
                .copied();
            if let Some(idx) = entry_idx {
                app.active_panel_mut().cursor = idx;
                refresh_preview = true;
            }
        }
        // Navigate DOWN in the filtered list (with wrap-around)
        KeyCode::Down => {
            let matches_len = app.active_panel().quicksearch_matches().len();
            let p = app.active_panel_mut();
            if matches_len > 0 {
                if p.qs_match_pos + 1 < matches_len {
                    p.qs_match_pos += 1;
                } else {
                    p.qs_match_pos = 0;
                }
            }
            let entry_idx = app
                .active_panel()
                .quicksearch_matches()
                .get(app.active_panel().qs_match_pos)
                .copied();
            if let Some(idx) = entry_idx {
                app.active_panel_mut().cursor = idx;
                refresh_preview = true;
            }
        }
        // Page Up: jump up 10 items
        KeyCode::PageUp => {
            let matches_len = app.active_panel().quicksearch_matches().len();
            let p = app.active_panel_mut();
            if matches_len > 0 {
                p.qs_match_pos = p.qs_match_pos.saturating_sub(10).min(matches_len - 1);
            }
            let entry_idx = app
                .active_panel()
                .quicksearch_matches()
                .get(app.active_panel().qs_match_pos)
                .copied();
            if let Some(idx) = entry_idx {
                app.active_panel_mut().cursor = idx;
                refresh_preview = true;
            }
        }
        // Page Down: jump down 10 items
        KeyCode::PageDown => {
            let matches_len = app.active_panel().quicksearch_matches().len();
            let p = app.active_panel_mut();
            if matches_len > 0 {
                p.qs_match_pos = (p.qs_match_pos + 10).min(matches_len - 1);
            }
            let entry_idx = app
                .active_panel()
                .quicksearch_matches()
                .get(app.active_panel().qs_match_pos)
                .copied();
            if let Some(idx) = entry_idx {
                app.active_panel_mut().cursor = idx;
                refresh_preview = true;
            }
        }
        // Delete last char
        KeyCode::Backspace => {
            app.active_panel_mut().quicksearch_pop();
            app.active_panel_mut().qs_match_pos = 0;
            if app.active_panel().quicksearch.is_empty() {
                app.mode = AppMode::Browse;
            } else {
                let first = app.active_panel().quicksearch_matches().into_iter().next();
                if let Some(idx) = first {
                    app.active_panel_mut().cursor = idx;
                    refresh_preview = true;
                }
            }
        }
        // Append char to query
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.active_panel_mut().quicksearch_append(ch);
            app.active_panel_mut().qs_match_pos = 0;
            let first = app.active_panel().quicksearch_matches().into_iter().next();
            if let Some(idx) = first {
                app.active_panel_mut().cursor = idx;
                refresh_preview = true;
            }
        }
        // Any other key: close palette and pass through
        _ => {
            app.active_panel_mut().quicksearch_clear();
            app.active_panel_mut().qs_match_pos = 0;
            app.mode = AppMode::Browse;
            return handle_browse(app, key);
        }
    }
    if refresh_preview {
        app.refresh_quick_preview();
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Confirm dialog
// ---------------------------------------------------------------------------

fn handle_confirm(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::Confirm(_) = &app.mode else {
        return Ok(false);
    };

    match key.code {
        KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
            if let AppMode::Confirm(dlg) = &mut app.mode
                && confirm_has_secondary_button(dlg)
            {
                dlg.active_button = match dlg.active_button {
                    ConfirmButton::Primary => ConfirmButton::Secondary,
                    ConfirmButton::Secondary => ConfirmButton::Primary,
                };
            }
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            return confirm_primary(app);
        }
        KeyCode::Enter => {
            if let AppMode::Confirm(dlg) = &app.mode {
                if let Some(callback) = crate::lua_dialog::confirm_button_callback(
                    dlg,
                    confirm_button_index(dlg.active_button),
                ) {
                    return match callback.as_str() {
                        "cancel" => confirm_secondary(app),
                        _ => confirm_primary(app),
                    };
                }
            };
            let active = match &app.mode {
                AppMode::Confirm(dlg) => dlg.active_button,
                _ => ConfirmButton::Primary,
            };
            return match active {
                ConfirmButton::Primary => confirm_primary(app),
                ConfirmButton::Secondary => confirm_secondary(app),
            };
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            return confirm_secondary(app);
        }
        KeyCode::Esc => {
            let keep_editor_focused = matches!(
                &app.mode,
                AppMode::Confirm(ConfirmDialog {
                    action: ConfirmAction::CloseTextEditorUnsaved,
                    ..
                }) | AppMode::Confirm(ConfirmDialog {
                    action: ConfirmAction::SaveEditorBeforeQuit,
                    ..
                })
            );
            app.mode = AppMode::Browse;
            if keep_editor_focused {
                app.panel_text_editor_active = true;
            }
        }
        _ => {}
    }
    Ok(false)
}

fn confirm_has_secondary_button(dlg: &ConfirmDialog) -> bool {
    if dlg.macro_name.is_some() {
        return crate::lua_dialog::confirm_render_spec(dlg)
            .map(|spec| spec.buttons.len() >= 2)
            .unwrap_or(false);
    }
    matches!(
        dlg.action,
        ConfirmAction::Quit | ConfirmAction::Delete(_) | ConfirmAction::DeleteRemote(_)
    )
}

fn confirm_button_index(button: ConfirmButton) -> usize {
    match button {
        ConfirmButton::Primary => 0,
        ConfirmButton::Secondary => 1,
    }
}

fn confirm_primary(app: &mut App) -> Result<bool> {
    let AppMode::Confirm(dlg) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
        return Ok(false);
    };
    match dlg.action {
        ConfirmAction::Message => {}
        ConfirmAction::MessageThen(next) => {
            app.mode = *next;
        }
        ConfirmAction::Quit => return Ok(true),
        ConfirmAction::Delete(paths) => {
            app.cmd_delete_confirmed(paths)?;
        }
        ConfirmAction::DeleteRemote(targets) => {
            app.cmd_delete_remote_confirmed(targets)?;
        }
        ConfirmAction::CloseTextEditorUnsaved => {
            if let Err(e) = app.save_panel_text_editor() {
                app.notify(format!("Cannot save text editor file: {}", e));
                app.panel_text_editor_active = true;
            } else {
                app.close_panel_text_editor();
            }
        }
        ConfirmAction::SaveEditorBeforeQuit => {
            if let Err(e) = app.save_panel_text_editor() {
                app.notify(format!("Cannot save text editor file: {}", e));
                app.panel_text_editor_active = true;
            } else {
                app.close_panel_text_editor();
                return confirm_quit(app);
            }
        }
    }
    Ok(false)
}

fn confirm_secondary(app: &mut App) -> Result<bool> {
    let AppMode::Confirm(dlg) = std::mem::replace(&mut app.mode, AppMode::Browse) else {
        return Ok(false);
    };
    match dlg.action {
        ConfirmAction::CloseTextEditorUnsaved => {
            app.close_panel_text_editor();
        }
        ConfirmAction::SaveEditorBeforeQuit => {
            app.close_panel_text_editor();
            return confirm_quit(app);
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Input dialog
// ---------------------------------------------------------------------------

fn handle_input(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Read button-focus state and macro_name without holding a mutable borrow.
    let (focused_button, macro_name) = if let AppMode::Input(ref dlg) = app.mode {
        (dlg.focused_button, dlg.macro_name)
    } else {
        return Ok(false);
    };

    // --- Button-focus mode: Tab/arrows cycle buttons, Enter activates, Esc returns to input ---
    if let Some(focused_idx) = focused_button {
        let button_count = macro_name
            .and_then(|_| {
                if let AppMode::Input(ref dlg) = app.mode {
                    crate::lua_dialog::input_render_spec(dlg)
                } else {
                    None
                }
            })
            .map(|s| s.buttons.len())
            .unwrap_or(0);

        match key.code {
            KeyCode::Tab | KeyCode::Right => {
                let AppMode::Input(ref mut dlg) = app.mode else {
                    return Ok(false);
                };
                dlg.focused_button = if focused_idx + 1 < button_count {
                    Some(focused_idx + 1)
                } else {
                    None // wrap back to input field
                };
            }
            KeyCode::BackTab | KeyCode::Left => {
                let AppMode::Input(ref mut dlg) = app.mode else {
                    return Ok(false);
                };
                dlg.focused_button = if focused_idx > 0 {
                    Some(focused_idx - 1)
                } else {
                    None // wrap back to input field
                };
            }
            KeyCode::Esc => {
                // Return focus to input field
                let AppMode::Input(ref mut dlg) = app.mode else {
                    return Ok(false);
                };
                dlg.focused_button = None;
            }
            KeyCode::Enter => {
                let (is_cancel, value, action) = if let AppMode::Input(ref dlg) = app.mode {
                    let spec = dlg
                        .macro_name
                        .and_then(|_| crate::lua_dialog::input_render_spec(dlg));
                    let is_cancel = spec
                        .as_ref()
                        .and_then(|s| s.buttons.get(focused_idx))
                        .map(|b| b.callback == "cancel")
                        .unwrap_or(false);
                    (is_cancel, dlg.text().to_string(), dlg.action.clone())
                } else {
                    return Ok(false);
                };
                app.mode = AppMode::Browse;
                if !is_cancel {
                    input_execute_action(app, action, &value)?;
                }
            }
            _ => {
                // Any other key: return focus to input and forward to textarea
                let AppMode::Input(ref mut dlg) = app.mode else {
                    return Ok(false);
                };
                dlg.focused_button = None;
                if let Some(input) = textarea_input_from_key_event(key) {
                    dlg.textarea.input(input);
                }
            }
        }
        return Ok(false);
    }

    // --- Normal text-input mode ---
    let AppMode::Input(ref mut dlg) = app.mode else {
        return Ok(false);
    };

    // Ctrl+V: paste from system clipboard (single-line — strip newlines)
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
    {
        if let Some(text) = paste_text_from_clipboard() {
            for ch in text.chars().filter(|&c| c != '\n' && c != '\r') {
                dlg.textarea.input(ratatui_textarea::Input {
                    key: ratatui_textarea::Key::Char(ch),
                    ctrl: false,
                    alt: false,
                    shift: false,
                });
            }
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Tab if macro_name.is_some() => {
            dlg.focused_button = Some(0);
        }
        KeyCode::Esc => app.mode = AppMode::Browse,
        KeyCode::Enter => {
            let value = dlg.text().to_string();
            let action = dlg.action.clone();
            app.mode = AppMode::Browse;
            input_execute_action(app, action, &value)?;
        }
        _ => {
            if let Some(input) = textarea_input_from_key_event(key) {
                dlg.textarea.input(input);
            }
        }
    }
    Ok(false)
}

fn input_execute_action(app: &mut App, action: InputAction, value: &str) -> Result<()> {
    match action {
        InputAction::Rename(path) => match crate::file_ops::rename_entry(&path, value) {
            Ok(_) => {
                app.notify(format!("Renamed to '{}'", value));
                if app.config.auto_reload {
                    app.reload_panels();
                }
            }
            Err(e) => app.notify(format!("Rename error: {}", e)),
        },
        InputAction::Mkdir(parent) => match crate::file_ops::make_dir(&parent, value) {
            Ok(path) => {
                app.enter_dir(path)?;
                app.notify(format!("Created directory '{}'", value));
                if app.config.auto_reload {
                    app.reload_panels();
                }
            }
            Err(e) => app.notify(format!("mkdir error: {}", e)),
        },
        InputAction::RemoteRename { profile, path } => {
            let Some(parent) = std::path::Path::new(&path).parent() else {
                app.notify("Rename error: invalid remote path");
                return Ok(());
            };
            let dst = join_remote(&parent.to_string_lossy(), value);
            match app.run_with_busy("Remote: renaming...", |_| {
                remote_rename_path(&profile, &path, &dst)
            }) {
                Ok(_) => {
                    app.notify(format!("Renamed to '{}'", value));
                    if app.config.auto_reload {
                        app.reload_panels();
                    }
                }
                Err(e) => app.notify(format!("Rename error: {}", e)),
            }
        }
        InputAction::RemoteMkdir { profile, parent } => {
            let path = join_remote(&parent, value);
            match app.run_with_busy("Remote: creating directory...", |_| {
                remote_make_dir(&profile, &path)
            }) {
                Ok(_) => {
                    app.enter_dir(std::path::PathBuf::from(path))?;
                    app.notify(format!("Created directory '{}'", value));
                    if app.config.auto_reload {
                        app.reload_panels();
                    }
                }
                Err(e) => app.notify(format!("mkdir error: {}", e)),
            }
        }
        InputAction::SelectPattern => {
            app.active_panel_mut().select_pattern(value, true);
        }
        InputAction::DeselectPattern => {
            app.active_panel_mut().select_pattern(value, false);
        }
        InputAction::GoToPath => {
            let path = std::path::PathBuf::from(value);
            if path.is_dir() {
                app.enter_dir(path)?;
            } else {
                app.notify(format!("Not a directory: {}", value));
            }
        }
        InputAction::PluginAction { plugin, id, cwd } => {
            match app.run_with_busy("Running plugin action...", |_| {
                crate::plugins::run_action(&plugin, &id, &cwd, Some(value))
            }) {
                Ok(message) => app.notify(if message.trim().is_empty() {
                    "Action complete".to_string()
                } else {
                    message
                }),
                Err(e) => app.notify(format!("Action error: {}", e)),
            }
            app.needs_full_redraw = true;
            if app.config.auto_reload {
                app.reload_panels();
            }
        }
        InputAction::SaveSelectionSession => {
            app.cmd_save_selection_session(value);
        }
    }
    Ok(())
}

fn handle_assoc_input(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::AssocInput(ref mut dlg) = app.mode else {
        return Ok(false);
    };

    let is_openers = matches!(dlg.action, AssocInputAction::Openers { .. });
    let save_openers = is_openers
        && (matches!(key.code, KeyCode::F(2))
            || (key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'))));

    // Ctrl+V: paste — allow newlines only in openers mode
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
    {
        if let Some(text) = paste_text_from_clipboard() {
            for ch in text.chars() {
                if ch == '\n' || ch == '\r' {
                    if is_openers {
                        dlg.textarea.input(ratatui_textarea::Input {
                            key: ratatui_textarea::Key::Enter,
                            ctrl: false,
                            alt: false,
                            shift: false,
                        });
                    }
                } else {
                    dlg.textarea.input(ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Char(ch),
                        ctrl: false,
                        alt: false,
                        shift: false,
                    });
                }
            }
        }
        return Ok(false);
    }

    // Ctrl+J: insert newline in openers mode
    if is_openers
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('j') | KeyCode::Char('J'))
    {
        dlg.textarea.input(ratatui_textarea::Input {
            key: ratatui_textarea::Key::Enter,
            ctrl: false,
            alt: false,
            shift: false,
        });
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::AssocEditor(AssocEditorState::from_config(&app.config));
        }
        _ if save_openers => {
            let value = dlg.textarea.lines().join("\n");
            let ext = match &dlg.action {
                AssocInputAction::Openers { ext, .. } => ext.clone(),
                AssocInputAction::MimeType => String::new(),
            };
            app.mode = AppMode::Browse;
            apply_assoc_openers_input(app, &ext, &value);
            app.mode = AppMode::AssocEditor(AssocEditorState::from_config(&app.config));
        }
        KeyCode::Enter => {
            if is_openers {
                let AppMode::AssocInput(ref mut dlg) = app.mode else {
                    return Ok(false);
                };
                dlg.textarea.input(ratatui_textarea::Input {
                    key: ratatui_textarea::Key::Enter,
                    ctrl: false,
                    alt: false,
                    shift: false,
                });
                return Ok(false);
            }

            let value = dlg.first_line().to_string();
            let action = dlg.action.clone();
            app.mode = AppMode::Browse;

            match action {
                AssocInputAction::MimeType => {
                    let mime_type = value.trim().to_ascii_lowercase();
                    if mime_type.is_empty() {
                        app.mode = AppMode::AssocEditor(AssocEditorState::from_config(&app.config));
                    } else {
                        let existing = app.config.openers_for_mime(&mime_type).join("\n");
                        app.mode = AppMode::AssocInput(AssocInputDialog {
                            title: "Association".into(),
                            prompt: format!("Openers for {} (one command per line):", mime_type),
                            textarea: AssocInputDialog::make_textarea(&existing),
                            action: AssocInputAction::Openers {
                                ext: mime_type,
                                edit_index: None,
                            },
                        });
                    }
                }
                AssocInputAction::Openers { ext, edit_index } => {
                    let _ = edit_index;
                    apply_assoc_openers_input(app, &ext, &value);
                    app.mode = AppMode::AssocEditor(AssocEditorState::from_config(&app.config));
                }
            }
        }
        _ => {
            if let Some(input) =
                crate::events::panel_text_editor::textarea_input_from_key_event(key)
            {
                let AppMode::AssocInput(ref mut dlg) = app.mode else {
                    return Ok(false);
                };
                dlg.textarea.input(input);
            }
        }
    }
    Ok(false)
}

fn apply_assoc_openers_input(app: &mut App, ext: &str, value: &str) {
    let mime_type = ext.trim().to_ascii_lowercase();
    let openers: Vec<String> = value
        .lines()
        .flat_map(|line| line.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if openers.is_empty() {
        app.config
            .file_assoc
            .retain(|a| !a.mime_type.eq_ignore_ascii_case(&mime_type));
    } else if let Some(existing) = app
        .config
        .file_assoc
        .iter_mut()
        .find(|a| a.mime_type.eq_ignore_ascii_case(&mime_type))
    {
        existing.openers = openers;
    } else {
        app.config
            .file_assoc
            .push(crate::config::FileAssoc { mime_type, openers });
    }
    app.save_config().ok();
}

fn handle_text_input_paste<T: TextInputState>(dlg: &mut T, key: KeyEvent) -> bool {
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
    {
        if let Some(text) = paste_text_from_clipboard() {
            let cursor = dlg.cursor();
            dlg.value_mut().insert_str(cursor, &text);
            *dlg.cursor_mut() = cursor + text.len();
        }
        return true;
    }

    false
}

fn handle_text_input_edit_key<T: TextInputState>(dlg: &mut T, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => dlg.insert_char(ch),
        KeyCode::Backspace => dlg.backspace(),
        KeyCode::Delete => dlg.delete_char(),
        KeyCode::Left => dlg.move_left(),
        KeyCode::Right => dlg.move_right(),
        KeyCode::Home => dlg.home(),
        KeyCode::End => dlg.end(),
        _ => return false,
    }

    true
}

fn handle_copy_dialog(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::CopyDialog(ref mut dlg) = app.mode else {
        return Ok(false);
    };

    if dlg.waiting_to_start {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                app.cancel_copy_scan();
                app.mode = AppMode::Browse;
                app.set_status("Copy aborted");
            }
            _ => {}
        }
        return Ok(false);
    }

    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
        && dlg.field == CopyDialogState::DESTINATION
    {
        if let Some(text) = paste_text_from_clipboard() {
            dlg.destination.insert_str(dlg.cursor, &text);
            dlg.cursor += text.len();
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => {
            app.cancel_copy_scan();
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            dlg.field = dlg.field.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Tab => {
            dlg.field = (dlg.field + 1).min(CopyDialogState::CANCEL);
        }
        KeyCode::BackTab => {
            dlg.field = dlg.field.saturating_sub(1);
        }
        KeyCode::Left => {
            if dlg.field == CopyDialogState::DESTINATION && dlg.cursor > 0 {
                dlg.cursor -= 1;
            }
        }
        KeyCode::Right => {
            if dlg.field == CopyDialogState::DESTINATION {
                dlg.cursor = (dlg.cursor + 1).min(dlg.destination.len());
            }
        }
        KeyCode::Backspace => {
            if dlg.field == CopyDialogState::DESTINATION && dlg.cursor > 0 {
                dlg.destination.remove(dlg.cursor - 1);
                dlg.cursor -= 1;
            }
        }
        KeyCode::Delete => {
            if dlg.field == CopyDialogState::DESTINATION && dlg.cursor < dlg.destination.len() {
                dlg.destination.remove(dlg.cursor);
            }
        }
        KeyCode::Char(' ') => match dlg.field {
            CopyDialogState::OVERWRITE => dlg.overwrite = !dlg.overwrite,
            CopyDialogState::NEWER_ONLY => dlg.newer_only = !dlg.newer_only,
            CopyDialogState::KEEP_ATTRIBUTES => dlg.keep_attributes = !dlg.keep_attributes,
            _ => {}
        },
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if dlg.field == CopyDialogState::DESTINATION {
                dlg.destination.insert(dlg.cursor, ch);
                dlg.cursor += ch.len_utf8();
            }
        }
        KeyCode::Enter => match dlg.field {
            CopyDialogState::OVERWRITE => dlg.overwrite = !dlg.overwrite,
            CopyDialogState::NEWER_ONLY => dlg.newer_only = !dlg.newer_only,
            CopyDialogState::KEEP_ATTRIBUTES => dlg.keep_attributes = !dlg.keep_attributes,
            CopyDialogState::CANCEL => app.mode = AppMode::Browse,
            _ => {
                if dlg.stats_pending {
                    dlg.waiting_to_start = true;
                } else {
                    let state = dlg.clone();
                    app.mode = AppMode::Browse;
                    app.execute_copy_dialog(state)?;
                }
            }
        },
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Search panel
// ---------------------------------------------------------------------------

fn handle_search(app: &mut App, key: KeyEvent) -> Result<bool> {
    let page_size = 10usize;
    let fn_key = fx_shortcut(key);
    match key.code {
        KeyCode::F(1) => {
            app.open_help_with_anchor_and_return("search-file-s", "search");
            return Ok(false);
        }
        KeyCode::Esc => {
            // If a search is running, cancel it then close the panel
            app.cancel_search();
            app.mode = AppMode::Browse;
        }
        _ if fn_key == Some(10) => {
            app.cancel_search();
            app.mode = AppMode::Browse;
        }
        KeyCode::Tab => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            // Cycle input fields: 0→1→2→0; ↓ from any input field enters results
            s.input_field = (s.input_field + 1) % 3;
        }
        KeyCode::BackTab => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                s.input_field = 2;
            } else {
                s.input_field = (s.input_field + 2) % 3;
            }
        }
        _ if fn_key == Some(5) => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            s.backend = s.backend.next_available();
        }
        KeyCode::Enter => {
            let input_field = if let AppMode::SearchPanel(ref s) = app.mode {
                s.input_field
            } else {
                return Ok(false);
            };
            if input_field == 3 {
                // Navigate to selected file
                let selected = if let AppMode::SearchPanel(ref s) = app.mode {
                    s.results.get(s.cursor).map(|r| r.path.clone())
                } else {
                    None
                };
                if let Some(path) = selected {
                    app.mode = AppMode::Browse;
                    if let Some(dir) = path.parent() {
                        app.enter_dir(dir.to_path_buf())?;
                        let file_name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        app.active_panel_mut().cursor = app
                            .active_panel()
                            .entries
                            .iter()
                            .position(|e| e.name == file_name)
                            .unwrap_or(0);
                    }
                }
            } else {
                app.run_search();
                // Auto-focus results if any
                if let AppMode::SearchPanel(ref mut s) = app.mode {
                    if !s.results.is_empty() {
                        s.input_field = 3;
                    }
                }
            }
        }
        KeyCode::Up => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                if s.cursor > 0 {
                    s.cursor -= 1;
                    if s.cursor < s.scroll {
                        s.scroll = s.cursor;
                    }
                } else {
                    // Leave results focus back to inputs
                    s.input_field = 2;
                }
            } else {
                s.input_field = (s.input_field + 2) % 3;
            }
        }
        KeyCode::Down => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                if s.cursor + 1 < s.results.len() {
                    s.cursor += 1;
                }
            } else if !s.results.is_empty() {
                s.input_field = 3;
                s.cursor = 0;
                s.scroll = 0;
            } else {
                s.input_field = (s.input_field + 1) % 3;
            }
        }
        KeyCode::PageUp => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                s.cursor = s.cursor.saturating_sub(page_size);
                if s.cursor < s.scroll {
                    s.scroll = s.cursor;
                }
            }
        }
        KeyCode::PageDown => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                let max = s.results.len().saturating_sub(1);
                s.cursor = (s.cursor + page_size).min(max);
            }
        }
        KeyCode::Home => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                s.cursor = 0;
                s.scroll = 0;
            }
        }
        KeyCode::End => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            if s.input_field == 3 {
                s.cursor = s.results.len().saturating_sub(1);
            }
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            match s.input_field {
                0 => s.query.push(ch),
                1 => s.content_query.push(ch),
                2 => s.dir_query.push(ch),
                _ => {}
            }
        }
        KeyCode::Backspace => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            match s.input_field {
                0 => {
                    s.query.pop();
                }
                1 => {
                    s.content_query.pop();
                }
                2 => {
                    s.dir_query.pop();
                }
                _ => {}
            }
        }
        KeyCode::Delete => {
            let AppMode::SearchPanel(ref mut s) = app.mode else {
                return Ok(false);
            };
            // Clear the active input field
            match s.input_field {
                0 => {
                    s.query.clear();
                    s.query.push('*');
                }
                1 => s.content_query.clear(),
                2 => s.dir_query = s.start_dir.to_string_lossy().into_owned(),
                _ => {}
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_tree_view(app: &mut App, key: KeyEvent) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Esc => {
            if let AppMode::TreeView(state) = &mut app.mode {
                state.cancel_scan();
            }
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            if let AppMode::TreeView(state) = &mut app.mode {
                state.move_prev();
            }
        }
        KeyCode::Down => {
            if let AppMode::TreeView(state) = &mut app.mode {
                state.move_next();
            }
        }
        KeyCode::PageUp => {
            if let AppMode::TreeView(state) = &mut app.mode {
                for _ in 0..10 {
                    state.move_prev();
                }
            }
        }
        KeyCode::PageDown => {
            if let AppMode::TreeView(state) = &mut app.mode {
                for _ in 0..10 {
                    state.move_next();
                }
            }
        }
        KeyCode::Home => {
            if let AppMode::TreeView(state) = &mut app.mode {
                state.match_pos = 0;
            }
        }
        KeyCode::End => {
            if let AppMode::TreeView(state) = &mut app.mode {
                state.match_pos = state.filtered_indices().len().saturating_sub(1);
            }
        }
        KeyCode::F(5) => {
            if let AppMode::TreeView(state) = &mut app.mode {
                state.start_scan();
            }
        }
        KeyCode::Char('r') if ctrl => {
            if let AppMode::TreeView(state) = &mut app.mode {
                state.start_scan();
            }
        }
        KeyCode::Backspace => {
            if let AppMode::TreeView(state) = &mut app.mode {
                state.pop_query();
            }
        }
        KeyCode::Char(ch) if !ctrl && !alt => {
            if let AppMode::TreeView(state) = &mut app.mode {
                state.push_query(ch);
            }
        }
        KeyCode::Enter => {
            let selected = if let AppMode::TreeView(state) = &app.mode {
                state.selected_entry().cloned()
            } else {
                None
            };
            if let Some(entry) = selected {
                let dir = if entry.is_dir {
                    entry.path
                } else {
                    entry
                        .path
                        .parent()
                        .map(std::path::Path::to_path_buf)
                        .unwrap_or(entry.path)
                };
                if let AppMode::TreeView(state) = &mut app.mode {
                    state.cancel_scan();
                }
                app.mode = AppMode::Browse;
                app.enter_dir(dir)?;
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_compare_panel(app: &mut App, key: KeyEvent) -> Result<bool> {
    if let AppMode::ComparePanel(state) = &mut app.mode
        && state.search_active
    {
        match key.code {
            KeyCode::Esc => {
                state.search_active = false;
                return Ok(false);
            }
            KeyCode::Enter => {
                let _ = jump_to_compare_search_match(state, true, true);
                state.search_active = false;
                return Ok(false);
            }
            KeyCode::Up => {
                let _ = jump_to_compare_search_match(state, false, false);
                return Ok(false);
            }
            KeyCode::Down => {
                let _ = jump_to_compare_search_match(state, true, false);
                return Ok(false);
            }
            _ => {
                let pasted = handle_text_input_paste(state, key);
                let edited = pasted || handle_text_input_edit_key(state, key);
                if edited {
                    let _ = jump_to_compare_search_match(state, true, true);
                    return Ok(false);
                }
            }
        }
    }

    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Char('/') => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                state.search_active = true;
                state.search_cursor = state.search_query.len();
            }
        }
        KeyCode::Char('n') => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                let _ = jump_to_compare_search_match(state, true, false);
            }
        }
        KeyCode::Char('N') => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                let _ = jump_to_compare_search_match(state, false, false);
            }
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                state.show_only_differences = !state.show_only_differences;
                rebuild_compare_panel_state(state);
            }
        }
        KeyCode::Char('w') | KeyCode::Char('W') => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                state.ignore_whitespace = !state.ignore_whitespace;
                rebuild_compare_panel_state(state);
            }
        }
        KeyCode::Char('l') | KeyCode::Char('L') => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                state.ignore_crlf = !state.ignore_crlf;
                rebuild_compare_panel_state(state);
            }
        }
        KeyCode::Up => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                state.move_prev();
            }
        }
        KeyCode::Down => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                state.move_next();
            }
        }
        KeyCode::PageUp => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                for _ in 0..10 {
                    state.move_prev();
                }
            }
        }
        KeyCode::PageDown => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                for _ in 0..10 {
                    state.move_next();
                }
            }
        }
        KeyCode::Home => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                state.cursor = 0;
                state.scroll = 0;
            }
        }
        KeyCode::End => {
            if let AppMode::ComparePanel(state) = &mut app.mode {
                state.cursor = state.rows.len().saturating_sub(1);
            }
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Dir history
// ---------------------------------------------------------------------------

fn handle_dir_bookmarks(app: &mut App, key: KeyEvent) -> Result<bool> {
    let _fn_key = fx_shortcut(key);
    if matches!(key.code, KeyCode::F(1)) {
        app.open_help_with_anchor_and_return("directory-bookmarks", "bookmarks");
        return Ok(false);
    }
    // Continue with existing logic
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            app.move_prev_bookmark();
        }
        KeyCode::Down => {
            app.move_next_bookmark();
        }
        KeyCode::Backspace => {
            app.pop_bookmark_query();
        }
        KeyCode::Enter => {
            let selected = app
                .filtered_bookmark_items()
                .get(app.bookmark_match_pos)
                .cloned();
            match selected {
                Some(BookmarkListItem::AddCurrentDir(_)) => {
                    if app.add_current_dir_bookmark() {
                        if let Err(e) = app.save_config() {
                            app.set_status(format!("Save error: {}", e));
                        }
                    }
                }
                Some(BookmarkListItem::Existing(idx)) => {
                    let path = app.bookmarks.get(idx).cloned();
                    app.mode = AppMode::Browse;
                    if let Some(p) = path {
                        let s = p.to_string_lossy();
                        if let Some(rest) = s.strip_prefix("remote://") {
                            let (profile_name, cwd) = rest.split_once('/').unwrap_or((rest, ""));
                            let target_cwd = format!("/{}", cwd);
                            let profiles = load_profiles().unwrap_or_default();
                            if let Some(profile) =
                                profiles.into_iter().find(|pr| pr.name == profile_name)
                            {
                                app.start_remote_connect_with_cwd(profile, target_cwd);
                            } else {
                                app.notify(format!("Remote profile not found: {}", profile_name));
                            }
                        } else {
                            if let Some(target) = first_accessible_bookmark_dir(&p) {
                                if app.active_panel().is_remote_view() {
                                    app.active_panel_mut().disconnect();
                                }
                                if target != p {
                                    app.set_status(format!(
                                        "Bookmark path unreachable, opened parent: {}",
                                        target.display()
                                    ));
                                }
                                app.enter_dir(target)?;
                            } else {
                                app.notify(format!(
                                    "Bookmark path is not accessible: {}",
                                    p.display()
                                ));
                            }
                        }
                    }
                }
                None => app.mode = AppMode::Browse,
            }
        }
        KeyCode::F(8) => {
            delete_selected_bookmark(app);
        }
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !ch.is_control() =>
        {
            app.append_bookmark_query(ch);
        }
        _ => {}
    }
    Ok(false)
}

fn first_accessible_bookmark_dir(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut cur = Some(path);
    while let Some(candidate) = cur {
        if std::fs::metadata(candidate)
            .map(|meta| meta.file_type().is_dir())
            .unwrap_or(false)
        {
            return Some(candidate.to_path_buf());
        }
        cur = candidate.parent();
    }
    None
}

fn delete_selected_bookmark(app: &mut App) {
    if let Some(BookmarkListItem::Existing(idx)) = app
        .filtered_bookmark_items()
        .get(app.bookmark_match_pos)
        .cloned()
        && idx < app.bookmarks.len()
    {
        app.bookmarks.remove(idx);
        if app.bookmark_cursor >= app.bookmarks.len() && app.bookmark_cursor > 0 {
            app.bookmark_cursor -= 1;
        }
        app.sync_bookmark_cursor();
        if let Err(e) = app.save_config() {
            app.set_status(format!("Save error: {}", e));
        }
    }
}

fn handle_plugins(app: &mut App, key: KeyEvent) -> Result<bool> {
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
            if let AppMode::Plugins(ref mut s) = app.mode {
                s.move_prev();
            }
        }
        KeyCode::Down => {
            if let AppMode::Plugins(ref mut s) = app.mode {
                s.move_next();
            }
        }
        KeyCode::Home => {
            if let AppMode::Plugins(ref mut s) = app.mode {
                s.cursor = 0;
            }
        }
        KeyCode::End => {
            if let AppMode::Plugins(ref mut s) = app.mode {
                s.cursor = s.filtered_indices().len().saturating_sub(1);
            }
        }
        KeyCode::Backspace => {
            if let AppMode::Plugins(ref mut s) = app.mode {
                s.pop_query();
            }
        }
        KeyCode::Char(ch) if !ctrl && !alt && !ch.is_control() => {
            if let AppMode::Plugins(ref mut s) = app.mode {
                s.append_query(ch);
            }
        }
        KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('O') => {
            let dir = if let AppMode::Plugins(ref s) = app.mode {
                // Use the selected plugin's own directory; fall back to the global plugins_dir
                let selected = s
                    .filtered_indices()
                    .get(s.cursor)
                    .and_then(|idx| s.plugins.get(*idx));
                selected
                    .map(|p| p.dir.clone())
                    .filter(|d| !d.as_os_str().is_empty())
                    .unwrap_or_else(|| s.plugins_dir.clone())
            } else {
                return Ok(false);
            };
            if dir.as_os_str().is_empty() {
                app.set_status("Plugin directory unavailable");
            } else {
                app.mode = AppMode::Browse;
                if let Err(e) = app.enter_dir(dir.clone()) {
                    app.set_status(format!("Cannot enter plugin directory: {}", e));
                } else {
                    app.set_status(format!("Plugin directory: {}", dir.display()));
                }
            }
        }
        KeyCode::Char('s') | KeyCode::Char('S') if ctrl && !alt => {
            let index_path = crate::plugins::store_index_path();
            match crate::app::StoreInstallPaletteState::load(index_path.clone()) {
                Ok(state) => app.mode = AppMode::StoreInstallPalette(state),
                Err(e) => app.notify(format!(
                    "Cannot load plugin store index {}: {}",
                    index_path.display(),
                    e
                )),
            }
        }
        KeyCode::Delete => {
            let selected_dir = if let AppMode::Plugins(ref s) = app.mode {
                s.filtered_indices()
                    .get(s.cursor)
                    .and_then(|idx| s.plugins.get(*idx))
                    .map(|p| p.dir.clone())
            } else {
                None
            };

            if let Some(dir) = selected_dir {
                match crate::plugins::remove_plugin(&dir) {
                    Ok(()) => {
                        let cursor = if let AppMode::Plugins(ref s) = app.mode {
                            s.cursor
                        } else {
                            0
                        };
                        let mut refreshed = crate::app::PluginsState::load();
                        if !refreshed.plugins.is_empty() {
                            refreshed.cursor =
                                cursor.min(refreshed.plugins.len().saturating_sub(1));
                        }
                        app.notify("Plugin removed");
                        app.reload_panels();
                        app.mode = AppMode::Plugins(refreshed);
                    }
                    Err(e) => {
                        app.notify(format!("Cannot remove plugin: {}", e));
                        app.mode = AppMode::Plugins(crate::app::PluginsState::load());
                    }
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_action_palette(app: &mut App, key: KeyEvent) -> Result<bool> {
    let fn_key = fx_shortcut(key);
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        _ if fn_key == Some(10) => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up => {
            if let AppMode::ActionPalette(ref mut state) = app.mode {
                state.cursor = state.cursor.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if let AppMode::ActionPalette(ref mut state) = app.mode {
                let max = state.actions.len().saturating_sub(1);
                state.cursor = (state.cursor + 1).min(max);
            }
        }
        KeyCode::Enter => {
            let (action, cwd) = if let AppMode::ActionPalette(ref state) = app.mode {
                let Some(action) = state.actions.get(state.cursor).cloned() else {
                    app.mode = AppMode::Browse;
                    return Ok(false);
                };
                (action, state.cwd.clone())
            } else {
                return Ok(false);
            };

            app.mode = AppMode::Browse;
            if let Some(prompt) = action.prompt.clone() {
                app.mode = AppMode::Input(InputDialog {
                    title: Some(action.title),
                    prompt: Some(prompt),
                    textarea: InputDialog::make_textarea(""),
                    action: InputAction::PluginAction {
                        plugin: action.plugin,
                        id: action.id,
                        cwd,
                    },
                    macro_name: Some("input_plugin_action"),
                    focused_button: Some(0),
                });
            } else {
                match app.run_with_busy("Running plugin action...", |_| {
                    crate::plugins::run_action(&action.plugin, &action.id, &cwd, None)
                }) {
                    Ok(message) => app.notify(if message.trim().is_empty() {
                        "Action complete".to_string()
                    } else {
                        message
                    }),
                    Err(e) => app.notify(format!("Action error: {}", e)),
                }
                app.needs_full_redraw = true;
                if app.config.auto_reload {
                    app.reload_panels();
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Config screen
// ---------------------------------------------------------------------------

fn handle_config(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::Config(ref mut cs) = app.mode else {
        return Ok(false);
    };

    let total = ConfigState::NUM_TOTAL; // booleans + 4 text + OK + Cancel
    let ok_idx = ConfigState::ok_cursor();
    let cancel_idx = ConfigState::cancel_cursor();

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }

        // Navigate rows
        KeyCode::Up | KeyCode::BackTab => {
            if let AppMode::Config(ref mut cs) = app.mode {
                let first = ConfigState::first_cursor_for_tab(cs.tab);
                let last = ConfigState::last_cursor_for_tab(cs.tab);
                if cs.cursor == ok_idx {
                    cs.cursor = last;
                } else if cs.cursor == cancel_idx {
                    cs.cursor = ok_idx;
                } else if cs.cursor > first {
                    cs.cursor -= 1;
                } else {
                    cs.cursor = cancel_idx;
                }
                cs.sync_tab_to_cursor();
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            if let AppMode::Config(ref mut cs) = app.mode {
                let last = ConfigState::last_cursor_for_tab(cs.tab);
                if cs.cursor < last {
                    cs.cursor += 1;
                } else if cs.cursor == ok_idx {
                    cs.cursor = cancel_idx;
                } else if cs.cursor == cancel_idx {
                    cs.cursor = ConfigState::first_cursor_for_tab(cs.tab);
                } else {
                    cs.cursor = ok_idx;
                }
                cs.sync_tab_to_cursor();
            }
        }
        KeyCode::Left
            if key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            if let AppMode::Config(ref mut cs) = app.mode {
                let tab = if cs.tab == 0 {
                    ConfigState::TAB_COUNT - 1
                } else {
                    cs.tab - 1
                };
                cs.set_tab(tab);
            }
        }
        KeyCode::Right
            if key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            if let AppMode::Config(ref mut cs) = app.mode {
                cs.set_tab((cs.tab + 1) % ConfigState::TAB_COUNT);
            }
        }
        KeyCode::PageUp => {
            if let AppMode::Config(ref mut cs) = app.mode {
                let tab = if cs.tab == 0 {
                    ConfigState::TAB_COUNT - 1
                } else {
                    cs.tab - 1
                };
                cs.set_tab(tab);
            }
        }
        KeyCode::PageDown => {
            if let AppMode::Config(ref mut cs) = app.mode {
                cs.set_tab((cs.tab + 1) % ConfigState::TAB_COUNT);
            }
        }
        KeyCode::Left => {
            if let AppMode::Config(ref mut cs) = app.mode {
                let tab = if cs.tab == 0 {
                    ConfigState::TAB_COUNT - 1
                } else {
                    cs.tab - 1
                };
                cs.set_tab(tab);
            }
        }
        KeyCode::Right => {
            if let AppMode::Config(ref mut cs) = app.mode {
                cs.set_tab((cs.tab + 1) % ConfigState::TAB_COUNT);
            }
        }

        // Toggle checkbox or activate button
        KeyCode::Char(' ') | KeyCode::Enter => {
            let cursor = cs.cursor;
            match cursor {
                0 => cs.confirm_exit = !cs.confirm_exit,
                1 => cs.confirm_delete = !cs.confirm_delete,
                2 => cs.auto_reload = !cs.auto_reload,
                3 => cs.insert_moves_down = !cs.insert_moves_down,
                4 => cs.select_dirs = !cs.select_dirs,
                5 => cs.show_hidden = !cs.show_hidden,
                6 => cs.color_by_type = !cs.color_by_type,
                7 => cs.show_cloud_icons = !cs.show_cloud_icons,
                8 => cs.show_file_icons = !cs.show_file_icons,
                9 => cs.show_fkey_bar = !cs.show_fkey_bar,
                10 => cs.word_wrap = !cs.word_wrap,
                11 => cs.default_zoom = !cs.default_zoom,
                12 => cs.debug_log = !cs.debug_log,
                // text fields: Enter moves focus to next
                13 | 14 | 15 | 16 => {
                    if let AppMode::Config(ref mut cs) = app.mode {
                        if cs.cursor + 1 < total {
                            cs.cursor += 1;
                        }
                        cs.sync_tab_to_cursor();
                    }
                }
                c if c == ok_idx => {
                    // Apply & save
                    let cs_clone = cs.clone();
                    app.mode = AppMode::Browse;
                    cs_clone.apply_to(&mut app.config);
                    // Sync hidden flag on live panels
                    app.left.show_hidden = app.config.left.show_hidden;
                    app.right.show_hidden = app.config.right.show_hidden;
                    // Apply debug-log toggle immediately
                    crate::viewer::set_debug_log_enabled(app.config.debug_log);
                    let _ = app.left.reload();
                    let _ = app.right.reload();
                    match app.save_config() {
                        Ok(_) => {
                            if app.config.debug_log {
                                let log_path = crate::viewer::debug_log_path()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_else(|| "?".into());
                                app.set_status(format!("Config saved — debug log: {}", log_path));
                            } else {
                                app.set_status("Config saved");
                            }
                        }
                        Err(e) => app.set_status(format!("Save error: {}", e)),
                    }
                }
                c if c == cancel_idx => {
                    app.mode = AppMode::Browse;
                }
                _ => {}
            }
        }

        // Text field editing
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let AppMode::Config(ref mut cs) = app.mode {
                match cs.cursor {
                    13 => cs.screensaver_idle_minutes.push(ch),
                    14 => cs.editor.push(ch),
                    15 => cs.pager.push(ch),
                    16 => cs.dir_history_max.push(ch),
                    _ => {}
                }
            }
        }
        KeyCode::Backspace => {
            if let AppMode::Config(ref mut cs) = app.mode {
                match cs.cursor {
                    13 => {
                        cs.screensaver_idle_minutes.pop();
                    }
                    14 => {
                        cs.editor.pop();
                    }
                    15 => {
                        cs.pager.pop();
                    }
                    16 => {
                        cs.dir_history_max.pop();
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Opener picker
// ---------------------------------------------------------------------------

fn handle_opener(app: &mut App, key: KeyEvent) -> Result<bool> {
    let AppMode::Opener(ref mut s) = app.mode else {
        return Ok(false);
    };

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up | KeyCode::BackTab => {
            s.move_prev();
        }
        KeyCode::Down | KeyCode::Tab => {
            s.move_next();
        }
        KeyCode::Backspace => {
            s.pop_query();
        }
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !ch.is_control() =>
        {
            s.append_query(ch);
        }
        KeyCode::Enter => {
            let (action, path) = if let AppMode::Opener(s) = &app.mode {
                let Some(action) = s.selected_item().cloned() else {
                    return Ok(false);
                };
                (action, s.path.clone())
            } else {
                return Ok(false);
            };
            app.mode = AppMode::Browse;
            execute_opener_action(app, action, &path)?;
        }
        _ => {}
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Association editor
// ---------------------------------------------------------------------------

fn handle_assoc_editor(app: &mut App, key: KeyEvent) -> Result<bool> {
    let _fn_key = fx_shortcut(key);
    if matches!(key.code, KeyCode::F(1)) {
        app.open_help_with_anchor_and_return("file-associations", "associations");
        return Ok(false);
    }
    // Continue with existing logic
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
    {
        if let Some(text) = paste_text_from_clipboard()
            && let AppMode::AssocEditor(ref mut s) = app.mode
        {
            s.query.push_str(&text);
            s.clamp_match();
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Up | KeyCode::BackTab => {
            if let AppMode::AssocEditor(ref mut s) = app.mode {
                s.move_prev();
            }
        }
        KeyCode::Down | KeyCode::Tab => {
            if let AppMode::AssocEditor(ref mut s) = app.mode {
                s.move_next();
            }
        }
        KeyCode::Backspace => {
            if let AppMode::AssocEditor(ref mut s) = app.mode {
                s.pop_query();
            }
        }
        // Add new association
        KeyCode::Char('a') | KeyCode::Char('A') | KeyCode::Char('+') => {
            app.mode = assoc_mime_input_dialog(app);
        }
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !ch.is_control() =>
        {
            if let AppMode::AssocEditor(ref mut s) = app.mode {
                s.push_query(ch);
            }
        }
        // Edit selected
        KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char('E') => {
            let (mime_type, openers_str, idx) = if let AppMode::AssocEditor(ref s) = app.mode {
                let Some(idx) = s.selected_index() else {
                    return Ok(false);
                };
                let (mime_type, openers) = &s.assocs[idx];
                (mime_type.clone(), openers.join("\n"), idx)
            } else {
                return Ok(false);
            };
            app.mode = AppMode::AssocInput(AssocInputDialog {
                title: "Edit association".into(),
                prompt: format!("Openers for {} (one command per line):", mime_type),
                textarea: AssocInputDialog::make_textarea(&openers_str),
                action: AssocInputAction::Openers {
                    ext: mime_type,
                    edit_index: Some(idx),
                },
            });
        }
        // Delete selected
        KeyCode::Delete | KeyCode::Char('d') | KeyCode::Char('D') => {
            let (mime_type, query, match_pos) = if let AppMode::AssocEditor(ref s) = app.mode {
                let Some(idx) = s.selected_index() else {
                    return Ok(false);
                };
                let (mime_type, _) = &s.assocs[idx];
                (mime_type.clone(), s.query.clone(), s.match_pos)
            } else {
                return Ok(false);
            };
            app.config
                .file_assoc
                .retain(|a| !a.mime_type.eq_ignore_ascii_case(&mime_type));
            app.save_config().ok();
            let mut new_s = AssocEditorState::from_config(&app.config);
            new_s.query = query;
            new_s.match_pos = match_pos;
            new_s.clamp_match();
            app.mode = AppMode::AssocEditor(new_s);
        }
        _ => {}
    }
    Ok(false)
}

fn assoc_mime_input_dialog(app: &App) -> AppMode {
    let value = default_assoc_mime_type(app).unwrap_or_default();
    AppMode::AssocInput(AssocInputDialog {
        title: "New association".into(),
        prompt: "MIME type:".into(),
        textarea: AssocInputDialog::make_textarea(&value),
        action: AssocInputAction::MimeType,
    })
}

fn default_assoc_mime_type(app: &App) -> Option<String> {
    let panel = app.active_panel();
    if panel.is_remote_view() {
        return None;
    }

    let entry = panel.current_entry()?;
    if entry.name == ".." || entry.is_dir || entry.cloud_only {
        return None;
    }

    crate::idf::probe_path(&entry.path)
        .and_then(|info| info.mime_types.into_iter().next())
        .filter(|mime_type| !mime_type.trim().is_empty())
}

fn handle_remote_connect(app: &mut App, key: KeyEvent) -> Result<bool> {
    let fn_key = fx_shortcut(key);
    if matches!(key.code, KeyCode::F(1)) {
        app.open_help_with_anchor_and_return("remote-browsing", "remote");
        return Ok(false);
    }
    // Continue with existing logic
    match key.code {
        KeyCode::Esc => app.mode = AppMode::Browse,
        KeyCode::Tab => {
            launch_ssh_for_profile(app)?;
        }
        KeyCode::Up => {
            if let AppMode::RemoteConnect(ref mut s) = app.mode {
                s.move_prev();
            }
        }
        KeyCode::Down => {
            if let AppMode::RemoteConnect(ref mut s) = app.mode {
                s.move_next();
            }
        }
        KeyCode::Backspace => {
            if let AppMode::RemoteConnect(ref mut s) = app.mode {
                s.pop_query();
            }
        }
        _ if fn_key == Some(6) => {
            app.open_remote_edit();
        }
        _ if fn_key == Some(7) => {
            app.open_remote_add_menu();
        }
        KeyCode::Enter => {
            let profile = if let AppMode::RemoteConnect(ref s) = app.mode {
                s.filtered_indices()
                    .get(s.match_pos)
                    .and_then(|idx| s.items.get(*idx))
                    .cloned()
            } else {
                None
            };
            if let Some(profile) = profile {
                let return_state = if let AppMode::RemoteConnect(ref s) = app.mode {
                    s.clone()
                } else {
                    crate::app::RemoteConnectState::load()
                };
                app.start_remote_connect(profile, return_state);
            }
        }
        KeyCode::Char(ch)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !ch.is_control() =>
        {
            if let AppMode::RemoteConnect(ref mut s) = app.mode {
                s.append_query(ch);
            }
        }
        _ => {}
    }
    Ok(false)
}

fn launch_ssh_for_profile(app: &mut App) -> Result<()> {
    let profile = if let AppMode::RemoteConnect(ref s) = app.mode {
        s.filtered_indices()
            .get(s.match_pos)
            .and_then(|idx| s.items.get(*idx))
            .cloned()
    } else {
        None
    };
    let Some(profile) = profile else {
        return Ok(());
    };
    let sftp = match &profile.kind {
        RemoteKind::Sftp(sftp) => sftp.clone(),
        _ => return Ok(()),
    };
    let mut args: Vec<String> = vec!["ssh".to_string()];
    match profile.source {
        RemoteSource::SshConfig => {
            args.push(profile.name.clone());
        }
        RemoteSource::UserToml => {
            if let Some(ref identity) = sftp.identity_file {
                args.push("-i".to_string());
                args.push(identity.clone());
            }
            if let Some(port) = sftp.port {
                args.push("-p".to_string());
                args.push(port.to_string());
            }
            let host = sftp.host.clone().unwrap_or_else(|| profile.name.clone());
            let target = if let Some(ref user) = sftp.user {
                format!("{}@{}", user, host)
            } else {
                host
            };
            args.push(target);
        }
        RemoteSource::PluginAuto => return Ok(()),
    }
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    let _ = std::process::Command::new(&args[0])
        .args(&args[1..])
        .status();
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    app.needs_clear = true;
    Ok(())
}

fn handle_remote_add_menu(app: &mut App, key: KeyEvent) -> Result<bool> {
    let choices = RemoteEditKind::all();
    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::RemoteConnect(crate::app::RemoteConnectState::load());
        }
        KeyCode::Up => {
            if let AppMode::RemoteAddMenu(ref mut c) = app.mode {
                if *c > 0 {
                    *c -= 1;
                }
            }
        }
        KeyCode::Down => {
            if let AppMode::RemoteAddMenu(ref mut c) = app.mode {
                if *c + 1 < choices.len() {
                    *c += 1;
                }
            }
        }
        KeyCode::Enter => {
            if let AppMode::RemoteAddMenu(cursor) = app.mode {
                if let Some(kind) = choices.get(cursor).cloned() {
                    app.mode = AppMode::RemoteEdit(crate::app::RemoteEditState::new(kind));
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_remote_connecting(app: &mut App, key: KeyEvent) -> Result<bool> {
    let fn_key = fx_shortcut(key);
    match key.code {
        KeyCode::Esc | KeyCode::Enter => app.cancel_remote_connect(),
        _ if fn_key == Some(10) => app.cancel_remote_connect(),
        _ => {}
    }
    Ok(false)
}

fn handle_remote_edit(app: &mut App, key: KeyEvent) -> Result<bool> {
    let fn_key = fx_shortcut(key);
    let AppMode::RemoteEdit(ref mut s) = app.mode else {
        return Ok(false);
    };

    // ── Share picker navigation (intercepts all keys when open) ──────────
    if s.share_picker.is_some() {
        match key.code {
            KeyCode::Esc => {
                s.share_picker = None;
            }
            _ if fn_key == Some(5) => {
                s.share_picker = None;
            }
            KeyCode::Up => {
                if let Some((_, ref mut cur)) = s.share_picker {
                    *cur = cur.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some((ref shares, ref mut cur)) = s.share_picker {
                    *cur = (*cur + 1).min(shares.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                if let Some((ref shares, cur)) = s.share_picker {
                    let path_idx = s.path_field_index();
                    s.set_field_text(path_idx, shares[cur].clone());
                }
                s.share_picker = None;
                // Move cursor to Password field after selecting share
                s.focus_field_at_end(crate::app::RemoteEditState::SECRET);
            }
            _ => {}
        }
        return Ok(false);
    }

    // ── F5/F6: remote plugin authentication via ABI ───────────────────────
    if s.plugin_auth_enabled && s.is_remote_plugin() {
        if fn_key == Some(5) {
            let Some(plugin_id) = s.kind.plugin_id().map(str::to_string) else {
                return Ok(false);
            };
            let config_json = s.plugin_config_json().unwrap_or_else(|| "{}".to_string());
            crate::viewer::debug_log(&format!(
                "remote-plugin-auth: start requested for '{}'",
                plugin_id
            ));
            if s.build_profile().is_none() {
                let message = s.kind.validation_message();
                app.set_status(message);
                return Ok(false);
            }
            match crate::remote::remote_plugin_auth_start(&plugin_id, &config_json) {
                Ok(auth_session) => {
                    let (status, details) = remote_plugin_auth_start_feedback(&auth_session);
                    for line in details {
                        crate::viewer::debug_log(&format!("remote-plugin-auth: {}", line));
                    }
                    s.plugin_auth_session_json = Some(auth_session);
                    s.cursor = s.auth_field_index().unwrap_or(s.cursor);
                    s.sync_cursor();
                    app.set_status(status);
                }
                Err(e) => {
                    crate::viewer::debug_log(&format!(
                        "remote-plugin-auth: start failed for '{}': {}",
                        plugin_id, e
                    ));
                    app.set_status(format!("Plugin auth start failed: {}", e));
                }
            }
            return Ok(false);
        }
        if fn_key == Some(6) {
            let Some(plugin_id) = s.kind.plugin_id().map(str::to_string) else {
                return Ok(false);
            };
            let Some(auth_session) = s.plugin_auth_session_json.clone() else {
                app.set_status("Start plugin auth first with F5");
                return Ok(false);
            };
            let config_json = s.plugin_config_json().unwrap_or_else(|| "{}".to_string());
            let input = s.plugin_auth_input().unwrap_or_default();
            crate::viewer::debug_log(&format!(
                "remote-plugin-auth: complete requested for '{}'",
                plugin_id
            ));
            match crate::remote::remote_plugin_auth_complete(
                &plugin_id,
                &config_json,
                &auth_session,
                &input,
            ) {
                Ok(updated_config_json) => {
                    if serde_json::from_str::<serde_json::Value>(&updated_config_json).is_err() {
                        app.set_status("Plugin auth returned invalid config JSON");
                        return Ok(false);
                    }
                    s.set_remote_plugin_config_json(&updated_config_json);
                    if let Some(auth_idx) = s.auth_field_index() {
                        s.set_field_text(auth_idx, "");
                    }
                    s.plugin_auth_session_json = None;
                    app.set_status("Plugin auth completed");
                }
                Err(e) => {
                    crate::viewer::debug_log(&format!(
                        "remote-plugin-auth: complete failed for '{}': {}",
                        plugin_id, e
                    ));
                    app.set_status(format!("Plugin auth complete failed: {}", e));
                }
            }
            return Ok(false);
        }
    }

    // ── F5: fetch SMB share list ──────────────────────────────────────────
    if fn_key == Some(5)
        && matches!(&s.kind, crate::app::RemoteEditKind::Smb)
        && s.cursor == s.path_field_index()
    {
        s.commit_textarea();
        let host = s.fields[crate::app::RemoteEditState::HOST]
            .trim()
            .to_string();
        if host.is_empty() {
            app.set_status("Enter host first");
            return Ok(false);
        }
        let user = s.fields[crate::app::RemoteEditState::USER].trim();
        let workgroup = s.fields[crate::app::RemoteEditState::PORT].trim();
        let password = s.fields[crate::app::RemoteEditState::SECRET].trim();
        let smb = crate::remote::SmbProfile {
            host: host.clone(),
            user: if user.is_empty() {
                None
            } else {
                Some(user.to_string())
            },
            workgroup: if workgroup.is_empty() {
                None
            } else {
                Some(workgroup.to_string())
            },
            password: if password.is_empty() {
                None
            } else {
                Some(password.to_string())
            },
            share: None,
            path: None,
        };
        let profile = crate::remote::RemoteProfile {
            name: "tmp".into(),
            source: crate::remote::RemoteSource::UserToml,
            kind: crate::remote::RemoteKind::Smb(smb),
        };
        match crate::remote::list_smb_shares(&profile) {
            Ok(shares) => {
                let current = s.fields[s.path_field_index()].trim().to_lowercase();
                let cur = shares
                    .iter()
                    .position(|sh| sh.to_lowercase() == current)
                    .unwrap_or(0);
                s.share_picker = Some((shares, cur));
            }
            Err(e) => {
                app.set_status(format!("Share list: {}", e));
            }
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => app.mode = AppMode::RemoteConnect(crate::app::RemoteConnectState::load()),
        KeyCode::Tab | KeyCode::Down => {
            s.focus_field_at_end((s.cursor + 1).min(s.cancel_index()));
        }
        KeyCode::BackTab | KeyCode::Up => {
            s.focus_field_at_end(s.cursor.saturating_sub(1));
        }
        KeyCode::Char(_) => {
            if s.cursor < s.input_count() {
                if let Some(input) = textarea_input_from_key_event(key) {
                    s.textarea.input(input);
                    s.commit_textarea();
                }
                if s.is_remote_plugin() && s.is_remote_plugin_config_cursor() {
                    s.plugin_auth_session_json = None;
                }
            }
        }
        KeyCode::Enter => {
            if s.cursor == s.cancel_index() {
                app.mode = AppMode::RemoteConnect(crate::app::RemoteConnectState::load());
            } else if s.cursor == s.save_index() {
                let save_request = s
                    .build_profile()
                    .map(|profile| (profile, s.edit_original_name.clone()));
                if let Some((profile, old_name)) = save_request {
                    match app.save_remote_profile(profile, old_name) {
                        Ok(()) => {
                            app.mode =
                                AppMode::RemoteConnect(crate::app::RemoteConnectState::load())
                        }
                        Err(e) => app.set_status(format!("Cannot save connection: {}", e)),
                    }
                } else {
                    let message = s.kind.validation_message();
                    app.set_status(message);
                }
            } else {
                s.focus_field_at_end((s.cursor + 1).min(s.cancel_index()));
            }
        }
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Backspace
        | KeyCode::Delete
        | KeyCode::Home
        | KeyCode::End => {
            if s.cursor < s.input_count()
                && let Some(input) = textarea_input_from_key_event(key)
            {
                s.textarea.input(input);
                s.commit_textarea();
                if s.is_remote_plugin() && s.is_remote_plugin_config_cursor() {
                    s.plugin_auth_session_json = None;
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

fn handle_help(app: &mut App, key: KeyEvent) -> Result<bool> {
    use crate::help::HelpView;
    let fn_key = fx_shortcut(key);

    let Some(state) = (match &mut app.mode {
        AppMode::Help(state) => Some(state),
        _ => None,
    }) else {
        return Ok(false);
    };

    match state.view {
        HelpView::Index { ref mut cursor } => match key.code {
            KeyCode::Esc => app.mode = AppMode::Browse,
            _ if fn_key == Some(10) => app.mode = AppMode::Browse,
            KeyCode::Up => *cursor = cursor.saturating_sub(1),
            KeyCode::Down => {
                let max = state.system.sections.len().saturating_sub(1);
                *cursor = (*cursor + 1).min(max);
            }
            KeyCode::Home => *cursor = 0,
            KeyCode::End => *cursor = state.system.sections.len().saturating_sub(1),
            KeyCode::Enter => {
                let section = *cursor;
                let prev = state.view;
                state.history.push(prev);
                state.view = HelpView::Topics { section, cursor: 0 };
            }
            _ => {}
        },
        HelpView::Topics {
            section,
            ref mut cursor,
        } => match key.code {
            KeyCode::Esc => {
                if !state.back() {
                    app.mode = AppMode::Browse;
                }
            }
            KeyCode::Backspace => {
                let _ = state.back();
            }
            KeyCode::Up => *cursor = cursor.saturating_sub(1),
            KeyCode::Down => {
                let max = state.system.sections[section]
                    .topics
                    .len()
                    .saturating_sub(1);
                *cursor = (*cursor + 1).min(max);
            }
            KeyCode::Home => *cursor = 0,
            KeyCode::End => {
                *cursor = state.system.sections[section]
                    .topics
                    .len()
                    .saturating_sub(1)
            }
            KeyCode::Enter => {
                let topic = state.system.sections[section].topics[*cursor];
                let prev = state.view;
                state.history.push(prev);
                state.view = HelpView::Page {
                    topic,
                    scroll: 0,
                    selected_link: 0,
                };
            }
            _ => {}
        },
        HelpView::Page {
            topic,
            ref mut scroll,
            ref mut selected_link,
        } => match key.code {
            KeyCode::Esc => {
                if !state.back() {
                    app.mode = AppMode::Browse;
                }
            }
            KeyCode::Backspace => {
                let _ = state.back();
            }
            KeyCode::Up => *scroll = scroll.saturating_sub(1),
            KeyCode::Down => *scroll = scroll.saturating_add(1),
            KeyCode::PageUp => *scroll = scroll.saturating_sub(12),
            KeyCode::PageDown => *scroll = scroll.saturating_add(12),
            KeyCode::Home => *scroll = 0,
            KeyCode::Tab => {
                let count = state.system.topics[topic].link_count();
                if count > 0 {
                    *selected_link = (*selected_link + 1) % count;
                }
            }
            KeyCode::BackTab => {
                let count = state.system.topics[topic].link_count();
                if count > 0 {
                    *selected_link = if *selected_link == 0 {
                        count - 1
                    } else {
                        *selected_link - 1
                    };
                }
            }
            KeyCode::Enter => {
                let target = state.system.topics[topic]
                    .selected_link_target(*selected_link)
                    .map(str::to_string);
                if let Some(target) = target {
                    if !state.open_topic_by_name(&target) {
                        app.set_status(format!("Unknown help topic: {}", target));
                    }
                }
            }
            _ => {}
        },
    }

    Ok(false)
}
