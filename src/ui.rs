mod assoc;
mod bookmarks;
mod command_palette;
mod compare;
mod config;
mod confirm;
mod copy;
mod help;
mod panel;
mod panel_text_editor;
mod plugins;
mod remote;
mod search;
mod terminal;
mod tree_view;
mod viewer;
pub(crate) use self::assoc::assoc_editor_shortcuts;
use self::assoc::{render_action_palette, render_assoc_editor, render_opener};
pub(crate) use self::bookmarks::dir_bookmarks_shortcuts;
use self::bookmarks::{
    render_audio_player_palette, render_dir_bookmarks, render_quicksearch_palette,
    render_store_install_palette, render_viewer_plugin_palette,
};
pub(crate) use self::bookmarks::{store_detect_shortcuts, store_install_shortcuts};
use self::config::render_config;
pub(crate) use self::confirm::assoc_input_shortcuts;
use self::confirm::{render_assoc_input, render_confirm, render_input};
use self::copy::{render_copy_dialog, render_copy_progress};
pub(crate) use self::help::help_shortcuts;
use self::help::render_help;
pub(crate) use self::remote::{
    remote_add_menu_shortcuts, remote_connect_shortcuts, remote_connecting_shortcuts,
    remote_edit_shortcuts,
};
use self::remote::{
    render_remote_add_menu, render_remote_connect, render_remote_connecting, render_remote_edit,
};
use self::search::render_search;
pub(crate) use self::search::search_panel_shortcuts;
use self::terminal::render_terminal;
use self::tree_view::render_tree_view;
pub(crate) use self::viewer::viewer_area;
pub(crate) use self::viewer::viewer_footer_shortcuts;
pub use self::viewer::{kitty_image_area, kitty_image_area_quick_preview};
use self::viewer::{
    menu_dropdown_line, mnemonics_for_labels, render_viewer, render_viewer_goto, render_viewer_menu,
};

pub(crate) use self::command_palette::command_palette_shortcuts;
use self::command_palette::render_command_palette;
use self::compare::render_compare_panel;
use self::panel::{render_center_buttons, render_panel_or_file_id};
pub(crate) use self::plugins::plugins_shortcuts;
use self::plugins::render_plugins;
use crate::app::{
    ActionPaletteState, ActivePanel, App, AppMode, AssocEditorState, AudioPlayerPaletteState,
    BookmarkListItem, ComparePanelState, ConfigState, ConfirmAction, ConfirmButton, ConfirmDialog,
    InputDialog, MENU_DATA, MENU_HEADERS, MenuAction, MenuState, OpenerState, PluginsState,
    RemoteConnectState, RemoteConnectingState, RemoteEditKind, RemoteEditState, SearchState,
    StoreInstallPaletteState, ViewerGotoState, ViewerMenuKind, ViewerMenuState,
    ViewerPluginPaletteState,
};
use crate::config::SortMode;
use crate::copy::{CopyDialogState, CopyProgressState};
use crate::file_ops::format_size;
use crate::file_types::FileCategory;
use crate::help::HelpView;
use crate::idf::{IdfKind, probe_path};
use crate::panel::Entry;
use crate::remote::RemoteSource;
use crate::tree_mode::TreeViewState;
use crate::viewer::{ViewMode, Viewer};
use chrono::{DateTime, Local};
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Tabs, Wrap,
    },
};
use unicode_width::UnicodeWidthStr;

// ---------------------------------------------------------------------------
// Colour accessors — delegate to the runtime theme (set via crate::theme)
// ---------------------------------------------------------------------------
use crate::theme::theme;

