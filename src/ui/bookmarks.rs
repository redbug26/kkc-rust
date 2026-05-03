use super::*;

pub(crate) fn dir_bookmarks_shortcuts() -> Vec<FooterShortcut> {
    vec![
        FooterShortcut {
            label: "a..z:Filter",
            key: KeyCode::Null,
        },
        FooterShortcut {
            label: " \u{23ce} :OpenAdd",
            key: KeyCode::Enter,
        },
        FooterShortcut {
            label: " \u{232B} :Remove",
            key: KeyCode::Delete,
        },
        FooterShortcut {
            label: " \u{238B} :Cancel",
            key: KeyCode::Esc,
        },
    ]
}

pub(crate) fn store_install_shortcuts(state: &StoreInstallPaletteState) -> Vec<FooterShortcut> {
    if state.detect.is_some() {
        vec![
            FooterShortcut {
                label: "Space:Toggle",
                key: KeyCode::Char(' '),
            },
            FooterShortcut {
                label: "LeftRight:Toggle",
                key: KeyCode::Right,
            },
            FooterShortcut {
                label: " \u{23ce} :Apply",
                key: KeyCode::Enter,
            },
            FooterShortcut {
                label: " \u{238B} :Cancel",
                key: KeyCode::Esc,
            },
        ]
    } else {
        vec![
            FooterShortcut {
                label: " \u{23ce} :Install",
                key: KeyCode::Enter,
            },
            FooterShortcut {
                label: "Ctrl+D:Detect",
                key: KeyCode::Char('d'),
            },
            FooterShortcut {
                label: "Ctrl+U:Update",
                key: KeyCode::Char('u'),
            },
            FooterShortcut {
                label: "Ctrl+R:Refresh",
                key: KeyCode::Char('r'),
            },
            FooterShortcut {
                label: " \u{238B} :Close",
                key: KeyCode::Esc,
            },
        ]
    }
}

pub(crate) fn store_detect_shortcuts() -> Vec<FooterShortcut> {
    vec![
        FooterShortcut {
            label: "Space:Toggle",
            key: KeyCode::Char(' '),
        },
        FooterShortcut {
            label: "LeftRight:Toggle",
            key: KeyCode::Right,
        },
        FooterShortcut {
            label: " \u{23ce} :Apply",
            key: KeyCode::Enter,
        },
        FooterShortcut {
            label: " \u{238B} :Cancel",
            key: KeyCode::Esc,
        },
    ]
}

