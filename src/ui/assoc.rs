use super::*;

pub(crate) fn assoc_editor_shortcuts() -> Vec<FooterShortcut> {
    vec![
        FooterShortcut {
            label: "Enter:Edit",
            key: KeyCode::Enter,
        },
        FooterShortcut {
            label: "A:Add",
            key: KeyCode::Char('a'),
        },
        FooterShortcut {
            label: "Del:Delete",
            key: KeyCode::Delete,
        },
        FooterShortcut {
            label: "Esc:Close",
            key: KeyCode::Esc,
        },
    ]
}

pub(super) fn render_opener(f: &mut Frame, s: &OpenerState, area: Rect, preferred_area: Rect) {
    enum DisplayRow {
        Header(&'static str),
        Item { match_row: usize, item_idx: usize },
    }

    let matches = s.filtered_indices();
    let mut rows = Vec::new();
    let mut last_category: Option<&str> = None;
    for (match_row, item_idx) in matches.iter().copied().enumerate() {
        let item = &s.items[item_idx];
        if last_category != Some(item.category) {
            rows.push(DisplayRow::Header(item.category));
            last_category = Some(item.category);
        }
        rows.push(DisplayRow::Item {
            match_row,
            item_idx,
        });
    }

    let mime_type = crate::idf::probe_path(&s.path)
        .map(|info| info.mime_type)
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let title = format!(" Open {} ", mime_type);
    let hint = "  ↑↓ Select  Enter Open  Esc Close";
    let max_label_width = matches
        .iter()
        .filter_map(|idx| s.items.get(*idx))
        .map(|item| item.label.chars().count())
        .max()
        .unwrap_or(12)
        .min(28);
    let max_detail_width = matches
        .iter()
        .filter_map(|idx| s.items.get(*idx))
        .map(|item| item.detail.chars().count())
        .max()
        .unwrap_or(0)
        .min(28);
    let content_w = (max_label_width + max_detail_width + 10).max(title.chars().count() + 4);
    let hint_w = hint.chars().count();
    let max_w = area.width.saturating_sub(4).max(1) as usize;
    let w = content_w.max(hint_w).max(36).min(max_w) as u16;
    let row_count = if matches.is_empty() {
        1
    } else {
        rows.len().max(1)
    } as u16;
    let max_h = area.height.saturating_sub(4).max(6);
    let h = (row_count + 5).min(24).min(max_h).max(6);
    let place_in_panel =
        w <= preferred_area.width.saturating_sub(2) && h <= preferred_area.height.saturating_sub(2);
    let target = if place_in_panel { preferred_area } else { area };
    let x = target.x + (target.width.saturating_sub(w)) / 2;
    let y = target.y + (target.height.saturating_sub(h)) / 2;
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
            Block::default().style(Style::default().bg(Color::Black)),
            sh,
        );
    }
    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_QS_BORDER).bg(CLR_QS_BG))
        .title(Span::styled(
            title,
            Style::default()
                .fg(CLR_QS_INPUT_FG)
                .bg(CLR_QS_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(CLR_QS_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    let count_hint = if s.query.is_empty() {
        format!(" {}/{} ", s.match_pos + 1, s.items.len())
    } else if matches.is_empty() {
        " 0/0 ".to_string()
    } else {
        format!(" {}/{} ", s.match_pos + 1, matches.len())
    };
    let hint_w = count_hint.len() as u16;
    let input_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" ⌕ {}▁", s.query);
    let input_row = Line::from(vec![
        Span::styled(
            truncate_str(&input_text, input_w),
            Style::default().fg(CLR_QS_INPUT_FG).bg(CLR_QS_INPUT_BG),
        ),
        Span::styled(
            count_hint,
            Style::default().fg(CLR_QS_MATCH_HI).bg(CLR_QS_INPUT_BG),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(CLR_QS_INPUT_BG)),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    safe_render_widget(
        f,
        Paragraph::new(hint).style(Style::default().fg(CLR_QS_LIST_FG).bg(CLR_QS_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );
    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: 1,
        },
    );

    let list_y = inner.y + 3;
    let list_h = inner.height.saturating_sub(3) as usize;

    if matches.is_empty() {
        safe_render_widget(
            f,
            Paragraph::new("  (no match)")
                .style(Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_BG)),
            Rect {
                x: inner.x,
                y: list_y,
                width: inner.width,
                height: 1,
            },
        );
        return;
    }

    let selected_row = rows
        .iter()
        .position(|row| {
            matches!(
                row,
                DisplayRow::Item { match_row, .. } if *match_row == s.match_pos
            )
        })
        .unwrap_or(0);
    let scroll = if selected_row < list_h {
        0
    } else {
        selected_row.saturating_sub(list_h - 1)
    };

    for (display_offset, row_data) in rows.iter().skip(scroll).take(list_h).enumerate() {
        let row = list_y + display_offset as u16;
        match row_data {
            DisplayRow::Header(category) => {
                safe_render_widget(
                    f,
                    Paragraph::new(format!("  {}", category)).style(
                        Style::default()
                            .fg(CLR_QS_DIR_FG)
                            .bg(CLR_QS_BG)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Rect {
                        x: inner.x,
                        y: row,
                        width: inner.width,
                        height: 1,
                    },
                );
            }
            DisplayRow::Item {
                match_row,
                item_idx,
            } => {
                let item = &s.items[*item_idx];
                let selected = s.match_pos == *match_row;
                let style = if selected {
                    Style::default()
                        .fg(CLR_QS_SEL_FG)
                        .bg(CLR_QS_SEL_BG)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(CLR_QS_LIST_FG).bg(CLR_QS_BG)
                };
                let icon = if selected { " > " } else { "   " };
                let available = inner.width as usize;
                let detail_w = max_detail_width.min(available.saturating_sub(8));
                let label_w = available.saturating_sub(detail_w).saturating_sub(5).max(8);
                let text = format!(
                    "{}{} {}",
                    icon,
                    truncate_str(&item.label, label_w),
                    truncate_str(&item.detail, detail_w)
                );
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
    const W: u16 = 88;
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
            Block::default().style(Style::default().bg(Color::Black)),
            sh,
        );
    }
    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_QS_BORDER).bg(CLR_QS_BG))
        .title(Span::styled(
            " Associations ",
            Style::default()
                .fg(CLR_QS_INPUT_FG)
                .bg(CLR_QS_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(CLR_QS_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    let matches = s.filtered_indices();
    let count_hint = if matches.is_empty() {
        " 0/0 ".to_string()
    } else {
        format!(" {}/{} ", s.match_pos + 1, matches.len())
    };
    let hint_w = count_hint.len() as u16;
    let input_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" search {}_", s.query);
    let input_row = Line::from(vec![
        Span::styled(
            truncate_str(&input_text, input_w),
            Style::default().fg(CLR_QS_INPUT_FG).bg(CLR_QS_INPUT_BG),
        ),
        Span::styled(
            count_hint,
            Style::default().fg(CLR_QS_MATCH_HI).bg(CLR_QS_INPUT_BG),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(CLR_QS_INPUT_BG)),
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

    // Column header
    let header = format!("  {:<28} {}", "MIME type", "Commands");
    safe_render_widget(
        f,
        Paragraph::new(header).style(
            Style::default()
                .fg(CLR_QS_DIR_FG)
                .bg(CLR_QS_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new(sep.clone()).style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 3,
            width: inner.width,
            height: 1,
        },
    );

    // List rows
    let list_h = inner.height.saturating_sub(6) as usize;
    let start = if matches.is_empty() || s.match_pos < list_h {
        0
    } else {
        s.match_pos.saturating_sub(list_h - 1)
    };

    if matches.is_empty() {
        safe_render_widget(
            f,
            Paragraph::new("  (no match)")
                .style(Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_BG)),
            Rect {
                x: inner.x,
                y: inner.y + 4,
                width: inner.width,
                height: 1,
            },
        );
    } else {
        for (filtered_row, draw_row) in (start..).zip(0..list_h) {
            if filtered_row >= matches.len() {
                break;
            }
            let row_y = inner.y + 4 + draw_row as u16;
            if row_y >= inner.y + inner.height {
                break;
            }
            let assoc_idx = matches[filtered_row];
            let (mime_type, openers) = &s.assocs[assoc_idx];
            let selected = s.match_pos == filtered_row;
            let style = if selected {
                Style::default()
                    .fg(CLR_QS_SEL_FG)
                    .bg(CLR_QS_SEL_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(CLR_QS_LIST_FG).bg(CLR_QS_BG)
            };
            let icon = if selected { ">" } else { " " };
            let openers_str = openers.join(" | ");
            let avail = inner.width.saturating_sub(32) as usize;
            let openers_disp = if openers_str.chars().count() > avail {
                format!("{}...", truncate_str(&openers_str, avail.saturating_sub(3)))
            } else {
                openers_str
            };
            let text = format!(" {} {:<28} {}", icon, mime_type, openers_disp);
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
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        Rect {
            x: inner.x,
            y: hint_sep_y,
            width: inner.width,
            height: 1,
        },
    );
    let hint_items = footer_shortcut_items(&assoc_editor_shortcuts());
    render_shortcut_bar(
        f,
        Rect {
            x: inner.x,
            y: hint_sep_y + 1,
            width: inner.width,
            height: 1,
        },
        &hint_items,
        secondary_shortcut_bar_style(),
    );
}