#[inline] fn clr_app_bg() -> Color { theme().app.background }
#[inline] fn clr_panel_bg() -> Color { theme().panel.background }
#[inline] fn clr_panel_border() -> Color { theme().panel.border }
#[inline] fn clr_panel_border_dim() -> Color { theme().panel.border_dim }
#[inline] fn clr_panel_title() -> Color { theme().panel.title }
#[inline] fn clr_header_bg() -> Color { theme().header.background }
#[inline] fn clr_header_fg() -> Color { theme().header.foreground }
#[inline] fn clr_cursor_bg() -> Color { theme().cursor.background }
#[inline] fn clr_cursor_fg() -> Color { theme().cursor.foreground }
#[inline] fn clr_selected() -> Color { theme().selection.foreground }
#[inline] fn clr_dir() -> Color { theme().files.directory }
#[inline] fn clr_tree() -> Color { theme().panel.tree_connector }
#[inline] fn clr_exec() -> Color { theme().files.executable }
#[inline] fn clr_archive() -> Color { theme().files.archive }
#[inline] fn clr_audio() -> Color { theme().files.audio }
#[inline] fn clr_image() -> Color { theme().files.image }
#[inline] fn clr_video() -> Color { theme().files.video }
#[inline] fn clr_doc() -> Color { theme().files.document }
#[inline] fn clr_source() -> Color { theme().files.source }
#[inline] fn clr_data() -> Color { theme().files.data }
#[inline] fn clr_text() -> Color { theme().files.text }
#[inline] fn clr_unknown() -> Color { theme().files.unknown }
#[inline] fn clr_status_bg() -> Color { theme().status.background }
#[inline] fn clr_status_fg() -> Color { theme().status.foreground }
#[inline] fn clr_fkey_bg() -> Color { theme().fkeys.background }
#[inline] fn clr_fkey_num() -> Color { theme().fkeys.number_foreground }
#[inline] fn clr_fkey_label() -> Color { theme().fkeys.label }
#[inline] fn clr_fkey_num_bg() -> Color { theme().fkeys.number_background }
#[inline] fn clr_button_bg() -> Color { theme().buttons.background }
#[inline] fn clr_button_fg() -> Color { theme().buttons.foreground }

#[inline] fn clr_menu_bar_bg() -> Color { theme().menu.bar_background }
#[inline] fn clr_menu_bar_fg() -> Color { theme().menu.bar_foreground }
#[inline] fn clr_menu_sel_bg() -> Color { theme().menu.selected_background }
#[inline] fn clr_menu_sel_fg() -> Color { theme().menu.selected_foreground }
#[inline] fn clr_menu_dd_bg() -> Color { theme().menu.dropdown_background }
#[inline] fn clr_menu_dd_fg() -> Color { theme().menu.dropdown_foreground }
#[inline] fn clr_menu_dd_sep() -> Color { theme().menu.dropdown_separator }
#[inline] fn clr_menu_border() -> Color { theme().menu.border }
#[inline] fn clr_menu_hotkey() -> Color { theme().menu.hotkey }

// Quick-palette (VSCode-style)
#[inline] fn clr_qs_bg() -> Color { theme().palette.background }
#[inline] fn clr_qs_border() -> Color { theme().palette.border }
#[inline] fn clr_qs_input_bg() -> Color { theme().palette.input_background }
#[inline] fn clr_qs_input_fg() -> Color { theme().palette.input_foreground }
#[inline] fn clr_qs_sep() -> Color { theme().palette.separator }
#[inline] fn clr_qs_list_fg() -> Color { theme().palette.list_foreground }
#[inline] fn clr_qs_sel_bg() -> Color { theme().palette.selected_background }
#[inline] fn clr_qs_sel_fg() -> Color { theme().palette.selected_foreground }
#[inline] fn clr_qs_match_hi() -> Color { theme().palette.match_highlight }
#[inline] fn clr_qs_match_hi_sel() -> Color { theme().palette.match_highlight_selected }
#[inline] fn clr_qs_no_match() -> Color { theme().palette.no_match }
#[inline] fn clr_qs_dir_fg() -> Color { theme().palette.directory_foreground }

#[inline] fn clr_dialog_bg() -> Color { theme().dialog.background }
#[inline] fn clr_dialog_fg() -> Color { theme().dialog.foreground }
#[inline] fn clr_dialog_border() -> Color { theme().dialog.border }
#[inline] fn clr_dialog_title() -> Color { theme().dialog.title }
#[inline] fn clr_dialog_selected_bg() -> Color { theme().dialog.selected_background }
#[inline] fn clr_dialog_selected_fg() -> Color { theme().dialog.selected_foreground }
#[inline] fn clr_dialog_hint() -> Color { theme().dialog.hint }

// ---------------------------------------------------------------------------
// Entry style by category
// ---------------------------------------------------------------------------

