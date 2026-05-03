use super::*;

pub(crate) fn viewer_area(v: &Viewer, area: Rect) -> Rect {
    // Plugin document views always use the full area (like zoomed) so that
    // (a) no stale file-manager content bleeds through the margins, and
    // (b) the actual panel width is available to the plugin renderer.
    if v.zoomed || v.viewer_plugin.is_some() {
        return area;
    }

    let max_width = match v.mode {
        ViewMode::Hex => 80u16,
        ViewMode::Image => area.width.saturating_sub(4).max(40).min(area.width),
        _ => {
            let ln = v.line_number_width() as u16;
            let text_max = v
                .current_plain_lines()
                .iter()
                .map(|line| line.chars().count() as u16)
                .max()
                .unwrap_or(40);
            (text_max + ln).clamp(40, area.width.saturating_sub(4).max(40))
        }
    };
    let width = (max_width + 2).min(area.width);
    let height = (v.line_count() as u16 + 3).clamp(8, area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

pub fn kitty_image_area(v: &Viewer, area: Rect) -> Option<Rect> {
    if !v.is_image_mode() {
        return None;
    }
    let viewer_host = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(1),
    };
    let area = viewer_area(v, viewer_host);
    let inner = Block::default().borders(Borders::ALL).inner(area);
    if inner.width == 0 || inner.height == 0 {
        None
    } else {
        Some(inner)
    }
}

/// Returns the kitty image rect for the quick_preview panel, if it's showing an image.
pub fn kitty_image_area_quick_preview(app: &App, term_area: Rect) -> Option<Rect> {
    let v = app.quick_preview.as_ref()?;
    if !v.is_image_mode() {
        return None;
    }
    let has_fbar = app.config.show_fkey_bar;
    let main_vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if has_fbar {
            vec![
                Constraint::Min(5),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
        } else {
            vec![Constraint::Min(5), Constraint::Length(1)]
        })
        .split(term_area);
    let panels_area = main_vert[0];
    let panel_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(28),
            Constraint::Length(13),
            Constraint::Min(28),
        ])
        .split(panels_area);
    let left_active = app.active == crate::app::ActivePanel::Left;
    let preview_area = if left_active {
        panel_chunks[2]
    } else {
        panel_chunks[0]
    };
    kitty_image_area(v, preview_area)
}

