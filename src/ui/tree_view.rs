use super::*;

pub(super) fn render_tree_view(f: &mut Frame, state: &TreeViewState, area: Rect) {
    let width = 110u16.min(area.width.saturating_sub(2));
    let height = (area.height * 4 / 5).clamp(18, area.height.saturating_sub(2));
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
            " Tree View ",
            Style::default()
                .fg(clr_header_fg())
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(clr_panel_border()))
        .style(Style::default().bg(clr_qs_bg()));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    if inner.height < 6 {
        return;
    }

    let progress_h = if state.scanning {
        state
            .progress_levels
            .len()
            .max(1)
            .min(inner.height.saturating_sub(4).max(1) as usize) as u16
    } else {
        1
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(progress_h),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let query_label = if state.query.is_empty() {
        "Filter".to_string()
    } else {
        format!("Filter: {}", state.query)
    };
    let root = truncate_path(
        &state.root.to_string_lossy(),
        inner.width.saturating_sub(32) as usize,
    );
    let top = Line::from(vec![
        Span::styled("  ", Style::default().bg(clr_qs_input_bg())),
        Span::styled(
            format!(" {}_ ", query_label),
            Style::default().fg(clr_qs_input_fg()).bg(clr_qs_input_bg()),
        ),
        Span::raw("  "),
        Span::styled(root, Style::default().fg(clr_qs_no_match())),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(top).style(Style::default().bg(clr_qs_bg())),
        chunks[0],
    );

    if state.scanning {
        let levels = if state.progress_levels.is_empty() {
            state
                .current
                .as_ref()
                .map(|path| {
                    vec![crate::tree_mode::TreeProgressLevel {
                        depth: 0,
                        ratio: state.progress,
                        path: path.clone(),
                    }]
                })
                .unwrap_or_default()
        } else {
            state.progress_levels.clone()
        };
        let visible_levels = levels
            .iter()
            .rev()
            .take(progress_h as usize)
            .collect::<Vec<_>>();
        for (row, level) in visible_levels.into_iter().rev().enumerate() {
            let area = Rect {
                x: chunks[1].x,
                y: chunks[1].y + row as u16,
                width: chunks[1].width,
                height: 1,
            };
            let label_room = inner.width.saturating_sub(24) as usize;
            let path = truncate_path(&level.path.to_string_lossy(), label_room);
            let level_name = if level.depth == 0 {
                "Root".to_string()
            } else {
                format!("Level {}", level.depth)
            };
            let label = format!("{level_name} {:>3}% {path}", (level.ratio * 100.0) as u8);
            safe_render_widget(
                f,
                Gauge::default()
                    .gauge_style(Style::default().fg(clr_header_fg()).bg(clr_qs_input_bg()))
                    .label(label)
                    .ratio(level.ratio.clamp(0.0, 1.0)),
                area,
            );
        }
    } else {
        let gauge_label = if let Some(scanned_at) = state.scanned_at.as_deref() {
            format!(
                "Cached: {} item(s), scanned {}",
                state.entries.len(),
                scanned_at
            )
        } else {
            format!("{} item(s)", state.entries.len())
        };
        safe_render_widget(
            f,
            Gauge::default()
                .gauge_style(Style::default().fg(clr_header_fg()).bg(clr_qs_input_bg()))
                .label(gauge_label)
                .ratio(1.0),
            chunks[1],
        );
    }

    let filtered = state.filtered_indices();
    let display = &state.display;
    let count_line = if state.scanning {
        " Esc: cancel  Ctrl+R/F5: refresh scan ".to_string()
    } else {
        format!(
            " {} match(es)  Enter: open directory  Ctrl+R/F5: refresh scan  Esc: close ",
            filtered.len()
        )
    };
    safe_render_widget(
        f,
        Paragraph::new(count_line).style(Style::default().fg(clr_qs_no_match()).bg(clr_qs_bg())),
        chunks[2],
    );

    let list_area = chunks[3];
    let visible_h = list_area.height as usize;
    // Anchor scroll to the selected match's position within display.
    let selected_disp = state.selected_display_pos();
    let mut scroll = state.scroll;
    if selected_disp < scroll {
        scroll = selected_disp;
    } else if selected_disp >= scroll.saturating_add(visible_h) {
        scroll = selected_disp + 1 - visible_h;
    }

    let items = display
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_h)
        .filter_map(|(disp_idx, item)| {
            use crate::tree_mode::DisplayItem;
            let (entry_idx, is_context) = match item {
                DisplayItem::Context(i) => (*i, true),
                DisplayItem::Match(i) => (*i, false),
            };
            let entry = state.entries.get(entry_idx)?;
            let selected = !is_context && disp_idx == selected_disp;
            let zebra = if disp_idx % 2 == 0 {
                clr_qs_bg()
            } else {
                clr_menu_dd_bg()
            };
            let bg = if selected { clr_cursor_bg() } else { zebra };
            let fg = if selected {
                clr_cursor_fg()
            } else if is_context {
                clr_qs_sep()
            } else {
                clr_dir()
            };
            let connector = tree_connector(
                entry.depth,
                state
                    .display_prefixes
                    .get(disp_idx)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                state.display_is_last.get(disp_idx).copied().unwrap_or(true),
            );
            let icon = "\u{e5ff} ";
            let available = list_area.width.saturating_sub(2) as usize;
            let text = truncate_str(&format!("{connector}{icon}{}", entry.name), available);
            let (connector_part, content_part) = if let Some(rest) = text.strip_prefix(&connector) {
                (connector.clone(), rest.to_string())
            } else {
                (text, String::new())
            };
            Some(ListItem::new(Line::from(vec![
                Span::styled(" ", Style::default().fg(fg).bg(bg)),
                Span::styled(connector_part, Style::default().fg(clr_tree()).bg(bg)),
                Span::styled(content_part, Style::default().fg(fg).bg(bg)),
            ])))
        })
        .collect::<Vec<_>>();

    safe_render_widget(
        f,
        List::new(items).style(Style::default().bg(clr_qs_bg())),
        list_area,
    );

    if display.len() > visible_h {
        let mut sb_state = ScrollbarState::new(display.len()).position(selected_disp);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(clr_qs_border())),
            list_area,
            &mut sb_state,
        );
    }

    safe_render_widget(
        f,
        Paragraph::new(" [Refresh] Ctrl+R / F5 ")
            .alignment(Alignment::Right)
            .style(Style::default().fg(Color::Black).bg(clr_button_bg())),
        chunks[4],
    );
}

fn tree_connector(depth: usize, prefix_flags: &[bool], is_last: bool) -> String {
    if depth == 0 {
        return String::new();
    }
    let mut s = String::with_capacity(depth * 3 + 4);
    // For ancestor depths 0..depth-1 draw │ or space.
    for &has_more in prefix_flags {
        s.push_str(if has_more { "│  " } else { "   " });
    }
    // At the entry's own depth draw the branch character.
    s.push_str(if is_last { "└─ " } else { "├─ " });
    s
}

// ---------------------------------------------------------------------------
// Directory history
// ---------------------------------------------------------------------------