fn entry_fg(entry: &Entry, color_by_type: bool) -> Color {
    if !color_by_type {
        return clr_text();
    }
    if entry.selected {
        return clr_selected();
    }
    match entry.category {
        FileCategory::Directory => clr_dir(),
        FileCategory::Executable => clr_exec(),
        FileCategory::Archive => clr_archive(),
        FileCategory::Audio => clr_audio(),
        FileCategory::Image => clr_image(),
        FileCategory::Video => clr_video(),
        FileCategory::Document => clr_doc(),
        FileCategory::Source => clr_source(),
        FileCategory::Data => clr_data(),
        FileCategory::Text => clr_text(),
        FileCategory::Unknown => clr_unknown(),
    }
}

// ---------------------------------------------------------------------------
// Top-level render
// ---------------------------------------------------------------------------

pub fn render(f: &mut Frame, app: &App) {
    f.render_widget(
        Block::default().style(Style::default().bg(clr_app_bg())),
        f.area(),
    );

    match &app.mode {
        AppMode::MatrixScreensaver(state) => {
            crate::matrix_screensaver::render(f, state, f.area());
            return;
        }
        AppMode::Viewer(v) => {
            render_viewer(
                f,
                v,
                false,
                None,
                f.area(),
                true,
                true,
                None,
                app.config.viewer.autoplay_delay_secs,
            );
            return;
        }
        AppMode::ViewerSearching(v) => {
            render_viewer(
                f,
                v,
                true,
                None,
                f.area(),
                true,
                true,
                None,
                app.config.viewer.autoplay_delay_secs,
            );
            return;
        }
        AppMode::ViewerGotoLine(v, input) => {
            render_viewer(
                f,
                v,
                false,
                Some(input),
                f.area(),
                true,
                true,
                None,
                app.config.viewer.autoplay_delay_secs,
            );
            return;
        }
        AppMode::ViewerGoto(v, state) => {
            render_viewer(
                f,
                v,
                false,
                None,
                f.area(),
                true,
                true,
                None,
                app.config.viewer.autoplay_delay_secs,
            );
            render_viewer_goto(f, state, f.area());
            return;
        }
        AppMode::ViewerMenu(v, menu) => {
            render_viewer(
                f,
                v,
                false,
                None,
                f.area(),
                true,
                true,
                None,
                app.config.viewer.autoplay_delay_secs,
            );
            render_viewer_menu(f, v, menu, f.area());
            return;
        }
        AppMode::ViewerPluginPalette(v, state) => {
            render_viewer(
                f,
                v,
                false,
                None,
                f.area(),
                true,
                true,
                None,
                app.config.viewer.autoplay_delay_secs,
            );
            render_viewer_plugin_palette(f, state, f.area());
            return;
        }
        AppMode::AudioPlayerPalette(v, state) => {
            render_viewer(
                f,
                v,
                false,
                None,
                f.area(),
                true,
                true,
                None,
                app.config.viewer.autoplay_delay_secs,
            );
            render_audio_player_palette(f, state, f.area());
            return;
        }
        AppMode::Help(state) => {
            render_help(f, state, f.area());
            return;
        }
        _ => {}
    }

    // Compute layout
    let has_fbar = app.config.show_fkey_bar;
    let main_vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if has_fbar {
            vec![
                Constraint::Min(5),
                Constraint::Length(1), // status bar
                Constraint::Length(1), // fkey bar
            ]
        } else {
            vec![Constraint::Min(5), Constraint::Length(1)]
        })
        .split(f.area());

    let panels_area = main_vert[0];
    let status_area = main_vert[1];

    let left_active = app.active == ActivePanel::Left;
    let editor_on_left = matches!(app.panel_text_editor_side(), Some(ActivePanel::Left));
    let editor_on_right = matches!(app.panel_text_editor_side(), Some(ActivePanel::Right));
    let left_panel_editor = if editor_on_left {
        app.panel_text_editor.as_ref()
    } else {
        None
    };
    let right_panel_editor = if editor_on_right {
        app.panel_text_editor.as_ref()
    } else {
        None
    };
    let panel_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(28),
            Constraint::Length(13),
            Constraint::Min(28),
        ])
        .split(panels_area);

    render_panel_or_file_id(
        f,
        app,
        &app.left,
        panel_chunks[0],
        left_active,
        app.config.color_by_type,
        app.config.show_cloud_icons,
        app.config.show_file_icons,
        app.file_preview_info && !left_active,
        if left_active {
            None
        } else {
            app.quick_preview.as_ref()
        },
        app.quick_preview_active && !left_active,
        left_panel_editor,
        app.panel_text_editor_active && editor_on_left && left_active,
        app.left_panel_tab_index(),
        app.left_panel_tab_count(),
    );
    render_center_buttons(f, app, panel_chunks[1]);
    render_panel_or_file_id(
        f,
        app,
        &app.right,
        panel_chunks[2],
        !left_active,
        app.config.color_by_type,
        app.config.show_cloud_icons,
        app.config.show_file_icons,
        app.file_preview_info && left_active,
        if !left_active {
            None
        } else {
            app.quick_preview.as_ref()
        },
        app.quick_preview_active && left_active,
        right_panel_editor,
        app.panel_text_editor_active && editor_on_right && !left_active,
        app.right_panel_tab_index(),
        app.right_panel_tab_count(),
    );
    render_status(f, app, status_area);

    if has_fbar {
        render_fkey_bar(f, app, main_vert[2]);
    }

    // Overlays
    match &app.mode {
        AppMode::Confirm(dlg) => render_confirm(f, dlg, f.area()),
        AppMode::Input(dlg) => render_input(f, dlg, f.area()),
        AppMode::AssocInput(dlg) => render_assoc_input(f, dlg, f.area()),
        AppMode::CopyDialog(state) => render_copy_dialog(f, state, f.area()),
        AppMode::CopyProgress(state) => render_copy_progress(f, state, f.area()),
        AppMode::SearchPanel(s) => render_search(f, s, f.area()),
        AppMode::ComparePanel(s) => render_compare_panel(f, s, f.area()),
        AppMode::TreeView(s) => render_tree_view(f, s, f.area()),
        AppMode::DirBookmarks => render_dir_bookmarks(f, app, f.area()),
        AppMode::Config(cs) => render_config(f, cs, f.area()),
        AppMode::Plugins(s) => render_plugins(f, s, f.area()),
        AppMode::ActionPalette(s) => render_action_palette(f, s, f.area()),
        AppMode::CommandPalette(s) => render_command_palette(f, app, s, f.area()),
        AppMode::StoreInstallPalette(s) => render_store_install_palette(f, s, f.area()),
        AppMode::Opener(s) => {
            let active_panel_area = if left_active {
                panel_chunks[0]
            } else {
                panel_chunks[2]
            };
            render_opener(f, s, f.area(), active_panel_area);
        }
        AppMode::AssocEditor(s) => render_assoc_editor(f, s, f.area()),
        AppMode::RemoteConnect(s) => render_remote_connect(f, s, f.area()),
        AppMode::RemoteEdit(s) => render_remote_edit(f, s, f.area()),
        AppMode::RemoteAddMenu(cursor) => {
            let choices = RemoteEditKind::all();
            render_remote_add_menu(f, &choices, *cursor, f.area())
        }
        AppMode::RemoteConnecting(s) => render_remote_connecting(f, s, f.area()),
        AppMode::Menu(ms) => render_menu(f, app, ms, f.area()),
        AppMode::QuickSearch => {
            render_quicksearch_palette(f, app, f.area());
        }
        AppMode::Terminal => render_terminal(f, app, f.area()),
        AppMode::About(state) => crate::about::render_about(f, state, f.area()),
        _ => {}
    }
}

