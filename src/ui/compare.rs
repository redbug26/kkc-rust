use super::*;

fn compare_marker(kind: crate::app::CompareRowKind) -> (&'static str, Color) {
    match kind {
        crate::app::CompareRowKind::Equal => ("=", Color::Rgb(110, 110, 125)),
        crate::app::CompareRowKind::Added => ("+", Color::Rgb(80, 180, 120)),
        crate::app::CompareRowKind::Removed => ("-", Color::Rgb(220, 100, 100)),
        crate::app::CompareRowKind::Changed => ("~", Color::Rgb(220, 180, 80)),
    }
}

pub(super) fn render_compare_panel(f: &mut Frame, state: &ComparePanelState, area: Rect) {
    let width = 120u16.min(area.width.saturating_sub(2));
    let height = (area.height * 4 / 5).clamp(16, area.height.saturating_sub(2));
    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        },
    );
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(Span::styled(
            " Compare Files ",
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
    if inner.height < 5 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let header = vec![
        Line::from(vec![
            Span::styled(" Left : ", Style::default().fg(Color::Rgb(120, 170, 230))),
            Span::styled(
                truncate_path(
                    &state.left_label,
                    chunks[0].width.saturating_sub(10) as usize,
                ),
                Style::default().fg(Color::Rgb(220, 225, 235)),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Right: ", Style::default().fg(Color::Rgb(120, 170, 230))),
            Span::styled(
                truncate_path(
                    &state.right_label,
                    chunks[0].width.saturating_sub(10) as usize,
                ),
                Style::default().fg(Color::Rgb(220, 225, 235)),
            ),
            Span::raw("  "),
            Span::styled(
                state.summary.clone(),
                Style::default().fg(Color::Rgb(150, 160, 180)),
            ),
        ]),
        Line::from(vec![Span::styled(
            format!(
                " [D]iff only:{}  [W]hitespace:{}  [L]ine endings:{}  [/] search:{}{} ",
                if state.show_only_differences {
                    "on"
                } else {
                    "off"
                },
                if state.ignore_whitespace { "on" } else { "off" },
                if state.ignore_crlf { "on" } else { "off" },
                if state.search_query.is_empty() {
                    "-"
                } else {
                    state.search_query.as_str()
                },
                if state.search_active { "_" } else { "" }
            ),
            Style::default().fg(Color::Rgb(160, 170, 200)),
        )]),
    ];
    safe_render_widget(
        f,
        Paragraph::new(header).style(Style::default().bg(Color::Rgb(18, 18, 24))),
        chunks[0],
    );

    let title_line = Line::from(vec![
        Span::styled(
            format!(" {:>4} ", "L#"),
            Style::default().fg(Color::Rgb(130, 140, 160)),
        ),
        Span::styled(
            truncate_str("Left", (chunks[1].width as usize).saturating_sub(24) / 2),
            Style::default().fg(Color::Rgb(130, 140, 160)),
        ),
        Span::raw("  "),
        Span::styled(" M ", Style::default().fg(Color::Rgb(130, 140, 160))),
        Span::raw("  "),
        Span::styled(
            format!(" {:>4} ", "R#"),
            Style::default().fg(Color::Rgb(130, 140, 160)),
        ),
        Span::styled(
            truncate_str("Right", (chunks[1].width as usize).saturating_sub(24) / 2),
            Style::default().fg(Color::Rgb(130, 140, 160)),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(title_line).style(Style::default().bg(Color::Rgb(24, 24, 32))),
        chunks[1],
    );

    let list_area = chunks[2];
    let visible_h = list_area.height as usize;
    let mut scroll = state.scroll;
    if state.cursor < scroll {
        scroll = state.cursor;
    } else if state.cursor >= scroll.saturating_add(visible_h) {
        scroll = state.cursor + 1 - visible_h;
    }

    let avail = list_area.width as usize;
    let left_w = avail.saturating_sub(15) / 2;
    let right_w = avail.saturating_sub(15) - left_w;

    let items = if state.rows.is_empty() {
        vec![ListItem::new(Line::from(vec![Span::styled(
            state.message.as_deref().unwrap_or("No differences"),
            Style::default().fg(Color::Rgb(150, 200, 150)),
        )]))]
    } else {
        state
            .rows
            .iter()
            .enumerate()
            .skip(scroll)
            .take(visible_h)
            .map(|(idx, row)| {
                let selected = idx == state.cursor;
                let zebra = if idx % 2 == 0 {
                    Color::Rgb(18, 18, 24)
                } else {
                    Color::Rgb(22, 22, 32)
                };
                let bg = if selected { CLR_CURSOR_BG } else { zebra };
                let fg = if selected {
                    CLR_CURSOR_FG
                } else if row.kind == crate::app::CompareRowKind::Equal {
                    Color::Rgb(120, 125, 140)
                } else {
                    Color::Rgb(220, 225, 235)
                };
                let (marker, marker_color) = compare_marker(row.kind);
                let left_no = row
                    .left_no
                    .map(|value| format!("{value:>4}"))
                    .unwrap_or_else(|| "    ".to_string());
                let right_no = row
                    .right_no
                    .map(|value| format!("{value:>4}"))
                    .unwrap_or_else(|| "    ".to_string());
                let left_text = truncate_str(&row.left_text, left_w);
                let right_text = truncate_str(&row.right_text, right_w);
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {left_no} "), Style::default().fg(fg).bg(bg)),
                    Span::styled(left_text, Style::default().fg(fg).bg(bg)),
                    Span::styled("  ", Style::default().bg(bg)),
                    Span::styled(
                        format!(" {marker} "),
                        Style::default()
                            .fg(marker_color)
                            .bg(bg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  ", Style::default().bg(bg)),
                    Span::styled(format!(" {right_no} "), Style::default().fg(fg).bg(bg)),
                    Span::styled(right_text, Style::default().fg(fg).bg(bg)),
                ]))
            })
            .collect::<Vec<_>>()
    };

    safe_render_widget(
        f,
        List::new(items).style(Style::default().bg(Color::Rgb(18, 18, 24))),
        list_area,
    );

    let footer = if state.rows.is_empty() {
        if state.search_active {
            " Search: type text, Enter confirm, Esc cancel, Up/Down next match ".to_string()
        } else {
            " / search  D:diff only  W:ignore spaces  L:ignore CR/LF  Esc/Enter: close ".to_string()
        }
    } else {
        if state.search_active {
            format!(
                " Search: type text, Enter confirm, Esc cancel, Up/Down next match  {} row(s) ",
                state.rows.len()
            )
        } else {
            format!(
                " {} row(s)  Up/Down navigate  PgUp/PgDn move  / search  n/N next-prev  D/W/L toggle options  Esc/Enter close ",
                state.rows.len()
            )
        }
    };
    safe_render_widget(
        f,
        Paragraph::new(footer).style(
            Style::default()
                .fg(Color::Rgb(150, 160, 180))
                .bg(Color::Rgb(18, 18, 24)),
        ),
        chunks[3],
    );
}