pub(super) fn render_dir_bookmarks(f: &mut Frame, app: &App, area: Rect) {
    let list_h = app.bookmarks.len().max(3) as u16;
    // 2 border + input + separator + hint + list
    let height = (list_h + 5).min(area.height.saturating_sub(4)).max(8);
    let width = 64u16.min(area.width.saturating_sub(4));
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
        .title(" Bookmarks ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_MENU_BORDER))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

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
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height - 1,
        width: inner.width,
        height: 1,
    };

    let matches = app.filtered_bookmark_items();
    let total = matches.len();
    let count_hint = if app.bookmark_query.is_empty() {
        format!(" {} ", app.bookmarks.len())
    } else if total > 0 {
        format!(" {}/{} ", app.bookmark_match_pos + 1, total)
    } else {
        " 0/0 ".to_owned()
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" ⌕ {}\u{2581}", app.bookmark_query);
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

    let max_w = list_area.width as usize;
    let rows = list_area.height as usize;
    let tokens: Vec<String> = app
        .bookmark_query
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .collect();
    let scroll = if app.bookmark_match_pos >= rows && rows > 0 {
        app.bookmark_match_pos - rows + 1
    } else {
        0
    };

    let items: Vec<ListItem> = if app.bookmarks.is_empty() && matches.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "(no bookmarks)",
            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG),
        )))]
    } else if matches.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            " No matching bookmark ",
            Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_MENU_DD_BG),
        )))]
    } else {
        matches
            .iter()
            .enumerate()
            .skip(scroll)
            .take(rows)
            .map(|(match_idx, item)| {
                let selected = match_idx == app.bookmark_match_pos;
                let (label, style) = match item {
                    BookmarkListItem::AddCurrentDir(path) => {
                        let path = truncate_str(&path.to_string_lossy(), max_w.saturating_sub(20));
                        let label = format!(" <add current dir> {}", path);
                        let style = if selected {
                            Style::default()
                                .fg(CLR_MENU_SEL_FG)
                                .bg(CLR_MENU_SEL_BG)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                                .fg(CLR_HEADER_FG)
                                .bg(CLR_MENU_DD_BG)
                                .add_modifier(Modifier::BOLD)
                        };
                        (label, style)
                    }
                    BookmarkListItem::Existing(idx) => {
                        let p = &app.bookmarks[*idx];
                        let s = p.to_string_lossy();
                        let is_remote = s.starts_with("remote://");
                        let label = if is_remote {
                            let rest = &s["remote://".len()..];
                            format!(
                                " \u{2039}remote\u{203a} {}",
                                truncate_str(rest, max_w.saturating_sub(11))
                            )
                        } else {
                            format!(" {}", truncate_str(&s, max_w.saturating_sub(1)))
                        };
                        let style = if selected {
                            Style::default()
                                .fg(CLR_MENU_SEL_FG)
                                .bg(CLR_MENU_SEL_BG)
                                .add_modifier(Modifier::BOLD)
                        } else if !is_remote && !p.is_dir() {
                            Style::default()
                                .fg(CLR_MENU_DD_FG)
                                .bg(CLR_MENU_DD_BG)
                                .add_modifier(Modifier::DIM)
                        } else {
                            Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
                        };
                        (label, style)
                    }
                };
                let hi = if selected {
                    CLR_QS_MATCH_HI_SEL
                } else {
                    CLR_QS_MATCH_HI
                };
                ListItem::new(highlight_tokens(
                    &label,
                    &tokens,
                    style.fg.unwrap_or(CLR_MENU_DD_FG),
                    style.bg.unwrap_or(CLR_MENU_DD_BG),
                    hi,
                ))
            })
            .collect()
    };
    safe_render_widget(
        f,
        List::new(items).style(Style::default().bg(CLR_MENU_DD_BG)),
        list_area,
    );

    let hint_items = footer_shortcut_items(&dir_bookmarks_shortcuts());
    render_shortcut_bar(f, hint_area, &hint_items, secondary_shortcut_bar_style());
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Quick-palette (VSCode style)
// ---------------------------------------------------------------------------

/// Build a `Line` with each whitespace-separated token highlighted in the name.
pub(super) fn highlight_tokens(
    name: &str,
    tokens: &[String],
    base_fg: Color,
    base_bg: Color,
    hi_fg: Color,
) -> Line<'static> {
    // Build a boolean mask by character, not byte. Lowercasing UTF-8 text can
    // change byte lengths, so byte slicing here can panic on names with accents.
    let name_chars: Vec<char> = name.chars().collect();
    let lower_chars: Vec<char> = name.to_lowercase().chars().collect();
    let mut mask = vec![false; name_chars.len()];
    for token in tokens {
        if token.is_empty() {
            continue;
        }
        let token_chars: Vec<char> = token.to_lowercase().chars().collect();
        if token_chars.is_empty() || token_chars.len() > lower_chars.len() {
            continue;
        }
        let mut idx = 0usize;
        while idx + token_chars.len() <= lower_chars.len() {
            if lower_chars[idx..idx + token_chars.len()] == token_chars[..] {
                for slot in mask.iter_mut().skip(idx).take(token_chars.len()) {
                    *slot = true;
                }
                idx += 1;
            } else {
                idx += 1;
            }
        }
    }

    // Walk the name char by char, grouping consecutive same-style chars into spans
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut segment = String::new();
    let mut current_hi = mask.first().copied().unwrap_or(false);
    let base = Style::default().fg(base_fg).bg(base_bg);
    let hi = Style::default()
        .fg(hi_fg)
        .bg(base_bg)
        .add_modifier(Modifier::BOLD);

    for (idx, ch) in name_chars.into_iter().enumerate() {
        let this_hi = mask.get(idx).copied().unwrap_or(false);
        if this_hi != current_hi {
            spans.push(Span::styled(
                std::mem::take(&mut segment),
                if current_hi { hi } else { base },
            ));
            current_hi = this_hi;
        }
        segment.push(ch);
    }
    // Push the last segment
    spans.push(Span::styled(segment, if current_hi { hi } else { base }));

    Line::from(spans)
}

