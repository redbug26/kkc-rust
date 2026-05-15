use super::bookmarks::highlight_tokens;
use super::*;
use crate::remote::RemoteKind;

fn remote_protocol_color(protocol: crate::remote::RemoteProtocol) -> Color {
    match protocol {
        crate::remote::RemoteProtocol::Sftp => clr_qs_dir_fg(),
        crate::remote::RemoteProtocol::Smb => clr_archive(),
        crate::remote::RemoteProtocol::RemotePlugin => clr_exec(),
    }
}

fn remote_edit_kind_color(kind: &RemoteEditKind) -> Color {
    match kind {
        RemoteEditKind::Sftp => clr_qs_dir_fg(),
        RemoteEditKind::Smb => clr_archive(),
        RemoteEditKind::RemotePlugin { .. } => clr_exec(),
    }
}

pub(crate) fn remote_connect_shortcuts() -> Vec<FooterShortcut> {
    vec![
        FooterShortcut {
            label: "a..z:Filter",
            key: KeyCode::Null,
        },
        FooterShortcut {
            label: " \u{23ce} :Connect",
            key: KeyCode::Enter,
        },
        FooterShortcut {
            label: " \u{21E5} :SSH",
            key: KeyCode::Tab,
        },
        FooterShortcut {
            label: "F6:Edit",
            key: KeyCode::F(6),
        },
        FooterShortcut {
            label: "F7:Add",
            key: KeyCode::F(7),
        },
        FooterShortcut {
            label: " \u{238B} :Cancel",
            key: KeyCode::Esc,
        },
    ]
}

pub(crate) fn remote_add_menu_shortcuts() -> Vec<FooterShortcut> {
    vec![
        FooterShortcut {
            label: " \u{23ce} :OK",
            key: KeyCode::Enter,
        },
        FooterShortcut {
            label: " \u{238B} :Cancel",
            key: KeyCode::Esc,
        },
    ]
}

pub(crate) fn remote_edit_shortcuts(state: &RemoteEditState) -> Vec<FooterShortcut> {
    if state.plugin_auth_enabled
        && state.is_remote_plugin()
        && state.cursor == state.auth_field_index().unwrap_or(usize::MAX)
    {
        vec![
            FooterShortcut {
                label: " \u{21E5} :Next",
                key: KeyCode::Tab,
            },
            FooterShortcut {
                label: "F5:AuthStart",
                key: KeyCode::F(5),
            },
            FooterShortcut {
                label: "F6:AuthDone",
                key: KeyCode::F(6),
            },
            FooterShortcut {
                label: " \u{238B} :Cancel",
                key: KeyCode::Esc,
            },
        ]
    } else if matches!(&state.kind, crate::app::RemoteEditKind::Smb)
        && state.cursor == state.path_field_index()
        && state.share_picker.is_none()
    {
        vec![
            FooterShortcut {
                label: " \u{21E5} :Next",
                key: KeyCode::Tab,
            },
            FooterShortcut {
                label: "F5:Shares",
                key: KeyCode::F(5),
            },
            FooterShortcut {
                label: " \u{238B} :Cancel",
                key: KeyCode::Esc,
            },
        ]
    } else {
        vec![
            FooterShortcut {
                label: " \u{21E5} :Next",
                key: KeyCode::Tab,
            },
            FooterShortcut {
                label: " \u{23ce} :Select",
                key: KeyCode::Enter,
            },
            FooterShortcut {
                label: " \u{238B} :Cancel",
                key: KeyCode::Esc,
            },
        ]
    }
}