fn safe_set_cursor_position(f: &mut Frame, x: u16, y: u16) {
    let area = f.area();
    let max_x = area.x + area.width.saturating_sub(1);
    let max_y = area.y + area.height.saturating_sub(1);
    if area.width == 0 || area.height == 0 {
        return;
    }
    if x <= max_x && y <= max_y {
        f.set_cursor_position((x, y));
    }
}

fn clamp_rect(area: Rect, rect: Rect) -> Rect {
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

fn safe_render_widget<W: ratatui::widgets::Widget>(f: &mut Frame, widget: W, rect: Rect) {
    let rect = clamp_rect(f.area(), rect);
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    f.render_widget(widget, rect);
}

fn safe_render_stateful_widget<W, S>(f: &mut Frame, widget: W, rect: Rect, state: &mut S)
where
    W: ratatui::widgets::StatefulWidget<State = S>,
{
    let rect = clamp_rect(f.area(), rect);
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    f.render_stateful_widget(widget, rect, state);
}

// ---------------------------------------------------------------------------
// Menu bar + dropdown (F2)
// ---------------------------------------------------------------------------

fn render_menu(f: &mut Frame, app: &App, state: &MenuState, area: Rect) {
    // ── top bar ────────────────────────────────────────────────────────────
    let bar_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };

    let mut spans = vec![Span::styled(" ", Style::default().bg(clr_menu_bar_bg()))];
    for (i, header) in MENU_HEADERS.iter().enumerate() {
        let selected = i == state.bar_pos;
        let base_style = if selected {
            Style::default()
                .bg(clr_menu_sel_bg())
                .fg(clr_menu_sel_fg())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(clr_menu_bar_bg()).fg(clr_menu_bar_fg())
        };
        let hotkey_style = if selected {
            base_style
        } else {
            Style::default()
                .bg(clr_menu_bar_bg())
                .fg(clr_menu_hotkey())
                .add_modifier(Modifier::BOLD)
        };
        spans.push(Span::styled(" ", base_style));
        let mut chars = header.chars();
        if let Some(first) = chars.next() {
            spans.push(Span::styled(first.to_string(), hotkey_style));
            spans.push(Span::styled(format!("{} ", chars.as_str()), base_style));
        } else {
            spans.push(Span::styled(format!("{} ", header), base_style));
        }
        spans.push(Span::styled("  ", Style::default().bg(clr_menu_bar_bg())));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(clr_menu_bar_bg())),
        bar_area,
    );

    if !state.open {
        return;
    }

    // ── dropdown ───────────────────────────────────────────────────────────
    let items = MENU_DATA[state.bar_pos];

    // Compute dropdown x: 1 (leading) + sum of (" header  ") widths preceding
    let dd_x: u16 = {
        let mut x = 1u16;
        for header in MENU_HEADERS.iter().take(state.bar_pos) {
            x += header.len() as u16 + 4; // " header  "
        }
        x
    };

    // Width: widest label + key hint + padding
    let max_label = items
        .iter()
        .map(|action| UnicodeWidthStr::width(menu_action_label(*action)))
        .max()
        .unwrap_or(6);
    let max_key = items
        .iter()
        .filter_map(|action| menu_action_shortcut(app, *action))
        .map(|k| k.len())
        .max()
        .unwrap_or(0);
    let inner_w = (max_label + max_key + 4).max(18) as u16;
    let dd_width = inner_w + 2; // borders
    let dd_height = items.len() as u16 + 2;

    // Clamp so it doesn't disappear off the right edge
    let dd_x_clamped = dd_x.min(area.width.saturating_sub(dd_width));
    let dd_area = Rect {
        x: area.x + dd_x_clamped,
        y: area.y + 1,
        width: dd_width.min(area.width),
        height: dd_height.min(area.height.saturating_sub(1)),
    };

    f.render_widget(Clear, dd_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(clr_menu_border()))
        .style(Style::default().bg(clr_menu_dd_bg()));
    let inner = block.inner(dd_area);
    f.render_widget(block, dd_area);

    let avail = inner.width as usize;
    let menu_labels = items
        .iter()
        .map(|action| {
            if *action == MenuAction::Separator {
                String::new()
            } else {
                menu_action_label(*action).to_string()
            }
        })
        .collect::<Vec<_>>();
    let menu_mnemonics = mnemonics_for_labels(&menu_labels);
    for (idx, action) in items.iter().enumerate() {
        if idx as u16 >= inner.height {
            break;
        }
        let row = Rect {
            x: inner.x,
            y: inner.y + idx as u16,
            width: inner.width,
            height: 1,
        };

        if *action == MenuAction::Separator {
            let sep = "─".repeat(avail);
            f.render_widget(
                Paragraph::new(sep).style(Style::default().fg(clr_menu_dd_sep()).bg(clr_menu_dd_bg())),
                row,
            );
        } else {
            let style = if idx == state.item_pos {
                Style::default()
                    .bg(clr_menu_sel_bg())
                    .fg(clr_menu_sel_fg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(clr_menu_dd_bg()).fg(clr_menu_dd_fg())
            };
            let label = menu_action_label(*action);
            let key_text = menu_action_shortcut(app, *action).unwrap_or_default();
            let used =
                UnicodeWidthStr::width(label) + UnicodeWidthStr::width(key_text.as_str()) + 2; // leading " " + trailing " "
            let pad = avail.saturating_sub(used);
            let line = menu_dropdown_line(
                label,
                &key_text,
                pad,
                menu_mnemonics.get(idx).copied().flatten(),
                style,
            );
            f.render_widget(Paragraph::new(line).style(style), row);
        }
    }
}

