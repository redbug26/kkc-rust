use super::*;
use super::bookmarks::highlight_tokens;
use crate::remote::RemoteKind;

pub(super) fn render_remote_connect(f: &mut Frame, state: &RemoteConnectState, area: Rect) {
    let width = 76u16.min(area.width.saturating_sub(4));
    let height = 20u16.min(area.height.saturating_sub(2)).max(10);
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
    let block = Block::default()
        .title(Span::styled(
            " Remote Connections ",
            Style::default()
                .fg(CLR_MENU_BAR_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER).bg(CLR_MENU_DD_BG))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    if inner.height < 4 {
        return;
    }

    let input_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let sep_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: 1,
    };
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(3),
    };
    let hint_area = clamp_rect(
        area,
        Rect {
            x: inner.x,
            y: inner.y + inner.height - 1,
            width: inner.width,
            height: 1,
        },
    );

    let matches = state.filtered_indices();
    let total = matches.len();
    let count_hint = if state.query.is_empty() {
        format!(" {} ", state.items.len())
    } else if total > 0 {
        format!(" {}/{} ", state.match_pos + 1, total)
    } else {
        " 0/0 ".to_owned()
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" ⌕ {}\u{2581}", state.query);
    let input_row = Line::from(vec![
        Span::styled(
            truncate_str(&input_text, input_inner_w),
            Style::default().fg(CLR_QS_INPUT_FG).bg(CLR_QS_INPUT_BG),
        ),
        Span::styled(
            count_hint,
            Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_INPUT_BG),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(CLR_QS_INPUT_BG)),
        input_area,
    );

    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_MENU_DD_BG)),
        sep_area,
    );

    let rows = list_area.height as usize;
    let tokens: Vec<String> = state
        .query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();
    let scroll = if state.match_pos >= rows && rows > 0 {
        state.match_pos - rows + 1
    } else {
        0
    };

    let items = if state.items.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            " No server entry found in ~/.ssh/config or connections.toml ",
            Style::default().fg(CLR_UNKNOWN),
        )))]
    } else if matches.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            " No matching connection ",
            Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_MENU_DD_BG),
        )))]
    } else {
        matches
            .iter()
            .enumerate()
            .skip(scroll)
            .take(rows)
            .map(|(match_idx, item_idx)| {
                let item = &state.items[*item_idx];
                let protocol = item.protocol();
                let (r, g, b) = protocol.color_rgb();
                let proto = match &item.kind {
                    RemoteKind::RemotePlugin(plugin) => plugin.scheme.as_str(),
                    _ => protocol.name(),
                };
                let proto_style = Style::default().fg(Color::Rgb(r, g, b)).bg(CLR_MENU_DD_BG);
                let (source, badge_style) = match item.source {
                    RemoteSource::SshConfig => (
                        "ssh",
                        Style::default()
                            .fg(Color::Rgb(255, 208, 124))
                            .bg(CLR_MENU_DD_BG),
                    ),
                    RemoteSource::UserToml => (
                        "toml",
                        Style::default()
                            .fg(Color::Rgb(246, 237, 212))
                            .bg(CLR_MENU_DD_BG),
                    ),
                    RemoteSource::PluginAuto => (
                        "plugin",
                        Style::default()
                            .fg(Color::Rgb(168, 232, 174))
                            .bg(CLR_MENU_DD_BG),
                    ),
                };
                let host = item.host_label();
                let selected = match_idx == state.match_pos;
                let row_style = if selected {
                    Style::default()
                        .fg(CLR_MENU_SEL_FG)
                        .bg(CLR_MENU_SEL_BG)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
                };
                let badge_style = if selected {
                    badge_style.bg(CLR_MENU_SEL_BG).add_modifier(Modifier::BOLD)
                } else {
                    badge_style
                };
                let proto_style = if selected {
                    proto_style.bg(CLR_MENU_SEL_BG).add_modifier(Modifier::BOLD)
                } else {
                    proto_style
                };
                let alias_style = row_style.add_modifier(Modifier::BOLD);
                let host_style = if selected {
                    Style::default()
                        .fg(CLR_MENU_SEL_FG)
                        .bg(CLR_MENU_SEL_BG)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Rgb(198, 184, 156))
                        .bg(CLR_MENU_DD_BG)
                };
                let alias_line = highlight_tokens(
                    &format!("{:<16}", truncate_str(&item.name, 16)),
                    &tokens,
                    alias_style.fg.unwrap_or(CLR_MENU_DD_FG),
                    alias_style.bg.unwrap_or(CLR_MENU_DD_BG),
                    CLR_QS_MATCH_HI_SEL,
                );
                let host_text = truncate_str(&host, inner.width.saturating_sub(35) as usize);
                let host_line = highlight_tokens(
                    &host_text,
                    &tokens,
                    host_style.fg.unwrap_or(CLR_MENU_DD_FG),
                    host_style.bg.unwrap_or(CLR_MENU_DD_BG),
                    if selected {
                        CLR_QS_MATCH_HI_SEL
                    } else {
                        CLR_QS_MATCH_HI
                    },
                );
                let mut spans = vec![Span::styled(" ", row_style)];
                spans.extend(alias_line.spans);
                spans.extend([
                    Span::styled(" ", row_style),
                    Span::styled(format!("{:^6}", proto), proto_style),
                    Span::styled(" ", row_style),
                    Span::styled(format!("{:^6}", source), badge_style),
                    Span::styled("  ", row_style),
                ]);
                spans.extend(host_line.spans);
                let used: usize = spans.iter().map(|s| s.content.len()).sum();
                if used < list_area.width as usize {
                    spans.push(Span::styled(
                        " ".repeat(list_area.width as usize - used),
                        row_style,
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect()
    };
    safe_render_widget(f, List::new(items), list_area);
    safe_render_widget(
        f,
        Paragraph::new(" Type:Filter  Enter:Connect  Tab:SSH  F6:Edit  F7:Add  Esc:Cancel ")
            .style(Style::default().fg(CLR_BUTTON_FG).bg(CLR_STATUS_BG)),
        hint_area,
    );
}

pub(super) fn render_remote_add_menu(
    f: &mut Frame,
    choices: &[RemoteEditKind],
    cursor: usize,
    area: Rect,
) {
    let width: u16 = 22;
    let height: u16 = (choices.len() as u16) + 4; // border(2) + title row + items + hint
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
    let block = Block::default()
        .title(Span::styled(
            " Add Connection ",
            Style::default()
                .fg(CLR_MENU_BAR_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER).bg(CLR_MENU_DD_BG))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    for (i, kind) in choices.iter().enumerate() {
        let (r, g, b) = kind.color_rgb();
        let label = kind.name();
        let row = Rect {
            x: inner.x,
            y: inner.y + i as u16,
            width: inner.width,
            height: 1,
        };
        let selected = i == cursor;
        let text = if selected {
            format!(" ► {:<16}", label)
        } else {
            format!("   {:<16}", label)
        };
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(r, g, b))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(r, g, b)).bg(CLR_MENU_DD_BG)
        };
        safe_render_widget(f, Paragraph::new(text).style(style), row);
    }

    // hint row
    let hint_row = Rect {
        x: inner.x,
        y: inner.y + choices.len() as u16,
        width: inner.width,
        height: 1,
    };
    safe_render_widget(
        f,
        Paragraph::new(" ↑↓:Select  Enter:OK  Esc ")
            .style(Style::default().fg(CLR_BUTTON_FG).bg(CLR_STATUS_BG)),
        hint_row,
    );
}

pub(super) fn render_remote_edit(f: &mut Frame, state: &RemoteEditState, area: Rect) {
    let width = 72u16.min(area.width.saturating_sub(4));
    let height = 14u16.min(area.height.saturating_sub(2)).max(10);
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
    let block = Block::default()
        .title(Span::styled(
            state.kind.title(),
            Style::default()
                .fg(CLR_MENU_BAR_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER).bg(CLR_MENU_DD_BG))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    let labels = state.kind.field_labels();
    let value_w = (inner.width as usize).saturating_sub(9);
    let mut lines = Vec::new();
    for (idx, label) in labels.iter().enumerate() {
        let selected = state.cursor == idx;
        // Label: always dark background; arrow prefix on selected row
        let label_style = Style::default()
            .fg(CLR_HEADER_FG)
            .bg(CLR_MENU_DD_BG)
            .add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        let prefix = if selected { ">" } else { " " };
        // Active input field: white bg / black fg so the terminal cursor is clearly visible
        let value_style = if selected {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{}{:<8}", prefix, format!("{label}:")), label_style),
            Span::styled(
                format!("{:<width$}", state.fields[idx], width = value_w),
                value_style,
            ),
        ]));
    }
    lines.push(Line::default());
    let save_style = if state.cursor == RemoteEditState::SAVE {
        Style::default()
            .fg(CLR_MENU_SEL_FG)
            .bg(CLR_MENU_SEL_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
    };
    let cancel_style = if state.cursor == RemoteEditState::CANCEL {
        Style::default()
            .fg(CLR_MENU_SEL_FG)
            .bg(CLR_MENU_SEL_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
    };
    lines.push(Line::from(vec![
        Span::styled(" [ Save ] ", save_style),
        Span::raw("  "),
        Span::styled(" [ Cancel ] ", cancel_style),
    ]));
    lines.push(Line::default());
    let hint_text = if state.plugin_auth_enabled
        && state.is_remote_plugin()
        && state.cursor == RemoteEditState::PORT
    {
        " Tab:Next  F5:Auth start  F6:Auth complete  Esc:Cancel "
    } else if matches!(&state.kind, crate::app::RemoteEditKind::Smb)
        && state.cursor == RemoteEditState::PATH
        && state.share_picker.is_none()
    {
        " Tab:Next  F5:Browse shares  Esc:Cancel "
    } else {
        " Tab/Shift-Tab:Next  Enter:Select  Esc:Cancel "
    };
    lines.push(Line::from(Span::styled(
        hint_text,
        Style::default().fg(CLR_UNKNOWN).bg(CLR_MENU_DD_BG),
    )));
    safe_render_widget(
        f,
        Paragraph::new(lines).style(Style::default().bg(CLR_MENU_DD_BG)),
        inner,
    );
    if state.cursor < 6 {
        let cursor_x =
            (inner.x + 9 + state.input_cursor as u16).min(inner.x + inner.width.saturating_sub(2));
        let cursor_y = inner.y + state.cursor as u16;
        safe_set_cursor_position(f, cursor_x, cursor_y);
    }

    // ── SMB share picker dropdown ─────────────────────────────────────────
    if let Some((ref shares, picker_cur)) = state.share_picker {
        // Anchor: Share field is at cursor row PATH (4); dropdown sits below it.
        const PATH_ROW: u16 = crate::app::RemoteEditState::PATH as u16;
        let dd_x = inner.x + 9;
        let dd_y = inner.y + PATH_ROW + 1;
        let dd_w = inner.width.saturating_sub(9).min(40).max(16);
        let max_visible: usize = 8;
        let visible = shares.len().min(max_visible);
        let dd_h = (visible as u16 + 2).min(area.height.saturating_sub(dd_y));

        let dd_area = clamp_rect(
            area,
            Rect {
                x: dd_x,
                y: dd_y,
                width: dd_w,
                height: dd_h,
            },
        );
        safe_render_widget(f, Clear, dd_area);
        let dd_block = Block::default()
            .title(Span::styled(
                " Shares ",
                Style::default().fg(CLR_MENU_BAR_FG).bg(CLR_MENU_DD_BG),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(CLR_QS_BORDER).bg(CLR_MENU_DD_BG))
            .style(Style::default().bg(CLR_MENU_DD_BG));
        let dd_inner = dd_block.inner(dd_area);
        safe_render_widget(f, dd_block, dd_area);

        let scroll = if picker_cur >= max_visible {
            picker_cur - max_visible + 1
        } else {
            0
        };
        for (row, idx) in (scroll..shares.len())
            .take(dd_inner.height as usize)
            .enumerate()
        {
            let selected = idx == picker_cur;
            let (fg, bg) = if selected {
                (CLR_MENU_SEL_FG, CLR_MENU_SEL_BG)
            } else {
                (CLR_MENU_DD_FG, CLR_MENU_DD_BG)
            };
            let marker = if selected { "▶ " } else { "  " };
            let name = truncate_str(&shares[idx], dd_inner.width.saturating_sub(2) as usize);
            let padded = format!(
                "{}{:<width$}",
                marker,
                name,
                width = dd_inner.width.saturating_sub(2) as usize
            );
            safe_render_widget(
                f,
                Paragraph::new(padded).style(Style::default().fg(fg).bg(bg)),
                Rect {
                    x: dd_inner.x,
                    y: dd_inner.y + row as u16,
                    width: dd_inner.width,
                    height: 1,
                },
            );
        }
    }
}

pub(super) fn render_remote_connecting(f: &mut Frame, state: &RemoteConnectingState, area: Rect) {
    let width = 46u16.min(area.width.saturating_sub(4)).max(30);
    let height = 7u16.min(area.height.saturating_sub(2)).max(6);
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
    let block = Block::default()
        .title(Span::styled(
            " Connecting ",
            Style::default()
                .fg(CLR_MENU_BAR_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER).bg(CLR_MENU_DD_BG))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    let lines = vec![
        Line::from(Span::styled(
            format!(" {} connection in progress", state.protocol_label),
            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG),
        )),
        Line::from(Span::styled(
            format!(" {}", state.profile_name),
            Style::default()
                .fg(CLR_HEADER_FG)
                .bg(CLR_MENU_DD_BG)
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(
            " Please wait... ",
            Style::default().fg(CLR_TEXT).bg(CLR_MENU_DD_BG),
        )),
        Line::from(Span::styled(
            format!(" {}", state.phase),
            Style::default().fg(CLR_HEADER_FG).bg(CLR_MENU_DD_BG),
        )),
        Line::default(),
        Line::from(Span::styled(
            " Esc/Enter/F10:Abort ",
            Style::default().fg(CLR_UNKNOWN).bg(CLR_MENU_DD_BG),
        )),
    ];
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(CLR_MENU_DD_BG)),
        inner,
    );
}

// ---------------------------------------------------------------------------
// Search panel
// ---------------------------------------------------------------------------

