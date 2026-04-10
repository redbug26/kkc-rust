use crate::app::{ActivePanel, App, AppMode, ConfirmDialog, InputDialog, MenuState, MenuAction, SearchState, MENU_DATA, MENU_HEADERS};
use crate::help::HelpView;
use crate::config::SortMode;
use crate::file_ops::format_size;
use crate::file_types::FileCategory;
use crate::panel::Entry;
use crate::viewer::{ViewMode, Viewer};
use chrono::{DateTime, Local};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame,
};

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
    f.render_widget(Block::default().style(Style::default().bg(CLR_APP_BG)), f.area());

    match &app.mode {
        AppMode::Viewer(v) => {
            render_viewer(f, v, false, f.area());
            return;
        }
        AppMode::ViewerSearching(v) => {
            render_viewer(f, v, true, f.area());
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
            vec![
                Constraint::Min(5),
                Constraint::Length(1),
            ]
        })
        .split(f.area());

    let panels_area = main_vert[0];
    let status_area = main_vert[1];

    let panel_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(28),
            Constraint::Length(13),
            Constraint::Min(28),
        ])
        .split(panels_area);

    let left_active = app.active == ActivePanel::Left;

    render_panel(f, &app.left, panel_chunks[0], left_active, app.config.color_by_type);
    render_center_buttons(f, panel_chunks[1]);
    render_panel(f, &app.right, panel_chunks[2], !left_active, app.config.color_by_type);
    render_status(f, app, status_area);

    if has_fbar {
        render_fkey_bar(f, main_vert[2]);
    }

    // Overlays
    match &app.mode {
        AppMode::Confirm(dlg) => render_confirm(f, dlg, f.area()),
        AppMode::Input(dlg) => render_input(f, dlg, f.area()),
        AppMode::SearchPanel(s) => render_search(f, s, f.area()),
        AppMode::DirHistory => render_dir_history(f, app, f.area()),
        AppMode::Menu(ms) => render_menu(f, ms, f.area()),
        AppMode::QuickSearch => {
            let qs = &app.active_panel().quicksearch;
            if !qs.is_empty() {
                render_quicksearch_label(f, qs, panels_area);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------

fn render_panel(
    f: &mut Frame,
    panel: &crate::panel::Panel,
    area: Rect,
    active: bool,
    color_by_type: bool,
) {
    let border_style = if active {
        Style::default().fg(CLR_PANEL_BORDER).bg(CLR_APP_BG).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)
    };

    let title_text = truncate_path(&panel.path.to_string_lossy(), area.width.saturating_sub(4) as usize);
    let title = format!(" {} ", title_text);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(CLR_PANEL_BG))
        .title(Span::styled(title, Style::default().fg(CLR_PANEL_TITLE).bg(CLR_APP_BG)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    if inner.height < 4 {
        return;
    }

    let header_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: inner.height.saturating_sub(2),
    };
    let footer_area = Rect {
        x: inner.x,
        y: inner.y + inner.height - 1,
        width: inner.width,
        height: 1,
    };

    let list_height = list_area.height as usize;

    // Column widths: name | size | date | time
    let size_w = 10usize;
    let date_w = 8usize;
    let time_w = 5usize;
    let name_w = (inner.width as usize)
        .saturating_sub(size_w + date_w + time_w + 3);

    let header_line = Line::from(vec![
        Span::styled(format!("{:^width$}", "Name", width = name_w), Style::default().fg(CLR_HEADER_FG).bg(CLR_HEADER_BG).add_modifier(Modifier::BOLD)),
        Span::styled("│", Style::default().fg(CLR_PANEL_BORDER).bg(CLR_PANEL_BG)),
        Span::styled(format!("{:^width$}", "Size", width = size_w), Style::default().fg(CLR_HEADER_FG).bg(CLR_HEADER_BG).add_modifier(Modifier::BOLD)),
        Span::styled("│", Style::default().fg(CLR_PANEL_BORDER).bg(CLR_PANEL_BG)),
        Span::styled(format!("{:^width$}", "Date", width = date_w), Style::default().fg(CLR_HEADER_FG).bg(CLR_HEADER_BG).add_modifier(Modifier::BOLD)),
        Span::styled("│", Style::default().fg(CLR_PANEL_BORDER).bg(CLR_PANEL_BG)),
        Span::styled(format!("{:^width$}", "Time", width = time_w), Style::default().fg(CLR_HEADER_FG).bg(CLR_HEADER_BG).add_modifier(Modifier::BOLD)),
    ]);
    f.render_widget(Paragraph::new(header_line).style(Style::default().bg(CLR_PANEL_BG)), header_area);

    let items: Vec<ListItem> = panel
        .entries
        .iter()
        .enumerate()
        .skip(panel.scroll)
        .take(list_height)
        .map(|(idx, entry)| {
            let is_cursor = active && idx == panel.cursor;
            let fg = if is_cursor {
                CLR_CURSOR_FG
            } else if entry.selected {
                CLR_SELECTED
            } else {
                entry_fg(entry, color_by_type)
            };

            let base_style = if is_cursor {
                Style::default().fg(fg).bg(CLR_CURSOR_BG).add_modifier(Modifier::BOLD)
            } else if entry.selected {
                Style::default().fg(fg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(fg)
            };

            let name_str = format!("{:<width$}", &entry.name, width = name_w);
            let name_str = truncate_str(&name_str, name_w);

            let size_str = if entry.name == ".." {
                format!("{:>width$}", "↑ up-dir ↑", width = size_w)
            } else if entry.is_dir {
                format!("{:>width$}", "⌦sub-dir⌫", width = size_w)
            } else {
                format!("{:>width$}", format_dos_number(entry.size), width = size_w)
            };

            let date_str = match entry.modified {
                Some(dt) => format!("{:>width$}", dt.format("%d/%m/%y"), width = date_w),
                None => format!("{:>width$}", "", width = date_w),
            };
            let time_str = match entry.modified {
                Some(dt) => format!("{:>width$}", dt.format("%H:%M"), width = time_w),
                None => format!("{:>width$}", "", width = time_w),
            };

            let line = Line::from(vec![
                Span::styled(name_str, base_style),
                Span::styled("│", Style::default().fg(CLR_PANEL_BORDER_DIM).bg(base_style.bg.unwrap_or(CLR_PANEL_BG))),
                Span::styled(size_str, base_style),
                Span::styled("│", Style::default().fg(CLR_PANEL_BORDER_DIM).bg(base_style.bg.unwrap_or(CLR_PANEL_BG))),
                Span::styled(date_str, base_style),
                Span::styled("│", Style::default().fg(CLR_PANEL_BORDER_DIM).bg(base_style.bg.unwrap_or(CLR_PANEL_BG))),
                Span::styled(time_str, base_style),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).style(Style::default().bg(CLR_PANEL_BG));
    f.render_widget(list, list_area);

    // Scrollbar
    if panel.entries.len() > list_height {
        let mut sb_state = ScrollbarState::new(panel.entries.len())
            .position(panel.scroll);
        let sb_area = Rect {
            x: area.x + area.width - 1,
            y: list_area.y,
            width: 1,
            height: list_area.height,
        };
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(CLR_PANEL_BORDER))
                .track_style(Style::default().bg(CLR_PANEL_BG))
                .begin_symbol(Some("^"))
                .end_symbol(Some("v")),
            sb_area,
            &mut sb_state,
        );
    }

    // Footer (selection info)
    if inner.height > 1 {
        let sel_count = panel.selected_count();
        let footer = if sel_count > 0 {
            if sel_count == 1 {
                format!("{:<10} b. in one selected file", format_dos_number(panel.selected_bytes()))
            } else {
                format!("{:<10} b. in {:3} selected files", format_dos_number(panel.selected_bytes()), sel_count)
            }
        } else {
            let total: u64 = panel.entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
            let files = panel.entries.iter().filter(|e| !e.is_dir && e.name != "..").count();
            if files == 1 {
                format!("{:<10} bytes in one file", format_dos_number(total))
            } else {
                format!("{:<10} bytes in {:3} files", format_dos_number(total), files)
            }
        };

        f.render_widget(
            Paragraph::new(truncate_str(&footer, footer_area.width as usize))
                .style(Style::default().fg(CLR_BUTTON_FG).bg(CLR_STATUS_BG)),
            footer_area,
        );
    }
}

fn render_center_buttons(f: &mut Frame, area: Rect) {
    f.render_widget(Block::default().style(Style::default().bg(CLR_APP_BG)), area);

    if area.height < 8 || area.width < 9 {
        return;
    }

    let slots = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

    for (slot, label) in [
        (slots[1], "ChgDrive"),
        (slots[3], "Swap"),
        (slots[5], "Go Trash"),
        (slots[7], "QuickDir"),
        (slots[9], "Select"),
        (slots[11], "Info"),
    ] {
        render_menu_button(f, slot, label);
    }

    let now = Local::now().format("%H:%M:%S").to_string();
    render_menu_button(f, slots[13], &now);
}

fn render_menu_button(f: &mut Frame, area: Rect, label: &str) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER))
        .style(Style::default().bg(CLR_BUTTON_BG));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(Style::default().fg(CLR_BUTTON_FG).bg(CLR_BUTTON_BG).add_modifier(Modifier::BOLD)),
        inner,
    );
}