pub(super) fn render_quicksearch_palette(f: &mut Frame, app: &App, area: Rect) {
    let panel = app.active_panel();
    let query = &panel.quicksearch;
    let matches = panel.quicksearch_matches();
    let qs_pos = panel.qs_match_pos;
    let total = matches.len();

    // Dimensions: 62% wide, near the top (like VSCode)
    let palette_w = ((area.width as u32 * 62 / 100) as u16)
        .max(44)
        .min(area.width.saturating_sub(4));
    let visible_items = (total as u16).min(14);
    // input row + separator + items (at least 3 for aesthetics) + borders
    let palette_h = (1 + 1 + visible_items.max(3) + 2).min(area.height.saturating_sub(3));

    let x = (area.width.saturating_sub(palette_w)) / 2 + area.x;
    let y = area.y + 2;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width: palette_w,
            height: palette_h,
        },
    );

    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_QS_BORDER))
        .style(Style::default().bg(CLR_QS_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    if inner.height < 2 {
        return;
    }

    // ── input field ────────────────────────────────────────────────────────
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
        height: inner.height.saturating_sub(2),
    };

    // counter hint on the right side of the input (e.g. "3/47")
    let count_hint = if !query.is_empty() && total > 0 {
        format!(" {}/{} ", qs_pos + 1, total)
    } else if !query.is_empty() {
        " 0/0 ".to_owned()
    } else {
        String::new()
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" \u{2315} {}\u{2581}", query);
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
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        sep_area,
    );

    // ── match list ─────────────────────────────────────────────────────────
    if total == 0 && !query.is_empty() {
        safe_render_widget(
            f,
            Paragraph::new(" No match").style(Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_BG)),
            list_area,
        );
        return;
    }

    let list_h = list_area.height as usize;

    // Keep qs_pos inside the visible window
    let scroll: usize = if qs_pos >= list_h {
        qs_pos - list_h + 1
    } else {
        0
    };

    let tokens: Vec<String> = query.split_whitespace().map(|t| t.to_lowercase()).collect();
    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(list_h)
        .map(|(match_idx, &entry_idx)| {
            let entry = &panel.entries[entry_idx];
            let is_sel = match_idx == qs_pos;
            let (bg, fg, hi) = if is_sel {
                (CLR_QS_SEL_BG, CLR_QS_SEL_FG, CLR_QS_MATCH_HI_SEL)
            } else if entry.is_dir {
                (CLR_QS_BG, CLR_QS_DIR_FG, CLR_QS_MATCH_HI)
            } else {
                (CLR_QS_BG, CLR_QS_LIST_FG, CLR_QS_MATCH_HI)
            };

            let icon = if entry.is_dir { " \u{25b6} " } else { "   " };
            let name_line = highlight_tokens(&entry.name, &tokens, fg, bg, hi);
            let icon_span = vec![Span::styled(icon, Style::default().fg(fg).bg(bg))];
            let mut all_spans = icon_span;
            all_spans.extend(name_line.spans);

            ListItem::new(Line::from(all_spans))
        })
        .collect();

    // Reserve the rightmost column for the scrollbar when the list overflows
    let (render_area, sb_area) = if total > list_h {
        let list_w = list_area.width.saturating_sub(1);
        (
            Rect {
                width: list_w,
                ..list_area
            },
            Some(Rect {
                x: list_area.x + list_w,
                y: list_area.y,
                width: 1,
                height: list_area.height,
            }),
        )
    } else {
        (list_area, None)
    };

    safe_render_widget(
        f,
        List::new(items).style(Style::default().bg(CLR_QS_BG)),
        render_area,
    );

    if let Some(sb) = sb_area {
        let mut sb_state = ScrollbarState::new(total).position(scroll);
        safe_render_stateful_widget(
            f,
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(CLR_QS_BORDER))
                .track_style(Style::default().bg(CLR_QS_BG))
                .begin_symbol(None)
                .end_symbol(None),
            sb,
            &mut sb_state,
        );
    }
}

