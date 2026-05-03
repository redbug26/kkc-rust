use super::*;

pub(crate) fn plugins_shortcuts() -> Vec<FooterShortcut> {
    vec![
        FooterShortcut {
            label: "\u{23ce}:OpenDir",
            key: KeyCode::Enter,
        },
        FooterShortcut {
            label: "Ctrl+S:Store",
            key: KeyCode::Char('s'),
        },
        FooterShortcut {
            label: "\u{232B}:Remove",
            key: KeyCode::Delete,
        },
        FooterShortcut {
            label: "\u{238B}:Close",
            key: KeyCode::Esc,
        },
    ]
}

pub(super) fn render_plugins(f: &mut Frame, s: &PluginsState, area: Rect) {
    let matches = s.filtered_indices();
    let total = matches.len();

    let w: u16 = area.width.saturating_sub(4).min(130).max(80);
    let h: u16 = area.height.saturating_sub(4).min(28).max(20);
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

    let sh = Rect {
        x: popup.x + 2,
        y: popup.y + 1,
        width: popup.width,
        height: popup.height,
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
            " Plugins ",
            Style::default()
                .fg(CLR_BUTTON_FG)
                .bg(CLR_APP_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(CLR_APP_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    // ── Directory line ──────────────────────────────────────────────────
    let dir_line = format!("  Directory: {}", s.plugins_dir.display());
    safe_render_widget(
        f,
        Paragraph::new(truncate_str(&dir_line, inner.width as usize))
            .style(Style::default().fg(Color::Rgb(72, 48, 28)).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    // ── Horizontal separator after dir line ─────────────────────────────
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

    let count_hint = if !s.query.is_empty() && total > 0 {
        format!(" {}/{} ", s.cursor + 1, total)
    } else if !s.query.is_empty() {
        " 0/0 ".to_owned()
    } else {
        format!(" {} ", s.plugins.len())
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" \u{2315} {}\u{2581}", s.query);
    let input_row = Line::from(vec![
        Span::styled(
            truncate_str(&input_text, input_inner_w),
            Style::default()
                .fg(Color::Rgb(34, 20, 12))
                .bg(Color::Rgb(232, 220, 192)),
        ),
        Span::styled(
            count_hint,
            Style::default()
                .fg(Color::Rgb(88, 66, 45))
                .bg(Color::Rgb(232, 220, 192)),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(Color::Rgb(232, 220, 192))),
        Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new(sep.clone()).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 3,
            width: inner.width,
            height: 1,
        },
    );

    // ── Footer separator + buttons ──────────────────────────────────────
    let button_y = inner.y + inner.height.saturating_sub(1);
    let footer_sep_y = button_y.saturating_sub(1);
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: footer_sep_y,
            width: inner.width,
            height: 1,
        },
    );
    let hint_area = Rect {
        x: inner.x,
        y: button_y,
        width: inner.width,
        height: 1,
    };
    let hint_items = footer_shortcut_items(&plugins_shortcuts());
    render_shortcut_bar(f, hint_area, &hint_items, default_shortcut_bar_style());

    // ── Body area (between dir separator and footer separator) ──────────
    let body_y = inner.y + 4;
    let body_h = footer_sep_y.saturating_sub(body_y);
    if body_h == 0 {
        return;
    }
    let body = Rect {
        x: inner.x,
        y: body_y,
        width: inner.width,
        height: body_h,
    };

    // Left column width: longest name + source tag + icon, at least 32, at most 56.
    let max_name = s
        .plugins
        .iter()
        .map(|p| {
            let src = crate::plugins::plugin_source_label(&p.dir, &s.plugins_dir);
            p.name.len() + src.len() + 6
        })
        .max()
        .unwrap_or(8);
    let left_w = ((max_name + 4) as u16)
        .clamp(32, 56)
        .min(body.width.saturating_sub(28));
    let right_w = body.width.saturating_sub(left_w + 1); // +1 for separator

    let left_area = Rect {
        x: body.x,
        y: body.y,
        width: left_w,
        height: body.height,
    };
    let sep_col = body.x + left_w;
    let right_area = Rect {
        x: sep_col + 1,
        y: body.y,
        width: right_w,
        height: body.height,
    };

    // ── Vertical separator ──────────────────────────────────────────────
    for row in 0..body.height {
        safe_render_widget(
            f,
            Paragraph::new("│").style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
            Rect {
                x: sep_col,
                y: body.y + row,
                width: 1,
                height: 1,
            },
        );
    }

    // ── Left: column header + plugin name list ──────────────────────────
    safe_render_widget(
        f,
        Paragraph::new(format!(
            "  {:<w$}",
            "Name                              Source",
            w = (left_w as usize).saturating_sub(2)
        ))
        .style(
            Style::default()
                .fg(CLR_HEADER_FG)
                .bg(CLR_HEADER_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Rect {
            x: left_area.x,
            y: left_area.y,
            width: left_area.width,
            height: 1,
        },
    );

    let list_h = (body.height.saturating_sub(1)) as usize;
    let start = if total == 0 || s.cursor < list_h {
        0
    } else {
        s.cursor.saturating_sub(list_h - 1)
    };

    if total == 0 {
        let msg = if s.query.is_empty() {
            "  (no plugins)"
        } else {
            "  (no match)"
        };
        safe_render_widget(
            f,
            Paragraph::new(msg).style(Style::default().fg(Color::Rgb(72, 48, 28)).bg(CLR_APP_BG)),
            Rect {
                x: left_area.x,
                y: left_area.y + 1,
                width: left_area.width,
                height: 1,
            },
        );
    } else {
        for (match_row, idx) in (start..).zip(0..list_h) {
            if match_row >= total {
                break;
            }
            let plugin = &s.plugins[matches[match_row]];
            let row_y = left_area.y + 1 + idx as u16;
            let selected = s.cursor == match_row;
            let type_fg = match plugin.kind.as_str() {
                "Archive" => Color::Rgb(130, 68, 18),
                "Viewer" => Color::Rgb(26, 58, 108),
                "Action" => Color::Rgb(52, 92, 34),
                _ => Color::Rgb(46, 28, 16),
            };
            let style = if selected {
                Style::default()
                    .fg(Color::Rgb(16, 10, 6))
                    .bg(Color::Rgb(235, 220, 188))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(type_fg).bg(CLR_APP_BG)
            };
            let icon = if selected { "▶ " } else { "  " };
            let source = crate::plugins::plugin_source_label(&plugin.dir, &s.plugins_dir);
            let available = (left_area.width as usize).saturating_sub(3);
            let source_tag = format!("[{source}]");
            let name_w = available.saturating_sub(source_tag.len() + 1);
            let text = format!(
                "{icon}{:<name_w$} {}",
                truncate_str(&plugin.name, name_w),
                source_tag,
            );
            safe_render_widget(
                f,
                Paragraph::new(text).style(style),
                Rect {
                    x: left_area.x,
                    y: row_y,
                    width: left_area.width,
                    height: 1,
                },
            );
        }
    }

    // ── Right: detail panel for selected plugin ─────────────────────────
    if right_area.width < 10 || right_area.height < 3 {
        return;
    }

    safe_render_widget(
        f,
        Paragraph::new(format!(
            "  {:<w$}",
            "Details",
            w = (right_area.width as usize).saturating_sub(2)
        ))
        .style(
            Style::default()
                .fg(CLR_HEADER_FG)
                .bg(CLR_HEADER_BG)
                .add_modifier(Modifier::BOLD),
        ),
        Rect {
            x: right_area.x,
            y: right_area.y,
            width: right_area.width,
            height: 1,
        },
    );

    let detail_y = right_area.y + 1;
    let detail_h = right_area.height.saturating_sub(1);

    let Some(plugin) = matches.get(s.cursor).and_then(|idx| s.plugins.get(*idx)) else {
        return;
    };

    let lbl_style = Style::default()
        .fg(Color::Rgb(48, 64, 96))
        .bg(CLR_APP_BG)
        .add_modifier(Modifier::BOLD);
    let val_style = Style::default().fg(Color::Rgb(34, 20, 12)).bg(CLR_APP_BG);
    let dim_style = Style::default().fg(Color::Rgb(88, 66, 45)).bg(CLR_APP_BG);
    let rw = right_area.width as usize;

    let mut lines: Vec<(Rect, Paragraph)> = Vec::new();
    let mut row: u16 = 0;

    let push_kv = |lines: &mut Vec<(Rect, Paragraph)>,
                   row: &mut u16,
                   label: &str,
                   value: &str,
                   lbl: Style,
                   val: Style| {
        if *row >= detail_h {
            return;
        }
        let text = Line::from(vec![
            Span::styled(format!("  {label:<12}"), lbl),
            Span::styled(truncate_str(value, rw.saturating_sub(14)), val),
        ]);
        lines.push((
            Rect {
                x: right_area.x,
                y: detail_y + *row,
                width: right_area.width,
                height: 1,
            },
            Paragraph::new(text).style(Style::default().bg(CLR_APP_BG)),
        ));
        *row += 1;
    };

    push_kv(
        &mut lines,
        &mut row,
        "Type :",
        &plugin.kind,
        lbl_style,
        val_style,
    );
    push_kv(
        &mut lines,
        &mut row,
        "Version :",
        &plugin.version,
        lbl_style,
        val_style,
    );
    push_kv(
        &mut lines,
        &mut row,
        "Source :",
        crate::plugins::plugin_source_label(&plugin.dir, &s.plugins_dir),
        lbl_style,
        val_style,
    );

    // Blank row before mimes
    if row < detail_h {
        row += 1;
    }

    // Mimes / extensions
    if !plugin.extensions.is_empty() {
        if row < detail_h {
            let text = Line::from(vec![Span::styled("  Mimes :", lbl_style)]);
            lines.push((
                Rect {
                    x: right_area.x,
                    y: detail_y + row,
                    width: right_area.width,
                    height: 1,
                },
                Paragraph::new(text).style(Style::default().bg(CLR_APP_BG)),
            ));
            row += 1;
        }
        for mime in &plugin.extensions {
            if row >= detail_h {
                break;
            }
            let text = Line::from(vec![Span::styled(
                format!("    • {}", truncate_str(mime, rw.saturating_sub(6))),
                dim_style,
            )]);
            lines.push((
                Rect {
                    x: right_area.x,
                    y: detail_y + row,
                    width: right_area.width,
                    height: 1,
                },
                Paragraph::new(text).style(Style::default().bg(CLR_APP_BG)),
            ));
            row += 1;
        }
    }

    // Blank row before description
    if row < detail_h {
        row += 1;
    }

    // Description (may wrap)
    if !plugin.description.is_empty() && row < detail_h {
        let text = Line::from(vec![Span::styled("  Description :", lbl_style)]);
        lines.push((
            Rect {
                x: right_area.x,
                y: detail_y + row,
                width: right_area.width,
                height: 1,
            },
            Paragraph::new(text).style(Style::default().bg(CLR_APP_BG)),
        ));
        row += 1;

        let desc_indent = "    ";
        let max_w = rw.saturating_sub(desc_indent.len());
        let mut rest = plugin.description.as_str();
        while !rest.is_empty() && row < detail_h {
            let (chunk, remainder) = if rest.len() <= max_w {
                (rest, "")
            } else {
                let cut = rest[..max_w].rfind(' ').unwrap_or(max_w);
                (&rest[..cut], rest[cut..].trim_start())
            };
            let text = format!("{desc_indent}{chunk}");
            lines.push((
                Rect {
                    x: right_area.x,
                    y: detail_y + row,
                    width: right_area.width,
                    height: 1,
                },
                Paragraph::new(text).style(dim_style),
            ));
            row += 1;
            rest = remainder;
        }
    }

    // Fill remaining rows with background
    while row < detail_h {
        lines.push((
            Rect {
                x: right_area.x,
                y: detail_y + row,
                width: right_area.width,
                height: 1,
            },
            Paragraph::new("").style(Style::default().bg(CLR_APP_BG)),
        ));
        row += 1;
    }

    for (rect, para) in lines {
        safe_render_widget(f, para, rect);
    }
}