pub(crate) fn remote_connecting_shortcuts() -> Vec<FooterShortcut> {
    vec![
        FooterShortcut {
            label: " \u{238B} :Abort",
            key: KeyCode::Esc,
        },
        FooterShortcut {
            label: " \u{23ce} :Abort",
            key: KeyCode::Enter,
        },
        FooterShortcut {
            label: "F10:Abort",
            key: KeyCode::F(10),
        },
    ]
}

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
                .fg(clr_menu_bar_fg())
                .bg(clr_menu_dd_bg())
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(clr_menu_border()).bg(clr_menu_dd_bg()))
        .style(Style::default().bg(clr_menu_dd_bg()));
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
            Style::default().fg(clr_qs_input_fg()).bg(clr_qs_input_bg()),
        ),
        Span::styled(
            count_hint,
            Style::default().fg(clr_qs_no_match()).bg(clr_qs_input_bg()),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(clr_qs_input_bg())),
        input_area,
    );

    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(clr_qs_sep()).bg(clr_menu_dd_bg())),
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
            Style::default().fg(clr_unknown()),
        )))]
    } else if matches.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            " No matching connection ",
            Style::default().fg(clr_qs_no_match()).bg(clr_menu_dd_bg()),
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
                let proto = match &item.kind {
                    RemoteKind::RemotePlugin(plugin) => plugin.scheme.as_str(),
                    _ => protocol.name(),
                };
                let proto_style = Style::default()
                    .fg(remote_protocol_color(protocol))
                    .bg(clr_menu_dd_bg());
                let (source, badge_style) = match item.source {
                    RemoteSource::SshConfig => (
                        "ssh",
                        Style::default().fg(clr_menu_hotkey()).bg(clr_menu_dd_bg()),
                    ),
                    RemoteSource::UserToml => (
                        "toml",
                        Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg()),
                    ),
                    RemoteSource::PluginAuto => (
                        "plugin",
                        Style::default().fg(clr_exec()).bg(clr_menu_dd_bg()),
                    ),
                };
                let host = item.host_label();
                let selected = match_idx == state.match_pos;
                let row_style = if selected {
                    Style::default()
                        .fg(clr_menu_sel_fg())
                        .bg(clr_menu_sel_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
                };
                let badge_style = if selected {
                    badge_style
                        .bg(clr_menu_sel_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    badge_style
                };
                let proto_style = if selected {
                    proto_style
                        .bg(clr_menu_sel_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    proto_style
                };
                let alias_style = row_style.add_modifier(Modifier::BOLD);
                let host_style = if selected {
                    Style::default()
                        .fg(clr_menu_sel_fg())
                        .bg(clr_menu_sel_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(clr_text()).bg(clr_menu_dd_bg())
                };
                let alias_line = highlight_tokens(
                    &format!("{:<16}", truncate_str(&item.name, 16)),
                    &tokens,
                    alias_style.fg.unwrap_or(clr_menu_dd_fg()),
                    alias_style.bg.unwrap_or(clr_menu_dd_bg()),
                    clr_qs_match_hi_sel(),
                );
                let proto_text = truncate_str(proto, 8);
                let host_text = truncate_str(&host, inner.width.saturating_sub(35) as usize);
                let host_line = highlight_tokens(
                    &host_text,
                    &tokens,
                    host_style.fg.unwrap_or(clr_menu_dd_fg()),
                    host_style.bg.unwrap_or(clr_menu_dd_bg()),
                    if selected {
                        clr_qs_match_hi_sel()
                    } else {
                        clr_qs_match_hi()
                    },
                );
                let mut spans = vec![Span::styled(" ", row_style)];
                spans.extend(alias_line.spans);
                spans.extend([
                    Span::styled(" ", row_style),
                    Span::styled(format!("{:^8}", proto_text), proto_style),
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
    let hint_items = footer_shortcut_items(&remote_connect_shortcuts());
    render_shortcut_bar(f, hint_area, &hint_items, secondary_shortcut_bar_style());
}

pub(super) fn render_remote_add_menu(
    f: &mut Frame,
    choices: &[RemoteEditKind],
    cursor: usize,
    area: Rect,
) {
    let width: u16 = 22;
    let height: u16 = (choices.len() as u16) + 3; // border(2) + items + hint
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
                .fg(clr_menu_bar_fg())
                .bg(clr_menu_dd_bg())
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(clr_menu_border()).bg(clr_menu_dd_bg()))
        .style(Style::default().bg(clr_menu_dd_bg()));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    for (i, kind) in choices.iter().enumerate() {
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
                .bg(remote_edit_kind_color(kind))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(remote_edit_kind_color(kind))
                .bg(clr_menu_dd_bg())
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
    let hint_items = footer_shortcut_items(&remote_add_menu_shortcuts());
    render_shortcut_bar(f, hint_row, &hint_items, secondary_shortcut_bar_style());
}

pub(super) fn render_remote_edit(f: &mut Frame, state: &RemoteEditState, area: Rect) {
    let width = 72u16.min(area.width.saturating_sub(4));
    let labels = state.kind.field_labels();
    let body_height = labels.len() as u16 + 5;
    let height = body_height.min(area.height.saturating_sub(2)).max(10);
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
                .fg(clr_menu_bar_fg())
                .bg(clr_menu_dd_bg())
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(clr_menu_border()).bg(clr_menu_dd_bg()))
        .style(Style::default().bg(clr_menu_dd_bg()));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    let mut lines = Vec::new();
    let mut selected_input_area = None;
    for (idx, label) in labels.iter().enumerate() {
        let selected = state.cursor == idx;
        let value_offset = state.field_value_offset(idx);
        let value_w = inner.width.saturating_sub(value_offset) as usize;
        // Label: always dark background; arrow prefix on selected row
        let label_style = Style::default()
            .fg(clr_header_fg())
            .bg(clr_menu_dd_bg())
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
            Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{}{:<8}", prefix, format!("{label}:")), label_style),
            Span::styled(
                if selected {
                    " ".repeat(value_w)
                } else {
                    format!("{:<width$}", state.fields[idx], width = value_w)
                },
                value_style,
            ),
        ]));
        if selected {
            selected_input_area = Some(Rect {
                x: inner.x + value_offset,
                y: inner.y + idx as u16,
                width: inner.width.saturating_sub(value_offset),
                height: 1,
            });
        }
    }
    lines.push(Line::default());
    let save_style = if state.cursor == state.save_index() {
        Style::default()
            .fg(clr_menu_sel_fg())
            .bg(clr_menu_sel_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
    };
    let cancel_style = if state.cursor == state.cancel_index() {
        Style::default()
            .fg(clr_menu_sel_fg())
            .bg(clr_menu_sel_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
    };
    lines.push(Line::from(vec![
        Span::styled(" [ Save ] ", save_style),
        Span::raw("  "),
        Span::styled(" [ Cancel ] ", cancel_style),
    ]));
    lines.push(Line::default());
    lines.push(Line::default());
    safe_render_widget(
        f,
        Paragraph::new(lines).style(Style::default().bg(clr_menu_dd_bg())),
        inner,
    );
    if state.cursor < state.input_count()
        && let Some(input_area) = selected_input_area
    {
        let mut textarea = state.textarea.clone();
        textarea.set_style(Style::default().fg(Color::Black).bg(Color::White));
        safe_render_widget(f, &textarea, input_area);
    }
    let hint_row = Rect {
        x: inner.x,
        y: inner.y + labels.len() as u16 + 3,
        width: inner.width,
        height: 1,
    };
    let hint_items = footer_shortcut_items(&remote_edit_shortcuts(state));
    render_shortcut_bar(f, hint_row, &hint_items, secondary_shortcut_bar_style());
    if state.cursor < state.input_count() {
        let cursor_col = state.textarea.cursor().1 as u16;
        let cursor_x = (inner.x + state.field_value_offset(state.cursor) + cursor_col)
            .min(inner.x + inner.width.saturating_sub(2));
        let cursor_y = inner.y + state.cursor as u16;
        safe_set_cursor_position(f, cursor_x, cursor_y);
    }

    // ── SMB share picker dropdown ─────────────────────────────────────────
    if let Some((ref shares, picker_cur)) = state.share_picker {
        // Anchor: Share field is at cursor row PATH (4); dropdown sits below it.
        let path_idx = state.path_field_index();
        let path_row: u16 = path_idx as u16;
        let value_offset = state.field_value_offset(path_idx);
        let dd_x = inner.x + value_offset;
        let dd_y = inner.y + path_row + 1;
        let dd_w = inner.width.saturating_sub(value_offset).min(40).max(16);
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
                Style::default().fg(clr_menu_bar_fg()).bg(clr_menu_dd_bg()),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(clr_qs_border()).bg(clr_menu_dd_bg()))
            .style(Style::default().bg(clr_menu_dd_bg()));
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
                (clr_menu_sel_fg(), clr_menu_sel_bg())
            } else {
                (clr_menu_dd_fg(), clr_menu_dd_bg())
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
                .fg(clr_menu_bar_fg())
                .bg(clr_menu_dd_bg())
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(clr_menu_border()).bg(clr_menu_dd_bg()))
        .style(Style::default().bg(clr_menu_dd_bg()));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    let lines = vec![
        Line::from(Span::styled(
            format!(" {} connection in progress", state.protocol_label),
            Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg()),
        )),
        Line::from(Span::styled(
            format!(" {}", state.profile_name),
            Style::default()
                .fg(clr_header_fg())
                .bg(clr_menu_dd_bg())
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(
            " Please wait... ",
            Style::default().fg(clr_text()).bg(clr_menu_dd_bg()),
        )),
        Line::from(Span::styled(
            format!(" {}", state.phase),
            Style::default().fg(clr_header_fg()).bg(clr_menu_dd_bg()),
        )),
        Line::default(),
        Line::default(),
    ];
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(clr_menu_dd_bg())),
        inner,
    );
    let hint_row = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    let hint_items = footer_shortcut_items(&remote_connecting_shortcuts());
    render_shortcut_bar(f, hint_row, &hint_items, secondary_shortcut_bar_style());
}

// ---------------------------------------------------------------------------
// Search panel
// ---------------------------------------------------------------------------
