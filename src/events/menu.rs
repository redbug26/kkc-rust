use super::{confirm_quit, launch_editor, open_wildcard_dialog, start_mkdir, start_rename};
use crate::app::{
    App, AppMode, AssocEditorState, ConfigState, InputAction, InputDialog, MENU_DATA, MENU_HEADERS,
    MenuAction, PluginsState,
};
use crate::config::SortMode;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_menu(app: &mut App, key: KeyEvent) -> Result<bool> {
    let (bar_pos, open, item_pos) = {
        let AppMode::Menu(ref s) = app.mode else {
            return Ok(false);
        };
        (s.bar_pos, s.open, s.item_pos)
    };

    match key.code {
        KeyCode::Esc => {
            app.mode = AppMode::Browse;
        }
        KeyCode::Left => {
            let new_pos = if bar_pos == 0 {
                MENU_HEADERS.len() - 1
            } else {
                bar_pos - 1
            };
            if let AppMode::Menu(ref mut s) = app.mode {
                s.bar_pos = new_pos;
                s.item_pos = first_selectable(MENU_DATA[new_pos]);
            }
        }
        KeyCode::Right => {
            let new_pos = if bar_pos + 1 >= MENU_HEADERS.len() {
                0
            } else {
                bar_pos + 1
            };
            if let AppMode::Menu(ref mut s) = app.mode {
                s.bar_pos = new_pos;
                s.item_pos = first_selectable(MENU_DATA[new_pos]);
            }
        }
        KeyCode::Char(c) => {
            let c_lower = c.to_ascii_lowercase();
            if let Some(pos) = MENU_HEADERS
                .iter()
                .position(|h| h.chars().next().map(|f| f.to_ascii_lowercase()) == Some(c_lower))
                && let AppMode::Menu(ref mut s) = app.mode
            {
                s.bar_pos = pos;
                s.open = true;
                s.item_pos = first_selectable(MENU_DATA[pos]);
            }
        }
        KeyCode::Down if !open => {
            if let AppMode::Menu(ref mut s) = app.mode {
                s.open = true;
                s.item_pos = first_selectable(MENU_DATA[bar_pos]);
            }
        }
        KeyCode::Enter if !open => {
            if let AppMode::Menu(ref mut s) = app.mode {
                s.open = true;
                s.item_pos = first_selectable(MENU_DATA[bar_pos]);
            }
        }
        KeyCode::Up if open => {
            let new_pos = prev_selectable(MENU_DATA[bar_pos], item_pos);
            if let AppMode::Menu(ref mut s) = app.mode {
                s.item_pos = new_pos;
            }
        }
        KeyCode::Down if open => {
            let new_pos = next_selectable(MENU_DATA[bar_pos], item_pos);
            if let AppMode::Menu(ref mut s) = app.mode {
                s.item_pos = new_pos;
            }
        }
        KeyCode::Enter if open => {
            let action = MENU_DATA[bar_pos][item_pos].2;
            app.mode = AppMode::Browse;
            return execute_menu_action(app, action);
        }
        _ => {}
    }
    Ok(false)
}

fn first_selectable(items: &[crate::app::MenuEntry]) -> usize {
    items
        .iter()
        .position(|(_, _, a)| *a != MenuAction::Separator)
        .unwrap_or(0)
}

fn next_selectable(items: &[crate::app::MenuEntry], current: usize) -> usize {
    let n = items.len();
    let mut pos = (current + 1) % n;
    let start = pos;
    loop {
        if items[pos].2 != MenuAction::Separator {
            break;
        }
        pos = (pos + 1) % n;
        if pos == start {
            break;
        }
    }
    pos
}

fn prev_selectable(items: &[crate::app::MenuEntry], current: usize) -> usize {
    let n = items.len();
    let mut pos = if current == 0 { n - 1 } else { current - 1 };
    let start = pos;
    loop {
        if items[pos].2 != MenuAction::Separator {
            break;
        }
        pos = if pos == 0 { n - 1 } else { pos - 1 };
        if pos == start {
            break;
        }
    }
    pos
}

fn execute_menu_action(app: &mut App, action: MenuAction) -> Result<bool> {
    match action {
        MenuAction::ViewFile => {
            app.open_viewer();
        }
        MenuAction::EditFile => {
            launch_editor(app)?;
        }
        MenuAction::CopyFile => {
            app.open_copy_dialog();
        }
        MenuAction::MoveFile => {
            app.cmd_move()?;
        }
        MenuAction::MkDir => {
            start_mkdir(app);
        }
        MenuAction::RenameFile => {
            start_rename(app);
        }
        MenuAction::DeleteFile => {
            app.cmd_delete();
        }
        MenuAction::Quit => {
            return confirm_quit(app);
        }
        MenuAction::SwapPanels => {
            app.swap_panels();
            app.status.text = "Panels swapped".into();
        }
        MenuAction::SortName => {
            app.set_sort(SortMode::Name);
        }
        MenuAction::SortExtension => {
            app.set_sort(SortMode::Extension);
        }
        MenuAction::SortDate => {
            app.set_sort(SortMode::Date);
        }
        MenuAction::SortSize => {
            app.set_sort(SortMode::Size);
        }
        MenuAction::SortUnsorted => {
            app.set_sort(SortMode::Unsorted);
        }
        MenuAction::ToggleHidden => {
            let p = app.active_panel_mut();
            p.show_hidden = !p.show_hidden;
            let _ = p.reload();
        }
        MenuAction::Reload => {
            app.reload_panels();
            app.status.text = "Reloaded".into();
        }
        MenuAction::GoToPath => {
            let current = app.active_panel().display_path();
            let cursor = current.len();
            app.mode = AppMode::Input(InputDialog {
                title: "Go to Path".into(),
                prompt: "Path:".into(),
                value: current,
                cursor,
                action: InputAction::GoToPath,
            });
        }
        MenuAction::SelectPattern => {
            app.mode = open_wildcard_dialog("Select pattern:", true);
        }
        MenuAction::DeselectPattern => {
            app.mode = open_wildcard_dialog("Deselect pattern:", false);
        }
        MenuAction::InvertSelection => {
            app.active_panel_mut().invert_selection();
        }
        MenuAction::SearchFiles => {
            app.open_search();
        }
        MenuAction::DirBookmarks => {
            app.open_dir_bookmarks();
        }
        MenuAction::ToggleFBar => {
            app.config.show_fkey_bar = !app.config.show_fkey_bar;
        }
        MenuAction::Setup => {
            let cs = ConfigState::from_config(&app.config);
            app.mode = AppMode::Config(cs);
        }
        MenuAction::Plugins => {
            app.mode = AppMode::Plugins(PluginsState::load());
        }
        MenuAction::Associations => {
            app.mode = AppMode::AssocEditor(AssocEditorState::from_config(&app.config));
        }
        MenuAction::SaveConfig => match app.save_config() {
            Ok(_) => app.status.text = "Config saved".into(),
            Err(e) => app.status.text = format!("Save error: {}", e),
        },
        MenuAction::Help => {
            app.open_help();
        }
        MenuAction::About => {
            app.status.text = format!(
                "KKC {} \u{2014} Rust reimplementation of KKC-DOS",
                env!("CARGO_PKG_VERSION")
            );
        }
        MenuAction::Separator => {}
    }
    Ok(false)
}
