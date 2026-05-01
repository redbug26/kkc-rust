use super::*;

pub(super) fn render_confirm(f: &mut Frame, dlg: &ConfirmDialog, area: Rect) {
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

pub(super) fn render_input(f: &mut Frame, dlg: &InputDialog, area: Rect) {
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

