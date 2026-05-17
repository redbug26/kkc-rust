use super::*;

pub(super) fn render_panel_or_file_id(
    f: &mut Frame,
    app: &App,
    panel: &crate::panel::Panel,
    area: Rect,
    active: bool,
    color_by_type: bool,
    show_cloud_icons: bool,
    show_file_icons: bool,
    show_file_id: bool,
    quick_preview: Option<&crate::viewer::Viewer>,
    quick_preview_active: bool,
    panel_text_editor: Option<&crate::app::PanelTextEditorState>,
    panel_text_editor_active: bool,
    tab_index: usize,
    tab_count: usize,
) {
    if show_file_id {
        render_file_id_panel(f, app, area);
    } else if let Some(editor) = panel_text_editor {
        super::panel_text_editor::render_panel_text_editor(
            f,
            editor,
            area,
            panel_text_editor_active,
        );
    } else if let Some(viewer) = quick_preview {
        render_quick_preview(f, app, viewer, area, quick_preview_active);
    } else {
        render_panel(
            f,
            panel,
            area,
            active,
            color_by_type,
            show_cloud_icons,
            show_file_icons,
            tab_index,
            tab_count,
        );
    }
}

fn render_quick_preview(
    f: &mut Frame,
    app: &App,
    viewer: &crate::viewer::Viewer,
    area: Rect,
    active: bool,
) {
    use crate::viewer::ViewMode;
    // Build a short title that shows the current mode and, when focused, the F4 hint.
    let mode_str = match app.quick_preview_forced_mode {
        None => format!("Auto:{}", viewer.mode_label()),
        Some(ViewMode::Text) => "Text".to_string(),
        Some(ViewMode::Markdown) => "Markdown".to_string(),
        Some(ViewMode::Hex) => "Hex".to_string(),
        Some(ViewMode::Ansi) => "Ansi".to_string(),
        Some(ViewMode::Image) => "Image".to_string(),
        Some(ViewMode::Module) => "Audio".to_string(),
    };
    let label = if active {
        format!("[{}]  F4:mode  \u{2191}\u{2193}  Tab=exit", mode_str)
    } else {
        format!("[{}]  Tab=focus", mode_str)
    };
    super::render_viewer(
        f,
        viewer,
        false,
        None,
        area,
        false,
        active,
        Some(&label),
        app.config.viewer.autoplay_delay_secs,
    );
}

pub(super) fn render_center_buttons(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(
        Block::default().style(Style::default().bg(clr_app_bg())),
        area,
    );

    if area.height == 0 || area.width < 9 {
        return;
    }

    let mut labels: Vec<String> = app
        .center_buttons
        .iter()
        .map(|action| crate::app::center_button_label(*action).to_string())
        .collect();
    labels.push(Local::now().format("%H:%M:%S").to_string());

    let button_count = labels.len() as u16;
    let button_h = if area.height >= button_count * 3 {
        3
    } else if area.height >= button_count * 2 {
        2
    } else {
        1
    };
    let total_button_h = button_count * button_h;
    if total_button_h > area.height {
        let skip = (total_button_h - area.height) as usize;
        if skip >= labels.len() {
            return;
        }
        labels.drain(0..skip);
    }

    let button_count = labels.len() as u16;
    let total_button_h = button_count * button_h;
    let gaps = button_count.saturating_add(1);
    let free = area.height.saturating_sub(total_button_h);
    let base_gap = free / gaps.max(1);
    let extra_gap = free % gaps.max(1);

    let mut y = area.y + base_gap;
    for (idx, label) in labels.iter().enumerate() {
        let is_clock = idx + 1 == button_count as usize;
        if is_clock {
            y += extra_gap;
        }
        let slot = Rect {
            x: area.x,
            y,
            width: area.width,
            height: button_h.min(area.bottom().saturating_sub(y)),
        };
        render_menu_button(f, slot, label);
        y = y.saturating_add(button_h);
        if idx + 1 < button_count as usize {
            y = y.saturating_add(base_gap);
        }
    }
}

