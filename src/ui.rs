mod command_palette;
mod panel;
mod plugins;

use self::command_palette::render_command_palette;
use self::panel::{render_center_buttons, render_panel_or_file_id};
use self::plugins::render_plugins;
use crate::app::{
    ActionPaletteState, ActivePanel, App, AppMode, AssocEditorState, BookmarkListItem,
    ConfigState, ConfirmAction, ConfirmDialog, InputDialog, MENU_DATA,
    MENU_HEADERS, MenuAction, MenuState, OpenerState, PluginsState,
    RemoteConnectState, RemoteConnectingState, RemoteEditKind, RemoteEditState, SearchState,
    ViewerMenuKind, ViewerMenuState, ViewerPluginPaletteState,
};
use crate::config::SortMode;
use crate::copy::{CopyDialogState, CopyProgressState};
use crate::file_ops::format_size;
use crate::file_types::FileCategory;
use crate::help::HelpView;
use crate::idf::{IdfKind, probe_path};
use crate::panel::Entry;
use crate::remote::RemoteSource;
use crate::viewer::{ViewMode, Viewer};
use chrono::{DateTime, Local};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use unicode_width::UnicodeWidthStr;

// ---------------------------------------------------------------------------
// Colour palette (closer to original KKC brown/beige theme)
// ---------------------------------------------------------------------------

const CLR_APP_BG: Color = Color::Rgb(182, 160, 132);
const CLR_PANEL_BG: Color = Color::Black;
const CLR_PANEL_BORDER: Color = Color::Rgb(239, 225, 196);
const CLR_PANEL_BORDER_DIM: Color = Color::Rgb(118, 95, 70);
const CLR_PANEL_TITLE: Color = Color::Rgb(246, 237, 212);
const CLR_HEADER_BG: Color = Color::Rgb(125, 107, 92);
const CLR_HEADER_FG: Color = Color::Rgb(255, 244, 114);
const CLR_CURSOR_BG: Color = Color::Rgb(214, 196, 167);
const CLR_CURSOR_FG: Color = Color::Black;
const CLR_SELECTED: Color = Color::Rgb(255, 244, 114);
const CLR_DIR: Color = Color::Rgb(228, 210, 181);
const CLR_EXEC: Color = Color::Rgb(184, 234, 120);
const CLR_ARCHIVE: Color = Color::Rgb(234, 166, 116);
const CLR_AUDIO: Color = Color::Rgb(161, 238, 188);
const CLR_IMAGE: Color = Color::Rgb(255, 188, 166);
const CLR_VIDEO: Color = Color::Rgb(255, 144, 116);
const CLR_DOC: Color = Color::Rgb(163, 208, 255);
const CLR_SOURCE: Color = Color::Rgb(255, 238, 143);
const CLR_DATA: Color = Color::Rgb(164, 230, 225);
const CLR_TEXT: Color = Color::Rgb(224, 214, 192);
const CLR_UNKNOWN: Color = Color::Rgb(172, 160, 142);
const CLR_STATUS_BG: Color = Color::Rgb(125, 107, 92);
const CLR_STATUS_FG: Color = Color::Rgb(244, 235, 208);
const CLR_FKEY_BG: Color = Color::Rgb(92, 78, 64);
const CLR_FKEY_NUM: Color = Color::Black;
const CLR_FKEY_LABEL: Color = Color::Rgb(245, 235, 206);
const CLR_FKEY_NUM_BG: Color = Color::Rgb(241, 228, 193);
const CLR_BUTTON_BG: Color = Color::Rgb(181, 160, 132);
const CLR_BUTTON_FG: Color = Color::Rgb(255, 244, 114);

const CLR_MENU_BAR_BG: Color = Color::Rgb(54, 42, 30);
const CLR_MENU_BAR_FG: Color = Color::Rgb(241, 228, 193);
const CLR_MENU_SEL_BG: Color = Color::Rgb(241, 228, 193);
const CLR_MENU_SEL_FG: Color = Color::Black;
const CLR_MENU_DD_BG: Color = Color::Rgb(44, 34, 24);
const CLR_MENU_DD_FG: Color = Color::Rgb(241, 228, 193);
const CLR_MENU_DD_SEP: Color = Color::Rgb(118, 95, 70);
const CLR_MENU_BORDER: Color = Color::Rgb(180, 148, 108);
const CLR_MENU_HOTKEY: Color = Color::Rgb(255, 200, 80);

// Quick-palette (VSCode-style)
const CLR_QS_BG: Color = Color::Rgb(30, 30, 30);
const CLR_QS_BORDER: Color = Color::Rgb(80, 80, 80);
const CLR_QS_INPUT_BG: Color = Color::Rgb(58, 58, 58);
const CLR_QS_INPUT_FG: Color = Color::White;
const CLR_QS_SEP: Color = Color::Rgb(70, 70, 70);
const CLR_QS_LIST_FG: Color = Color::Rgb(200, 200, 200);
const CLR_QS_SEL_BG: Color = Color::Rgb(40, 79, 135);
const CLR_QS_SEL_FG: Color = Color::White;
const CLR_QS_MATCH_HI: Color = Color::Rgb(255, 197, 61);
const CLR_QS_MATCH_HI_SEL: Color = Color::Rgb(255, 230, 120);
const CLR_QS_NO_MATCH: Color = Color::Rgb(130, 130, 130);
const CLR_QS_DIR_FG: Color = Color::Rgb(86, 156, 214);

// ---------------------------------------------------------------------------
// Entry style by category
// ---------------------------------------------------------------------------

fn entry_fg(entry: &Entry, color_by_type: bool) -> Color {
    if !color_by_type {
        return CLR_TEXT;
    }
    if entry.selected {
        return CLR_SELECTED;
    }
    match entry.category {
        FileCategory::Directory => CLR_DIR,
        FileCategory::Executable => CLR_EXEC,
        FileCategory::Archive => CLR_ARCHIVE,
        FileCategory::Audio => CLR_AUDIO,
        FileCategory::Image => CLR_IMAGE,
        FileCategory::Video => CLR_VIDEO,
        FileCategory::Document => CLR_DOC,
        FileCategory::Source => CLR_SOURCE,
        FileCategory::Data => CLR_DATA,
        FileCategory::Text => CLR_TEXT,
        FileCategory::Unknown => CLR_UNKNOWN,
    }
}

// ---------------------------------------------------------------------------
// Top-level render
// ---------------------------------------------------------------------------