// ---------------------------------------------------------------------------
// Menu bar + dropdown (F2)
// ---------------------------------------------------------------------------

fn render_menu(f: &mut Frame, state: &MenuState, area: Rect) {
    // ── top bar ────────────────────────────────────────────────────────────
    let bar_area = Rect { x: area.x, y: area.y, width: area.width, height: 1 };

    let mut spans = vec![Span::styled(" ", Style::default().bg(CLR_MENU_BAR_BG))];
    for (i, header) in MENU_HEADERS.iter().enumerate() {
        let style = if i == state.bar_pos && !state.open {
            Style::default().bg(CLR_MENU_SEL_BG).fg(CLR_MENU_SEL_FG).add_modifier(Modifier::BOLD)
        } else if i == state.bar_pos {
            Style::default().bg(CLR_MENU_SEL_BG).fg(CLR_MENU_SEL_FG).add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(CLR_MENU_BAR_BG).fg(CLR_MENU_BAR_FG)
        };
        spans.push(Span::styled(format!(" {} ", header), style));
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
    let max_key = items.iter().filter_map(|(_, k, _)| *k).map(|k| k.len()).max().unwrap_or(0);
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
    for (idx, (label, key_hint, action)) in items.iter().enumerate() {
        if idx as u16 >= inner.height {
            break;
        }
        let row = Rect { x: inner.x, y: inner.y + idx as u16, width: inner.width, height: 1 };

        if *action == MenuAction::Separator {
            let sep: String = std::iter::repeat('─').take(avail).collect();
            f.render_widget(
                Paragraph::new(sep).style(Style::default().fg(CLR_MENU_DD_SEP).bg(CLR_MENU_DD_BG)),
                row,
            );
        } else {
            let style = if idx == state.item_pos {
                Style::default().bg(CLR_MENU_SEL_BG).fg(CLR_MENU_SEL_FG).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(CLR_MENU_DD_BG).fg(CLR_MENU_DD_FG)
            };
            let key_text = key_hint.unwrap_or("");
            // " label .......... key "
            let used = label.len() + key_text.len() + 2; // leading " " + trailing " "
            let pad = avail.saturating_sub(used);
            let text = format!(" {}{}{} ", label, " ".repeat(pad), key_text);
            f.render_widget(
                Paragraph::new(truncate_str(&text, avail)).style(style),
                row,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let entry_info = if let Some(e) = app.active_panel().current_entry() {
        let kind = if e.is_symlink {
            "symlink"
        } else if e.is_dir {
            "dir"
        } else {
            "file"
        };
        let mode_str = format_mode(e.mode);
        format!("{}  {}  {}", e.name, mode_str, kind)
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
    let hidden_label = if app.active_panel().show_hidden { "H" } else { " " };
    let right_info = format!(" Sort:{} [{}] ", sort_label, hidden_label);

    let left_w = area.width.saturating_sub(right_info.len() as u16);

    let line = Line::from(vec![
        Span::styled(
            format!(" {:<width$}", status_text, width = left_w.saturating_sub(1) as usize),
            Style::default().fg(CLR_STATUS_FG).bg(CLR_STATUS_BG),
        ),
        Span::styled(right_info, Style::default().fg(CLR_BUTTON_FG).bg(CLR_STATUS_BG)),
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

fn render_viewer(f: &mut Frame, v: &Viewer, searching: bool, area: Rect) {
    let mode_label = match v.mode {
        ViewMode::Text => "Text",
        ViewMode::Hex => "Hex",
    };
    let file_name = v.path.file_name().unwrap_or_default().to_string_lossy();
    let match_info = if !v.search.is_empty() {
        format!(" [{}/{}]", v.match_pos + 1, v.matches.len())
    } else {
        String::new()
    };
    let title = format!(
        " {} [{}] {}/{}{} ",
        file_name,
        mode_label,
        v.scroll + 1,
        v.lines.len().max(1),
        match_info,
    );

    // Reserve last line for search bar when searching
    let content_area = if searching {
        Rect { height: area.height.saturating_sub(1), ..area }
    } else {
        area
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER));
    let inner = block.inner(content_area);
    f.render_widget(block, content_area);

    let height = inner.height as usize;
    let width = inner.width as usize;

    let search_lower = v.search.to_lowercase();

    let items: Vec<Line> = v
        .lines
        .iter()
        .skip(v.scroll)
        .take(height)
        .enumerate()
        .map(|(rel_idx, line)| {
            let abs_idx = v.scroll + rel_idx;
            let is_match = !search_lower.is_empty()
                && line.to_lowercase().contains(&search_lower);
            let is_current_match = is_match
                && v.matches.get(v.match_pos).copied() == Some(abs_idx);

            let style = if is_current_match {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else if is_match {
                Style::default().fg(Color::Black).bg(Color::LightYellow)
            } else {
                Style::default().fg(Color::White)
            };

            let display = truncate_str(line, width);
            Line::from(Span::styled(display, style))
        })
        .collect();

    if v.wrap && matches!(v.mode, ViewMode::Text) {
        f.render_widget(Paragraph::new(items).wrap(Wrap { trim: false }), inner);
    } else {
        let list = List::new(items.into_iter().map(ListItem::new).collect::<Vec<_>>());
        f.render_widget(list, inner);
    }

    if searching {
        // Live search bar at the very bottom
        let bar_area = Rect {
            x: area.x,
            y: area.y + area.height - 1,
            width: area.width,
            height: 1,
        };
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
            bar_area,
        );
        // Show cursor
        let cx = (area.x + 9 + v.search.len() as u16).min(area.x + area.width - 1);
        f.set_cursor_position((cx, area.y + area.height - 1));
    } else {
        // Normal help bar
        let help = Paragraph::new(" Esc:Close  Tab:Hex/Text  /:Search  n:Next  N:Prev  Home/End ")
            .style(Style::default().fg(Color::Black).bg(Color::Cyan));
        let bar_area = Rect {
            x: area.x,
            y: area.y + area.height - 1,
            width: area.width,
            height: 1,
        };
        f.render_widget(help, bar_area);
    }
}

// ---------------------------------------------------------------------------
// Confirm dialog
// ---------------------------------------------------------------------------

fn render_confirm(f: &mut Frame, dlg: &ConfirmDialog, area: Rect) {
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 7u16;
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    let popup = Rect { x, y, width, height };

    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(format!(" {} ", dlg.title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let text = format!("\n{}\n\n   [Y] Yes    [N] No", dlg.message);
    f.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
        inner,
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
    let popup = Rect { x, y, width, height };

    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(format!(" {} ", dlg.title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

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
        Paragraph::new(vec![Line::default(), prompt_line, Line::default(), input_line, hint_line]),
        inner,
    );

    // Draw cursor inside input field
    let cursor_x = (inner.x + 1 + dlg.cursor as u16).min(inner.x + inner.width.saturating_sub(2));
    let cursor_y = inner.y + 3;
    if cursor_y < inner.y + inner.height {
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

// ---------------------------------------------------------------------------
// Search panel
// ---------------------------------------------------------------------------

fn render_search(f: &mut Frame, state: &SearchState, area: Rect) {
    let width = 70u16.min(area.width.saturating_sub(4));
    let height = (area.height / 2).max(12);
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    let popup = Rect { x, y, width, height };

    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Search ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // --- Input area (top 5 rows) ---
    let input_h = 5u16.min(inner.height);
    let results_area = Rect {
        x: inner.x,
        y: inner.y + input_h,
        width: inner.width,
        height: inner.height.saturating_sub(input_h + 1),
    };

    let input_w = inner.width.saturating_sub(14) as usize;
    let pat_style0 = Style::default().fg(Color::Black).bg(Color::White);
    let pat_style1 = Style::default().fg(Color::Black).bg(Color::White);

    let (pat_s, cnt_s) = if state.input_field == 0 {
        (pat_style0.add_modifier(Modifier::BOLD), pat_style1)
    } else {
        (pat_style0, pat_style1.add_modifier(Modifier::BOLD))
    };

    let pat_str = format!("{:<width$}", state.query, width = input_w);
    let cnt_str = format!("{:<width$}", state.content_query, width = input_w);
    let start_str = state.start_dir.to_string_lossy();

    let lines = vec![
        Line::default(),
        Line::from(vec![
            Span::raw("  Pattern : "),
            Span::styled(pat_str, pat_s),
        ]),
        Line::default(),
        Line::from(vec![
            Span::raw("  Content : "),
            Span::styled(cnt_str, cnt_s),
        ]),
        Line::from(Span::styled(
            format!("  Start: {}", start_str),
            Style::default().fg(Color::DarkGray),
        )),
    ];

    f.render_widget(Paragraph::new(lines), Rect { x: inner.x, y: inner.y, width: inner.width, height: input_h });

    // --- Results ---
    let result_count = state.results.len();
    let date_w = 15usize;
    let size_w = 8usize;
    let path_w = (results_area.width as usize).saturating_sub(size_w + date_w + 2);

    let items: Vec<ListItem> = state
        .results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let style = if i == state.cursor {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };

            let path = truncate_str(&r.path.to_string_lossy(), path_w);
            let size = format!("{:>width$}", format_size(r.size), width = size_w);
            let modified = r
                .modified
                .map(|ts| {
                    let dt: DateTime<Local> = ts.into();
                    dt.format("%y-%m-%d %H:%M").to_string()
                })
                .unwrap_or_default();
            let modified = format!("{:>width$}", modified, width = date_w);

            ListItem::new(Line::from(vec![
                Span::styled(path, style),
                Span::styled(format!(" {}", size), style),
                Span::styled(format!(" {}", modified), style),
            ]))
        })
        .collect();

    let result_block = Block::default()
        .title(format!(" {} result(s) ", result_count))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));
    let result_inner = result_block.inner(results_area);
    f.render_widget(result_block, results_area);
    f.render_widget(List::new(items), result_inner);

    // Hint
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height - 1,
        width: inner.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            " Enter:Search  Tab:Switch field  Esc:Close ",
            Style::default().fg(Color::DarkGray),
        )),
        hint_area,
    );
}

// ---------------------------------------------------------------------------
// Directory history
// ---------------------------------------------------------------------------

fn render_dir_history(f: &mut Frame, app: &App, area: Rect) {
    let width = 60u16.min(area.width.saturating_sub(4));
    let height = (app.dir_history.len() as u16 + 4).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    let popup = Rect { x, y, width, height };

    f.render_widget(Clear, popup);
    let block = Block::default()
        .title(" Directory History ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let items: Vec<ListItem> = app
        .dir_history
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let style = if i == app.history_cursor {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(
                truncate_str(&p.to_string_lossy(), inner.width as usize),
                style,
            )))
        })
        .collect();

    f.render_widget(List::new(items), inner);
}

// ---------------------------------------------------------------------------
// Quicksearch label
// ---------------------------------------------------------------------------

fn render_quicksearch_label(f: &mut Frame, qs: &str, panels_area: Rect) {
    let label = format!(" Search: {} ", qs);
    let w = label.len() as u16 + 2;
    let area = Rect {
        x: panels_area.x + panels_area.width / 2 - w / 2,
        y: panels_area.y + panels_area.height - 3,
        width: w.min(panels_area.width),
        height: 1,
    };
    f.render_widget(
        Paragraph::new(label)
            .style(Style::default().fg(Color::Black).bg(Color::Yellow)),
        area,
    );
}

// ---------------------------------------------------------------------------
// Help overlay
// ---------------------------------------------------------------------------

fn render_help(f: &mut Frame, state: &crate::help::HelpState, area: Rect) {
    let popup = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    f.render_widget(Clear, popup);
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

fn render_help_with_title(
    f: &mut Frame,
    popup: Rect,
    title: &str,
    state: &crate::help::HelpState,
) {
    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER).bg(CLR_APP_BG))
        .style(Style::default().bg(Color::Black));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    if inner.height < 3 {
        return;
    }

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

    match state.view {
        HelpView::Index { cursor } => {
            let items: Vec<ListItem> = state
                .system
                .sections
                .iter()
                .enumerate()
                .map(|(idx, section)| {
                    let style = if idx == cursor {
                        Style::default().fg(Color::Black).bg(CLR_SELECTED).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(Span::styled(format!(" {}", section.title), style)))
                })
                .collect();
            f.render_widget(List::new(items), body);
            f.render_widget(
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
                        Style::default().fg(Color::Black).bg(CLR_SELECTED).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    ListItem::new(Line::from(Span::styled(
                        format!(" {}", state.system.topics[*topic_idx].title),
                        style,
                    )))
                })
                .collect();
            f.render_widget(List::new(items), body);
            f.render_widget(
                Paragraph::new(" Esc:Close  Backspace:Back  Enter:Open page ")
                    .style(Style::default().fg(Color::DarkGray)),
                footer,
            );
        }
        HelpView::Page { topic, scroll, selected_link } => {
            let topic = &state.system.topics[topic];
            f.render_widget(
                Paragraph::new(topic.to_render_lines(selected_link))
                    .style(Style::default().fg(Color::White))
                    .scroll((scroll, 0))
                    .wrap(Wrap { trim: false }),
                body,
            );
            f.render_widget(
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

fn truncate_path(p: &str, max: usize) -> String {
    if p.len() <= max {
        return p.to_string();
    }
    let trimmed = &p[p.len() - max.saturating_sub(3)..];
    format!("...{}", trimmed)
}

fn format_mode(mode: u32) -> String {
    let chars: Vec<char> = [
        (0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x'),
    ]
    .iter()
    .map(|(bit, ch)| if mode & bit != 0 { *ch } else { '-' })
    .collect();
    chars.into_iter().collect()
}
