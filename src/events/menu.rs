use super::{confirm_quit, launch_editor, open_wildcard_dialog, start_mkdir, start_rename};
use crate::app::{
    App, AppMode, AssocEditorState, ConfigState, InputAction, InputDialog, MENU_DATA, MENU_HEADERS,
    MenuAction, PluginsState, StoreInstallPaletteState,
};
use crate::compare::{build_compare_panel_state, load_compare_buffer};
use crate::config::SortMode;
use anyhow::{Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent};

fn materialize_compare_path(
    app: &mut App,
    entry: crate::panel::Entry,
    remote_profile: Option<crate::remote::RemoteProfile>,
    side: &str,
) -> Result<(std::path::PathBuf, bool)> {
    if let Some(profile) = remote_profile {
        let downloaded = app
            .run_with_busy(&format!("Remote: downloading {side} file..."), |_| {
                crate::remote::download_to_temp(&profile, &entry.path.to_string_lossy(), false)
            })?;
        Ok((downloaded, true))
    } else {
        Ok((entry.path, false))
    }
}

fn cleanup_compare_temp(path: &std::path::Path, is_temp: bool) {
    if is_temp {
        let _ = std::fs::remove_file(path);
    }
}

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
        KeyCode::Char(c) if open => {
            if let Some(pos) = menu_item_shortcut(MENU_DATA[bar_pos], item_pos, c) {
                let action = MENU_DATA[bar_pos][pos];
                app.mode = AppMode::Browse;
                return execute_menu_action(app, action);
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
            let action = MENU_DATA[bar_pos][item_pos];
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
        .position(|a| *a != MenuAction::Separator)
        .unwrap_or(0)
}

fn next_selectable(items: &[crate::app::MenuEntry], current: usize) -> usize {
    let n = items.len();
    let mut pos = (current + 1) % n;
    let start = pos;
    loop {
        if items[pos] != MenuAction::Separator {
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
        if items[pos] != MenuAction::Separator {
            break;
        }
        pos = if pos == 0 { n - 1 } else { pos - 1 };
        if pos == start {
            break;
        }
    }
    pos
}

fn menu_item_shortcut(items: &[crate::app::MenuEntry], current: usize, ch: char) -> Option<usize> {
    let ch = ch.to_ascii_lowercase();
    let n = items.len();
    if n == 0 {
        return None;
    }
    let labels = items
        .iter()
        .map(|action| {
            if *action == MenuAction::Separator {
                String::new()
            } else {
                crate::app::palette_label_for_action(*action).to_string()
            }
        })
        .collect::<Vec<_>>();
    let mnemonics = mnemonics_for_labels(&labels);

    (1..=n).map(|offset| (current + offset) % n).find(|&idx| {
        let action = items[idx];
        action != MenuAction::Separator && mnemonics.get(idx).copied().flatten() == Some(ch)
    })
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

pub(super) fn execute_menu_action(app: &mut App, action: MenuAction) -> Result<bool> {
    if action != MenuAction::Separator {
        app.last_menu_action = Some(action);
    }
    match action {
        MenuAction::OpenMenu => {
            let mut ms = crate::app::MenuState::new();
            ms.open = false;
            app.mode = AppMode::Menu(ms);
        }
        MenuAction::OpenCommandPalette => {
            let lua_apps = crate::lua_apps::list_installed_apps().unwrap_or_default();
            app.mode = AppMode::CommandPalette(crate::app::CommandPaletteState {
                recent: app.palette_recent.clone(),
                lua_apps,
                ..Default::default()
            });
        }
        MenuAction::OpenActionPalette => {
            let cwd = app.active_panel().path.clone();
            let state = crate::app::ActionPaletteState::load(cwd);
            if state.actions.is_empty() {
                app.notify("No plugin action available");
            } else {
                app.mode = AppMode::ActionPalette(state);
            }
        }
        MenuAction::SwitchPanel => {
            if app.quick_preview.is_some() {
                app.quick_preview_active = true;
            } else if app.file_preview_info {
                app.file_id_active = true;
                app.file_id_scroll = 0;
            } else {
                app.switch_panel();
            }
        }
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
            app.set_status("Panels swapped");
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
            app.set_status("Reloaded");
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
        MenuAction::TreeView => {
            app.open_tree_view();
        }
        MenuAction::EnterArchivePlugin => {
            let Some(entry) = app.active_panel().current_entry().cloned() else {
                app.notify("No selected entry");
                return Ok(false);
            };

            if entry.name == ".." || entry.name == "[disconnect]" {
                app.notify("Select a file or directory");
                return Ok(false);
            }

            if app.active_panel().is_remote_view() {
                app.notify("Archive plugin entry is only available on local files/directories");
                return Ok(false);
            }

            if !crate::archive::supports_archive_navigation(&entry.path) {
                app.notify("Selected entry is not handled by an archive plugin");
                return Ok(false);
            }

            if let Err(e) = app.enter_archive(entry.path.clone()) {
                app.notify(format!("Cannot enter archive: {}", e));
            }
        }
        MenuAction::ComparePanelFiles => {
            let Some(left_entry) = app.left.current_entry().cloned() else {
                app.notify("No selected file in left panel");
                return Ok(false);
            };
            let Some(right_entry) = app.right.current_entry().cloned() else {
                app.notify("No selected file in right panel");
                return Ok(false);
            };

            if matches!(left_entry.name.as_str(), ".." | "[disconnect]")
                || matches!(right_entry.name.as_str(), ".." | "[disconnect]")
            {
                app.notify("Select one file in each panel");
                return Ok(false);
            }
            if left_entry.is_dir || right_entry.is_dir {
                app.notify("Compare requires files in both panels");
                return Ok(false);
            }

            let left_label = app.left.display_path();
            let right_label = app.right.display_path();
            let left_remote = app.left.remote_profile();
            let right_remote = app.right.remote_profile();

            let comparison = (|| -> Result<String> {
                let (left_path, left_temp) =
                    materialize_compare_path(app, left_entry.clone(), left_remote, "left")?;
                let (right_path, right_temp) =
                    materialize_compare_path(app, right_entry.clone(), right_remote, "right")?;

                let result = app.run_with_busy("Comparing files...", |_| {
                    let output = std::process::Command::new("diff")
                        .args([
                            "-u",
                            "--color",
                            "--label",
                            left_label.as_str(),
                            "--label",
                            right_label.as_str(),
                        ])
                        .arg(&left_path)
                        .arg(&right_path)
                        .output()
                        .map_err(|err| anyhow!(err))?;

                    let text = if output.status.success() {
                        format!(
                            "Files are identical\n\nleft: {}\nright: {}\n",
                            left_label, right_label
                        )
                    } else if output.status.code() == Some(1) {
                        String::from_utf8_lossy(&output.stdout).to_string()
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        return Err(anyhow!("diff failed: {}", stderr.trim()));
                    };

                    Ok(text)
                });

                cleanup_compare_temp(&left_path, left_temp);
                cleanup_compare_temp(&right_path, right_temp);
                result
            })();

            match comparison {
                Ok(text) => {
                    let path = std::path::Path::new("Left/Right Compare");
                    app.mode = AppMode::Viewer(crate::viewer::Viewer::placeholder(
                        path,
                        &text,
                        app.config.viewer.word_wrap,
                    ));
                }
                Err(e) => app.notify(format!("Cannot compare files: {}", e)),
            }
        }
        MenuAction::ComparePanelInternal => {
            let Some(left_entry) = app.left.current_entry().cloned() else {
                app.notify("No selected file in left panel");
                return Ok(false);
            };
            let Some(right_entry) = app.right.current_entry().cloned() else {
                app.notify("No selected file in right panel");
                return Ok(false);
            };

            if matches!(left_entry.name.as_str(), ".." | "[disconnect]")
                || matches!(right_entry.name.as_str(), ".." | "[disconnect]")
            {
                app.notify("Select one file in each panel");
                return Ok(false);
            }
            if left_entry.is_dir || right_entry.is_dir {
                app.notify("Compare panel requires files in both panels");
                return Ok(false);
            }

            let left_label = app.left.display_path();
            let right_label = app.right.display_path();
            let left_remote = app.left.remote_profile();
            let right_remote = app.right.remote_profile();

            let comparison = (|| -> Result<crate::app::ComparePanelState> {
                let (left_path, left_temp) =
                    materialize_compare_path(app, left_entry.clone(), left_remote, "left")?;
                let (right_path, right_temp) =
                    materialize_compare_path(app, right_entry.clone(), right_remote, "right")?;

                let result = app.run_with_busy("Building compare panel...", |_| {
                    let left_buffer = load_compare_buffer(&left_path)?;
                    let right_buffer = load_compare_buffer(&right_path)?;
                    Ok(build_compare_panel_state(
                        left_label.clone(),
                        right_label.clone(),
                        left_buffer,
                        right_buffer,
                    ))
                });

                cleanup_compare_temp(&left_path, left_temp);
                cleanup_compare_temp(&right_path, right_temp);
                result
            })();

            match comparison {
                Ok(state) => app.mode = AppMode::ComparePanel(state),
                Err(e) => app.notify(format!("Cannot compare files: {}", e)),
            }
        }
        MenuAction::InstallPluginFromStore => {
            let index_path = crate::plugins::store_index_path();
            match StoreInstallPaletteState::load(index_path.clone()) {
                Ok(state) => app.mode = AppMode::StoreInstallPalette(state),
                Err(e) => app.notify(format!(
                    "Cannot load plugin store index {}: {}",
                    index_path.display(),
                    e
                )),
            }
        }
        MenuAction::RemoteConnect => {
            app.open_remote_connect();
        }
        MenuAction::FileIdPreview => {
            app.open_file_id_view();
        }
        MenuAction::DirBookmarks => {
            app.open_dir_bookmarks();
        }
        MenuAction::ToggleFBar => {
            app.config.show_fkey_bar = !app.config.show_fkey_bar;
            if let Err(e) = app.save_config() {
                app.set_status(format!("Save error: {}", e));
            }
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
            Ok(_) => app.set_status("Config saved"),
            Err(e) => app.set_status(format!("Save error: {}", e)),
        },
        MenuAction::Help => {
            app.open_help();
        }
        MenuAction::About => {
            app.mode = AppMode::About(crate::about::AboutState::new());
        }
        MenuAction::SystemInfo => {
            let text = crate::system_info::render_system_info(app);
            let path = std::path::Path::new("KKC information.md");
            app.mode = AppMode::Viewer(crate::viewer::Viewer::placeholder(
                path,
                &text,
                app.config.viewer.word_wrap,
            ));
        }
        MenuAction::NewTab => {
            app.new_active_tab();
        }
        MenuAction::CloseTab => {
            app.close_active_tab();
        }
        MenuAction::NextTab => {
            app.next_active_tab();
        }
        MenuAction::OpenTerminal => {
            app.mode = AppMode::Terminal;
        }
        MenuAction::CaptureGif => {
            app.capture_gif = true;
        }
        MenuAction::MatrixScreensaver => {
            app.mode = AppMode::MatrixScreensaver(crate::app::MatrixScreensaverState::new());
        }
        MenuAction::OpenInOs => {
            if let Some(entry) = app.active_panel().current_entry().cloned() {
                if let Err(e) = open::that(&entry.path) {
                    app.notify(format!("Cannot open file: {}", e));
                }
            }
        }
        MenuAction::OpenFolderInOs => {
            let cwd = app.active_panel().path.clone();
            if let Err(e) = open::that(&cwd) {
                app.notify(format!("Cannot open folder: {}", e));
            }
        }
        MenuAction::QuickPreview => {
            if let Err(e) = app.toggle_quick_preview() {
                app.notify(format!("Cannot open preview: {}", e));
            }
        }
        MenuAction::DebugLog => {
            app.config.debug_log = !app.config.debug_log;
            crate::viewer::set_debug_log_enabled(app.config.debug_log);
            match app.save_config() {
                Ok(_) if app.config.debug_log => {
                    let log_path = crate::viewer::debug_log_path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "?".into());
                    app.set_status(format!("Debug log enabled: {}", log_path));
                    crate::viewer::debug_log("debug_log command: enabled and saved");
                }
                Ok(_) => app.set_status("Debug log disabled"),
                Err(e) => app.set_status(format!("Save error: {}", e)),
            }
        }
        MenuAction::Separator => {}
        MenuAction::DownloadCloudFile => {
            app.cmd_download_cloud_files();
        }
    }
    Ok(false)
}