pub fn render(f: &mut Frame, app: &App) {
    f.render_widget(
        Block::default().style(Style::default().bg(CLR_APP_BG)),
        f.area(),
    );

    match &app.mode {
        AppMode::Viewer(v) => {
            render_viewer(f, v, false, None, f.area(), true, true, None);
            return;
        }
        AppMode::ViewerSearching(v) => {
            render_viewer(f, v, true, None, f.area(), true, true, None);
            return;
        }
        AppMode::ViewerGotoLine(v, input) => {
            render_viewer(f, v, false, Some(input), f.area(), true, true, None);
            return;
        }
        AppMode::ViewerMenu(v, menu) => {
            render_viewer(f, v, false, None, f.area(), true, true, None);
            render_viewer_menu(f, v, menu, f.area());
            return;
        }
        AppMode::ViewerPluginPalette(v, state) => {
            render_viewer(f, v, false, None, f.area(), true, true, None);
            render_viewer_plugin_palette(f, state, f.area());
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
        app.file_preview_info && !left_active,
        if left_active { None } else { app.quick_preview.as_ref() },
        app.quick_preview_active && !left_active,
        app.left_panel_tab_index(),
        app.left_panel_tab_count(),
    );
    render_center_buttons(f, panel_chunks[1]);
    render_panel_or_file_id(
        f,
        app,
        &app.right,
        panel_chunks[2],
        !left_active,
        app.config.color_by_type,
        app.file_preview_info && left_active,
        if !left_active { None } else { app.quick_preview.as_ref() },
        app.quick_preview_active && left_active,
        app.right_panel_tab_index(),
        app.right_panel_tab_count(),
    );
    render_status(f, app, status_area);

    if has_fbar {
        render_fkey_bar(f, main_vert[2]);
    }

    // Overlays
    match &app.mode {
        AppMode::Confirm(dlg) => render_confirm(f, dlg, f.area()),
        AppMode::Input(dlg) => render_input(f, dlg, f.area()),
        AppMode::CopyDialog(state) => render_copy_dialog(f, state, f.area()),
        AppMode::CopyProgress(state) => render_copy_progress(f, state, f.area()),
        AppMode::SearchPanel(s) => render_search(f, s, f.area()),
        AppMode::DirBookmarks => render_dir_bookmarks(f, app, f.area()),
        AppMode::Config(cs) => render_config(f, cs, f.area()),
        AppMode::Plugins(s) => render_plugins(f, s, f.area()),
        AppMode::ActionPalette(s) => render_action_palette(f, s, f.area()),
        AppMode::CommandPalette(s) => render_command_palette(f, s, f.area()),
        AppMode::Opener(s) => render_opener(f, s, f.area()),
        AppMode::AssocEditor(s) => render_assoc_editor(f, s, f.area()),
        AppMode::RemoteConnect(s) => render_remote_connect(f, s, f.area()),
        AppMode::RemoteEdit(s) => render_remote_edit(f, s, f.area()),
        AppMode::RemoteAddMenu(cursor) => render_remote_add_menu(f, *cursor, f.area()),
        AppMode::RemoteConnecting(s) => render_remote_connecting(f, s, f.area()),
        AppMode::Menu(ms) => render_menu(f, ms, f.area()),
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

fn render_menu(f: &mut Frame, state: &MenuState, area: Rect) {
    // ── top bar ────────────────────────────────────────────────────────────
    let bar_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };

    let mut spans = vec![Span::styled(" ", Style::default().bg(CLR_MENU_BAR_BG))];
    for (i, header) in MENU_HEADERS.iter().enumerate() {
        let selected = i == state.bar_pos;
        let base_style = if selected {
            Style::default()
                .bg(CLR_MENU_SEL_BG)
                .fg(CLR_MENU_SEL_FG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(CLR_MENU_BAR_BG).fg(CLR_MENU_BAR_FG)
        };
        let hotkey_style = if selected {
            base_style
        } else {
            Style::default()
                .bg(CLR_MENU_BAR_BG)
                .fg(CLR_MENU_HOTKEY)
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
        spans.push(Span::styled("  ", Style::default().bg(CLR_MENU_BAR_BG)));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(CLR_MENU_BAR_BG)),
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
        for i in 0..state.bar_pos {
            x += MENU_HEADERS[i].len() as u16 + 4; // " header  "
        }
        x
    };

    // Width: widest label + key hint + padding
    let max_label = items.iter().map(|(l, _, _)| l.len()).max().unwrap_or(6);
    let max_key = items
        .iter()
        .filter_map(|(_, k, _)| *k)
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
        .border_style(Style::default().fg(CLR_MENU_BORDER))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(dd_area);
    f.render_widget(block, dd_area);

    let avail = inner.width as usize;
    let menu_labels = items
        .iter()
        .map(|(label, _, action)| {
            if *action == MenuAction::Separator {
                String::new()
            } else {
                (*label).to_string()
            }
        })
        .collect::<Vec<_>>();
    let menu_mnemonics = mnemonics_for_labels(&menu_labels);
    for (idx, (label, key_hint, action)) in items.iter().enumerate() {
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
            let sep: String = std::iter::repeat('─').take(avail).collect();
            f.render_widget(
                Paragraph::new(sep).style(Style::default().fg(CLR_MENU_DD_SEP).bg(CLR_MENU_DD_BG)),
                row,
            );
        } else {
            let style = if idx == state.item_pos {
                Style::default()
                    .bg(CLR_MENU_SEL_BG)
                    .fg(CLR_MENU_SEL_FG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(CLR_MENU_DD_BG).fg(CLR_MENU_DD_FG)
            };
            let key_text = key_hint.unwrap_or("");
            let used = label.len() + key_text.len() + 2; // leading " " + trailing " "
            let pad = avail.saturating_sub(used);
            let line = menu_dropdown_line(
                label,
                key_text,
                pad,
                menu_mnemonics.get(idx).copied().flatten(),
                style,
            );
            f.render_widget(Paragraph::new(line).style(style), row);
        }
    }
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let entry_info = if let Some(e) = app.active_panel().current_entry() {
        if e.name == ".." {
            let mode_str = format_mode(e.mode);
            format!("Up directory  {}  dir", mode_str)
        } else if let Some(info) = probe_path(&e.path) {
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

    let status_text = if app.status.text.is_empty() {
        entry_info
    } else {
        app.status.text.clone()
    };

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

    let line = Line::from(vec![
        Span::styled(
            format!(
                " {:<width$}",
                status_text,
                width = left_w.saturating_sub(1) as usize
            ),
            Style::default().fg(CLR_STATUS_FG).bg(CLR_STATUS_BG),
        ),
        Span::styled(
            right_info,
            Style::default().fg(CLR_BUTTON_FG).bg(CLR_STATUS_BG),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

// ---------------------------------------------------------------------------
// Function key bar
// ---------------------------------------------------------------------------

fn render_fkey_bar(f: &mut Frame, area: Rect) {
    let labels: &[(&str, &str)] = &[
        ("1", "Help"),
        ("2", "Menu"),
        ("3", "View"),
        ("4", "Edit"),
        ("5", "Copy"),
        ("6", "Move"),
        ("7", "MDir"),
        ("8", "Delete"),
        ("9", "Sort"),
        ("10", "Quit"),
    ];

    let mut spans = Vec::new();
    for (num, label) in labels {
        spans.push(Span::styled(
            format!("{}", num),
            Style::default().fg(CLR_FKEY_NUM).bg(CLR_FKEY_NUM_BG),
        ));
        spans.push(Span::styled(
            format!("{} ", label),
            Style::default().fg(CLR_FKEY_LABEL).bg(CLR_FKEY_BG),
        ));
    }

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(CLR_FKEY_BG)),
        area,
    );
}

// ---------------------------------------------------------------------------
// Viewer
// ---------------------------------------------------------------------------

fn viewer_area(v: &Viewer, area: Rect) -> Rect {
    // Plugin document views always use the full area (like zoomed) so that
    // (a) no stale file-manager content bleeds through the margins, and
    // (b) the actual panel width is available to the plugin renderer.
    if v.zoomed || v.viewer_plugin.is_some() {
        return area;
    }

    let max_width = match v.mode {
        ViewMode::Hex => 80u16,
        ViewMode::Image => area.width.saturating_sub(4).max(40).min(area.width),
        _ => {
            let ln = v.line_number_width() as u16;
            let text_max = v
                .current_plain_lines()
                .iter()
                .map(|line| line.chars().count() as u16)
                .max()
                .unwrap_or(40);
            (text_max + ln).clamp(40, area.width.saturating_sub(4).max(40))
        }
    };
    let width = (max_width + 2).min(area.width);
    let height = (v.line_count() as u16 + 3).clamp(8, area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

pub fn kitty_image_area(v: &Viewer, area: Rect) -> Option<Rect> {
    if !v.is_image_mode() {
        return None;
    }
    let viewer_host = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(1),
    };
    let area = viewer_area(v, viewer_host);
    let inner = Block::default().borders(Borders::ALL).inner(area);
    if inner.width == 0 || inner.height == 0 {
        None
    } else {
        Some(inner)
    }
}

fn render_viewer(f: &mut Frame, v: &Viewer, searching: bool, goto_input: Option<&str>, area: Rect, show_footer: bool, active: bool, quick_preview_label: Option<&str>) {
    let (footer_area, viewer_host) = if show_footer {
        let footer = clamp_rect(
            area,
            Rect {
                x: area.x,
                y: area.y + area.height.saturating_sub(1),
                width: area.width,
                height: 1,
            },
        );
        let host = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        (footer, host)
    } else {
        // Embedded (quick-preview): no footer row, use full area for content
        (Rect::default(), area)
    };
    let area = viewer_area(v, viewer_host);
    let file_name = v.path.file_name().unwrap_or_default().to_string_lossy();
    let match_info = if !v.search.is_empty() {
        format!(" [{}/{}]", v.match_pos + 1, v.matches.len())
    } else {
        String::new()
    };
    let col_info = if matches!(v.mode, ViewMode::Text | ViewMode::Ansi) && !v.wrap && v.hscroll > 0
    {
        format!(" Col:{} ", v.hscroll)
    } else {
        String::new()
    };
    let lf_info = if matches!(v.mode, ViewMode::Text | ViewMode::Ansi) {
        format!(" LF:{} ", v.line_feed_label())
    } else {
        String::new()
    };
    let pre_info = if matches!(v.mode, ViewMode::Text | ViewMode::Ansi) {
        format!(" Pre:{} ", v.preproc_label())
    } else {
        String::new()
    };
    let enc_info = if matches!(v.mode, ViewMode::Text | ViewMode::Ansi | ViewMode::Hex) {
        format!(" Enc:{} ", v.encoding_label())
    } else {
        String::new()
    };
    let mask_info = if matches!(v.mode, ViewMode::Text | ViewMode::Ansi) {
        format!(" Syn:{} ", v.mask_label())
    } else {
        String::new()
    };
    let plugin_info = v
        .viewer_plugin
        .as_ref()
        .map(|name| format!(" Plugin:{} ", name))
        .unwrap_or_default();
    let zoom_info = format!(" Zoom:{} ", v.zoom_label());
    let image_info = if let Some(image) = v.image_info() {
        match (image.width, image.height) {
            (Some(w), Some(h)) => format!(" {} {}x{} ", image.format, w, h),
            _ => format!(" {} ", image.format),
        }
    } else {
        String::new()
    };
    let title = format!(
        " {} [{}] {}/{}{}{}{}{}{}{}{}{}{} ",
        file_name,
        v.mode_label(),
        v.scroll + 1,
        v.line_count(),
        image_info,
        lf_info,
        pre_info,
        enc_info,
        mask_info,
        plugin_info,
        zoom_info,
        col_info,
        match_info,
    );

    let (border_style, border_type, title_span) = if let Some(label) = quick_preview_label {
        // Quick-preview embedded panel: custom compact title
        if active {
            (
                Style::default().fg(CLR_HEADER_FG).add_modifier(Modifier::BOLD),
                BorderType::Thick,
                Span::styled(
                    format!(" {} ", label),
                    Style::default().fg(CLR_HEADER_FG).add_modifier(Modifier::BOLD),
                ),
            )
        } else {
            (
                Style::default().fg(CLR_PANEL_BORDER_DIM),
                BorderType::Rounded,
                Span::styled(
                    format!(" {} ", label),
                    Style::default().fg(CLR_PANEL_BORDER_DIM),
                ),
            )
        }
    } else if active {
        (
            Style::default().fg(CLR_PANEL_BORDER).add_modifier(Modifier::BOLD),
            BorderType::Thick,
            Span::raw(title.clone()),
        )
    } else {
        (
            Style::default().fg(CLR_PANEL_BORDER_DIM),
            BorderType::Rounded,
            Span::raw(title.clone()),
        )
    };
    let block = Block::default()
        .title(title_span)
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        inner,
    );

    if v.is_image_mode() && crate::viewer::kitty_graphics_supported() {
        let mut lines = vec![Line::from(Span::styled(
            "Image preview",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))];
        if let Some(image) = v.image_info() {
            let detail = match (image.width, image.height) {
                (Some(w), Some(h)) => format!("{} - {}x{}", image.format, w, h),
                _ => image.format.to_string(),
            };
            lines.push(Line::from(Span::styled(
                detail,
                Style::default().fg(Color::Gray),
            )));
        }
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "Rendered with Kitty Graphics Protocol",
            Style::default().fg(Color::Cyan),
        )));
        lines.push(Line::from(Span::styled(
            "Use F5 to toggle Auto/Full size",
            Style::default().fg(Color::Gray),
        )));
        f.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .style(Style::default().bg(Color::Black)),
            inner,
        );
        if show_footer {
            let help = Paragraph::new(" F10:Close  F4:Mode  F5:Zoom ")
                .style(Style::default().fg(Color::Black).bg(Color::Cyan));
            f.render_widget(help, footer_area);
        }
        return;
    }

    let height = inner.height as usize;
    let width = inner.width as usize;

    // Line-number gutter (text/ansi modes only, not for plugin documents).
    let ln_width = v.line_number_width();
    let total_lines = v.line_count();
    let ln_digits = if ln_width > 0 {
        total_lines.max(1).ilog10() as usize + 1
    } else {
        0
    };
    let text_width = width.saturating_sub(ln_width);

    let search_lower = v.search.to_lowercase();
    let items: Vec<Line> = v
        .render_lines(text_width, v.scroll, height)
        .into_iter()
        .enumerate()
        .map(|(rel_idx, line)| {
            let abs_idx = v.scroll + rel_idx;
            let plain = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            let is_match = !search_lower.is_empty() && plain.to_lowercase().contains(&search_lower);
            let is_current_match = is_match && v.matches.get(v.match_pos).copied() == Some(abs_idx);

            let content_line = if is_current_match {
                Line::from(vec![Span::styled(
                    truncate_str(&plain, text_width),
                    Style::default().fg(Color::Black).bg(Color::Yellow),
                )])
            } else if is_match {
                Line::from(vec![Span::styled(
                    truncate_str(&plain, text_width),
                    Style::default().fg(Color::Black).bg(Color::LightYellow),
                )])
            } else {
                line
            };

            if ln_width > 0 {
                let num_str = format!("{:>width$}\u{2502} ", abs_idx + 1, width = ln_digits);
                let mut spans = vec![Span::styled(
                    num_str,
                    Style::default().fg(Color::Rgb(90, 110, 150)),
                )];
                spans.extend(content_line.spans);
                Line::from(spans)
            } else {
                content_line
            }
        })
        .collect();

    if v.viewer_plugin.is_none() && v.wrap && matches!(v.mode, ViewMode::Text | ViewMode::Ansi) {
        f.render_widget(
            Paragraph::new(items)
                .wrap(Wrap { trim: false })
                .style(Style::default().bg(Color::Black)),
            inner,
        );
    } else {
        let list = List::new(items.into_iter().map(ListItem::new).collect::<Vec<_>>())
            .style(Style::default().bg(Color::Black));
        f.render_widget(list, inner);
    }

    if searching && show_footer {
        let label = format!(" Search: {}_ ", v.search);
        let found_count = v.matches.len();
        let found_label = if v.search.is_empty() {
            String::new()
        } else {
            format!("  {} match(es)  Enter:OK  Esc:Cancel", found_count)
        };
        let bar_text = format!("{}{}", label, found_label);
        f.render_widget(
            Paragraph::new(bar_text)
                .style(Style::default().fg(Color::Black).bg(Color::LightYellow)),
            footer_area,
        );
        let cx =
            (footer_area.x + 9 + v.search.len() as u16).min(footer_area.x + footer_area.width - 1);
        safe_set_cursor_position(f, cx, footer_area.y);
    } else if let Some(input) = goto_input
        && show_footer
    {
        let (label, label_len) = if matches!(v.mode, ViewMode::Hex) {
            (" Goto offset (hex): ", 21u16)
        } else {
            (" Goto line: ", 13u16)
        };
        let bar_text = format!("{}{}_", label, input);
        f.render_widget(
            Paragraph::new(bar_text).style(Style::default().fg(Color::Black).bg(Color::LightCyan)),
            footer_area,
        );
        let cx =
            (footer_area.x + label_len + input.len() as u16).min(footer_area.x + footer_area.width - 1);
        safe_set_cursor_position(f, cx, footer_area.y);
    } else if show_footer {
        let help = Paragraph::new(" F10:Close  F2:Wrap  F3:LnFeed  F4:Mode  F5:Zoom  F6:Prepro  F7:Search  F8:Enc  F9:Syntax  ^G:Goto ")
            .style(Style::default().fg(Color::Black).bg(Color::Cyan));
        f.render_widget(help, footer_area);
    }
}

fn render_viewer_menu(f: &mut Frame, viewer: &Viewer, menu: &ViewerMenuState, area: Rect) {
    let items: Vec<String> = match menu.kind {
        ViewerMenuKind::Mode => vec![
            "Text: as plain text",
            "Binary: as hex dump",
            "Ansi: with ANSI escapes",
            "Image: as inline preview",
            "Plugins viewer",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>(),
        ViewerMenuKind::LineFeed => vec!["DOS (CR/LF)", "Unix (LF)", "Mac (CR)", "Mixed"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ViewerMenuKind::Preproc => {
            let mut items = Vec::new();
            for idx in 0..viewer.preproc_len() {
                if let Some(label) = viewer.preproc_item_label(idx) {
                    items.push(label);
                }
            }
            if viewer.preproc_len() > 0 {
                items.push("────────".into());
            }
            items.extend([
                "Add XOR".into(),
                "Add AND".into(),
                "Add OR".into(),
                "Add NEG".into(),
                "Add ROR".into(),
                "Add ADD".into(),
                "Add Latin".into(),
                "Add Elite".into(),
                "Clear All".into(),
            ]);
            items
        }
        ViewerMenuKind::Encoding => vec!["Plain ASCII".into(), "DOS CP437".into()],
        ViewerMenuKind::Mask => vec![
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
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
    };

    let title = match menu.kind {
        ViewerMenuKind::Mode => " Change Viewer ",
        ViewerMenuKind::LineFeed => " Change Line Feed ",
        ViewerMenuKind::Preproc => " Preprocess ",
        ViewerMenuKind::Encoding => " Character Set ",
        ViewerMenuKind::Mask => " Syntax Highlight ",
    };

    let width = items
        .iter()
        .map(|s| UnicodeWidthStr::width(s.as_str()))
        .max()
        .unwrap_or(10) as u16
        + 6;
    let extra = if menu.kind == ViewerMenuKind::Preproc {
        3
    } else {
        0
    };
    let desired_height = items.len() as u16 + 2 + extra;
    let height = desired_height.min(area.height.saturating_sub(2).max(4));
    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );

    safe_render_widget(f, Clear, popup);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    safe_render_widget(
        f,
        Block::default().style(Style::default().bg(CLR_MENU_DD_BG)),
        inner,
    );

    let all_items = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_separator = menu.kind == ViewerMenuKind::Preproc
                && viewer.preproc_len() > 0
                && idx == viewer.preproc_len();
            let style = if is_separator {
                Style::default().fg(CLR_MENU_DD_SEP).bg(CLR_MENU_DD_BG)
            } else if idx == menu.cursor {
                Style::default()
                    .fg(CLR_MENU_SEL_FG)
                    .bg(CLR_MENU_SEL_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
            };
            let line = if menu.kind == ViewerMenuKind::Mode {
                viewer_mode_menu_line(idx, item, style)
            } else if is_separator {
                Line::from(Span::styled(format!(" {}", item), style))
            } else {
                viewer_submenu_line(viewer, menu.kind, idx, item, style)
            };
            ListItem::new(line).style(style)
        })
        .collect::<Vec<_>>();

    let list_height = inner.height.saturating_sub(extra);
    let list_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: list_height,
    };
    let scroll = menu
        .scroll
        .min(items.len().saturating_sub(list_height as usize));
    let visible_items = all_items
        .into_iter()
        .skip(scroll)
        .take(list_height as usize)
        .collect::<Vec<_>>();
    safe_render_widget(
        f,
        List::new(visible_items).style(Style::default().bg(CLR_MENU_DD_BG)),
        list_area,
    );

    if menu.kind == ViewerMenuKind::Preproc {
        let info_area = Rect {
            x: inner.x,
            y: inner.y + list_height,
            width: inner.width,
            height: inner.height.saturating_sub(list_height),
        };
        let info = format!(
            " Param:0x{:02X}  \u{2190}/\u{2192}:Edit  Ctrl+\u{2191}/\u{2193}:Move  Del:Remove ",
            menu.param
        );
        safe_render_widget(
            f,
            Paragraph::new(info).style(Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)),
            info_area,
        );
    }
}

fn viewer_mode_menu_line(idx: usize, item: &str, style: Style) -> Line<'static> {
    if idx == 4 {
        return Line::from(vec![
            Span::styled(" ", style),
            Span::styled("P. ", style.add_modifier(Modifier::BOLD)),
            Span::styled(item.to_string(), style),
        ]);
    }

    let number = if idx < 9 {
        format!("{} ", idx + 1)
    } else {
        "  ".into()
    };
    let shortcut = item.chars().next().unwrap_or_default().to_string();
    let rest = item.chars().skip(1).collect::<String>();
    let mut spans = vec![
        Span::styled(" ", style),
        Span::styled(number, style.add_modifier(Modifier::BOLD)),
    ];
    if idx < 6 {
        spans.push(Span::styled(shortcut, style.add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(rest, style));
    } else {
        spans.push(Span::styled(item.to_string(), style));
    }
    Line::from(spans)
}

fn viewer_submenu_line(
    viewer: &Viewer,
    kind: ViewerMenuKind,
    idx: usize,
    item: &str,
    style: Style,
) -> Line<'static> {
    let labels = viewer_menu_render_labels(viewer, kind);
    let mnemonics = mnemonics_for_labels(&labels);
    let shortcut = mnemonics.get(idx).copied().flatten();
    let mut spans = vec![Span::styled(" ", style)];
    append_highlighted_mnemonic(&mut spans, item, shortcut, style);
    Line::from(spans)
}

fn viewer_menu_render_labels(viewer: &Viewer, kind: ViewerMenuKind) -> Vec<String> {
    match kind {
        ViewerMenuKind::Mode => Vec::new(),
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
                [
                    "Add XOR",
                    "Add AND",
                    "Add OR",
                    "Add NEG",
                    "Add ROR",
                    "Add ADD",
                    "Add Latin",
                    "Add Elite",
                    "Clear All",
                ]
                .into_iter()
                .map(String::from),
            );
            labels
        }
        ViewerMenuKind::LineFeed => vec!["DOS (CR/LF)", "Unix (LF)", "Mac (CR)", "Mixed"]
            .into_iter()
            .map(String::from)
            .collect(),
        ViewerMenuKind::Encoding => vec!["Plain ASCII", "DOS CP437"]
            .into_iter()
            .map(String::from)
            .collect(),
        ViewerMenuKind::Mask => vec![
            "C Style",
            "Pascal Style",
            "Assembler Style",
            "Ketchup Style",
            "Mask OFF",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    }
}