fn menu_action_label(action: MenuAction) -> &'static str {
    crate::app::palette_label_for_action(action)
}

fn menu_action_shortcut(app: &App, action: MenuAction) -> Option<String> {
    crate::app::PALETTE_DATA
        .iter()
        .find(|entry| entry.action == action)
        .and_then(|entry| app.effective_shortcut_for(entry.fn_name, entry.shortcut))
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let status_text = status_line_left_text(app);

    let sort_label = match app.active_panel().sort {
        SortMode::Name => "Name",
        SortMode::Extension => "Ext",
        SortMode::Date => "Date",
        SortMode::Size => "Size",
        SortMode::Unsorted => "---",
    };
    let hidden_label = if app.active_panel().show_hidden {
        "H"
    } else {
        " "
    };
    let right_info = format!(" Sort:{} [{}] ", sort_label, hidden_label);

    let left_w = area.width.saturating_sub(right_info.len() as u16);
    let status_lead = if app.status_copy_icon_visible() {
        "⧉ "
    } else {
        "  "
    };

    let line = Line::from(vec![
        Span::styled(
            format!(
                "{}{:<width$}",
                status_lead,
                status_text,
                width = left_w.saturating_sub(1) as usize
            ),
            Style::default().fg(clr_status_fg()).bg(clr_status_bg()),
        ),
        Span::styled(
            right_info,
            Style::default().fg(clr_button_fg()).bg(clr_status_bg()),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn status_line_left_text(app: &App) -> String {
    let entry_info = if let Some(e) = app.active_panel().current_entry() {
        if e.name == ".." {
            let mode_str = format_mode(e.mode);
            format!("Up directory  {}  dir", mode_str)
        } else if !e.cloud_only
            && let Some(info) = probe_path(&e.path)
        {
            let prefix = match info.kind {
                IdfKind::Module => "MOD",
                IdfKind::Sample => "SMP",
                IdfKind::Archive => "ARC",
                IdfKind::Bitmap => "PIC",
                IdfKind::Animation => "ANI",
                IdfKind::Other => "IDF",
            };
            let detail = info
                .title
                .clone()
                .or_else(|| info.extra.first().cloned())
                .unwrap_or(info.detail);
            format!("{:<3}  {:<24} {}", prefix, info.format, detail)
        } else {
            let kind = if e.is_symlink {
                "symlink"
            } else if e.is_dir {
                "dir"
            } else {
                "file"
            };
            let mode_str = format_mode(e.mode);
            format!("{}  {}  {}", e.name, mode_str, kind)
        }
    } else {
        String::new()
    };

    if app.status.text.is_empty() {
        entry_info
    } else {
        app.status.text.clone()
    }
}

pub(crate) fn status_line_for_copy(app: &App) -> String {
    let sort_label = match app.active_panel().sort {
        SortMode::Name => "Name",
        SortMode::Extension => "Ext",
        SortMode::Date => "Date",
        SortMode::Size => "Size",
        SortMode::Unsorted => "---",
    };
    let hidden_label = if app.active_panel().show_hidden {
        "H"
    } else {
        " "
    };
    format!(
        "{} Sort:{} [{}]",
        status_line_left_text(app),
        sort_label,
        hidden_label
    )
}

pub(crate) struct FooterShortcut {
    pub label: &'static str,
    pub key: KeyCode,
}

pub(crate) struct FkeySlot {
    pub number: u8,
    pub label: String,
}

pub(crate) struct ShortcutBarItem {
    pub key: String,
    pub label: String,
}

#[derive(Clone, Copy)]
pub(crate) struct ShortcutBarStyle {
    pub key_fg: Color,
    pub key_bg: Color,
    pub label_fg: Color,
    pub label_bg: Color,
    pub bar_bg: Color,
    pub sep_fg: Color,
}

fn default_shortcut_bar_style() -> ShortcutBarStyle {
    ShortcutBarStyle {
        key_fg: clr_fkey_num(),
        key_bg: clr_fkey_num_bg(),
        label_fg: clr_fkey_label(),
        label_bg: clr_fkey_bg(),
        bar_bg: clr_fkey_bg(),
        sep_fg: clr_panel_border_dim(),
    }
}

fn secondary_shortcut_bar_style() -> ShortcutBarStyle {
    ShortcutBarStyle {
        key_fg: clr_qs_input_fg(),
        key_bg: clr_qs_sel_bg(),
        label_fg: clr_qs_list_fg(),
        label_bg: clr_qs_bg(),
        bar_bg: clr_qs_bg(),
        sep_fg: clr_qs_sep(),
    }
}

fn shortcut_bar_item_width(item: &ShortcutBarItem) -> usize {
    // Rendered as: "{key}" + " {label} " + " " (separator, always counted to simplify)
    // = key_width + (label_width + 2) + 1 = key_width + label_width + 3
    // The last item has no separator, but the 1-col overshoot is harmless.
    UnicodeWidthStr::width(item.key.as_str()) + UnicodeWidthStr::width(item.label.as_str()) + 3
}

pub(crate) fn shortcut_bar_item_index_at_column(
    items: &[ShortcutBarItem],
    area_x: u16,
    column: u16,
) -> Option<usize> {
    if column < area_x {
        return None;
    }
    let rel_col = column.saturating_sub(area_x) as usize;
    let mut x = 0usize;
    for (idx, item) in items.iter().enumerate() {
        let width = shortcut_bar_item_width(item);
        if rel_col >= x && rel_col < x + width {
            return Some(idx);
        }
        x += width;
    }
    None
}

pub(crate) fn render_shortcut_bar(
    f: &mut Frame,
    area: Rect,
    items: &[ShortcutBarItem],
    style: ShortcutBarStyle,
) {
    let mut spans = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        spans.push(Span::styled(
                item.key.to_string(),
            Style::default().fg(style.key_fg).bg(style.key_bg),
        ));
        spans.push(Span::styled(
            format!(" {} ", item.label),
            Style::default().fg(style.label_fg).bg(style.label_bg),
        ));
        if idx + 1 < items.len() {
            spans.push(Span::styled(
                " ",
                Style::default().fg(style.sep_fg).bg(style.bar_bg),
            ));
        }
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(style.bar_bg)),
        area,
    );
}

pub(crate) fn footer_shortcut_items(shortcuts: &[FooterShortcut]) -> Vec<ShortcutBarItem> {
    shortcuts
        .iter()
        .map(|shortcut| {
            if let Some((key, label)) = shortcut.label.split_once(':') {
                ShortcutBarItem {
                    key: key.to_string(),
                    label: label.to_string(),
                }
            } else {
                ShortcutBarItem {
                    key: shortcut.label.to_string(),
                    label: String::new(),
                }
            }
        })
        .collect()
}

pub(crate) fn footer_shortcut_key_at_column(
    shortcuts: &[FooterShortcut],
    area_x: u16,
    column: u16,
) -> Option<KeyCode> {
    let items = footer_shortcut_items(shortcuts);
    let idx = shortcut_bar_item_index_at_column(&items, area_x, column)?;
    shortcuts.get(idx).map(|shortcut| shortcut.key)
}

// ---------------------------------------------------------------------------
// Function key bar
// ---------------------------------------------------------------------------

pub(crate) fn fkey_slots(app: &App) -> Vec<FkeySlot> {
    let mut labels: Vec<FkeySlot> = (1..=10)
        .map(|n| FkeySlot {
            number: n as u8,
            label: String::new(),
        })
        .collect();

    for n in 1..=10 {
        let shortcut = format!("F{}", n);
        if let Some(entry) = crate::app::PALETTE_DATA.iter().find(|entry| {
            app.effective_shortcut_for(entry.fn_name, entry.shortcut)
                .as_deref()
                == Some(shortcut.as_str())
        }) {
            labels[n - 1].label = entry.shortname.to_string();
        }
    }

    labels
}

pub(crate) fn fkey_items(app: &App) -> Vec<ShortcutBarItem> {
    fkey_slots(app)
        .into_iter()
        .map(|slot| ShortcutBarItem {
            key: format!("F{}", slot.number),
            label: slot.label,
        })
        .collect()
}

pub(crate) fn fkey_number_at_column(app: &App, area_x: u16, column: u16) -> Option<u8> {
    let slots = fkey_slots(app);
    // Use the same key strings as fkey_items / render_fkey_bar ("F1".."F10")
    let items = fkey_items(app);
    let idx = shortcut_bar_item_index_at_column(&items, area_x, column)?;
    slots.get(idx).map(|slot| slot.number)
}

fn render_fkey_bar(f: &mut Frame, app: &App, area: Rect) {
    render_shortcut_bar(f, area, &fkey_items(app), default_shortcut_bar_style());
}

// ---------------------------------------------------------------------------
// Viewer
// ---------------------------------------------------------------------------

pub fn truncate_str(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    use unicode_width::UnicodeWidthChar;
    let mut width = 0usize;
    let mut result = String::new();
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(1);
        if width + cw > max {
            // pad
            while width < max {
                result.push(' ');
                width += 1;
            }
            break;
        }
        result.push(ch);
        width += cw;
    }
    // pad to max
    while width < max {
        result.push(' ');
        width += 1;
    }
    result
}

fn push_display_prefix(out: &mut String, s: &str, max_width: usize) -> usize {
    use unicode_width::UnicodeWidthChar;
    let mut width = 0usize;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(1);
        if width + cw > max_width {
            break;
        }
        out.push(ch);
        width += cw;
    }
    width
}