fn render_panel(
    f: &mut Frame,
    panel: &crate::panel::Panel,
    area: Rect,
    active: bool,
    color_by_type: bool,
    show_cloud_icons: bool,
    show_file_icons: bool,
    tab_index: usize,
    tab_count: usize,
) {
    let border_style = if active {
        Style::default()
            .fg(clr_panel_border())
            .bg(clr_app_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(clr_panel_border_dim()).bg(clr_app_bg())
    };
    let border_type = if active {
        BorderType::Thick
    } else {
        BorderType::Rounded
    };

    let display_path = panel.display_path();
    let tab_prefix = if tab_count > 1 {
        format!("[{}/{}] ", tab_index + 1, tab_count)
    } else {
        String::new()
    };
    let title_room = area.width.saturating_sub(4) as usize;
    let path_room = title_room.saturating_sub(tab_prefix.len());
    let title_text = format!("{}{}", tab_prefix, truncate_path(&display_path, path_room));
    let title = format!(" {} ", title_text);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style)
        .style(Style::default().bg(clr_panel_bg()))
        .title(Span::styled(
            title,
            Style::default().fg(clr_panel_title()).bg(clr_app_bg()),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.height < 4 {
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
    let footer_area = clamp_rect(
        area,
        Rect {
            x: inner.x,
            y: inner.y + inner.height - 1,
            width: inner.width,
            height: 1,
        },
    );

    let list_height = list_area.height as usize;

    // Column widths: name | size | date | time
    let size_w = 10usize;
    let date_w = 8usize;
    let time_w = 5usize;
    let name_w = (inner.width as usize).saturating_sub(size_w + date_w + time_w + 3);

    render_panel_header(f, header_area, name_w, size_w, date_w, time_w);
    render_panel_entries(
        f,
        panel,
        list_area,
        active,
        color_by_type,
        show_cloud_icons,
        show_file_icons,
        name_w,
        size_w,
        date_w,
        time_w,
    );
    render_panel_scrollbar(f, panel, area, list_area, list_height);
    render_panel_footer(f, panel, footer_area);
}

fn render_panel_header(
    f: &mut Frame,
    header_area: Rect,
    name_w: usize,
    size_w: usize,
    date_w: usize,
    time_w: usize,
) {
    let header_line = Line::from(vec![
        Span::styled(
            format!("{:^width$}", "Name", width = name_w),
            Style::default()
                .fg(clr_header_fg())
                .bg(clr_header_bg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "│",
            Style::default().fg(clr_panel_border()).bg(clr_panel_bg()),
        ),
        Span::styled(
            format!("{:^width$}", "Size", width = size_w),
            Style::default()
                .fg(clr_header_fg())
                .bg(clr_header_bg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "│",
            Style::default().fg(clr_panel_border()).bg(clr_panel_bg()),
        ),
        Span::styled(
            format!("{:^width$}", "Date", width = date_w),
            Style::default()
                .fg(clr_header_fg())
                .bg(clr_header_bg())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "│",
            Style::default().fg(clr_panel_border()).bg(clr_panel_bg()),
        ),
        Span::styled(
            format!("{:^width$}", "Time", width = time_w),
            Style::default()
                .fg(clr_header_fg())
                .bg(clr_header_bg())
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(
        Paragraph::new(header_line).style(Style::default().bg(clr_panel_bg())),
        header_area,
    );
}

fn render_panel_entries(
    f: &mut Frame,
    panel: &crate::panel::Panel,
    list_area: Rect,
    active: bool,
    color_by_type: bool,
    show_cloud_icons: bool,
    show_file_icons: bool,
    name_w: usize,
    size_w: usize,
    date_w: usize,
    time_w: usize,
) {
    let list_height = list_area.height as usize;
    let items: Vec<ListItem> = panel
        .entries
        .iter()
        .enumerate()
        .skip(panel.scroll)
        .take(list_height)
        .map(|(idx, entry)| {
            let is_disconnect_entry = entry.name == "[disconnect]";
            let is_cursor = active && idx == panel.cursor;
            let fg = if is_cursor {
                clr_cursor_fg()
            } else if entry.selected {
                clr_selected()
            } else {
                entry_fg(entry, color_by_type)
            };

            let base_style = if is_cursor {
                Style::default()
                    .fg(fg)
                    .bg(clr_cursor_bg())
                    .add_modifier(Modifier::BOLD)
            } else if entry.selected || entry.is_dir {
                Style::default().fg(fg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(fg)
            };

            let display_name = if show_file_icons && entry.is_dir && !is_disconnect_entry {
                format!("{} {}", entry.file_icon.unwrap_or("\u{e5ff}"), entry.name)
            } else if entry.is_dir && entry.name != ".." && !is_disconnect_entry {
                format!("/{}", entry.name)
            } else {
                format!(" {}", entry.name)
            };
            let suffix_icon = if show_cloud_icons && entry.cloud_only {
                Some("\u{f0c2}")
            } else if entry.is_dir || !show_file_icons {
                None
            } else {
                entry.file_icon
            };
            let name_str = format_panel_name(&display_name, suffix_icon, name_w);

            let size_str = if entry.name == ".." {
                format!("{:>width$}", "↑ up-dir ↑", width = size_w)
            } else if is_disconnect_entry {
                truncate_str(&format!("{:^width$}", "action", width = size_w), size_w)
            } else if entry.is_dir {
                format!("{:>width$}", "⌦sub--dir⌫", width = size_w)
            } else {
                format!(
                    "{:>width$}",
                    format_panel_size(entry.size, size_w),
                    width = size_w
                )
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
                column_separator(base_style),
                Span::styled(size_str, base_style),
                column_separator(base_style),
                Span::styled(date_str, base_style),
                column_separator(base_style),
                Span::styled(time_str, base_style),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).style(Style::default().bg(clr_panel_bg()));
    f.render_widget(list, list_area);
}

fn format_panel_name(display_name: &str, suffix_icon: Option<&str>, width: usize) -> String {
    let Some(icon) = suffix_icon else {
        let name = format!("{:<width$}", display_name, width = width);
        return truncate_str(&name, width);
    };

    if width <= 4 {
        return truncate_str(display_name, width);
    }

    let name_w = width.saturating_sub(4);
    let name = truncate_str(display_name, name_w);
    let with_icon = format!("{} {}", name.trim_end(), icon);
    truncate_str(&with_icon, width)
}

fn column_separator(base_style: Style) -> Span<'static> {
    Span::styled(
        "│",
        Style::default()
            .fg(clr_panel_border_dim())
            .bg(base_style.bg.unwrap_or(clr_panel_bg())),
    )
}

fn render_panel_scrollbar(
    f: &mut Frame,
    panel: &crate::panel::Panel,
    area: Rect,
    list_area: Rect,
    list_height: usize,
) {
    if panel.entries.len() <= list_height {
        return;
    }

    let mut sb_state = ScrollbarState::new(panel.entries.len()).position(panel.scroll);
    let sb_area = Rect {
        x: area.x + area.width - 1,
        y: list_area.y,
        width: 1,
        height: list_area.height,
    };
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_style(Style::default().fg(clr_panel_border()))
            .track_style(Style::default().bg(clr_panel_bg()))
            .begin_symbol(Some("^"))
            .end_symbol(Some("v")),
        sb_area,
        &mut sb_state,
    );
}

fn render_panel_footer(f: &mut Frame, panel: &crate::panel::Panel, footer_area: Rect) {
    if footer_area.height == 0 {
        return;
    }

    let sel_count = panel.selected_count();
    let footer = if sel_count > 0 {
        if sel_count == 1 {
            format!(
                "{:<10} b. in one selected file",
                format_dos_number(panel.selected_bytes())
            )
        } else {
            format!(
                "{:<10} b. in {:3} selected files",
                format_dos_number(panel.selected_bytes()),
                sel_count
            )
        }
    } else {
        let total: u64 = panel
            .entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.size)
            .sum();
        let files = panel
            .entries
            .iter()
            .filter(|e| !e.is_dir && e.name != "..")
            .count();
        if files == 1 {
            format!("{:<10} bytes in one file", format_dos_number(total))
        } else {
            format!(
                "{:<10} bytes in {:3} files",
                format_dos_number(total),
                files
            )
        }
    };

    f.render_widget(
        Paragraph::new(truncate_str(&footer, footer_area.width as usize))
            .style(Style::default().fg(clr_button_fg()).bg(clr_status_bg())),
        footer_area,
    );
}

fn colorize_idf_line(line: &str) -> Line<'static> {
    // Header "Ketchup Killers IDF"
    if line.starts_with("Ketchup Killers") {
        return Line::from(Span::styled(
            line.to_string(),
            Style::default()
                .fg(clr_header_fg())
                .add_modifier(Modifier::BOLD),
        ));
    }
    // "Key: value" lines
    let known_labels = [
        "Title", "Type", "Mime", "Composer", "Date", "Size", "Attr", "Viewers",
    ];
    for label in &known_labels {
        let prefix = format!("{}:", label);
        if line.starts_with(prefix.as_str()) {
            let value = &line[prefix.len()..];
            let label_color = match *label {
                "Type" => clr_header_fg(),
                "Title" | "Composer" => clr_panel_title(),
                _ => clr_panel_border_dim(),
            };
            let value_color = match *label {
                "Type" => clr_panel_title(),
                "Mime" => clr_data(),
                "Attr" => clr_exec(),
                "Viewers" => clr_audio(),
                _ => clr_text(),
            };
            return Line::from(vec![
                Span::styled(prefix, Style::default().fg(label_color)),
                Span::styled(value.to_string(), Style::default().fg(value_color)),
            ]);
        }
    }
    // Extra detail lines (start with space)
    if line.starts_with(' ') {
        return Line::from(Span::styled(
            line.to_string(),
            Style::default().fg(clr_data()),
        ));
    }
    // Empty line
    if line.is_empty() {
        return Line::from("");
    }
    // Filename (unlabeled line after header)
    Line::from(Span::styled(
        line.to_string(),
        Style::default()
            .fg(clr_panel_title())
            .add_modifier(Modifier::BOLD),
    ))
}

fn hex_dump_line(chunk: &[u8], bytes_per_row: usize) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, &b) in chunk.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            format!("{:02X}", b),
            Style::default().fg(clr_data()),
        ));
    }
    // Pad if this is a short last row
    for _ in chunk.len()..bytes_per_row {
        spans.push(Span::raw("   "));
    }
    // Separator between hex and ASCII
    spans.push(Span::raw("  "));
    // ASCII representation
    let ascii: String = chunk
        .iter()
        .map(|&b| {
            if (0x20..0x7F).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect();
    spans.push(Span::styled(ascii, Style::default().fg(clr_unknown())));
    Line::from(spans)
}

fn render_file_id_panel(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.file_id_active;
    let (border_style, title_style, title_text) = if focused {
        (
            Style::default().fg(clr_header_fg()).bg(clr_app_bg()),
            Style::default()
                .fg(clr_header_fg())
                .bg(clr_app_bg())
                .add_modifier(Modifier::BOLD),
            " FileID ↑↓  Tab=exit ",
        )
    } else {
        (
            Style::default().fg(clr_panel_border()).bg(clr_app_bg()),
            Style::default().fg(clr_panel_title()).bg(clr_app_bg()),
            " FileID ",
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(border_style)
        .style(Style::default().bg(clr_panel_bg()))
        .title(Span::styled(title_text, title_style));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width < 5 {
        return;
    }

    // Split inner into: │<sp> left gutter | content area | right gutter <sp>│
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(inner);
    let left_col = cols[0];
    let content_area = cols[1];
    let right_col = cols[2];

    // Render decorative inner gutter bars (pipe + padding space)
    let gutter_style = Style::default()
        .fg(clr_panel_border_dim())
        .bg(clr_panel_bg());
    let left_gutter_lines: Vec<Line<'static>> = (0..inner.height as usize)
        .map(|_| Line::from(Span::styled("│ ", gutter_style)))
        .collect();
    let right_gutter_lines: Vec<Line<'static>> = (0..inner.height as usize)
        .map(|_| Line::from(Span::styled(" │", gutter_style)))
        .collect();
    f.render_widget(
        Paragraph::new(left_gutter_lines).style(Style::default().bg(clr_panel_bg())),
        left_col,
    );
    f.render_widget(
        Paragraph::new(right_gutter_lines).style(Style::default().bg(clr_panel_bg())),
        right_col,
    );

    // IDF text lines
    let text = app.build_file_id_preview();
    let mut lines: Vec<Line<'static>> = text.lines().map(colorize_idf_line).collect();

    // Hex dump section — only for regular local files
    // Layout: N*2 hex + (N-1) spaces + 2 separator + N ascii = 4N+1 chars per row
    let cw = content_area.width as usize;
    let bytes_per_row: usize = ((cw.saturating_sub(1)) / 4).clamp(4, 32);
    let max_bytes = bytes_per_row * 64;
    let hex_data: Option<Vec<u8>> = app
        .active_panel()
        .current_entry()
        .filter(|e| !e.is_dir && e.name != "..")
        .and_then(|e| {
            use std::io::Read;
            let mut file = std::fs::File::open(&e.path).ok()?;
            let mut buf = vec![0u8; max_bytes];
            let n = file.read(&mut buf).ok()?;
            buf.truncate(n);
            Some(buf)
        });

    if let Some(data) = hex_data
        && !data.is_empty()
    {
        lines.push(Line::from(Span::styled(
            "─".repeat(cw),
            Style::default().fg(clr_panel_border_dim()),
        )));
        for chunk in data.chunks(bytes_per_row) {
            lines.push(hex_dump_line(chunk, bytes_per_row));
        }
    }

    f.render_widget(
        Paragraph::new(lines)
            .scroll((app.file_id_scroll, 0))
            .style(Style::default().bg(clr_panel_bg())),
        content_area,
    );
}

fn render_menu_button(f: &mut Frame, area: Rect, label: &str) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let content_w = if area.height < 3 {
        area.width
    } else {
        area.width.saturating_sub(2)
    };
    let text = truncate_str(label, content_w.max(1) as usize)
        .trim_end()
        .to_string();
    if area.height < 3 {
        let top_pad = area.height.saturating_sub(1) / 2;
        let text_area = Rect {
            x: area.x,
            y: area.y + top_pad,
            width: area.width,
            height: 1,
        };
        safe_render_widget(
            f,
            Block::default().style(Style::default().bg(clr_button_bg())),
            area,
        );
        safe_render_widget(
            f,
            Paragraph::new(text).alignment(Alignment::Center).style(
                Style::default()
                    .fg(clr_button_fg())
                    .bg(clr_button_bg())
                    .add_modifier(Modifier::BOLD),
            ),
            text_area,
        );
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(clr_panel_border()))
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(clr_button_bg()));
    let inner = block.inner(area);
    safe_render_widget(f, block, area);
    let top_pad = inner.height.saturating_sub(1) / 2;
    let text_area = Rect {
        x: inner.x,
        y: inner.y + top_pad,
        width: inner.width,
        height: 1,
    };
    safe_render_widget(
        f,
        Paragraph::new(text).alignment(Alignment::Center).style(
            Style::default()
                .fg(clr_button_fg())
                .bg(clr_button_bg())
                .add_modifier(Modifier::BOLD),
        ),
        text_area,
    );
}
