use super::*;

pub(crate) fn search_panel_shortcuts(state: &SearchState) -> Vec<FooterShortcut> {
    if state.input_field == 3 {
        vec![
            FooterShortcut {
                label: " \u{23ce} :GoToFile",
                key: KeyCode::Enter,
            },
            FooterShortcut {
                label: "UpDown:Navigate",
                key: KeyCode::Null,
            },
            FooterShortcut {
                label: "PgUpPgDn:Navigate",
                key: KeyCode::Null,
            },
            FooterShortcut {
                label: " \u{21E5} :Fields",
                key: KeyCode::Tab,
            },
            FooterShortcut {
                label: "F5:Backend",
                key: KeyCode::F(5),
            },
            FooterShortcut {
                label: " \u{238B} :Close",
                key: KeyCode::Esc,
            },
        ]
    } else if state.input_field == 2 {
        vec![
            FooterShortcut {
                label: " \u{23ce} :Search",
                key: KeyCode::Enter,
            },
            FooterShortcut {
                label: " \u{21E5} :SwitchField",
                key: KeyCode::Tab,
            },
            FooterShortcut {
                label: " \u{232B} :ResetDir",
                key: KeyCode::Delete,
            },
            FooterShortcut {
                label: "F5:Backend",
                key: KeyCode::F(5),
            },
            FooterShortcut {
                label: " \u{238B} :Close",
                key: KeyCode::Esc,
            },
        ]
    } else {
        vec![
            FooterShortcut {
                label: " \u{23ce} :Search",
                key: KeyCode::Enter,
            },
            FooterShortcut {
                label: " \u{21E5} :SwitchField",
                key: KeyCode::Tab,
            },
            FooterShortcut {
                label: "Down:Results",
                key: KeyCode::Down,
            },
            FooterShortcut {
                label: "F5:Backend",
                key: KeyCode::F(5),
            },
            FooterShortcut {
                label: " \u{232B} :Reset",
                key: KeyCode::Delete,
            },
            FooterShortcut {
                label: " \u{238B}:Close",
                key: KeyCode::Esc,
            },
        ]
    }
}