fn menu_dropdown_line(
    label: &str,
    key_text: &str,
    pad: usize,
    mnemonic: Option<char>,
    style: Style,
) -> Line<'static> {
    let mut spans = vec![Span::styled(" ", style)];
    append_highlighted_mnemonic(&mut spans, label, mnemonic, style);
    spans.push(Span::styled(" ".repeat(pad), style));
    if !key_text.is_empty() {
        spans.push(Span::styled(key_text.to_string(), style));
    }
    spans.push(Span::styled(" ", style));
    Line::from(spans)
}

fn append_highlighted_mnemonic(
    spans: &mut Vec<Span<'static>>,
    label: &str,
    mnemonic: Option<char>,
    style: Style,
) {
    let mut highlighted = false;
    for ch in label.chars() {
        let matches = mnemonic == Some(ch.to_ascii_lowercase()) && !highlighted;
        let item_style = if matches {
            highlighted = true;
            style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            style
        };
        spans.push(Span::styled(ch.to_string(), item_style));
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

// ---------------------------------------------------------------------------
// Confirm dialog
// ---------------------------------------------------------------------------

fn render_confirm(f: &mut Frame, dlg: &ConfirmDialog, area: Rect) {
    match &dlg.action {
        ConfirmAction::Message | ConfirmAction::MessageThen(_) => {
            render_confirm_message(f, dlg, area)
        }
        ConfirmAction::Quit => render_confirm_quit(f, area),
        ConfirmAction::Delete(paths) => render_confirm_delete(f, &dlg.message, paths.len(), area),
        ConfirmAction::DeleteRemote(targets) => {
            render_confirm_delete(f, &dlg.message, targets.len(), area)
        }
    }
}

/// Hard-wrap `msg` to fit within `max_width` display columns.
/// Tabs are expanded to 2 spaces. Word-breaks are preferred; hard-breaks
/// are used when a single token exceeds the available width.
fn wrap_message(msg: &str, max_width: usize) -> String {
    if max_width == 0 {
        return msg.to_string();
    }
    let mut result = String::new();
    for raw_line in msg.lines() {
        let line = raw_line.replace('\t', "  ");
        if line.is_empty() {
            result.push('\n');
            continue;
        }
        let mut remaining: &str = &line;
        while !remaining.is_empty() {
            let mut acc = 0usize;
            let mut last_space: Option<usize> = None;
            let mut hard_cut: Option<usize> = None;
            for (i, ch) in remaining.char_indices() {
                let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                if acc + cw > max_width {
                    hard_cut = Some(i);
                    break;
                }
                acc += cw;
                if ch == ' ' {
                    last_space = Some(i);
                }
            }
            match hard_cut {
                None => {
                    result.push_str(remaining);
                    result.push('\n');
                    break;
                }
                Some(cut) => {
                    let split = last_space.unwrap_or(cut);
                    result.push_str(&remaining[..split]);
                    result.push('\n');
                    remaining = remaining[split..].trim_start_matches(' ');
                }
            }
        }
    }
    result
}

fn render_confirm_message(f: &mut Frame, dlg: &ConfirmDialog, area: Rect) {
    let max_w = area.width.saturating_sub(4);
    let width = 72u16.min(max_w).max(40);
    // 2 border cols + 2 padding cols = 4 reserved
    let text_w = width.saturating_sub(4) as usize;

    // Pre-wrap so we know the exact row count (and avoid ratatui overflowing
    // long unbreakable tokens like Lua file paths).
    let wrapped = wrap_message(&dlg.message, text_w);
    let msg_rows = wrapped.lines().count().max(1) as u16;

    // borders(2) + top_pad(1) + text rows + bottom_pad(1) + ok_btn(1) + hint(1)
    let desired_h = msg_rows + 6;
    let height = desired_h.max(8).min(area.height.saturating_sub(2).max(8));

    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    safe_render_widget(f, Clear, popup);

    let title_str = if dlg.title.is_empty() {
        " Notice ".to_string()
    } else {
        format!(" {} ", dlg.title)
    };

    let block = Block::default()
        .title(title_str)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    // Available rows for message text (leave room for OK button + hint)
    let msg_h = inner.height.saturating_sub(3).max(1);
    safe_render_widget(
        f,
        Paragraph::new(wrapped.as_str())
            .style(Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)),
        Rect {
            x: inner.x + 1,
            y: inner.y + 1,
            width: inner.width.saturating_sub(2),
            height: msg_h,
        },
    );

    safe_render_widget(
        f,
        Paragraph::new(" [ OK ] ")
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(CLR_MENU_SEL_FG)
                    .bg(CLR_MENU_SEL_BG)
                    .add_modifier(Modifier::BOLD),
            ),
        Rect {
            x: inner.x + inner.width.saturating_sub(8) / 2,
            y: inner.y + inner.height.saturating_sub(2),
            width: 8,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new("Enter / Esc")
            .alignment(Alignment::Center)
            .style(Style::default().fg(CLR_MENU_DD_SEP).bg(CLR_MENU_DD_BG)),
        Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        },
    );
}

// ---------------------------------------------------------------------------
// Quit dialog
// ---------------------------------------------------------------------------

fn render_confirm_quit(f: &mut Frame, area: Rect) {
    const W: u16 = 38;
    const H: u16 = 11;
    let x = (area.width.saturating_sub(W)) / 2 + area.x;
    let y = (area.height.saturating_sub(H)) / 2 + area.y;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width: W,
            height: H,
        },
    );

    // Shadow
    let sh = Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: W,
        height: H,
    };
    if sh.x + sh.width <= area.x + area.width && sh.y + sh.height <= area.y + area.height {
        safe_render_widget(
            f,
            Block::default().style(Style::default().bg(Color::Rgb(20, 15, 10))),
            sh,
        );
    }
    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG))
        .style(Style::default().bg(CLR_APP_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    // Title band
    let logo_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    safe_render_widget(
        f,
        Paragraph::new(" KK Commander ")
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(CLR_BUTTON_FG)
                    .bg(CLR_STATUS_BG)
                    .add_modifier(Modifier::BOLD),
            ),
        logo_area,
    );

    // Top separator
    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep.clone()).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );

    // Message
    safe_render_widget(
        f,
        Paragraph::new("\nDo you really want to quit?")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Rgb(50, 36, 22)).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: 3,
        },
    );

    // Bottom separator
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 5,
            width: inner.width,
            height: 1,
        },
    );

    // Buttons
    let btn_y = inner.y + 7;
    let yes_w: u16 = 11;
    let no_w: u16 = 11;
    let gap: u16 = 4;
    let btn_x = inner.x + (inner.width.saturating_sub(yes_w + gap + no_w)) / 2;

    safe_render_widget(
        f,
        Paragraph::new("  [ Yes ]  ").style(
            Style::default()
                .fg(Color::Black)
                .bg(CLR_PANEL_BORDER)
                .add_modifier(Modifier::BOLD),
        ),
        Rect {
            x: btn_x,
            y: btn_y,
            width: yes_w,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new("  [  No ]  ")
            .style(Style::default().fg(Color::Rgb(80, 60, 40)).bg(CLR_APP_BG)),
        Rect {
            x: btn_x + yes_w + gap,
            y: btn_y,
            width: no_w,
            height: 1,
        },
    );

    // Key hints
    safe_render_widget(
        f,
        Paragraph::new("Y / Enter  ·  N / Esc")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Rgb(120, 90, 60)).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 8,
            width: inner.width,
            height: 1,
        },
    );
}

// ---------------------------------------------------------------------------
// Delete confirm dialog
// ---------------------------------------------------------------------------

fn render_confirm_delete(f: &mut Frame, message: &str, count: usize, area: Rect) {
    const W: u16 = 44;
    const H: u16 = 9;
    let x = (area.width.saturating_sub(W)) / 2 + area.x;
    let y = (area.height.saturating_sub(H)) / 2 + area.y;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width: W,
            height: H,
        },
    );
    safe_render_widget(f, Clear, popup);

    let title = Span::styled(
        " Delete ",
        Style::default()
            .fg(Color::Rgb(255, 100, 80))
            .add_modifier(Modifier::BOLD),
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(180, 60, 40)))
        .style(Style::default().bg(Color::Rgb(38, 18, 14)));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    // Warning header
    let icon_label = if count == 1 {
        "\u{26a0}  Delete this item?"
    } else {
        "\u{26a0}  Delete these items?"
    };
    safe_render_widget(
        f,
        Paragraph::new(icon_label)
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(Color::Rgb(255, 160, 60))
                    .bg(Color::Rgb(38, 18, 14))
                    .add_modifier(Modifier::BOLD),
            ),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    // Message
    let short_msg = truncate_str(message, inner.width as usize);
    safe_render_widget(
        f,
        Paragraph::new(short_msg)
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(Color::Rgb(240, 200, 180))
                    .bg(Color::Rgb(38, 18, 14)),
            ),
        Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: 2,
        },
    );

    // Buttons
    let btn_y = inner.y + 5;
    let yes_w: u16 = 13;
    let no_w: u16 = 13;
    let gap: u16 = 4;
    let btn_x = inner.x + (inner.width.saturating_sub(yes_w + gap + no_w)) / 2;

    safe_render_widget(
        f,
        Paragraph::new("  [ Delete ]  ").style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Rgb(160, 40, 30))
                .add_modifier(Modifier::BOLD),
        ),
        Rect {
            x: btn_x,
            y: btn_y,
            width: yes_w,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new("  [ Cancel ]  ").style(
            Style::default()
                .fg(Color::Rgb(180, 140, 120))
                .bg(Color::Rgb(38, 18, 14)),
        ),
        Rect {
            x: btn_x + yes_w + gap,
            y: btn_y,
            width: no_w,
            height: 1,
        },
    );

    // Hints
    safe_render_widget(
        f,
        Paragraph::new("Y / Enter  ·  N / Esc")
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(Color::Rgb(130, 90, 70))
                    .bg(Color::Rgb(38, 18, 14)),
            ),
        Rect {
            x: inner.x,
            y: btn_y + 1,
            width: inner.width,
            height: 1,
        },
    );
}

// ---------------------------------------------------------------------------
// Input dialog
// ---------------------------------------------------------------------------