pub(super) fn render_viewer(
    f: &mut Frame,
    v: &Viewer,
    searching: bool,
    goto_input: Option<&str>,
    area: Rect,
    show_footer: bool,
    active: bool,
    quick_preview_label: Option<&str>,
) {
    let (footer_area, viewer_host) = if show_footer {
        let footer = clamp_rect(
            area,
            Rect {
                x: area.x,
                y: area.y + area.height.saturating_sub(1),
                width: area.width,
                height: 1,
            },
        );
        let host = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        (footer, host)
    } else {
        // Embedded (quick-preview): no footer row, use full area for content
        (Rect::default(), area)
    };
    let area = viewer_area(v, viewer_host);
    let file_name = v.path.file_name().unwrap_or_default().to_string_lossy();
    let match_info = if !v.search.is_empty() {
        format!(" [{}/{}]", v.match_pos + 1, v.matches.len())
    } else {
        String::new()
    };
    let col_info = if matches!(v.mode, ViewMode::Text | ViewMode::Ansi) && !v.wrap && v.hscroll > 0
    {
        format!(" Col:{} ", v.hscroll)
    } else {
        String::new()
    };
    let lf_info = if matches!(v.mode, ViewMode::Text | ViewMode::Ansi) {
        format!(" LF:{} ", v.line_feed_label())
    } else {
        String::new()
    };
    let pre_info = if matches!(v.mode, ViewMode::Text | ViewMode::Ansi) {
        format!(" Pre:{} ", v.preproc_label())
    } else {
        String::new()
    };
    let enc_info = if matches!(v.mode, ViewMode::Text | ViewMode::Ansi | ViewMode::Hex) {
        format!(" Enc:{} ", v.encoding_label())
    } else {
        String::new()
    };
    let mask_info = if matches!(v.mode, ViewMode::Text | ViewMode::Ansi) {
        format!(" Syn:{} ", v.mask_label())
    } else {
        String::new()
    };
    let plugin_info = v
        .viewer_plugin
        .as_ref()
        .map(|name| format!(" Plugin:{} ", name))
        .unwrap_or_default();
    let zoom_info = format!(" Zoom:{} ", v.zoom_label());
    let image_info = if let Some(image) = v.image_info() {
        match (image.width, image.height) {
            (Some(w), Some(h)) => format!(" {} {}x{} ", image.format, w, h),
            _ => format!(" {} ", image.format),
        }
    } else {
        String::new()
    };
    let title = format!(
        " {} [{}] {}/{}{}{}{}{}{}{}{}{}{} ",
        file_name,
        v.mode_label(),
        v.scroll + 1,
        v.line_count(),
        image_info,
        lf_info,
        pre_info,
        enc_info,
        mask_info,
        plugin_info,
        zoom_info,
        col_info,
        match_info,
    );

    let (border_style, border_type, title_span) = if let Some(label) = quick_preview_label {
        // Quick-preview embedded panel: custom compact title
        if active {
            (
                Style::default()
                    .fg(CLR_HEADER_FG)
                    .add_modifier(Modifier::BOLD),
                BorderType::Thick,
                Span::styled(
                    format!(" {} ", label),
                    Style::default()
                        .fg(CLR_HEADER_FG)
                        .add_modifier(Modifier::BOLD),
                ),
            )
        } else {
            (
                Style::default().fg(CLR_PANEL_BORDER_DIM),
                BorderType::Rounded,
                Span::styled(
                    format!(" {} ", label),
                    Style::default().fg(CLR_PANEL_BORDER_DIM),
                ),
            )
        }
    } else if active {
        (
            Style::default()
                .fg(CLR_PANEL_BORDER)
                .add_modifier(Modifier::BOLD),
            BorderType::Thick,
            Span::raw(title.clone()),
        )
    } else {
        (
            Style::default().fg(CLR_PANEL_BORDER_DIM),
            BorderType::Rounded,
            Span::raw(title.clone()),
        )
    };
    let block = Block::default()
        .title(title_span)
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        inner,
    );

    if v.is_image_mode() && crate::viewer::kitty_graphics_supported() {
        let mut lines = vec![Line::from(Span::styled(
            "Image preview",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))];
        if let Some(image) = v.image_info() {
            let detail = match (image.width, image.height) {
                (Some(w), Some(h)) => format!("{} - {}x{}", image.format, w, h),
                _ => image.format.to_string(),
            };
            lines.push(Line::from(Span::styled(
                detail,
                Style::default().fg(Color::Gray),
            )));
        }
        lines.push(Line::from(Span::raw("")));
        lines.push(Line::from(Span::styled(
            "Rendered with Kitty Graphics Protocol",
            Style::default().fg(Color::Cyan),
        )));
        lines.push(Line::from(Span::styled(
            "Use F5 to toggle Auto/Full size",
            Style::default().fg(Color::Gray),
        )));
        f.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .style(Style::default().bg(Color::Black)),
            inner,
        );
        if show_footer {
            let help = Paragraph::new(" F10:Close  F4:Mode  F5:Zoom ")
                .style(Style::default().fg(Color::Black).bg(Color::Cyan));
            f.render_widget(help, footer_area);
        }
        return;
    }

    let height = inner.height as usize;
    let width = inner.width as usize;

    // Line-number gutter (text/ansi modes only, not for plugin documents).
    let ln_width = v.line_number_width();
    let total_lines = v.line_count();
    let ln_digits = if ln_width > 0 {
        total_lines.max(1).ilog10() as usize + 1
    } else {
        0
    };
    let text_width = width.saturating_sub(ln_width);

    let search_lower = v.search.to_lowercase();
    let items: Vec<Line> = v
        .render_lines(text_width, v.scroll, height)
        .into_iter()
        .enumerate()
        .map(|(rel_idx, line)| {
            let abs_idx = v.scroll + rel_idx;
            let plain = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            let is_match = !search_lower.is_empty() && plain.to_lowercase().contains(&search_lower);
            let is_current_match = is_match && v.matches.get(v.match_pos).copied() == Some(abs_idx);

            let content_line = if let Some((before, selected, after)) =
                v.selection_display_segments_for_visible_row(rel_idx, text_width, height)
            {
                let mut spans = Vec::new();
                if !before.is_empty() {
                    spans.push(Span::styled(before, Style::default().fg(Color::White)));
                }
                if !selected.is_empty() {
                    spans.push(Span::styled(
                        selected,
                        Style::default().fg(Color::Black).bg(Color::LightGreen),
                    ));
                }
                if !after.is_empty() {
                    spans.push(Span::styled(after, Style::default().fg(Color::White)));
                }
                Line::from(spans)
            } else if is_current_match {
                Line::from(vec![Span::styled(
                    truncate_str(&plain, text_width),
                    Style::default().fg(Color::Black).bg(Color::Yellow),
                )])
            } else if is_match {
                Line::from(vec![Span::styled(
                    truncate_str(&plain, text_width),
                    Style::default().fg(Color::Black).bg(Color::LightYellow),
                )])
            } else {
                line
            };

            if ln_width > 0 {
                let num_str = format!("{:>width$}\u{2502} ", abs_idx + 1, width = ln_digits);
                let mut spans = vec![Span::styled(
                    num_str,
                    Style::default().fg(Color::Rgb(90, 110, 150)),
                )];
                spans.extend(content_line.spans);
                Line::from(spans)
            } else {
                content_line
            }
        })
        .collect();

    if v.viewer_plugin.is_none() && v.wrap && matches!(v.mode, ViewMode::Text | ViewMode::Ansi) {
        f.render_widget(
            Paragraph::new(items)
                .wrap(Wrap { trim: false })
                .style(Style::default().bg(Color::Black)),
            inner,
        );
    } else {
        let list = List::new(items.into_iter().map(ListItem::new).collect::<Vec<_>>())
            .style(Style::default().bg(Color::Black));
        f.render_widget(list, inner);
    }

    if searching && show_footer {
        let label = format!(" Search: {}_ ", v.search);
        let found_count = v.matches.len();
        let found_label = if v.search.is_empty() {
            String::new()
        } else {
            format!("  {} match(es)  Enter:OK  Esc:Cancel", found_count)
        };
        let bar_text = format!("{}{}", label, found_label);
        f.render_widget(
            Paragraph::new(bar_text)
                .style(Style::default().fg(Color::Black).bg(Color::LightYellow)),
            footer_area,
        );
        let cx =
            (footer_area.x + 9 + v.search.len() as u16).min(footer_area.x + footer_area.width - 1);
        safe_set_cursor_position(f, cx, footer_area.y);
    } else if let Some(input) = goto_input
        && show_footer
    {
        let (label, label_len) = if matches!(v.mode, ViewMode::Hex) {
            (" Goto offset (hex): ", 21u16)
        } else {
            (" Goto line: ", 13u16)
        };
        let bar_text = format!("{}{}_", label, input);
        f.render_widget(
            Paragraph::new(bar_text).style(Style::default().fg(Color::Black).bg(Color::LightCyan)),
            footer_area,
        );
        let cx = (footer_area.x + label_len + input.len() as u16)
            .min(footer_area.x + footer_area.width - 1);
        safe_set_cursor_position(f, cx, footer_area.y);
    } else if show_footer {
        let help = Paragraph::new(" F10:Close  F2:Wrap  F3:LnFeed  F4:Mode  F5:Zoom  F6:Prepro  F7:Search  F8:Enc  F9:Syntax  g:Goto ")
            .style(Style::default().fg(Color::Black).bg(Color::Cyan));
        f.render_widget(help, footer_area);
    }
}