pub(super) fn render_search(f: &mut Frame, state: &SearchState, area: Rect) {
    // --- popup geometry ---------------------------------------------------
    let width = 100u16.min(area.width.saturating_sub(2));
    let height = (area.height * 4 / 5).clamp(18, area.height.saturating_sub(2));
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
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

    // Outer frame
    let backend_label = state.backend.label();
    let title = format!(" \u{1f50d} Search  [{backend_label}] ");
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(clr_header_fg())
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(clr_panel_border()))
        .style(Style::default().bg(crate::theme::theme().search.background));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // --- input section: 1 blank + 3 fields + 1 blank = 5 rows ------------
    let input_h = 5u16.min(inner.height);
    let results_area = Rect {
        x: inner.x,
        y: inner.y + input_h,
        width: inner.width,
        height: inner.height.saturating_sub(input_h + 1),
    };
    let hint_area = clamp_rect(
        area,
        Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        },
    );

    // Label column is 12 chars ("▶ Content : "), input fills the rest
    let label_w = 12usize;
    let iw = inner.width.saturating_sub(label_w as u16 + 4) as usize;

    let srch = &crate::theme::theme().search;
    let clr_label = srch.label_foreground;
    let clr_active_label = srch.active_label_foreground;
    let clr_input_bg_active = srch.input_background_active;
    let clr_input_bg_idle = srch.input_background_idle;
    let clr_input_fg = srch.input_foreground;
    let clr_placeholder = srch.placeholder;

    let (lbl0, bg0) = if state.input_field == 0 {
        (clr_active_label, clr_input_bg_active)
    } else {
        (clr_label, clr_input_bg_idle)
    };
    let (lbl1, bg1) = if state.input_field == 1 {
        (clr_active_label, clr_input_bg_active)
    } else {
        (clr_label, clr_input_bg_idle)
    };
    let (lbl2, bg2) = if state.input_field == 2 {
        (clr_active_label, clr_input_bg_active)
    } else {
        (clr_label, clr_input_bg_idle)
    };

    // Build field strings with trailing cursor '_' when focused
    let make_field = |text: &str, focused: bool, placeholder_star: bool| -> (String, Color) {
        let cursor = if focused { "_" } else { "" };
        let avail = iw.saturating_sub(cursor.len());
        if text.is_empty() && !focused && !placeholder_star {
            (format!("{:<w$}", "", w = iw), clr_placeholder)
        } else if placeholder_star && text == "*" && !focused {
            // Show '*' in dim color as default
            (
                format!("*{:<w$}", cursor, w = avail.saturating_sub(1)),
                clr_placeholder,
            )
        } else {
            let displayed = truncate_str(text, avail);
            (
                format!(
                    "{displayed}{cursor}{:<w$}",
                    "",
                    w = avail.saturating_sub(displayed.len())
                ),
                clr_input_fg,
            )
        }
    };

    let (pat_str, pat_fg) = make_field(&state.query, state.input_field == 0, true);
    let (cnt_str, cnt_fg) = make_field(&state.content_query, state.input_field == 1, false);
    let (dir_str, dir_fg) = make_field(&state.dir_query, state.input_field == 2, false);

    let input_lines = vec![
        Line::default(),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "\u{25b6} Name   : ",
                Style::default().fg(lbl0).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {pat_str} "), Style::default().fg(pat_fg).bg(bg0)),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "\u{25b6} Content: ",
                Style::default().fg(lbl1).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {cnt_str} "), Style::default().fg(cnt_fg).bg(bg1)),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "\u{25b6} Dir    : ",
                Style::default().fg(lbl2).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {dir_str} "), Style::default().fg(dir_fg).bg(bg2)),
        ]),
        Line::default(),
    ];

    safe_render_widget(
        f,
        Paragraph::new(input_lines).style(Style::default().bg(crate::theme::theme().search.background)),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: input_h,
        },
    );

    // --- separator with result count --------------------------------------
    let result_count = state.results.len();
    let suffix = if result_count >= 1000 {
        " (limit 1000)"
    } else {
        ""
    };
    let sep_title = if state.running {
        if result_count > 0 {
            format!(" Searching...  {result_count} found so far  [Esc to cancel] ")
        } else {
            " Searching...  [Esc to cancel] ".to_string()
        }
    } else if result_count > 0 {
        format!(" {result_count} result(s){suffix} ")
    } else {
        " No results - press Enter to search ".to_string()
    };
    let sep_block = Block::default()
        .title(Span::styled(
            sep_title,
            Style::default().fg(if state.running {
                crate::theme::theme().search.running
            } else {
                crate::theme::theme().search.separator
            }),
        ))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(crate::theme::theme().search.separator));
    let results_body = sep_block.inner(results_area);
    safe_render_widget(f, sep_block, results_area);

    if results_body.height == 0 {
        return;
    }

    // --- column header row ------------------------------------------------
    let date_w = 14usize;
    let size_w = 9usize;
    let name_w = 26usize;
    let dir_w = (results_body.width as usize).saturating_sub(name_w + size_w + date_w + 7); // + 7 ? for spacing and truncation padding

    let header_area = Rect {
        x: results_body.x,
        y: results_body.y,
        width: results_body.width,
        height: 1,
    };
    let result_inner = Rect {
        x: results_body.x,
        y: results_body.y + 1,
        width: results_body.width,
        height: results_body.height.saturating_sub(1),
    };
    let clr_hdr = crate::theme::theme().search.header;
    safe_render_widget(
        f,
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {:<name_w$}", "Name"),
                Style::default().fg(clr_hdr).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().fg(clr_hdr)),
            Span::styled(
                format!("{:<dir_w$}", "Directory"),
                Style::default().fg(clr_hdr).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {:>size_w$}", "Size"),
                Style::default().fg(clr_hdr).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {:>date_w$}", "Modified"),
                Style::default().fg(clr_hdr).add_modifier(Modifier::BOLD),
            ),
        ]))
        .style(Style::default().bg(crate::theme::theme().search.background)),
        header_area,
    );

    // --- result list ------------------------------------------------------
    let visible_h = result_inner.height as usize;
    let scroll = {
        let mut s = state.scroll;
        if state.cursor < s {
            s = state.cursor;
        } else if state.cursor >= s + visible_h {
            s = state.cursor + 1 - visible_h;
        }
        s
    };

    let is_results_focused = state.input_field == 3;

    let items: Vec<ListItem> = state
        .results
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_h)
        .map(|(i, r)| {
            let is_cursor = i == state.cursor && is_results_focused;
            // Zebra: alternating row backgrounds
            let srch_bg = crate::theme::theme().search.background;
            let zebra_bg = if (i % 2) == 0 {
                srch_bg
            } else {
                // Slightly lighter row for zebra striping.
                // Blend the search background with white by 10 %.
                match srch_bg {
                    ratatui::style::Color::Rgb(r, g, b) => ratatui::style::Color::Rgb(
                        r.saturating_add(4),
                        g.saturating_add(4),
                        b.saturating_add(8),
                    ),
                    other => other,
                }
            };

            let row_bg = if is_cursor { clr_cursor_bg() } else { zebra_bg };

            let file_name = r
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let dir = r
                .path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();

            let ext = r
                .path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let name_clr = if is_cursor {
                Color::Black
            } else {
                match ext.as_str() {
                    "rs" | "c" | "h" | "cpp" | "py" | "js" | "ts" => clr_source(),
                    "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "lha" | "lzh" => {
                        clr_archive()
                    }
                    "mp3" | "flac" | "ogg" | "wav" | "mod" | "xm" | "s3m" | "ayt" => clr_audio(),
                    "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" => clr_image(),
                    "mp4" | "mkv" | "avi" | "mov" => clr_video(),
                    "pdf" | "doc" | "docx" | "odt" => clr_doc(),
                    "json" | "toml" | "yaml" | "xml" | "csv" => clr_data(),
                    "txt" | "md" | "rst" => clr_text(),
                    _ => clr_unknown(),
                }
            };

            let dir_clr = if is_cursor {
                Color::Black
            } else {
                clr_qs_no_match()
            };
            let size_clr = if is_cursor { Color::Black } else { clr_data() };
            let date_clr = if is_cursor {
                Color::Black
            } else {
                crate::theme::theme().search.header
            };

            let name_str = truncate_search_file_name(&file_name, name_w);
            let dir_str = format!("{:width$}", truncate_path(&dir, dir_w), width = dir_w);
            let size_str = format!("{:>width$}", format_size(r.size), width = size_w);
            let date_str = r
                .modified
                .map(|ts| {
                    let dt: DateTime<Local> = ts.into();
                    dt.format("%Y-%m-%d %H:%M").to_string()
                })
                .unwrap_or_default();
            let date_str = format!("{:>width$}", date_str, width = date_w);

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {name_str}"),
                    Style::default().fg(name_clr).bg(row_bg),
                ),
                Span::styled(" ", Style::default().bg(row_bg)),
                Span::styled(dir_str, Style::default().fg(dir_clr).bg(row_bg)),
                Span::styled(
                    format!(" {size_str}"),
                    Style::default().fg(size_clr).bg(row_bg),
                ),
                Span::styled(
                    format!(" {date_str}"),
                    Style::default().fg(date_clr).bg(row_bg),
                ),
            ]))
        })
        .collect();

    safe_render_widget(
        f,
        List::new(items).style(Style::default().bg(crate::theme::theme().search.background)),
        result_inner,
    );

    // Scrollbar
    if result_count > visible_h {
        let mut sb_state = ScrollbarState::new(result_count).position(state.cursor);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(crate::theme::theme().search.separator)),
            result_inner,
            &mut sb_state,
        );
    }

    // --- hint bar ---------------------------------------------------------
    let items = footer_shortcut_items(&search_panel_shortcuts(state));
    render_shortcut_bar(f, hint_area, &items, secondary_shortcut_bar_style());
}