fn render_input(f: &mut Frame, dlg: &InputDialog, area: Rect) {
    let width = 60u16.min(area.width.saturating_sub(4));
    let height = 7u16;
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width,
            height,
        },
    );

    safe_render_widget(f, Clear, popup);
    let block = Block::default()
        .title(format!(" {} ", dlg.title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    let input_w = inner.width.saturating_sub(2) as usize;
    let value_display = format!("{:<width$}", dlg.value, width = input_w);

    let prompt_line = Line::from(Span::styled(
        format!(" {} ", dlg.prompt),
        Style::default().fg(Color::White),
    ));
    let input_line = Line::from(Span::styled(
        format!(" {} ", value_display),
        Style::default().fg(Color::Black).bg(Color::White),
    ));
    let hint_line = Line::from(Span::styled(
        "  Enter:OK  Esc:Cancel",
        Style::default().fg(Color::DarkGray),
    ));

    f.render_widget(
        Paragraph::new(vec![
            Line::default(),
            prompt_line,
            Line::default(),
            input_line,
            hint_line,
        ]),
        inner,
    );

    // Draw cursor inside input field
    let cursor_x = (inner.x + 1 + dlg.cursor as u16).min(inner.x + inner.width.saturating_sub(2));
    let cursor_y = inner.y + 3;
    if cursor_y < inner.y + inner.height {
        safe_set_cursor_position(f, cursor_x, cursor_y);
    }
}

fn render_copy_dialog(f: &mut Frame, dlg: &CopyDialogState, area: Rect) {
    let width = 66u16.min(area.width.saturating_sub(4));
    let height = 14u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width,
            height,
        },
    );

    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Copy ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let dest_style = if dlg.field == CopyDialogState::DESTINATION {
        Style::default().fg(CLR_MENU_SEL_FG).bg(CLR_MENU_SEL_BG)
    } else {
        Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
    };
    let overwrite_style = if dlg.field == CopyDialogState::OVERWRITE {
        Style::default()
            .fg(CLR_MENU_SEL_FG)
            .bg(CLR_MENU_SEL_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
    };
    let newer_style = if dlg.field == CopyDialogState::NEWER_ONLY {
        Style::default()
            .fg(CLR_MENU_SEL_FG)
            .bg(CLR_MENU_SEL_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
    };
    let keep_attr_style = if dlg.field == CopyDialogState::KEEP_ATTRIBUTES {
        Style::default()
            .fg(CLR_MENU_SEL_FG)
            .bg(CLR_MENU_SEL_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
    };
    let start_style = if dlg.field == CopyDialogState::START {
        Style::default()
            .fg(CLR_MENU_SEL_FG)
            .bg(CLR_MENU_SEL_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
    };
    let cancel_style = if dlg.field == CopyDialogState::CANCEL {
        Style::default()
            .fg(CLR_MENU_SEL_FG)
            .bg(CLR_MENU_SEL_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
    };

    let dest_width = inner.width.saturating_sub(6) as usize;
    let dest_value = truncate_str(
        &format!("{:<width$}", dlg.destination, width = dest_width),
        dest_width,
    );
    let summary = if dlg.waiting_to_start {
        "Waiting...".to_string()
    } else if dlg.stats_pending && dlg.file_count == 0 && dlg.total_bytes == 0 {
        "Calculating remote size...".to_string()
    } else if dlg.file_count == 1 {
        format!("Copy one file ({} bytes) to", dlg.total_bytes)
    } else {
        format!(
            "Copy {} files ({} bytes) to",
            dlg.file_count, dlg.total_bytes
        )
    };
    let counters = if dlg.file_count == 1 {
        format!(" 1 file  {} bytes", dlg.total_bytes)
    } else {
        format!(" {} files  {} bytes", dlg.file_count, dlg.total_bytes)
    };
    let lines = vec![
        Line::from(Span::styled(
            summary,
            Style::default()
                .fg(CLR_HEADER_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            if dlg.waiting_to_start {
                " Waiting for size calculation to finish..."
            } else if dlg.stats_pending {
                " Scanning subdirectories..."
            } else {
                " "
            },
            Style::default().fg(CLR_UNKNOWN).bg(CLR_MENU_DD_BG),
        )),
        Line::from(Span::styled(
            counters,
            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG),
        )),
        Line::from(Span::styled(
            " Destination:",
            Style::default().fg(CLR_HEADER_FG).bg(CLR_MENU_DD_BG),
        )),
        Line::from(Span::styled(format!(" {}", dest_value), dest_style)),
        Line::from(Span::styled(
            format!(
                " [{}] Keep attributes",
                if dlg.keep_attributes { 'x' } else { ' ' }
            ),
            keep_attr_style,
        )),
        Line::from(Span::styled(
            format!(
                " [{}] Overwrite existing",
                if dlg.overwrite { 'x' } else { ' ' }
            ),
            overwrite_style,
        )),
        Line::from(Span::styled(
            format!(
                " [{}] Newer files only",
                if dlg.newer_only { 'x' } else { ' ' }
            ),
            newer_style,
        )),
        Line::default(),
        Line::from(if dlg.waiting_to_start {
            vec![Span::styled(" [ Abort ] ", start_style)]
        } else {
            vec![
                Span::styled(" [ Start Copy ] ", start_style),
                Span::raw("  "),
                Span::styled(" [ Cancel ] ", cancel_style),
            ]
        }),
        Line::default(),
        Line::from(Span::styled(
            if dlg.waiting_to_start {
                " Enter/Esc:Abort"
            } else {
                " Up/Down:Select  Space:Toggle  Enter:OK  Esc:Cancel"
            },
            Style::default().fg(CLR_UNKNOWN).bg(CLR_MENU_DD_BG),
        )),
    ];
    safe_render_widget(
        f,
        Paragraph::new(lines).style(Style::default().bg(CLR_MENU_DD_BG)),
        inner,
    );

    if dlg.field == CopyDialogState::DESTINATION && !dlg.stats_pending && !dlg.waiting_to_start {
        let cursor_x =
            (inner.x + 1 + dlg.cursor as u16).min(inner.x + inner.width.saturating_sub(1));
        let cursor_y = inner.y + 3;
        safe_set_cursor_position(f, cursor_x, cursor_y);
    }
}

fn render_copy_progress(f: &mut Frame, state: &CopyProgressState, area: Rect) {
    let width = 70u16.min(area.width.saturating_sub(4));
    let height = 10u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width,
            height,
        },
    );

    safe_render_widget(f, Clear, popup);
    let block = Block::default()
        .title(" Copy ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    if inner.height < 6 {
        return;
    }

    let file_ratio = if state.file_total == 0 {
        0.0
    } else {
        state.file_done as f64 / state.file_total as f64
    };
    let total_ratio = if state.total_bytes == 0 {
        0.0
    } else {
        state.total_done as f64 / state.total_bytes as f64
    };
    let bar_width = inner.width.saturating_sub(10) as usize;
    let lines = vec![
        Line::from(Span::styled(
            truncate_str(&state.current_name, inner.width as usize),
            Style::default()
                .fg(CLR_HEADER_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("File  {}", progress_bar_string(bar_width, file_ratio)),
            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG),
        )),
        Line::from(Span::styled(
            format!("Total {}", progress_bar_string(bar_width, total_ratio)),
            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG),
        )),
        Line::from(Span::styled(
            format!(
                "{}/{}  {} / {} bytes",
                state.item_index, state.item_count, state.total_done, state.total_bytes
            ),
            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG),
        )),
        Line::from(Span::styled(
            format!(
                "Remaining: {}",
                state
                    .remaining_secs
                    .map(|s| format!("{s} sec"))
                    .unwrap_or_else(|| "--".into())
            ),
            Style::default().fg(CLR_UNKNOWN).bg(CLR_MENU_DD_BG),
        )),
        Line::default(),
        Line::from(Span::styled(
            " Enter/Esc/F10:Abort",
            Style::default().fg(CLR_UNKNOWN).bg(CLR_MENU_DD_BG),
        )),
    ];
    safe_render_widget(
        f,
        Paragraph::new(lines).style(Style::default().bg(CLR_MENU_DD_BG)),
        inner,
    );
}

fn progress_bar_string(width: usize, ratio: f64) -> String {
    let width = width.max(8);
    let filled = ((width as f64) * ratio.clamp(0.0, 1.0)).round() as usize;
    let filled = filled.min(width);
    format!(
        "[{}{}] {:>3}%",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled)),
        (ratio.clamp(0.0, 1.0) * 100.0).round() as u64
    )
}

