use super::*;

pub(super) fn render_copy_dialog(f: &mut Frame, dlg: &CopyDialogState, area: Rect) {
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
        .border_style(Style::default().fg(clr_menu_border()))
        .style(Style::default().bg(clr_menu_dd_bg()));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let dest_style = if dlg.field == CopyDialogState::DESTINATION {
        Style::default().fg(clr_menu_sel_fg()).bg(clr_menu_sel_bg())
    } else {
        Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
    };
    let overwrite_style = if dlg.field == CopyDialogState::OVERWRITE {
        Style::default()
            .fg(clr_menu_sel_fg())
            .bg(clr_menu_sel_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
    };
    let newer_style = if dlg.field == CopyDialogState::NEWER_ONLY {
        Style::default()
            .fg(clr_menu_sel_fg())
            .bg(clr_menu_sel_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
    };
    let keep_attr_style = if dlg.field == CopyDialogState::KEEP_ATTRIBUTES {
        Style::default()
            .fg(clr_menu_sel_fg())
            .bg(clr_menu_sel_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
    };
    let start_style = if dlg.field == CopyDialogState::START {
        Style::default()
            .fg(clr_menu_sel_fg())
            .bg(clr_menu_sel_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
    };
    let cancel_style = if dlg.field == CopyDialogState::CANCEL {
        Style::default()
            .fg(clr_menu_sel_fg())
            .bg(clr_menu_sel_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
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
                .fg(clr_header_fg())
                .bg(clr_menu_dd_bg())
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
            Style::default().fg(clr_unknown()).bg(clr_menu_dd_bg()),
        )),
        Line::from(Span::styled(
            counters,
            Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg()),
        )),
        Line::from(Span::styled(
            " Destination:",
            Style::default().fg(clr_header_fg()).bg(clr_menu_dd_bg()),
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
            Style::default().fg(clr_unknown()).bg(clr_menu_dd_bg()),
        )),
    ];
    safe_render_widget(
        f,
        Paragraph::new(lines).style(Style::default().bg(clr_menu_dd_bg())),
        inner,
    );

    if dlg.field == CopyDialogState::DESTINATION && !dlg.stats_pending && !dlg.waiting_to_start {
        let cursor_x =
            (inner.x + 1 + dlg.cursor as u16).min(inner.x + inner.width.saturating_sub(1));
        let cursor_y = inner.y + 3;
        safe_set_cursor_position(f, cursor_x, cursor_y);
    }
}

pub(super) fn render_copy_progress(f: &mut Frame, state: &CopyProgressState, area: Rect) {
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
        .border_style(Style::default().fg(clr_menu_border()))
        .style(Style::default().bg(clr_menu_dd_bg()));
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
                .fg(clr_header_fg())
                .bg(clr_menu_dd_bg())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("File  {}", progress_bar_string(bar_width, file_ratio)),
            Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg()),
        )),
        Line::from(Span::styled(
            format!("Total {}", progress_bar_string(bar_width, total_ratio)),
            Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg()),
        )),
        Line::from(Span::styled(
            format!(
                "{}/{}  {} / {} bytes",
                state.item_index, state.item_count, state.total_done, state.total_bytes
            ),
            Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg()),
        )),
        Line::from(Span::styled(
            format!(
                "Remaining: {}",
                state
                    .remaining_secs
                    .map(|s| format!("{s} sec"))
                    .unwrap_or_else(|| "--".into())
            ),
            Style::default().fg(clr_unknown()).bg(clr_menu_dd_bg()),
        )),
        Line::default(),
        Line::from(Span::styled(
            " Enter/Esc/F10:Abort",
            Style::default().fg(clr_unknown()).bg(clr_menu_dd_bg()),
        )),
    ];
    safe_render_widget(
        f,
        Paragraph::new(lines).style(Style::default().bg(clr_menu_dd_bg())),
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