pub(super) fn render_viewer_goto(f: &mut Frame, state: &ViewerGotoState, area: Rect) {
    let items = [
        ("g", "Goto line number <n> else file start"),
        ("e", "Goto last line"),
        ("s", "Goto first non-blank"),
        ("n", "Goto next page"),
        ("p", "Goto previous page"),
    ];
    let width = 48u16;
    let height = items.len() as u16 + 3;
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
    let title = if state.count.is_empty() {
        " Goto ".to_string()
    } else {
        format!(" Goto {} ", state.count)
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    safe_render_widget(
        f,
        Block::default().style(Style::default().bg(CLR_MENU_DD_BG)),
        inner,
    );

    let list_items = items
        .iter()
        .enumerate()
        .map(|(idx, (shortcut, label))| {
            let style = if idx == state.cursor {
                Style::default()
                    .fg(CLR_MENU_SEL_FG)
                    .bg(CLR_MENU_SEL_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
            };
            let mut spans = vec![
                Span::styled(" ", style),
                Span::styled(format!("{shortcut}  "), style.add_modifier(Modifier::BOLD)),
            ];
            spans.push(Span::styled((*label).to_string(), style));
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect::<Vec<_>>();

    let list_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: items.len() as u16,
    };
    safe_render_widget(
        f,
        List::new(list_items).style(Style::default().bg(CLR_MENU_DD_BG)),
        list_area,
    );

    let hint_area = Rect {
        x: inner.x,
        y: inner.y + items.len() as u16,
        width: inner.width,
        height: inner.height.saturating_sub(items.len() as u16),
    };
    safe_render_widget(
        f,
        Paragraph::new(" digits set <n>  Enter:run  Esc:close ")
            .style(Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)),
        hint_area,
    );
}

pub(super) fn render_viewer_menu(f: &mut Frame, viewer: &Viewer, menu: &ViewerMenuState, area: Rect) {
    let items: Vec<String> = match menu.kind {
        ViewerMenuKind::Mode => vec![
            "Text: as plain text",
            "Binary: as hex dump",
            "Ansi: with ANSI escapes",
            "Image: as inline preview",
            "Plugins viewer",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>(),
        ViewerMenuKind::LineFeed => vec!["DOS (CR/LF)", "Unix (LF)", "Mac (CR)", "Mixed"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ViewerMenuKind::Preproc => {
            let mut items = Vec::new();
            for idx in 0..viewer.preproc_len() {
                if let Some(label) = viewer.preproc_item_label(idx) {
                    items.push(label);
                }
            }
            if viewer.preproc_len() > 0 {
                items.push("────────".into());
            }
            items.extend([
                "Add XOR".into(),
                "Add AND".into(),
                "Add OR".into(),
                "Add NEG".into(),
                "Add ROR".into(),
                "Add ADD".into(),
                "Add Latin".into(),
                "Add Elite".into(),
                "Clear All".into(),
            ]);
            items
        }
        ViewerMenuKind::Encoding => vec!["Plain ASCII".into(), "DOS CP437".into()],
        ViewerMenuKind::Mask => vec![
            "Auto detect",
            "C / C++",
            "Rust",
            "JavaScript / TS",
            "Python",
            "PHP",
            "HTML / XML",
            "CSS / SCSS",
            "SQL",
            "Shell / Bash",
            "Pascal",
            "Assembler",
            "Ketchup",
            "Syntax OFF",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
    };

    let title = match menu.kind {
        ViewerMenuKind::Mode => " Change Viewer ",
        ViewerMenuKind::LineFeed => " Change Line Feed ",
        ViewerMenuKind::Preproc => " Preprocess ",
        ViewerMenuKind::Encoding => " Character Set ",
        ViewerMenuKind::Mask => " Syntax Highlight ",
    };

    let width = items
        .iter()
        .map(|s| UnicodeWidthStr::width(s.as_str()))
        .max()
        .unwrap_or(10) as u16
        + 6;
    let extra = if menu.kind == ViewerMenuKind::Preproc {
        3
    } else {
        0
    };
    let desired_height = items.len() as u16 + 2 + extra;
    let height = desired_height.min(area.height.saturating_sub(2).max(4));
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
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    safe_render_widget(
        f,
        Block::default().style(Style::default().bg(CLR_MENU_DD_BG)),
        inner,
    );

    let all_items = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_separator = menu.kind == ViewerMenuKind::Preproc
                && viewer.preproc_len() > 0
                && idx == viewer.preproc_len();
            let style = if is_separator {
                Style::default().fg(CLR_MENU_DD_SEP).bg(CLR_MENU_DD_BG)
            } else if idx == menu.cursor {
                Style::default()
                    .fg(CLR_MENU_SEL_FG)
                    .bg(CLR_MENU_SEL_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)
            };
            let line = if menu.kind == ViewerMenuKind::Mode {
                viewer_mode_menu_line(idx, item, style)
            } else if is_separator {
                Line::from(Span::styled(format!(" {}", item), style))
            } else {
                viewer_submenu_line(viewer, menu.kind, idx, item, style)
            };
            ListItem::new(line).style(style)
        })
        .collect::<Vec<_>>();

    let list_height = inner.height.saturating_sub(extra);
    let list_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: list_height,
    };
    let scroll = menu
        .scroll
        .min(items.len().saturating_sub(list_height as usize));
    let visible_items = all_items
        .into_iter()
        .skip(scroll)
        .take(list_height as usize)
        .collect::<Vec<_>>();
    safe_render_widget(
        f,
        List::new(visible_items).style(Style::default().bg(CLR_MENU_DD_BG)),
        list_area,
    );

    if menu.kind == ViewerMenuKind::Preproc {
        let info_area = Rect {
            x: inner.x,
            y: inner.y + list_height,
            width: inner.width,
            height: inner.height.saturating_sub(list_height),
        };
        let info = format!(
            " Param:0x{:02X}  \u{2190}/\u{2192}:Edit  Ctrl+\u{2191}/\u{2193}:Move  Del:Remove ",
            menu.param
        );
        safe_render_widget(
            f,
            Paragraph::new(info).style(Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)),
            info_area,
        );
    }
}

fn viewer_mode_menu_line(idx: usize, item: &str, style: Style) -> Line<'static> {
    if idx == 4 {
        return Line::from(vec![
            Span::styled(" ", style),
            Span::styled("P. ", style.add_modifier(Modifier::BOLD)),
            Span::styled(item.to_string(), style),
        ]);
    }

    let number = if idx < 9 {
        format!("{} ", idx + 1)
    } else {
        "  ".into()
    };
    let shortcut = item.chars().next().unwrap_or_default().to_string();
    let rest = item.chars().skip(1).collect::<String>();
    let mut spans = vec![
        Span::styled(" ", style),
        Span::styled(number, style.add_modifier(Modifier::BOLD)),
    ];
    if idx < 6 {
        spans.push(Span::styled(shortcut, style.add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(rest, style));
    } else {
        spans.push(Span::styled(item.to_string(), style));
    }
    Line::from(spans)
}

fn viewer_submenu_line(
    viewer: &Viewer,
    kind: ViewerMenuKind,
    idx: usize,
    item: &str,
    style: Style,
) -> Line<'static> {
    let labels = viewer_menu_render_labels(viewer, kind);
    let mnemonics = mnemonics_for_labels(&labels);
    let shortcut = mnemonics.get(idx).copied().flatten();
    let mut spans = vec![Span::styled(" ", style)];
    append_highlighted_mnemonic(&mut spans, item, shortcut, style);
    Line::from(spans)
}