fn render_remote_connect(f: &mut Frame, state: &RemoteConnectState, area: Rect) {
    let width = 76u16.min(area.width.saturating_sub(4));
    let height = 20u16.min(area.height.saturating_sub(2)).max(10);
    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    safe_render_widget(f, Clear, popup);
    let block = Block::default()
        .title(Span::styled(
            " Remote Connections ",
            Style::default()
                .fg(CLR_MENU_BAR_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER).bg(CLR_MENU_DD_BG))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    if inner.height < 4 {
        return;
    }

    let input_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let sep_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: 1,
    };
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };
    let hint_area = clamp_rect(
        area,
        Rect {
            x: inner.x,
            y: inner.y + inner.height - 1,
            width: inner.width,
            height: 1,
        },
    );

    let matches = state.filtered_indices();
    let total = matches.len();
    let count_hint = if state.query.is_empty() {
        format!(" {} ", state.items.len())
    } else if total > 0 {
        format!(" {}/{} ", state.match_pos + 1, total)
    } else {
        " 0/0 ".to_owned()
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" ⌕ {}\u{2581}", state.query);
    let input_row = Line::from(vec![
        Span::styled(
            truncate_str(&input_text, input_inner_w),
            Style::default().fg(CLR_QS_INPUT_FG).bg(CLR_QS_INPUT_BG),
        ),
        Span::styled(
            count_hint,
            Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_INPUT_BG),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(CLR_QS_INPUT_BG)),
        input_area,
    );

    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_MENU_DD_BG)),
        sep_area,
    );

    let rows = list_area.height as usize;
    let tokens: Vec<String> = state
        .query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();
    let scroll = if state.match_pos >= rows && rows > 0 {
        state.match_pos - rows + 1
    } else {
        0
    };

    let items = if state.items.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            " No server entry found in ~/.ssh/config or connections.toml ",
            Style::default().fg(CLR_UNKNOWN),
        )))]
    } else if matches.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            " No matching connection ",
            Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_MENU_DD_BG),
        )))]
    } else {
        matches
            .iter()
            .enumerate()
            .skip(scroll)
            .take(rows)
            .map(|(match_idx, item_idx)| {
                let item = &state.items[*item_idx];
                let protocol = item.protocol();
                let (r, g, b) = protocol.color_rgb();
                let proto = protocol.name();
                let proto_style = Style::default()
                    .fg(Color::Rgb(r, g, b))
                    .bg(CLR_MENU_DD_BG);
                let (source, badge_style) = match item.source {
                    RemoteSource::SshConfig => (
                        "ssh",
                        Style::default()
                            .fg(Color::Rgb(255, 208, 124))
                            .bg(CLR_MENU_DD_BG),
                    ),
                    RemoteSource::UserToml => (
                        "toml",
                        Style::default()
                            .fg(Color::Rgb(246, 237, 212))
                            .bg(CLR_MENU_DD_BG),
                    ),
                };
                let host = item.host_label();
                let selected = match_idx == state.match_pos;
                let row_style = if selected {
                    Style::default()
                        .fg(CLR_MENU_SEL_FG)
                        .bg(CLR_MENU_SEL_BG)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
                };
                let badge_style = if selected {
                    badge_style.bg(CLR_MENU_SEL_BG).add_modifier(Modifier::BOLD)
                } else {
                    badge_style
                };
                let proto_style = if selected {
                    proto_style.bg(CLR_MENU_SEL_BG).add_modifier(Modifier::BOLD)
                } else {
                    proto_style
                };
                let alias_style = row_style.add_modifier(Modifier::BOLD);
                let host_style = if selected {
                    Style::default()
                        .fg(CLR_MENU_SEL_FG)
                        .bg(CLR_MENU_SEL_BG)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Rgb(198, 184, 156))
                        .bg(CLR_MENU_DD_BG)
                };
                let alias_line = highlight_tokens(
                    &format!("{:<16}", truncate_str(&item.name, 16)),
                    &tokens,
                    alias_style.fg.unwrap_or(CLR_MENU_DD_FG),
                    alias_style.bg.unwrap_or(CLR_MENU_DD_BG),
                    CLR_QS_MATCH_HI_SEL,
                );
                let host_text = truncate_str(&host, inner.width.saturating_sub(35) as usize);
                let host_line = highlight_tokens(
                    &host_text,
                    &tokens,
                    host_style.fg.unwrap_or(CLR_MENU_DD_FG),
                    host_style.bg.unwrap_or(CLR_MENU_DD_BG),
                    if selected {
                        CLR_QS_MATCH_HI_SEL
                    } else {
                        CLR_QS_MATCH_HI
                    },
                );
                let mut spans = vec![Span::styled(" ", row_style)];
                spans.extend(alias_line.spans);
                spans.extend([
                    Span::styled(" ", row_style),
                    Span::styled(format!("{:^6}", proto), proto_style),
                    Span::styled(" ", row_style),
                    Span::styled(format!("{:^6}", source), badge_style),
                    Span::styled("  ", row_style),
                ]);
                spans.extend(host_line.spans);
                let used: usize = spans.iter().map(|s| s.content.len()).sum();
                if used < list_area.width as usize {
                    spans.push(Span::styled(
                        " ".repeat(list_area.width as usize - used),
                        row_style,
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect()
    };
    safe_render_widget(f, List::new(items), list_area);
    safe_render_widget(
        f,
        Paragraph::new(" Type:Filter  Enter:Connect  Tab:SSH  F6:Edit  F7:Add  Esc:Cancel ")
            .style(Style::default().fg(CLR_BUTTON_FG).bg(CLR_STATUS_BG)),
        hint_area,
    );
}

fn render_remote_add_menu(f: &mut Frame, cursor: usize, area: Rect) {
    let choices = RemoteEditKind::all();
    let width: u16 = 22;
    let height: u16 = (choices.len() as u16) + 4; // border(2) + title row + items + hint
    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    safe_render_widget(f, Clear, popup);
    let block = Block::default()
        .title(Span::styled(
            " Add Connection ",
            Style::default()
                .fg(CLR_MENU_BAR_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER).bg(CLR_MENU_DD_BG))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    for (i, kind) in choices.iter().enumerate() {
        let (r, g, b) = kind.color_rgb();
        let label = kind.name();
        let row = Rect { x: inner.x, y: inner.y + i as u16, width: inner.width, height: 1 };
        let selected = i == cursor;
        let text = if selected {
            format!(" ► {:<16}", label)
        } else {
            format!("   {:<16}", label)
        };
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(r, g, b))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Rgb(r, g, b))
                .bg(CLR_MENU_DD_BG)
        };
        safe_render_widget(f, Paragraph::new(text).style(style), row);
    }

    // hint row
    let hint_row = Rect {
        x: inner.x,
        y: inner.y + choices.len() as u16,
        width: inner.width,
        height: 1,
    };
    safe_render_widget(
        f,
        Paragraph::new(" ↑↓:Select  Enter:OK  Esc ")
            .style(Style::default().fg(CLR_BUTTON_FG).bg(CLR_STATUS_BG)),
        hint_row,
    );
}

fn render_remote_edit(f: &mut Frame, state: &RemoteEditState, area: Rect) {
    let width = 72u16.min(area.width.saturating_sub(4));
    let height = 14u16.min(area.height.saturating_sub(2)).max(10);
    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    safe_render_widget(f, Clear, popup);
    let block = Block::default()
        .title(Span::styled(
            state.kind.title(),
            Style::default()
                .fg(CLR_MENU_BAR_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER).bg(CLR_MENU_DD_BG))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    let labels = state.kind.field_labels();
    let value_w = (inner.width as usize).saturating_sub(9);
    let mut lines = Vec::new();
    for (idx, label) in labels.iter().enumerate() {
        let selected = state.cursor == idx;
        // Label: always dark background; arrow prefix on selected row
        let label_style = Style::default()
            .fg(CLR_HEADER_FG)
            .bg(CLR_MENU_DD_BG)
            .add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        let prefix = if selected { ">" } else { " " };
        // Active input field: white bg / black fg so the terminal cursor is clearly visible
        let value_style = if selected {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{}{:<8}", prefix, format!("{label}:")), label_style),
            Span::styled(
                format!("{:<width$}", state.fields[idx], width = value_w),
                value_style,
            ),
        ]));
    }
    lines.push(Line::default());
    let save_style = if state.cursor == RemoteEditState::SAVE {
        Style::default()
            .fg(CLR_MENU_SEL_FG)
            .bg(CLR_MENU_SEL_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
    };
    let cancel_style = if state.cursor == RemoteEditState::CANCEL {
        Style::default()
            .fg(CLR_MENU_SEL_FG)
            .bg(CLR_MENU_SEL_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
    };
    lines.push(Line::from(vec![
        Span::styled(" [ Save ] ", save_style),
        Span::raw("  "),
        Span::styled(" [ Cancel ] ", cancel_style),
    ]));
    lines.push(Line::default());
    let hint_text = if matches!(state.kind, crate::app::RemoteEditKind::Smb)
        && state.cursor == RemoteEditState::PATH
        && state.share_picker.is_none()
    {
        " Tab:Next  F5:Browse shares  Esc:Cancel "
    } else {
        " Tab/Shift-Tab:Next  Enter:Select  Esc:Cancel "
    };
    lines.push(Line::from(Span::styled(
        hint_text,
        Style::default().fg(CLR_UNKNOWN).bg(CLR_MENU_DD_BG),
    )));
    safe_render_widget(
        f,
        Paragraph::new(lines).style(Style::default().bg(CLR_MENU_DD_BG)),
        inner,
    );
    if state.cursor < 6 {
        let cursor_x =
            (inner.x + 9 + state.input_cursor as u16).min(inner.x + inner.width.saturating_sub(2));
        let cursor_y = inner.y + state.cursor as u16;
        safe_set_cursor_position(f, cursor_x, cursor_y);
    }

    // ── SMB share picker dropdown ─────────────────────────────────────────
    if let Some((ref shares, picker_cur)) = state.share_picker {
        // Anchor: Share field is at cursor row PATH (4); dropdown sits below it.
        const PATH_ROW: u16 = crate::app::RemoteEditState::PATH as u16;
        let dd_x = inner.x + 9;
        let dd_y = inner.y + PATH_ROW + 1;
        let dd_w = inner.width.saturating_sub(9).min(40).max(16);
        let max_visible: usize = 8;
        let visible = shares.len().min(max_visible);
        let dd_h = (visible as u16 + 2).min(area.height.saturating_sub(dd_y));

        let dd_area = clamp_rect(
            area,
            Rect { x: dd_x, y: dd_y, width: dd_w, height: dd_h },
        );
        safe_render_widget(f, Clear, dd_area);
        let dd_block = Block::default()
            .title(Span::styled(" Shares ", Style::default().fg(CLR_MENU_BAR_FG).bg(CLR_MENU_DD_BG)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(CLR_QS_BORDER).bg(CLR_MENU_DD_BG))
            .style(Style::default().bg(CLR_MENU_DD_BG));
        let dd_inner = dd_block.inner(dd_area);
        safe_render_widget(f, dd_block, dd_area);

        let scroll = if picker_cur >= max_visible {
            picker_cur - max_visible + 1
        } else {
            0
        };
        for (row, idx) in (scroll..shares.len()).take(dd_inner.height as usize).enumerate() {
            let selected = idx == picker_cur;
            let (fg, bg) = if selected {
                (CLR_MENU_SEL_FG, CLR_MENU_SEL_BG)
            } else {
                (CLR_MENU_DD_FG, CLR_MENU_DD_BG)
            };
            let marker = if selected { "▶ " } else { "  " };
            let name = truncate_str(&shares[idx], dd_inner.width.saturating_sub(2) as usize);
            let padded = format!("{}{:<width$}", marker, name, width = dd_inner.width.saturating_sub(2) as usize);
            safe_render_widget(
                f,
                Paragraph::new(padded).style(Style::default().fg(fg).bg(bg)),
                Rect { x: dd_inner.x, y: dd_inner.y + row as u16, width: dd_inner.width, height: 1 },
            );
        }
    }
}

fn render_remote_connecting(f: &mut Frame, state: &RemoteConnectingState, area: Rect) {
    let width = 46u16.min(area.width.saturating_sub(4)).max(30);
    let height = 7u16.min(area.height.saturating_sub(2)).max(6);
    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    safe_render_widget(f, Clear, popup);
    let block = Block::default()
        .title(Span::styled(
            " Connecting ",
            Style::default()
                .fg(CLR_MENU_BAR_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER).bg(CLR_MENU_DD_BG))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    let lines = vec![
        Line::from(Span::styled(
            format!(" {} connection in progress", state.protocol_label),
            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG),
        )),
        Line::from(Span::styled(
            format!(" {}", state.profile_name),
            Style::default()
                .fg(CLR_HEADER_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(
            " Please wait... ",
            Style::default().fg(CLR_TEXT).bg(CLR_MENU_DD_BG),
        )),
        Line::from(Span::styled(
            format!(" {}", state.phase),
            Style::default().fg(CLR_HEADER_FG).bg(CLR_MENU_DD_BG),
        )),
        Line::default(),
        Line::from(Span::styled(
            " Esc/Enter/F10:Abort ",
            Style::default().fg(CLR_UNKNOWN).bg(CLR_MENU_DD_BG),
        )),
    ];
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(CLR_MENU_DD_BG)),
        inner,
    );
}

// ---------------------------------------------------------------------------
// Search panel
// ---------------------------------------------------------------------------

fn render_search(f: &mut Frame, state: &SearchState, area: Rect) {
    // --- popup geometry ---------------------------------------------------
    let width = 100u16.min(area.width.saturating_sub(2));
    let height = (area.height * 4 / 5).clamp(18, area.height.saturating_sub(2));
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width,
            height,
        },
    );

    f.render_widget(Clear, popup);

    // Outer frame
    let backend_label = state.backend.label();
    let title = format!(" \u{1f50d} Search  [{backend_label}] ");
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(CLR_HEADER_FG)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(CLR_PANEL_BORDER))
        .style(Style::default().bg(Color::Rgb(18, 18, 24)));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // --- input section: 1 blank + 3 fields + 1 blank = 5 rows ------------
    let input_h = 5u16.min(inner.height);
    let results_area = Rect {
        x: inner.x,
        y: inner.y + input_h,
        width: inner.width,
        height: inner.height.saturating_sub(input_h + 1),
    };
    let hint_area = clamp_rect(
        area,
        Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        },
    );

    // Label column is 12 chars ("▶ Content : "), input fills the rest
    let label_w = 12usize;
    let iw = inner.width.saturating_sub(label_w as u16 + 4) as usize;

    let clr_label = Color::Rgb(120, 140, 180);
    let clr_active_label = CLR_HEADER_FG;
    let clr_input_bg_active = Color::Rgb(30, 40, 60);
    let clr_input_bg_idle = Color::Rgb(22, 22, 30);
    let clr_input_fg = Color::Rgb(230, 225, 210);
    let clr_placeholder = Color::Rgb(80, 90, 110);

    let (lbl0, bg0) = if state.input_field == 0 {
        (clr_active_label, clr_input_bg_active)
    } else {
        (clr_label, clr_input_bg_idle)
    };
    let (lbl1, bg1) = if state.input_field == 1 {
        (clr_active_label, clr_input_bg_active)
    } else {
        (clr_label, clr_input_bg_idle)
    };
    let (lbl2, bg2) = if state.input_field == 2 {
        (clr_active_label, clr_input_bg_active)
    } else {
        (clr_label, clr_input_bg_idle)
    };

    // Build field strings with trailing cursor '_' when focused
    let make_field = |text: &str, focused: bool, placeholder_star: bool| -> (String, Color) {
        let cursor = if focused { "_" } else { "" };
        let avail = iw.saturating_sub(cursor.len());
        if text.is_empty() && !focused && !placeholder_star {
            (format!("{:<w$}", "", w = iw), clr_placeholder)
        } else if placeholder_star && text == "*" && !focused {
            // Show '*' in dim color as default
            (
                format!("*{:<w$}", cursor, w = avail.saturating_sub(1)),
                clr_placeholder,
            )
        } else {
            let displayed = truncate_str(text, avail);
            (
                format!(
                    "{displayed}{cursor}{:<w$}",
                    "",
                    w = avail.saturating_sub(displayed.len())
                ),
                clr_input_fg,
            )
        }
    };

    let (pat_str, pat_fg) = make_field(&state.query, state.input_field == 0, true);
    let (cnt_str, cnt_fg) = make_field(&state.content_query, state.input_field == 1, false);
    let (dir_str, dir_fg) = make_field(&state.dir_query, state.input_field == 2, false);

    let input_lines = vec![
        Line::default(),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "\u{25b6} Name   : ",
                Style::default().fg(lbl0).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {pat_str} "), Style::default().fg(pat_fg).bg(bg0)),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "\u{25b6} Content: ",
                Style::default().fg(lbl1).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {cnt_str} "), Style::default().fg(cnt_fg).bg(bg1)),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "\u{25b6} Dir    : ",
                Style::default().fg(lbl2).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {dir_str} "), Style::default().fg(dir_fg).bg(bg2)),
        ]),
        Line::default(),
    ];

    safe_render_widget(
        f,
        Paragraph::new(input_lines).style(Style::default().bg(Color::Rgb(18, 18, 24))),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: input_h,
        },
    );

    // --- separator with result count --------------------------------------
    let result_count = state.results.len();
    let suffix = if result_count >= 1000 {
        " (limit 1000)"
    } else {
        ""
    };
    let sep_title = if state.running {
        if result_count > 0 {
            format!(" Searching...  {result_count} found so far  [Esc to cancel] ")
        } else {
            " Searching...  [Esc to cancel] ".to_string()
        }
    } else if result_count > 0 {
        format!(" {result_count} result(s){suffix} ")
    } else {
        " No results - press Enter to search ".to_string()
    };
    let sep_block = Block::default()
        .title(Span::styled(
            sep_title,
            Style::default().fg(if state.running {
                Color::Rgb(220, 180, 80)
            } else {
                Color::Rgb(160, 170, 200)
            }),
        ))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::Rgb(60, 70, 100)));
    let results_body = sep_block.inner(results_area);
    safe_render_widget(f, sep_block, results_area);

    if results_body.height == 0 {
        return;
    }

    // --- column header row ------------------------------------------------
    let date_w = 14usize;
    let size_w = 9usize;
    let name_w = 26usize;
    let dir_w = (results_body.width as usize).saturating_sub(name_w + size_w + date_w + 4);

    let header_area = Rect {
        x: results_body.x,
        y: results_body.y,
        width: results_body.width,
        height: 1,
    };
    let result_inner = Rect {
        x: results_body.x,
        y: results_body.y + 1,
        width: results_body.width,
        height: results_body.height.saturating_sub(1),
    };
    let clr_hdr = Color::Rgb(100, 110, 140);
    safe_render_widget(
        f,
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {:<name_w$}", "Name"),
                Style::default().fg(clr_hdr).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().fg(clr_hdr)),
            Span::styled(
                format!("{:<dir_w$}", "Directory"),
                Style::default().fg(clr_hdr).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {:>size_w$}", "Size"),
                Style::default().fg(clr_hdr).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {:>date_w$}", "Modified"),
                Style::default().fg(clr_hdr).add_modifier(Modifier::BOLD),
            ),
        ]))
        .style(Style::default().bg(Color::Rgb(24, 26, 36))),
        header_area,
    );

    // --- result list ------------------------------------------------------
    let visible_h = result_inner.height as usize;
    let scroll = {
        let mut s = state.scroll;
        if state.cursor < s {
            s = state.cursor;
        } else if state.cursor >= s + visible_h {
            s = state.cursor + 1 - visible_h;
        }
        s
    };

    let is_results_focused = state.input_field == 3;

    let items: Vec<ListItem> = state
        .results
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_h)
        .map(|(i, r)| {
            let is_cursor = i == state.cursor && is_results_focused;
            // Zebra: alternating row backgrounds
            let zebra_bg = if (i % 2) == 0 {
                Color::Rgb(18, 18, 24)
            } else {
                Color::Rgb(22, 22, 32)
            };

            let row_bg = if is_cursor { CLR_CURSOR_BG } else { zebra_bg };

            let file_name = r
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let dir = r
                .path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();

            let ext = r
                .path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let name_clr = if is_cursor {
                Color::Black
            } else {
                match ext.as_str() {
                    "rs" | "c" | "h" | "cpp" | "py" | "js" | "ts" => CLR_SOURCE,
                    "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "lha" | "lzh" => {
                        CLR_ARCHIVE
                    }
                    "mp3" | "flac" | "ogg" | "wav" | "mod" | "xm" | "s3m" => CLR_AUDIO,
                    "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" => CLR_IMAGE,
                    "mp4" | "mkv" | "avi" | "mov" => CLR_VIDEO,
                    "pdf" | "doc" | "docx" | "odt" => CLR_DOC,
                    "json" | "toml" | "yaml" | "xml" | "csv" => CLR_DATA,
                    "txt" | "md" | "rst" => CLR_TEXT,
                    _ => Color::Rgb(190, 185, 175),
                }
            };

            let dir_clr = if is_cursor {
                Color::Black
            } else {
                Color::Rgb(100, 110, 130)
            };
            let size_clr = if is_cursor { Color::Black } else { CLR_DATA };
            let date_clr = if is_cursor {
                Color::Black
            } else {
                Color::Rgb(130, 140, 160)
            };

            let name_str = truncate_str(&file_name, name_w);
            let dir_str = truncate_str(&dir, dir_w);
            let size_str = format!("{:>width$}", format_size(r.size), width = size_w);
            let date_str = r
                .modified
                .map(|ts| {
                    let dt: DateTime<Local> = ts.into();
                    dt.format("%Y-%m-%d %H:%M").to_string()
                })
                .unwrap_or_default();
            let date_str = format!("{:>width$}", date_str, width = date_w);

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {name_str:<name_w$}"),
                    Style::default().fg(name_clr).bg(row_bg),
                ),
                Span::styled(" ", Style::default().bg(row_bg)),
                Span::styled(
                    format!("{dir_str:<dir_w$}"),
                    Style::default().fg(dir_clr).bg(row_bg),
                ),
                Span::styled(
                    format!(" {size_str}"),
                    Style::default().fg(size_clr).bg(row_bg),
                ),
                Span::styled(
                    format!(" {date_str}"),
                    Style::default().fg(date_clr).bg(row_bg),
                ),
            ]))
        })
        .collect();

    safe_render_widget(
        f,
        List::new(items).style(Style::default().bg(Color::Rgb(18, 18, 24))),
        result_inner,
    );

    // Scrollbar
    if result_count > visible_h {
        let mut sb_state = ScrollbarState::new(result_count).position(state.cursor);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(Color::Rgb(60, 70, 100))),
            result_inner,
            &mut sb_state,
        );
    }

    // --- hint bar ---------------------------------------------------------
    let hint = match state.input_field {
        3 => {
            " \u{23ce}:Go to file   \u{2191}\u{2193} PgUp PgDn:Navigate   Tab:Fields   F5:Backend   Esc:Close "
        }
        2 => " \u{23ce}:Search   Tab:Switch field   Del:Reset dir   F5:Backend   Esc:Close ",
        _ => {
            " \u{23ce}:Search   Tab:Switch field   \u{2193}:Results   F5:Backend   Del:Reset   Esc:Close "
        }
    };
    safe_render_widget(
        f,
        Paragraph::new(Span::styled(
            hint,
            Style::default().fg(Color::Rgb(100, 110, 140)),
        ))
        .style(Style::default().bg(Color::Rgb(24, 24, 32))),
        hint_area,
    );
}

