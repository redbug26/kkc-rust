use super::*;

pub(super) fn render_config(f: &mut Frame, cs: &ConfigState, area: Rect) {
    const W: u16 = 62;
    const H: u16 = 17;
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

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let tabs = Tabs::new(vec!["Behaviour", "Display", "Viewer", "External"])
        .select(cs.tab)
        .style(Style::default().fg(Color::Rgb(80, 60, 40)).bg(CLR_APP_BG))
        .highlight_style(
            Style::default()
                .fg(CLR_BUTTON_FG)
                .bg(CLR_APP_BG)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled(
            "  ",
            Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG),
        ));
    safe_render_widget(f, tabs, chunks[0]);

    let top_sep: String = std::iter::repeat('─')
        .take(chunks[1].width as usize)
        .collect();
    safe_render_widget(
        f,
        Paragraph::new(top_sep).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
        chunks[1],
    );

    let content = chunks[2];

    match cs.tab {
        ConfigState::TAB_BEHAVIOUR => {
            render_config_checkbox(f, content, 0, "Confirm exit", 0, cs.confirm_exit, cs.cursor);
            render_config_checkbox(
                f,
                content,
                1,
                "Confirm delete",
                1,
                cs.confirm_delete,
                cs.cursor,
            );
            render_config_checkbox(f, content, 2, "Auto reload", 2, cs.auto_reload, cs.cursor);
            render_config_checkbox(
                f,
                content,
                3,
                "Insert moves down",
                3,
                cs.insert_moves_down,
                cs.cursor,
            );
            render_config_checkbox(
                f,
                content,
                4,
                "Select directories",
                4,
                cs.select_dirs,
                cs.cursor,
            );
        }
        ConfigState::TAB_DISPLAY => {
            render_config_checkbox(
                f,
                content,
                0,
                "Show hidden files",
                5,
                cs.show_hidden,
                cs.cursor,
            );
            render_config_checkbox(
                f,
                content,
                1,
                "Color by type",
                6,
                cs.color_by_type,
                cs.cursor,
            );
            render_config_checkbox(
                f,
                content,
                2,
                "Cloud icons",
                7,
                cs.show_cloud_icons,
                cs.cursor,
            );
            render_config_checkbox(
                f,
                content,
                3,
                "File icons",
                8,
                cs.show_file_icons,
                cs.cursor,
            );
            render_config_checkbox(
                f,
                content,
                4,
                "Show F-key bar",
                9,
                cs.show_fkey_bar,
                cs.cursor,
            );
        }
        ConfigState::TAB_VIEWER => {
            render_config_checkbox(f, content, 0, "Word wrap", 10, cs.word_wrap, cs.cursor);
            render_config_checkbox(
                f,
                content,
                1,
                "Default zoom",
                11,
                cs.default_zoom,
                cs.cursor,
            );
            render_config_checkbox(f, content, 2, "Debug log", 12, cs.debug_log, cs.cursor);
        }
        ConfigState::TAB_EXTERNAL => {
            render_config_field(
                f,
                content,
                0,
                "Screensaver (min)",
                13,
                cs.screensaver_idle_minutes.as_str(),
                cs.cursor,
            );
            render_config_field(f, content, 3, "Editor", 14, cs.editor.as_str(), cs.cursor);
            render_config_field(f, content, 6, "Pager", 15, cs.pager.as_str(), cs.cursor);
            render_config_field(
                f,
                content,
                9,
                "History max",
                16,
                cs.dir_history_max.as_str(),
                cs.cursor,
            );
        }
        _ => {}
    }

    let sep: String = std::iter::repeat('─')
        .take(chunks[3].width as usize)
        .collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
        chunks[3],
    );

    // ── OK / Cancel buttons ────────────────────────────────────────────────
    let ok_idx = ConfigState::ok_cursor();
    let cancel_idx = ConfigState::cancel_cursor();
    let btn_y = chunks[4].y;
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

pub(super) fn render_config_checkbox(
    f: &mut Frame,
    area: Rect,
    row: u16,
    label: &str,
    cursor_idx: usize,
    checked: bool,
    cursor: usize,
) {
    if row >= area.height {
        return;
    }
    let selected = cursor == cursor_idx;
    let tick = if checked { "X" } else { " " };
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(CLR_CURSOR_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(50, 36, 22)).bg(CLR_APP_BG)
    };
    let text = format!("  [{}] {}", tick, label);
    let padded = format!("{:<width$}", text, width = area.width as usize);
    safe_render_widget(
        f,
        Paragraph::new(padded).style(style),
        Rect {
            x: area.x,
            y: area.y + row,
            width: area.width,
            height: 1,
        },
    );
}

pub(super) fn render_config_field(
    f: &mut Frame,
    area: Rect,
    row: u16,
    label: &str,
    cursor_idx: usize,
    value: &str,
    cursor: usize,
) {
    if row >= area.height {
        return;
    }
    let label_style = Style::default()
        .fg(Color::Rgb(80, 60, 40))
        .bg(CLR_APP_BG)
        .add_modifier(Modifier::BOLD);
    safe_render_widget(
        f,
        Paragraph::new(format!("  {}:", label)).style(label_style),
        Rect {
            x: area.x,
            y: area.y + row,
            width: area.width,
            height: 1,
        },
    );

    if row + 1 >= area.height {
        return;
    }
    let selected = cursor == cursor_idx;
    let field_w = area.width.saturating_sub(4);
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
    safe_render_widget(
        f,
        Paragraph::new(truncate_str(value, field_w as usize))
            .style(Style::default().fg(input_fg).bg(input_bg)),
        Rect {
            x: area.x + 2,
            y: area.y + row + 1,
            width: field_w,
            height: 1,
        },
    );
}

// ---------------------------------------------------------------------------
// Opener picker
// ---------------------------------------------------------------------------
