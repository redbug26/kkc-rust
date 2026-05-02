use super::*;

pub(super) fn render_opener(f: &mut Frame, s: &OpenerState, area: Rect) {
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

    let mime_type = crate::idf::probe_path(&s.path)
        .map(|info| info.mime_type)
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let title = format!(" Open {} ", mime_type);
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

pub(super) fn render_action_palette(f: &mut Frame, s: &ActionPaletteState, area: Rect) {
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

pub(super) fn render_assoc_editor(f: &mut Frame, s: &AssocEditorState, area: Rect) {
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
    let header = format!("  {:<24} {}", "MIME type", "Openers");
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
            let (mime_type, openers) = &s.assocs[list_row];
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
            let avail = inner.width.saturating_sub(28) as usize;
            let openers_disp = if openers_str.len() > avail {
                format!("{}…", &openers_str[..avail.saturating_sub(1)])
            } else {
                openers_str
            };
            let text = format!(" {} {:<24} {}", icon, mime_type, openers_disp);
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