// ---------------------------------------------------------------------------
// Directory history
// ---------------------------------------------------------------------------

fn render_dir_bookmarks(f: &mut Frame, app: &App, area: Rect) {
    let list_h = app.bookmarks.len().max(3) as u16;
    // 2 border + input + separator + hint + list
    let height = (list_h + 5).min(area.height.saturating_sub(4)).max(8);
    let width = 64u16.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width,
            height,
        },
    );

    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .title(" Bookmarks ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    let input_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let sep_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: 1,
    };
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height - 1,
        width: inner.width,
        height: 1,
    };

    let matches = app.filtered_bookmark_items();
    let total = matches.len();
    let count_hint = if app.bookmark_query.is_empty() {
        format!(" {} ", app.bookmarks.len())
    } else if total > 0 {
        format!(" {}/{} ", app.bookmark_match_pos + 1, total)
    } else {
        " 0/0 ".to_owned()
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" ⌕ {}\u{2581}", app.bookmark_query);
    let input_row = Line::from(vec![
        Span::styled(
            truncate_str(&input_text, input_inner_w),
            Style::default().fg(CLR_QS_INPUT_FG).bg(CLR_QS_INPUT_BG),
        ),
        Span::styled(
            count_hint,
            Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_INPUT_BG),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(CLR_QS_INPUT_BG)),
        input_area,
    );

    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_MENU_DD_BG)),
        sep_area,
    );

    let max_w = list_area.width as usize;
    let rows = list_area.height as usize;
    let tokens: Vec<String> = app
        .bookmark_query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();
    let scroll = if app.bookmark_match_pos >= rows && rows > 0 {
        app.bookmark_match_pos - rows + 1
    } else {
        0
    };

    let items: Vec<ListItem> = if app.bookmarks.is_empty() && matches.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "(no bookmarks)",
            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG),
        )))]
    } else if matches.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            " No matching bookmark ",
            Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_MENU_DD_BG),
        )))]
    } else {
        matches
            .iter()
            .enumerate()
            .skip(scroll)
            .take(rows)
            .map(|(match_idx, item)| {
                let selected = match_idx == app.bookmark_match_pos;
                let (label, style) = match item {
                    BookmarkListItem::AddCurrentDir(path) => {
                        let path = truncate_str(&path.to_string_lossy(), max_w.saturating_sub(20));
                        let label = format!(" <add current dir> {}", path);
                        let style = if selected {
                            Style::default()
                                .fg(CLR_MENU_SEL_FG)
                                .bg(CLR_MENU_SEL_BG)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                                .fg(CLR_HEADER_FG)
                                .bg(CLR_MENU_DD_BG)
                                .add_modifier(Modifier::BOLD)
                        };
                        (label, style)
                    }
                    BookmarkListItem::Existing(idx) => {
                        let p = &app.bookmarks[*idx];
                        let s = p.to_string_lossy();
                        let is_remote = s.starts_with("remote://");
                        let label = if is_remote {
                            let rest = &s["remote://".len()..];
                            format!(
                                " \u{2039}remote\u{203a} {}",
                                truncate_str(rest, max_w.saturating_sub(11))
                            )
                        } else {
                            format!(" {}", truncate_str(&s, max_w.saturating_sub(1)))
                        };
                        let style = if selected {
                            Style::default()
                                .fg(CLR_MENU_SEL_FG)
                                .bg(CLR_MENU_SEL_BG)
                                .add_modifier(Modifier::BOLD)
                        } else if !is_remote && !p.is_dir() {
                            Style::default()
                                .fg(CLR_MENU_DD_FG)
                                .bg(CLR_MENU_DD_BG)
                                .add_modifier(Modifier::DIM)
                        } else {
                            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
                        };
                        (label, style)
                    }
                };
                let hi = if selected {
                    CLR_QS_MATCH_HI_SEL
                } else {
                    CLR_QS_MATCH_HI
                };
                ListItem::new(highlight_tokens(
                    &label,
                    &tokens,
                    style.fg.unwrap_or(CLR_MENU_DD_FG),
                    style.bg.unwrap_or(CLR_MENU_DD_BG),
                    hi,
                ))
            })
            .collect()
    };
    safe_render_widget(
        f,
        List::new(items).style(Style::default().bg(CLR_MENU_DD_BG)),
        list_area,
    );

    // Hint
    let key_style = Style::default()
        .fg(CLR_HEADER_FG)
        .bg(CLR_MENU_DD_BG)
        .add_modifier(Modifier::BOLD);
    let txt_style = Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG);
    safe_render_widget(
        f,
        Paragraph::new(Line::from(vec![
            Span::styled(" Type", key_style),
            Span::styled(":Filter  ", txt_style),
            Span::styled(" Enter", key_style),
            Span::styled(":Open/Add  ", txt_style),
            Span::styled("Del", key_style),
            Span::styled(":Remove  ", txt_style),
            Span::styled("Esc", key_style),
            Span::styled(":Cancel", txt_style),
        ]))
        .style(Style::default().bg(CLR_MENU_DD_BG)),
        hint_area,
    );
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Quick-palette (VSCode style)
// ---------------------------------------------------------------------------

/// Build a `Line` with each whitespace-separated token highlighted in the name.
fn highlight_tokens(
    name: &str,
    tokens: &[String],
    base_fg: Color,
    base_bg: Color,
    hi_fg: Color,
) -> Line<'static> {
    // Build a boolean mask: which byte positions are highlighted
    let name_lower = name.to_lowercase();
    let mut mask = vec![false; name.len()];
    for token in tokens {
        if token.is_empty() {
            continue;
        }
        let mut search_from = 0;
        while search_from < name_lower.len() {
            if let Some(pos) = name_lower[search_from..].find(token.as_str()) {
                let abs = search_from + pos;
                let end = abs + token.len();
                for b in abs..end.min(mask.len()) {
                    mask[b] = true;
                }
                search_from = abs + 1;
            } else {
                break;
            }
        }
    }

    // Walk the name char by char, grouping consecutive same-style chars into spans
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut seg_start = 0;
    let mut current_hi = mask.first().copied().unwrap_or(false);
    let base = Style::default().fg(base_fg).bg(base_bg);
    let hi = Style::default()
        .fg(hi_fg)
        .bg(base_bg)
        .add_modifier(Modifier::BOLD);

    for (byte_pos, ch) in name.char_indices() {
        let this_hi = mask[byte_pos];
        if this_hi != current_hi {
            let slice: String = name[seg_start..byte_pos].to_owned();
            spans.push(Span::styled(slice, if current_hi { hi } else { base }));
            seg_start = byte_pos;
            current_hi = this_hi;
        }
        let _ = ch; // consumed by char_indices
    }
    // Push the last segment
    let tail: String = name[seg_start..].to_owned();
    spans.push(Span::styled(tail, if current_hi { hi } else { base }));

    Line::from(spans)
}

fn render_quicksearch_palette(f: &mut Frame, app: &App, area: Rect) {
    let panel = app.active_panel();
    let query = &panel.quicksearch;
    let matches = panel.quicksearch_matches();
    let qs_pos = panel.qs_match_pos;
    let total = matches.len();

    // Dimensions: 62% wide, near the top (like VSCode)
    let palette_w = ((area.width as u32 * 62 / 100) as u16)
        .max(44)
        .min(area.width.saturating_sub(4));
    let visible_items = (total as u16).min(14);
    // input row + separator + items (at least 3 for aesthetics) + borders
    let palette_h = (1 + 1 + visible_items.max(3) + 2).min(area.height.saturating_sub(3));

    let x = (area.width.saturating_sub(palette_w)) / 2 + area.x;
    let y = area.y + 2;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width: palette_w,
            height: palette_h,
        },
    );

    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_QS_BORDER))
        .style(Style::default().bg(CLR_QS_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    if inner.height < 2 {
        return;
    }

    // ── input field ────────────────────────────────────────────────────────
    let input_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let sep_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: 1,
    };
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };

    // counter hint on the right side of the input (e.g. "3/47")
    let count_hint = if !query.is_empty() && total > 0 {
        format!(" {}/{} ", qs_pos + 1, total)
    } else if !query.is_empty() {
        " 0/0 ".to_owned()
    } else {
        String::new()
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" \u{2315} {}\u{2581}", query);
    let input_row = Line::from(vec![
        Span::styled(
            truncate_str(&input_text, input_inner_w),
            Style::default().fg(CLR_QS_INPUT_FG).bg(CLR_QS_INPUT_BG),
        ),
        Span::styled(
            count_hint,
            Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_INPUT_BG),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(CLR_QS_INPUT_BG)),
        input_area,
    );

    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        sep_area,
    );

    // ── match list ─────────────────────────────────────────────────────────
    if total == 0 && !query.is_empty() {
        safe_render_widget(
            f,
            Paragraph::new(" No match").style(Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_BG)),
            list_area,
        );
        return;
    }

    let list_h = list_area.height as usize;

    // Keep qs_pos inside the visible window
    let scroll: usize = if qs_pos >= list_h {
        qs_pos - list_h + 1
    } else {
        0
    };

    let tokens: Vec<String> = query.split_whitespace().map(|t| t.to_lowercase()).collect();
    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(list_h)
        .map(|(match_idx, &entry_idx)| {
            let entry = &panel.entries[entry_idx];
            let is_sel = match_idx == qs_pos;
            let (bg, fg, hi) = if is_sel {
                (CLR_QS_SEL_BG, CLR_QS_SEL_FG, CLR_QS_MATCH_HI_SEL)
            } else if entry.is_dir {
                (CLR_QS_BG, CLR_QS_DIR_FG, CLR_QS_MATCH_HI)
            } else {
                (CLR_QS_BG, CLR_QS_LIST_FG, CLR_QS_MATCH_HI)
            };

            let icon = if entry.is_dir { " \u{25b6} " } else { "   " };
            let name_line = highlight_tokens(&entry.name, &tokens, fg, bg, hi);
            let icon_span = vec![Span::styled(icon, Style::default().fg(fg).bg(bg))];
            let mut all_spans = icon_span;
            all_spans.extend(name_line.spans);

            ListItem::new(Line::from(all_spans))
        })
        .collect();

    // Reserve the rightmost column for the scrollbar when the list overflows
    let (render_area, sb_area) = if total > list_h {
        let list_w = list_area.width.saturating_sub(1);
        (
            Rect {
                width: list_w,
                ..list_area
            },
            Some(Rect {
                x: list_area.x + list_w,
                y: list_area.y,
                width: 1,
                height: list_area.height,
            }),
        )
    } else {
        (list_area, None)
    };

    safe_render_widget(
        f,
        List::new(items).style(Style::default().bg(CLR_QS_BG)),
        render_area,
    );

    if let Some(sb) = sb_area {
        let mut sb_state = ScrollbarState::new(total).position(scroll);
        safe_render_stateful_widget(
            f,
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(CLR_QS_BORDER))
                .track_style(Style::default().bg(CLR_QS_BG))
                .begin_symbol(None)
                .end_symbol(None),
            sb,
            &mut sb_state,
        );
    }
}