fn take_display_prefix(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    push_display_prefix(&mut out, s, max_width);
    out
}

fn take_display_suffix(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut width = 0usize;
    let mut chars = Vec::new();
    for ch in s.chars().rev() {
        let cw = ch.width().unwrap_or(1);
        if width + cw > max_width {
            break;
        }
        chars.push(ch);
        width += cw;
    }
    chars.into_iter().rev().collect()
}

fn pad_display_width(mut s: String, width: usize) -> String {
    while UnicodeWidthStr::width(s.as_str()) < width {
        s.push(' ');
    }
    s
}

fn truncate_search_file_name(name: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(name) <= width {
        return pad_display_width(name.to_string(), width);
    }
    if width <= 3 {
        return ".".repeat(width);
    }

    let remaining = width - 3;
    let suffix_w = remaining.div_ceil(2);
    let prefix_w = remaining.saturating_sub(suffix_w);
    let prefix = take_display_prefix(name, prefix_w);
    let suffix = take_display_suffix(name, suffix_w);
    pad_display_width(format!("{prefix}...{suffix}"), width)
}

fn format_dos_number(value: u64) -> String {
    let raw = value.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (idx, ch) in raw.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_panel_size(value: u64, width: usize) -> String {
    let dos = format_dos_number(value);
    if dos.chars().count() <= width {
        return dos;
    }
    format_compact_size(value)
}

fn format_compact_size(value: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = value as f64;
    let mut unit = 0usize;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", value, UNITS[unit])
    } else if size >= 100.0 {
        format!("{:.0} {}", size, UNITS[unit])
    } else if size >= 10.0 {
        format!("{:.1} {}", size, UNITS[unit])
    } else {
        format!("{:.2} {}", size, UNITS[unit])
    }
}