pub(super) fn render_viewer_plugin_palette(
    f: &mut Frame,
    state: &ViewerPluginPaletteState,
    area: Rect,
) {
    let query = &state.query;
    let matches = state.filtered_indices();
    let qs_pos = state.match_pos;
    let total = matches.len();

    let palette_w = ((area.width as u32 * 62 / 100) as u16)
        .max(50)
        .min(area.width.saturating_sub(4));
    let visible_items = (total as u16).min(14);
    let palette_h = (1 + 1 + visible_items.max(3) + 2).min(area.height.saturating_sub(3));

    let popup = clamp_rect(
        area,
        Rect {
            x: (area.width.saturating_sub(palette_w)) / 2 + area.x,
            y: area.y + 2,
            width: palette_w,
            height: palette_h,
        },
    );

    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .title(" Viewer Plugins ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_QS_BORDER))
        .style(Style::default().bg(CLR_QS_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    if inner.height < 2 {
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
        height: inner.height.saturating_sub(2),
    };

    let count_hint = if !query.is_empty() && total > 0 {
        format!(" {}/{} ", qs_pos + 1, total)
    } else if !query.is_empty() {
        " 0/0 ".to_owned()
    } else {
        format!(" {} ", state.items.len())
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" \u{2315} {}\u{2581}", query);
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
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        sep_area,
    );

    if total == 0 {
        let message = if query.is_empty() {
            " No viewer plugin"
        } else {
            " No match"
        };
        safe_render_widget(
            f,
            Paragraph::new(message).style(Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_BG)),
            list_area,
        );
        return;
    }

    let list_h = list_area.height as usize;
    let scroll = if qs_pos >= list_h {
        qs_pos - list_h + 1
    } else {
        0
    };
    let tokens: Vec<String> = query.split_whitespace().map(|t| t.to_lowercase()).collect();

    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .skip(scroll)
        .take(list_h)
        .map(|(match_idx, &plugin_idx)| {
            let plugin = &state.items[plugin_idx];
            let is_sel = match_idx == qs_pos;
            let (bg, fg, hi) = if is_sel {
                (CLR_QS_SEL_BG, CLR_QS_SEL_FG, CLR_QS_MATCH_HI_SEL)
            } else {
                (CLR_QS_BG, CLR_QS_LIST_FG, CLR_QS_MATCH_HI)
            };

            let mut spans = vec![Span::styled("   ", Style::default().fg(fg).bg(bg))];
            let name = highlight_tokens(&plugin.name, &tokens, fg, bg, hi);
            spans.extend(name.spans);
            if !plugin.description.is_empty() {
                spans.push(Span::styled("  ", Style::default().fg(fg).bg(bg)));
                spans.push(Span::styled(
                    truncate_str(&plugin.description, 42),
                    Style::default().fg(Color::Gray).bg(bg),
                ));
            }
            if !plugin.extensions.is_empty() {
                spans.push(Span::styled("  ", Style::default().fg(fg).bg(bg)));
                spans.push(Span::styled(
                    plugin.extensions.join(","),
                    Style::default().fg(Color::DarkGray).bg(bg),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let (render_area, sb_area) = if total > list_h {
        let list_w = list_area.width.saturating_sub(1);
        (
            Rect {
                width: list_w,
                ..list_area
            },
            Some(Rect {
                x: list_area.x + list_w,
                y: list_area.y,
                width: 1,
                height: list_area.height,
            }),
        )
    } else {
        (list_area, None)
    };

    safe_render_widget(
        f,
        List::new(items).style(Style::default().bg(CLR_QS_BG)),
        render_area,
    );

    if let Some(sb) = sb_area {
        let mut sb_state = ScrollbarState::new(total).position(scroll);
        safe_render_stateful_widget(
            f,
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(CLR_QS_BORDER))
                .track_style(Style::default().bg(CLR_QS_BG))
                .begin_symbol(None)
                .end_symbol(None),
            sb,
            &mut sb_state,
        );
    }
}

pub(super) fn render_store_install_palette(
    f: &mut Frame,
    state: &StoreInstallPaletteState,
    area: Rect,
) {
    let matches = state.filtered_indices();
    let total = matches.len();

    let w: u16 = area.width.saturating_sub(4).min(140).max(90);
    let h: u16 = area.height.saturating_sub(4).min(30).max(22);
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

    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .title(" Plugin Store ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER).bg(CLR_APP_BG))
        .style(Style::default().bg(CLR_APP_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    if inner.height < 8 || inner.width < 48 {
        return;
    }

    let count_hint = if !state.query.is_empty() && total > 0 {
        format!(" {}/{} ", state.match_pos + 1, total)
    } else if !state.query.is_empty() {
        " 0/0 ".to_owned()
    } else {
        format!(" {} ", state.items.len())
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" \u{2315} {}\u{2581}", state.query);
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
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(truncate_str(
            &format!("  {}", state.index_version_label()),
            inner.width as usize,
        ))
        .style(Style::default().fg(Color::Rgb(88, 66, 45)).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new(sep.clone()).style(Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: 1,
        },
    );

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
    let hint_items = footer_shortcut_items(&store_install_shortcuts(state));
    render_shortcut_bar(f, hint_area, &hint_items, secondary_shortcut_bar_style());

    if let Some(detect) = state.detect.as_ref() {
        render_store_detect_dialog(f, detect, inner);
        return;
    }

    let body_y = inner.y + 3;
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

    let max_name = state
        .items
        .iter()
        .map(|p| p.name.len() + 18)
        .max()
        .unwrap_or(8);
    let left_w = ((max_name + 4) as u16)
        .clamp(36, 64)
        .min(body.width.saturating_sub(30));
    let right_w = body.width.saturating_sub(left_w + 1);

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

    safe_render_widget(
        f,
        Paragraph::new(format!(
            "  {:<w$}",
            "Name                              Kind Status",
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
    let scroll = if total == 0 || state.match_pos < list_h {
        0
    } else {
        state.match_pos.saturating_sub(list_h - 1)
    };

    if total == 0 {
        let msg = if state.query.is_empty() {
            "  (no item available in store index)"
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
        return;
    }

    for (match_row, idx) in (scroll..).zip(0..list_h) {
        if match_row >= total {
            break;
        }
        let plugin = &state.items[matches[match_row]];
        let row_y = left_area.y + 1 + idx as u16;
        let selected = state.match_pos == match_row;
        let installed = state.is_installed(plugin);
        let has_update = state.has_update(plugin);
        let has_compatible_method =
            !matches!(plugin.item_kind, crate::plugins::StoreItemKind::Application)
                || plugin.install_method.is_some();

        let style = if selected {
            Style::default()
                .fg(Color::Rgb(16, 10, 6))
                .bg(Color::Rgb(235, 220, 188))
                .add_modifier(Modifier::BOLD)
        } else if has_update {
            Style::default().fg(Color::Rgb(150, 74, 10)).bg(CLR_APP_BG)
        } else if !has_compatible_method {
            Style::default().fg(Color::Rgb(118, 104, 88)).bg(CLR_APP_BG)
        } else if installed {
            Style::default().fg(Color::Rgb(26, 104, 46)).bg(CLR_APP_BG)
        } else {
            Style::default().fg(Color::Rgb(46, 28, 16)).bg(CLR_APP_BG)
        };

        let status = if has_update {
            "[UPDATE]"
        } else if !has_compatible_method {
            "[NO METHOD]"
        } else if installed {
            "[INSTALLED]"
        } else {
            "[NEW]"
        };
        let kind = match plugin.item_kind {
            crate::plugins::StoreItemKind::Plugin => "P",
            crate::plugins::StoreItemKind::Application => "A",
        };
        let icon = if selected { "▶ " } else { "  " };
        let available = (left_area.width as usize).saturating_sub(3);
        let name_w = available.saturating_sub(status.len() + kind.len() + 2);
        let text = format!(
            "{icon}{:<name_w$} {} {}",
            truncate_str(&plugin.name, name_w),
            kind,
            status,
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
    let Some(plugin) = matches
        .get(state.match_pos)
        .and_then(|idx| state.items.get(*idx))
    else {
        return;
    };

    let lbl_style = Style::default()
        .fg(Color::Rgb(48, 64, 96))
        .bg(CLR_APP_BG)
        .add_modifier(Modifier::BOLD);
    let val_style = Style::default().fg(Color::Rgb(34, 20, 12)).bg(CLR_APP_BG);
    let dim_style = Style::default().fg(Color::Rgb(88, 66, 45)).bg(CLR_APP_BG);
    let rw = right_area.width as usize;

    if let Some(progress) = state.progress.as_ref() {
        render_store_install_progress(f, progress, right_area);
        return;
    }

    let mut row: u16 = 0;
    let mut push_kv = |label: &str, value: &str, row: &mut u16| {
        if *row >= detail_h {
            return;
        }
        let text = Line::from(vec![
            Span::styled(format!("  {label:<12}"), lbl_style),
            Span::styled(truncate_str(value, rw.saturating_sub(14)), val_style),
        ]);
        safe_render_widget(
            f,
            Paragraph::new(text).style(Style::default().bg(CLR_APP_BG)),
            Rect {
                x: right_area.x,
                y: detail_y + *row,
                width: right_area.width,
                height: 1,
            },
        );
        *row += 1;
    };

    let kind_label = match plugin.item_kind {
        crate::plugins::StoreItemKind::Plugin => "Plugin",
        crate::plugins::StoreItemKind::Application => "Application",
    };
    push_kv("Kind :", kind_label, &mut row);
    push_kv("Type :", &plugin.plugin_type, &mut row);
    push_kv("Version :", &plugin.version, &mut row);
    push_kv("Id :", &plugin.id, &mut row);

    let installed_version = state.installed_version_for(plugin);
    let status = if state.has_update(plugin) {
        "Update available"
    } else if state.is_installed(plugin) {
        "Installed"
    } else {
        "Not installed"
    };
    push_kv("Status :", status, &mut row);
    if let Some(v) = installed_version {
        push_kv("Installed :", v, &mut row);
    }
    let compatible_method = plugin
        .install_method
        .as_deref()
        .unwrap_or("None for this OS");
    if matches!(plugin.item_kind, crate::plugins::StoreItemKind::Application) {
        push_kv("Method :", compatible_method, &mut row);
    }
    if let Some(bin) = plugin.install_bin.as_deref() {
        push_kv("Binary :", bin, &mut row);
    }

    if !plugin.install_methods.is_empty() && row < detail_h {
        row += 1;
        if row < detail_h {
            safe_render_widget(
                f,
                Paragraph::new(Line::from(vec![Span::styled("  Available :", lbl_style)]))
                    .style(Style::default().bg(CLR_APP_BG)),
                Rect {
                    x: right_area.x,
                    y: detail_y + row,
                    width: right_area.width,
                    height: 1,
                },
            );
            row += 1;
        }
        let indent = "    ";
        let max_w = rw.saturating_sub(indent.len());
        for method in &plugin.install_methods {
            if row >= detail_h {
                break;
            }
            let text = format!("{indent}{}", truncate_str(method, max_w));
            safe_render_widget(
                f,
                Paragraph::new(text).style(dim_style),
                Rect {
                    x: right_area.x,
                    y: detail_y + row,
                    width: right_area.width,
                    height: 1,
                },
            );
            row += 1;
        }
    }

    if row < detail_h {
        row += 1;
    }

    if !plugin.description.is_empty() && row < detail_h {
        safe_render_widget(
            f,
            Paragraph::new(Line::from(vec![Span::styled("  Description :", lbl_style)]))
                .style(Style::default().bg(CLR_APP_BG)),
            Rect {
                x: right_area.x,
                y: detail_y + row,
                width: right_area.width,
                height: 1,
            },
        );
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
            safe_render_widget(
                f,
                Paragraph::new(format!("{desc_indent}{chunk}")).style(dim_style),
                Rect {
                    x: right_area.x,
                    y: detail_y + row,
                    width: right_area.width,
                    height: 1,
                },
            );
            row += 1;
            rest = remainder;
        }
    }
}

fn render_store_install_progress(
    f: &mut Frame,
    progress: &crate::app::StoreInstallProgress,
    area: Rect,
) {
    if area.width < 12 || area.height < 6 {
        return;
    }

    let box_h = area.height.saturating_sub(2).min(9).max(6);
    let box_w = area.width.saturating_sub(4).max(12);
    let box_area = Rect {
        x: area.x + 2,
        y: area.y + 1 + area.height.saturating_sub(box_h + 1) / 2,
        width: box_w,
        height: box_h,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Rgb(160, 160, 160))
                .bg(Color::Black),
        )
        .style(
            Style::default()
                .fg(Color::Rgb(230, 230, 230))
                .bg(Color::Black),
        );
    let inner = block.inner(box_area);
    safe_render_widget(f, block, box_area);

    let width = inner.width as usize;
    let pct = progress.percent.min(100);
    let bar_w = width.saturating_sub(8).clamp(8, 38);
    let filled = (pct as usize * bar_w) / 100;
    let bar = format!(
        "[{}{}]",
        "#".repeat(filled),
        "-".repeat(bar_w.saturating_sub(filled))
    );

    let lines = vec![
        progress.title.clone(),
        progress.item_name.clone(),
        format!("{} {:>3}%", bar, pct),
        progress.phase.clone(),
    ];

    for (idx, line) in lines.iter().enumerate() {
        if idx as u16 >= inner.height {
            break;
        }
        safe_render_widget(
            f,
            Paragraph::new(truncate_str(line, width)).style(
                Style::default()
                    .fg(Color::Rgb(230, 230, 230))
                    .bg(Color::Black),
            ),
            Rect {
                x: inner.x,
                y: inner.y + idx as u16,
                width: inner.width,
                height: 1,
            },
        );
    }
}

fn render_store_detect_dialog(f: &mut Frame, detect: &crate::app::StoreDetectState, area: Rect) {
    let width = area.width.saturating_sub(4).min(104).max(56);
    let height = area.height.saturating_sub(4).min(18).max(10);
    let popup = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    safe_render_widget(f, Clear, popup);
    let block = Block::default()
        .title(" Detect Installed Applications ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER).bg(CLR_APP_BG))
        .style(Style::default().bg(CLR_APP_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    let summary = format!(
        "  Detected {} installed app(s). Missing remembered app(s): {}",
        detect.detected_count,
        detect.items.len()
    );
    safe_render_widget(
        f,
        Paragraph::new(truncate_str(&summary, inner.width as usize))
            .style(Style::default().fg(Color::Rgb(46, 28, 16)).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new("  Space/Left/Right changes action. Enter applies.")
            .style(Style::default().fg(Color::Rgb(88, 66, 45)).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );

    let list_y = inner.y + 3;
    let list_h = inner.height.saturating_sub(5) as usize;
    if detect.items.is_empty() {
        safe_render_widget(
            f,
            Paragraph::new("  No missing remembered applications.")
                .style(Style::default().fg(Color::Rgb(26, 104, 46)).bg(CLR_APP_BG)),
            Rect {
                x: inner.x,
                y: list_y,
                width: inner.width,
                height: 1,
            },
        );
    } else {
        let start = if detect.cursor >= list_h {
            detect.cursor.saturating_sub(list_h.saturating_sub(1))
        } else {
            0
        };
        for (row, idx) in (start..detect.items.len()).take(list_h).enumerate() {
            let item = &detect.items[idx];
            let selected = idx == detect.cursor;
            let action = match item.choice {
                crate::app::StoreDetectChoice::Keep => "keep",
                crate::app::StoreDetectChoice::Install => "install",
                crate::app::StoreDetectChoice::Remove => "remove",
            };
            let bin = item.app.install_bin.as_deref().unwrap_or("?");
            let text = format!(
                " {} [{:<7}] {:<24} {}",
                if selected { ">" } else { " " },
                action,
                truncate_str(&item.app.name, 24),
                bin
            );
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(CLR_CURSOR_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(46, 28, 16)).bg(CLR_APP_BG)
            };
            safe_render_widget(
                f,
                Paragraph::new(truncate_str(&text, inner.width as usize)).style(style),
                Rect {
                    x: inner.x,
                    y: list_y + row as u16,
                    width: inner.width,
                    height: 1,
                },
            );
        }
    }

    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    let hint_items = footer_shortcut_items(&store_detect_shortcuts());
    render_shortcut_bar(f, hint_area, &hint_items, secondary_shortcut_bar_style());
}

// ---------------------------------------------------------------------------
// Help overlay
// ---------------------------------------------------------------------------