fn render_viewer_plugin_palette(f: &mut Frame, state: &ViewerPluginPaletteState, area: Rect) {
    let query = &state.query;
    let matches = state.filtered_indices();
    let qs_pos = state.match_pos;
    let total = matches.len();

    let palette_w = ((area.width as u32 * 62 / 100) as u16)
        .max(50)
        .min(area.width.saturating_sub(4));
    let visible_items = (total as u16).min(14);
    let palette_h = (1 + 1 + visible_items.max(3) + 2).min(area.height.saturating_sub(3));

    let popup = clamp_rect(
        area,
        Rect {
            x: (area.width.saturating_sub(palette_w)) / 2 + area.x,
            y: area.y + 2,
            width: palette_w,
            height: palette_h,
        },
    );

    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .title(" Viewer Plugins ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_QS_BORDER))
        .style(Style::default().bg(CLR_QS_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    if inner.height < 2 {
        return;
    }

    let input_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let sep_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: 1,
    };
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };

    let count_hint = if !query.is_empty() && total > 0 {
        format!(" {}/{} ", qs_pos + 1, total)
    } else if !query.is_empty() {
        " 0/0 ".to_owned()
    } else {
        format!(" {} ", state.items.len())
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" \u{2315} {}\u{2581}", query);
    let input_row = Line::from(vec![
        Span::styled(
            truncate_str(&input_text, input_inner_w),
            Style::default().fg(CLR_QS_INPUT_FG).bg(CLR_QS_INPUT_BG),
        ),
        Span::styled(
            count_hint,
            Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_INPUT_BG),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(CLR_QS_INPUT_BG)),
        input_area,
    );

    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        sep_area,
    );

    if total == 0 {
        let message = if query.is_empty() {
            " No viewer plugin"
        } else {
            " No match"
        };
        safe_render_widget(
            f,
            Paragraph::new(message).style(Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_BG)),
            list_area,
        );
        return;
    }

    let list_h = list_area.height as usize;
    let scroll = if qs_pos >= list_h {
        qs_pos - list_h + 1
    } else {
        0
    };
    let tokens: Vec<String> = query.split_whitespace().map(|t| t.to_lowercase()).collect();

    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(list_h)
        .map(|(match_idx, &plugin_idx)| {
            let plugin = &state.items[plugin_idx];
            let is_sel = match_idx == qs_pos;
            let (bg, fg, hi) = if is_sel {
                (CLR_QS_SEL_BG, CLR_QS_SEL_FG, CLR_QS_MATCH_HI_SEL)
            } else {
                (CLR_QS_BG, CLR_QS_LIST_FG, CLR_QS_MATCH_HI)
            };

            let mut spans = vec![Span::styled("   ", Style::default().fg(fg).bg(bg))];
            let name = highlight_tokens(&plugin.name, &tokens, fg, bg, hi);
            spans.extend(name.spans);
            if !plugin.description.is_empty() {
                spans.push(Span::styled("  ", Style::default().fg(fg).bg(bg)));
                spans.push(Span::styled(
                    truncate_str(&plugin.description, 42),
                    Style::default().fg(Color::Gray).bg(bg),
                ));
            }
            if !plugin.extensions.is_empty() {
                spans.push(Span::styled("  ", Style::default().fg(fg).bg(bg)));
                spans.push(Span::styled(
                    plugin.extensions.join(","),
                    Style::default().fg(Color::DarkGray).bg(bg),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let (render_area, sb_area) = if total > list_h {
        let list_w = list_area.width.saturating_sub(1);
        (
            Rect {
                width: list_w,
                ..list_area
            },
            Some(Rect {
                x: list_area.x + list_w,
                y: list_area.y,
                width: 1,
                height: list_area.height,
            }),
        )
    } else {
        (list_area, None)
    };

    safe_render_widget(
        f,
        List::new(items).style(Style::default().bg(CLR_QS_BG)),
        render_area,
    );

    if let Some(sb) = sb_area {
        let mut sb_state = ScrollbarState::new(total).position(scroll);
        safe_render_stateful_widget(
            f,
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(CLR_QS_BORDER))
                .track_style(Style::default().bg(CLR_QS_BG))
                .begin_symbol(None)
                .end_symbol(None),
            sb,
            &mut sb_state,
        );
    }
}

// ---------------------------------------------------------------------------
// Help overlay
// ---------------------------------------------------------------------------

fn render_help(f: &mut Frame, state: &crate::help::HelpState, area: Rect) {
    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        },
    );

    safe_render_widget(f, Clear, popup);
    let title = match state.view {
        HelpView::Index { .. } => " KKC-DOS Help Index ",
        HelpView::Topics { section, .. } => {
            let title = &state.system.sections[section].title;
            return render_help_with_title(f, popup, title, state);
        }
        HelpView::Page { topic, .. } => {
            let title = &state.system.topics[topic].title;
            return render_help_with_title(f, popup, title, state);
        }
    };
    render_help_with_title(f, popup, title, state);
}

fn render_help_with_title(f: &mut Frame, popup: Rect, title: &str, state: &crate::help::HelpState) {
    let block = Block::default()
        .title(format!(" {} ", title))
        .title_bottom(
            Line::from(Span::styled(
                format!(" {} ", state.hlp_path),
                Style::default().fg(Color::DarkGray).bg(CLR_APP_BG),
            ))
            .right_aligned(),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER).bg(CLR_APP_BG))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    if inner.height < 3 {
        return;
    }

    let body = clamp_rect(
        popup,
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(1),
        },
    );
    let footer = clamp_rect(
        popup,
        Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        },
    );

    match state.view {
        HelpView::Index { cursor } => {
            let items: Vec<ListItem> = state
                .system
                .sections
                .iter()
                .enumerate()
                .map(|(idx, section)| {
                    let style = if idx == cursor {
                        Style::default()
                            .fg(Color::Black)
                            .bg(CLR_SELECTED)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(Span::styled(
                        format!(" {}", section.title),
                        style,
                    )))
                })
                .collect();
            safe_render_widget(f, List::new(items), body);
            safe_render_widget(
                f,
                Paragraph::new(" Esc/F10:Close  Enter:Open topic group ")
                    .style(Style::default().fg(Color::DarkGray)),
                footer,
            );
        }
        HelpView::Topics { section, cursor } => {
            let items: Vec<ListItem> = state.system.sections[section]
                .topics
                .iter()
                .enumerate()
                .map(|(idx, topic_idx)| {
                    let style = if idx == cursor {
                        Style::default()
                            .fg(Color::Black)
                            .bg(CLR_SELECTED)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(Span::styled(
                        format!(" {}", state.system.topics[*topic_idx].title),
                        style,
                    )))
                })
                .collect();
            safe_render_widget(f, List::new(items), body);
            safe_render_widget(
                f,
                Paragraph::new(" Esc:Close  Backspace:Back  Enter:Open page ")
                    .style(Style::default().fg(Color::DarkGray)),
                footer,
            );
        }
        HelpView::Page {
            topic,
            scroll,
            selected_link,
        } => {
            let topic = &state.system.topics[topic];
            safe_render_widget(
                f,
                Paragraph::new(topic.to_render_lines(selected_link))
                    .style(Style::default().fg(Color::White))
                    .scroll((scroll, 0))
                    .wrap(Wrap { trim: false }),
                body,
            );
            safe_render_widget(
                f,
                Paragraph::new(" Esc:Close  Backspace:Back  Up/Down/PgUp/PgDn:Scroll  Tab:Next link  Enter:Open ")
                    .style(Style::default().fg(Color::DarkGray)),
                footer,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Utility
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
    if p.len() <= max {
        return p.to_string();
    }
    let trimmed = &p[p.len() - max.saturating_sub(3)..];
    format!("...{}", trimmed)
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

fn render_config(f: &mut Frame, cs: &ConfigState, area: Rect) {
    const W: u16 = 62;
    const H: u16 = 26;
    let x = area.x + (area.width.saturating_sub(W)) / 2;
    let y = area.y + (area.height.saturating_sub(H)) / 2;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width: W,
            height: H,
        },
    );

    // Shadow
    let sh = Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: W,
        height: H,
    };
    if sh.right() <= area.right() && sh.bottom() <= area.bottom() {
        safe_render_widget(
            f,
            Block::default().style(Style::default().bg(Color::Rgb(20, 15, 10))),
            sh,
        );
    }

    safe_render_widget(f, Clear, popup);

    // Outer box
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER).bg(CLR_APP_BG))
        .title(Span::styled(
            " Setup ",
            Style::default()
                .fg(CLR_BUTTON_FG)
                .bg(CLR_APP_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(CLR_APP_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    let iw = inner.width as usize;

    // ── Section header helper ──────────────────────────────────────────────
    let section_style = Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG);
    let render_section_hdr = |f: &mut Frame, row: u16, label: &str| {
        let y = inner.y + row;
        if y >= inner.y + inner.height {
            return;
        }
        let prefix = format!("  \u{2500} {} ", label);
        let fill_len = iw.saturating_sub(prefix.chars().count());
        let line = format!("{}{}", prefix, "\u{2500}".repeat(fill_len));
        safe_render_widget(
            f,
            Paragraph::new(line).style(section_style),
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            },
        );
    };

    // ── Checkboxes with section grouping ──────────────────────────────────
    // Layout (row → cursor index):
    //  Row  0: section "Behaviour"
    //  Row  1: idx  0 confirm_exit
    //  Row  2: idx  1 confirm_delete
    //  Row  3: idx  2 auto_reload
    //  Row  4: idx  3 insert_moves_down
    //  Row  5: idx  4 select_dirs
    //  Row  6: section "Display"
    //  Row  7: idx  5 show_hidden
    //  Row  8: idx  6 color_by_type
    //  Row  9: idx  7 show_fkey_bar
    //  Row 10: section "Viewer"
    //  Row 11: idx  8 word_wrap
    //  Row 12: idx  9 default_zoom
    //  Row 13: idx 10 debug_log
    //  Row 14: section "External"
    //  Row 15: Editor label
    //  Row 16: idx 11 editor field
    //  Row 17: Pager label
    //  Row 18: idx 12 pager field
    //  Row 19: History label
    //  Row 20: idx 13 dir_history_max field
    //  Row 21: separator
    //  Row 22: OK / Cancel

    let checkbox_items: &[(u16, &str, usize, bool)] = &[
        (1, "Confirm exit", 0, cs.confirm_exit),
        (2, "Confirm delete", 1, cs.confirm_delete),
        (3, "Auto reload", 2, cs.auto_reload),
        (4, "Insert moves down", 3, cs.insert_moves_down),
        (5, "Select directories", 4, cs.select_dirs),
        (7, "Show hidden files", 5, cs.show_hidden),
        (8, "Color by type", 6, cs.color_by_type),
        (9, "Show F-key bar", 7, cs.show_fkey_bar),
        (11, "Word wrap", 8, cs.word_wrap),
        (12, "Default zoom", 9, cs.default_zoom),
        (13, "Debug log", 10, cs.debug_log),
    ];

    for &(row, label, cursor_idx, val) in checkbox_items {
        let y = inner.y + row;
        if y >= inner.y + inner.height {
            continue;
        }
        let tick = if val { "X" } else { " " };
        let text = format!("  [{}] {}", tick, label);
        let selected = cs.cursor == cursor_idx;
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(CLR_CURSOR_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(50, 36, 22)).bg(CLR_APP_BG)
        };
        let padded = format!("{:<width$}", text, width = iw);
        safe_render_widget(
            f,
            Paragraph::new(padded).style(style),
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            },
        );
    }

    // Section headers
    render_section_hdr(f, 0, "Behaviour");
    render_section_hdr(f, 6, "Display");
    render_section_hdr(f, 10, "Viewer");
    render_section_hdr(f, 14, "External");

    // ── Text fields ────────────────────────────────────────────────────────
    // cursor indices: Editor=11, Pager=12, History max=13
    let text_layout: &[(&str, u16, usize, &str)] = &[
        ("Editor", 15, 11, cs.editor.as_str()),
        ("Pager", 17, 12, cs.pager.as_str()),
        ("History max", 19, 13, cs.dir_history_max.as_str()),
    ];

    for &(label, label_row, cursor_idx, value) in text_layout {
        let lbl_y = inner.y + label_row;
        if lbl_y < inner.y + inner.height {
            safe_render_widget(
                f,
                Paragraph::new(format!("  {}:", label))
                    .style(Style::default().fg(Color::Rgb(80, 60, 40)).bg(CLR_APP_BG)),
                Rect {
                    x: inner.x,
                    y: lbl_y,
                    width: inner.width,
                    height: 1,
                },
            );
        }
        let field_y = inner.y + label_row + 1;
        if field_y < inner.y + inner.height {
            let selected = cs.cursor == cursor_idx;
            let field_w = inner.width.saturating_sub(4);
            let input_bg = if selected {
                CLR_CURSOR_BG
            } else {
                Color::Rgb(160, 140, 115)
            };
            let input_fg = if selected {
                Color::Black
            } else {
                Color::Rgb(40, 28, 18)
            };
            let padded = format!("{:<width$}", value, width = field_w as usize);
            let display = if padded.len() > field_w as usize {
                padded[padded.len() - field_w as usize..].to_string()
            } else {
                padded
            };
            safe_render_widget(
                f,
                Paragraph::new(display).style(Style::default().fg(input_fg).bg(input_bg)),
                Rect {
                    x: inner.x + 2,
                    y: field_y,
                    width: field_w,
                    height: 1,
                },
            );
        }
    }

    // ── Bottom separator ───────────────────────────────────────────────────
    let bot_sep_y = inner.y + inner.height.saturating_sub(3);
    if bot_sep_y < inner.y + inner.height {
        let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
        safe_render_widget(
            f,
            Paragraph::new(sep).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
            Rect {
                x: inner.x,
                y: bot_sep_y,
                width: inner.width,
                height: 1,
            },
        );
    }

    // ── OK / Cancel buttons ────────────────────────────────────────────────
    let ok_idx = ConfigState::NUM_CHECKBOXES + 3; // 14
    let cancel_idx = ConfigState::NUM_CHECKBOXES + 3 + 1; // 15
    let btn_y = inner.y + inner.height.saturating_sub(2);
    let btn_w: u16 = 10;
    let gap: u16 = 4;
    let btn_x = inner.x + (inner.width.saturating_sub(btn_w * 2 + gap)) / 2;

    let ok_style = if cs.cursor == ok_idx {
        Style::default()
            .fg(Color::Black)
            .bg(CLR_PANEL_BORDER)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(80, 60, 40)).bg(CLR_APP_BG)
    };
    let cancel_style = if cs.cursor == cancel_idx {
        Style::default()
            .fg(Color::Black)
            .bg(CLR_PANEL_BORDER)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(80, 60, 40)).bg(CLR_APP_BG)
    };

    safe_render_widget(
        f,
        Paragraph::new("  [ OK ]  ").style(ok_style),
        Rect {
            x: btn_x,
            y: btn_y,
            width: btn_w,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new(" [Cancel] ").style(cancel_style),
        Rect {
            x: btn_x + btn_w + gap,
            y: btn_y,
            width: btn_w,
            height: 1,
        },
    );
}