fn truncate_path(p: &str, max: usize) -> String {
    let sep = '…';

    if max == 0 {
        return String::new();
    }

    if p.chars().count() <= max {
        return p.to_string();
    }

    let components: Vec<&str> = p.split('/').collect();

    if components.len() < 3 {
        let keep = max.saturating_sub(1);
        let trimmed: String = p
            .chars()
            .rev()
            .take(keep)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        return format!("{}{}", sep, trimmed);
    }

    let leaf = components[components.len() - 1];

    // On garde toujours le début comme l’original :
    // d’abord les 2 premiers composants si possible, sinon le premier.
    for prefix_len in (1..=2).rev() {
        if components.len() <= prefix_len {
            continue;
        }

        let prefix = components[..prefix_len].join("/");

        // Puis on maximise le nombre de composants de fin.
        for suffix_len in (1..=(components.len() - prefix_len - 1)).rev() {
            let suffix = components[components.len() - suffix_len..].join("/");
            let candidate = format!("{}/{}/{}", prefix, sep, suffix);

            if candidate.chars().count() <= max && candidate.chars().count() < p.chars().count() {
                return candidate;
            }
        }
    }

    // Fichier seul
    if leaf.chars().count() <= max {
        return leaf.to_string();
    }

    // Fichier tronqué
    if max >= 2 {
        let truncated: String = leaf.chars().take(max - 1).collect();
        return format!("{}{}", truncated, sep);
    }

    String::new()
}

fn format_mode(mode: u32) -> String {
    let chars: Vec<char> = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ]
    .iter()
    .map(|(bit, ch)| if mode & bit != 0 { *ch } else { '-' })
    .collect();
    chars.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Config / Setup screen
// ---------------------------------------------------------------------------