fn viewer_menu_render_labels(viewer: &Viewer, kind: ViewerMenuKind) -> Vec<String> {
    match kind {
        ViewerMenuKind::Mode => Vec::new(),
        ViewerMenuKind::Preproc => {
            let mut labels = Vec::new();
            for idx in 0..viewer.preproc_len() {
                if let Some(label) = viewer.preproc_item_label(idx) {
                    labels.push(label);
                }
            }
            if viewer.preproc_len() > 0 {
                labels.push(String::new());
            }
            labels.extend(
                [
                    "Add XOR",
                    "Add AND",
                    "Add OR",
                    "Add NEG",
                    "Add ROR",
                    "Add ADD",
                    "Add Latin",
                    "Add Elite",
                    "Clear All",
                ]
                .into_iter()
                .map(String::from),
            );
            labels
        }
        ViewerMenuKind::LineFeed => vec!["DOS (CR/LF)", "Unix (LF)", "Mac (CR)", "Mixed"]
            .into_iter()
            .map(String::from)
            .collect(),
        ViewerMenuKind::Encoding => vec!["Plain ASCII", "DOS CP437"]
            .into_iter()
            .map(String::from)
            .collect(),
        ViewerMenuKind::Mask => vec![
            "C Style",
            "Pascal Style",
            "Assembler Style",
            "Ketchup Style",
            "Mask OFF",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    }
}

pub(super) fn menu_dropdown_line(
    label: &str,
    key_text: &str,
    pad: usize,
    mnemonic: Option<char>,
    style: Style,
) -> Line<'static> {
    let mut spans = vec![Span::styled(" ", style)];
    append_highlighted_mnemonic(&mut spans, label, mnemonic, style);
    spans.push(Span::styled(" ".repeat(pad), style));
    if !key_text.is_empty() {
        spans.push(Span::styled(key_text.to_string(), style));
    }
    spans.push(Span::styled(" ", style));
    Line::from(spans)
}

fn append_highlighted_mnemonic(
    spans: &mut Vec<Span<'static>>,
    label: &str,
    mnemonic: Option<char>,
    style: Style,
) {
    let mut highlighted = false;
    for ch in label.chars() {
        let matches = mnemonic == Some(ch.to_ascii_lowercase()) && !highlighted;
        let item_style = if matches {
            highlighted = true;
            style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            style
        };
        spans.push(Span::styled(ch.to_string(), item_style));
    }
}

pub(super) fn mnemonics_for_labels(labels: &[String]) -> Vec<Option<char>> {
    let mut used = Vec::new();
    labels
        .iter()
        .map(|label| {
            let candidates = label
                .chars()
                .filter(|ch| ch.is_alphanumeric())
                .map(|ch| ch.to_ascii_lowercase())
                .collect::<Vec<_>>();
            let chosen = candidates
                .iter()
                .copied()
                .find(|candidate| !used.contains(candidate))
                .or_else(|| candidates.first().copied());
            if let Some(ch) = chosen {
                used.push(ch);
            }
            chosen
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Confirm dialog
// ---------------------------------------------------------------------------