// ---------------------------------------------------------------------------
// Opener picker
// ---------------------------------------------------------------------------

fn render_opener(f: &mut Frame, s: &OpenerState, area: Rect) {
    let w = 52u16;
    let h = (s.items.len() as u16 + 4).min(20).max(6);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width: w,
            height: h,
        },
    );

    // Shadow
    let sh = Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: w,
        height: h,
    };
    if sh.right() <= area.right() && sh.bottom() <= area.bottom() {
        safe_render_widget(
            f,
            Block::default().style(Style::default().bg(Color::Rgb(20, 15, 10))),
            sh,
        );
    }
    safe_render_widget(f, Clear, popup);

    let ext = s.path.extension().and_then(|e| e.to_str()).unwrap_or("?");
    let title = format!(" Open .{} ", ext);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER).bg(CLR_APP_BG))
        .title(Span::styled(
            title,
            Style::default()
                .fg(CLR_BUTTON_FG)
                .bg(CLR_APP_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(CLR_APP_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    // Hint row
    safe_render_widget(
        f,
        Paragraph::new("  ↑↓ select  Enter open  Esc cancel")
            .style(Style::default().fg(Color::Rgb(110, 88, 65)).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );
    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );

    // Item list
    for (i, cmd) in s.items.iter().enumerate() {
        let row = inner.y + 2 + i as u16;
        if row >= inner.y + inner.height {
            break;
        }
        let selected = s.cursor == i;
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(CLR_CURSOR_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(50, 36, 22)).bg(CLR_APP_BG)
        };
        let icon = if selected { " ▶ " } else { "   " };
        let text = format!("{}{}", icon, cmd);
        let padded = format!("{:<width$}", text, width = inner.width as usize);
        safe_render_widget(
            f,
            Paragraph::new(padded).style(style),
            Rect {
                x: inner.x,
                y: row,
                width: inner.width,
                height: 1,
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Association editor
// ---------------------------------------------------------------------------

// render_plugins lives in src/ui/plugins.rs

fn render_action_palette(f: &mut Frame, s: &ActionPaletteState, area: Rect) {
    let w: u16 = area.width.saturating_sub(4).min(100).max(60);
    let visible = (s.actions.len() as u16).min(12).max(4);
    let h: u16 = (visible + 6).min(area.height.saturating_sub(4)).max(8);
    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + 2,
            width: w,
            height: h,
        },
    );

    safe_render_widget(f, Clear, popup);
    let block = Block::default()
        .title(" Actions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_QS_BORDER))
        .style(Style::default().bg(CLR_QS_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    let dir = format!("  {}", s.cwd.display());
    safe_render_widget(
        f,
        Paragraph::new(truncate_str(&dir, inner.width as usize))
            .style(Style::default().fg(Color::DarkGray).bg(CLR_QS_BG)),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep.clone()).style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );

    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(4),
    };
    let list_h = list_area.height as usize;
    let start = if s.cursor >= list_h {
        s.cursor - list_h + 1
    } else {
        0
    };

    for (idx, action_idx) in (start..s.actions.len()).take(list_h).enumerate() {
        let action = &s.actions[action_idx];
        let selected = action_idx == s.cursor;
        let (fg, bg) = if selected {
            (CLR_QS_SEL_FG, CLR_QS_SEL_BG)
        } else {
            (CLR_QS_LIST_FG, CLR_QS_BG)
        };
        let marker = if selected { ">" } else { " " };
        let mut text = format!(" {} {}  {}", marker, action.title, action.description);
        if let Some(prompt) = &action.prompt {
            text.push_str("  ");
            text.push_str(prompt);
        }
        let padded = format!(
            "{:<width$}",
            truncate_str(&text, inner.width as usize),
            width = inner.width as usize
        );
        safe_render_widget(
            f,
            Paragraph::new(padded).style(Style::default().fg(fg).bg(bg)),
            Rect {
                x: list_area.x,
                y: list_area.y + idx as u16,
                width: list_area.width,
                height: 1,
            },
        );
    }

    let hint_y = inner.y + inner.height.saturating_sub(1);
    safe_render_widget(
        f,
        Paragraph::new("  Enter Run   Esc Close ")
            .style(Style::default().fg(CLR_BUTTON_FG).bg(CLR_BUTTON_BG)),
        Rect {
            x: inner.x,
            y: hint_y,
            width: inner.width,
            height: 1,
        },
    );
}

fn render_assoc_editor(f: &mut Frame, s: &AssocEditorState, area: Rect) {
    const W: u16 = 64;
    const H: u16 = 24;
    let x = area.x + (area.width.saturating_sub(W)) / 2;
    let y = area.y + (area.height.saturating_sub(H)) / 2;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width: W,
            height: H,
        },
    );

    // Shadow
    let sh = Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: W,
        height: H,
    };
    if sh.right() <= area.right() && sh.bottom() <= area.bottom() {
        safe_render_widget(
            f,
            Block::default().style(Style::default().bg(Color::Rgb(20, 15, 10))),
            sh,
        );
    }
    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER).bg(CLR_APP_BG))
        .title(Span::styled(
            " Associations ",
            Style::default()
                .fg(CLR_BUTTON_FG)
                .bg(CLR_APP_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(CLR_APP_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    // Column header
    let header = format!("  {:<8} {}", "Ext", "Openers");
    safe_render_widget(
        f,
        Paragraph::new(header).style(
            Style::default()
                .fg(CLR_HEADER_FG)
                .bg(CLR_HEADER_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );
    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep.clone()).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );

    // List rows
    let list_h = inner.height.saturating_sub(4) as usize; // header + sep + hint_sep + hint
    let start = if s.assocs.is_empty() || s.cursor < list_h {
        0
    } else {
        s.cursor.saturating_sub(list_h - 1)
    };

    if s.assocs.is_empty() {
        safe_render_widget(
            f,
            Paragraph::new("  (no associations defined)")
                .style(Style::default().fg(Color::Rgb(110, 88, 65)).bg(CLR_APP_BG)),
            Rect {
                x: inner.x,
                y: inner.y + 2,
                width: inner.width,
                height: 1,
            },
        );
    } else {
        for (list_row, idx) in (start..).zip(0..list_h) {
            if list_row >= s.assocs.len() {
                break;
            }
            let row_y = inner.y + 2 + idx as u16;
            if row_y >= inner.y + inner.height {
                break;
            }
            let (ext, openers) = &s.assocs[list_row];
            let selected = s.cursor == list_row;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(CLR_CURSOR_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(50, 36, 22)).bg(CLR_APP_BG)
            };
            let icon = if selected { "▶" } else { " " };
            let openers_str = openers.join(", ");
            let avail = inner.width.saturating_sub(12) as usize;
            let openers_disp = if openers_str.len() > avail {
                format!("{}…", &openers_str[..avail.saturating_sub(1)])
            } else {
                openers_str
            };
            let text = format!(" {} .{:<8} {}", icon, ext, openers_disp);
            let padded = format!("{:<width$}", text, width = inner.width as usize);
            safe_render_widget(
                f,
                Paragraph::new(padded).style(style),
                Rect {
                    x: inner.x,
                    y: row_y,
                    width: inner.width,
                    height: 1,
                },
            );
        }
    }

    // Bottom hint
    let hint_sep_y = inner.y + inner.height.saturating_sub(2);
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: hint_sep_y,
            width: inner.width,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new("  A/+ Add   Enter/E Edit   Del/D Delete   Esc Close")
            .style(Style::default().fg(Color::Rgb(110, 88, 65)).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: hint_sep_y + 1,
            width: inner.width,
            height: 1,
        },
    );
}

// ---------------------------------------------------------------------------
// Ctrl-U pseudo-terminal overlay
// ---------------------------------------------------------------------------

const CLR_TERM_BG: Color = Color::Rgb(10, 10, 10);
const CLR_TERM_FG: Color = Color::Rgb(200, 200, 200);
const CLR_TERM_BORDER: Color = Color::Rgb(80, 180, 80);
const CLR_TERM_PROMPT: Color = Color::Rgb(100, 220, 100);
const CLR_TERM_INPUT: Color = Color::White;

fn render_terminal(f: &mut Frame, app: &App, area: Rect) {
    let ts = &app.terminal;
    let running = app.running_cmd.is_some();

    f.render_widget(Clear, area);
    let title = format!(
        " KKC Terminal — {}{}— Ctrl-U/Esc to close ",
        app.active_panel().path.display(),
        if running { " [running…] " } else { " " },
    );
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if running {
                Color::Rgb(220, 160, 60)
            } else {
                CLR_TERM_BORDER
            }))
            .style(Style::default().bg(CLR_TERM_BG))
            .title(Span::styled(title, Style::default().fg(CLR_TERM_PROMPT))),
        area,
    );

    let inner = Block::default().borders(Borders::ALL).inner(area);
    if inner.height < 2 {
        return;
    }

    // Split: scrollback lines + prompt input line at the bottom.
    let prompt_y = inner.y + inner.height - 1;
    let log_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height - 1,
    };

    // Scrollback — bottom-aligned
    let visible_lines = log_area.height as usize;
    let start = ts.output.len().saturating_sub(visible_lines);
    let lines: Vec<Line> = ts.output[start..]
        .iter()
        .map(|l| {
            // Lines emitted by the prompt itself get a fixed style
            if let Some(prompt_line) = l.strip_prefix(crate::terminal::PROMPT_LINE_MARKER) {
                return Line::from(Span::styled(
                    prompt_line.to_string(),
                    Style::default()
                        .fg(CLR_TERM_PROMPT)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if l.starts_with('[') {
                return Line::from(Span::styled(
                    l.clone(),
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ));
            }
            // For all other lines parse embedded ANSI escape codes
            let mut line = crate::terminal::ansi_line_to_line(l);
            // If the line has no spans with any explicit fg colour we fall back
            // to the default terminal foreground so plain text matches the theme.
            if line.spans.iter().all(|s| s.style.fg.is_none()) {
                line = line.style(Style::default().fg(CLR_TERM_FG));
            }
            line
        })
        .collect();

    safe_render_widget(
        f,
        Paragraph::new(lines).style(Style::default().bg(CLR_TERM_BG)),
        log_area,
    );

    // Prompt line (blocked while running)
    let prompt = crate::terminal::terminal_prompt(app, running);
    let prompt_len = prompt.chars().count() as u16;
    let input_x = inner.x + prompt_len;

    safe_render_widget(
        f,
        Paragraph::new(Line::from(vec![
            Span::styled(
                prompt,
                Style::default()
                    .fg(if running {
                        Color::Rgb(220, 160, 60)
                    } else {
                        CLR_TERM_PROMPT
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(ts.input.clone(), Style::default().fg(CLR_TERM_INPUT)),
        ]))
        .style(Style::default().bg(CLR_TERM_BG)),
        Rect {
            x: inner.x,
            y: prompt_y,
            width: inner.width,
            height: 1,
        },
    );

    // Show cursor only when not running
    if !running {
        let cursor_col = ts.input[..ts.cursor].chars().count() as u16;
        let cx = input_x + cursor_col;
        if cx < inner.x + inner.width {
            f.set_cursor_position((cx, prompt_y));
        }
    }
}
