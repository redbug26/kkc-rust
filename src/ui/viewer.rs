use super::*;

pub(crate) fn viewer_footer_shortcuts(v: &Viewer) -> Vec<FooterShortcut> {
    if v.is_image_mode() && crate::viewer::kitty_graphics_supported() {
        vec![
            FooterShortcut {
                label: "F4:Mode",
                key: KeyCode::F(4),
            },
            FooterShortcut {
                label: "F5:Zoom",
                key: KeyCode::F(5),
            },
            FooterShortcut {
                label: "F10:Close",
                key: KeyCode::F(10),
            },
            FooterShortcut {
                label: "p:Autoplay",
                key: KeyCode::Char('p'),
            },
        ]
    } else if matches!(v.mode, ViewMode::Module) {
        vec![
            FooterShortcut {
                label: "F4:Mode",
                key: KeyCode::F(4),
            },
            FooterShortcut {
                label: "F10:Close",
                key: KeyCode::F(10),
            },
            FooterShortcut {
                label: "Tab:Section",
                key: KeyCode::Tab,
            },
            FooterShortcut {
                label: "↑↓:Scroll",
                key: KeyCode::Down,
            },
            FooterShortcut {
                label: "F5:Zoom",
                key: KeyCode::F(5),
            },
            FooterShortcut {
                label: "p:Autoplay",
                key: KeyCode::Char('p'),
            },
        ]
    } else {
        let mut shortcuts = vec![
            FooterShortcut {
                label: "F2:Wrap",
                key: KeyCode::F(2),
            },
            FooterShortcut {
                label: "F3:LnFeed",
                key: KeyCode::F(3),
            },
            FooterShortcut {
                label: "F4:Mode",
                key: KeyCode::F(4),
            },
            FooterShortcut {
                label: "F5:Zoom",
                key: KeyCode::F(5),
            },
            FooterShortcut {
                label: "F6:Prepro",
                key: KeyCode::F(6),
            },
            FooterShortcut {
                label: "F8:Enc",
                key: KeyCode::F(8),
            },
            FooterShortcut {
                label: "F9:Syntax",
                key: KeyCode::F(9),
            },
            FooterShortcut {
                label: "/:Search",
                key: KeyCode::Char('/'),
            },
        ];
        if matches!(v.mode, ViewMode::Ansi) {
            shortcuts.push(FooterShortcut {
                label: "a:Canvas",
                key: KeyCode::Char('a'),
            });
        }
        shortcuts.push(FooterShortcut {
            label: "p:Autoplay",
            key: KeyCode::Char('p'),
        });
        shortcuts.push(FooterShortcut {
            label: "g:Goto",
            key: KeyCode::Char('g'),
        });
        shortcuts
    }
}

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
        ViewMode::Module => area.width,
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

fn render_viewer_backdrop(f: &mut Frame, host: Rect, panel: Rect) {
    if host.width == 0 || host.height == 0 {
        return;
    }

    let bg = clr_menu_dd_bg();
    let pattern_dim = Style::default().fg(clr_panel_border_dim()).bg(bg);
    let pattern_hi = Style::default().fg(clr_panel_border()).bg(bg);
    let mut lines = Vec::with_capacity(host.height as usize);
    for y in 0..host.height as usize {
        let mut spans = Vec::new();
        for x in 0..host.width as usize {
            let ch = match ((x + y * 2) % 17, (x * 3 + y) % 23) {
                (0, _) => '·',
                (_, 0) => '╱',
                _ => ' ',
            };
            let style = if ch == '╱' { pattern_hi } else { pattern_dim };
            spans.push(Span::styled(ch.to_string(), style));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines).style(Style::default().bg(bg)), host);

    let halo_x = panel.x.saturating_sub(1).max(host.x);
    let halo_y = panel.y.saturating_sub(1).max(host.y);
    let halo = Rect {
        x: halo_x,
        y: halo_y,
        width: panel
            .width
            .saturating_add(2)
            .min(host.right().saturating_sub(halo_x)),
        height: panel
            .height
            .saturating_add(2)
            .min(host.bottom().saturating_sub(halo_y)),
    };
    if halo.width > 0 && halo.height > 0 {
        f.render_widget(
            Block::default().style(Style::default().bg(clr_panel_border_dim())),
            halo,
        );
    }

    let shadow_x = panel.x.saturating_add(1).min(host.right());
    let shadow_y = panel.y.saturating_add(1).min(host.bottom());
    let shadow = Rect {
        x: shadow_x,
        y: shadow_y,
        width: panel.width.min(host.right().saturating_sub(shadow_x)),
        height: panel.height.min(host.bottom().saturating_sub(shadow_y)),
    };
    if shadow.width > 0 && shadow.height > 0 {
        f.render_widget(
            Block::default().style(Style::default().bg(clr_menu_dd_bg())),
            shadow,
        );
    }
}

fn render_viewer_full_width_header(f: &mut Frame, host: Rect, title: Line<'static>, active: bool) {
    if host.width == 0 || host.height == 0 {
        return;
    }
    let style = if active {
        Style::default()
            .fg(clr_header_fg())
            .bg(clr_menu_bar_bg())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(clr_panel_border_dim())
            .bg(clr_menu_bar_bg())
    };
    let header = Rect {
        x: host.x,
        y: host.y,
        width: host.width,
        height: 1,
    };
    f.render_widget(Paragraph::new(title).style(style), header);
}

fn line_with_default_viewer_style(mut line: Line<'static>) -> Line<'static> {
    let fg = Color::White;
    let bg = Color::Black;
    if line.style.fg.is_none() {
        line.style.fg = Some(fg);
    }
    if line.style.bg.is_none() {
        line.style.bg = Some(bg);
    }
    for span in &mut line.spans {
        if span.style.fg.is_none() {
            span.style.fg = Some(fg);
        }
        if span.style.bg.is_none() {
            span.style.bg = Some(bg);
        }
    }
    line
}

fn clip_line_to_width(line: Line<'static>, width: usize) -> Line<'static> {
    use unicode_width::UnicodeWidthChar;

    if width == 0 {
        return Line::from(Span::raw(String::new()));
    }

    let mut remaining = width;
    let mut spans = Vec::new();
    for span in line.spans {
        if remaining == 0 {
            break;
        }
        let mut text = String::new();
        for ch in span.content.chars() {
            let ch_width = ch.width().unwrap_or(0);
            if ch_width > remaining {
                break;
            }
            text.push(ch);
            remaining = remaining.saturating_sub(ch_width);
        }
        if !text.is_empty() {
            spans.push(Span::styled(text, span.style));
        }
    }
    if remaining > 0 {
        spans.push(Span::raw(" ".repeat(remaining)));
    }
    Line::from(spans)
}

fn render_solid_bg(f: &mut Frame, area: Rect, bg: Color) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let style = Style::default().bg(bg);
    let lines = (0..area.height)
        .map(|_| Line::from(Span::styled(" ".repeat(area.width as usize), style)))
        .collect::<Vec<_>>();
    f.render_widget(Paragraph::new(lines).style(style), area);
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
    autoplay_delay_secs: u64,
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
    let full_width_header =
        show_footer && quick_preview_label.is_none() && !v.zoomed && v.viewer_plugin.is_none();
    let panel_host = if full_width_header {
        Rect {
            x: viewer_host.x,
            y: viewer_host.y.saturating_add(1),
            width: viewer_host.width,
            height: viewer_host.height.saturating_sub(1),
        }
    } else {
        viewer_host
    };
    let area = viewer_area(v, panel_host);
    if full_width_header {
        render_viewer_backdrop(f, panel_host, area);
    }
    let file_name = v.path.file_name().unwrap_or_default().to_string_lossy();
    let match_info = if !v.search.is_empty() {
        format!(" [{}/{}]", v.match_pos + 1, v.matches.len())
    } else {
        String::new()
    };
    let col_info = (matches!(v.mode, ViewMode::Text | ViewMode::Ansi) && !v.wrap && v.hscroll > 0)
        .then(|| v.hscroll.to_string());
    let lf_info = matches!(v.mode, ViewMode::Text | ViewMode::Markdown | ViewMode::Ansi)
        .then(|| v.line_feed_label());
    let pre_info = matches!(v.mode, ViewMode::Text | ViewMode::Markdown | ViewMode::Ansi)
        .then(|| v.preproc_label());
    let enc_info = matches!(
        v.mode,
        ViewMode::Text | ViewMode::Markdown | ViewMode::Ansi | ViewMode::Hex
    )
        .then(|| v.encoding_label());
    let mask_info = matches!(v.mode, ViewMode::Text | ViewMode::Markdown | ViewMode::Ansi)
        .then(|| v.mask_label());
    let ansi_canvas_info = matches!(v.mode, ViewMode::Ansi).then(|| v.ansi_canvas_label());
    let plugin_info = v.viewer_plugin.as_deref();
    let zoom_info = v.zoom_label();
    let autoplay_info = v.autoplay_display(autoplay_delay_secs);
    let auto_detected_info =
        if matches!(v.mode, ViewMode::Text | ViewMode::Markdown | ViewMode::Ansi) {
        v.detected_mask_label()
            .map(|label| format!("({label}) "))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let image_info = if let Some(image) = v.image_info() {
        match (image.width, image.height) {
            (Some(w), Some(h)) => format!(" {} {}x{} ", image.format, w, h),
            _ => format!(" {} ", image.format),
        }
    } else {
        String::new()
    };
    let key_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default()
        .fg(Color::LightBlue)
        .add_modifier(Modifier::BOLD);
    let mut title_spans = vec![
        Span::styled(
            format!(" {} ", file_name),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("[{}] ", v.mode_label()),
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}/{}", v.scroll + 1, v.line_count()),
            Style::default().fg(Color::LightYellow),
        ),
    ];
    let push_kv = |spans: &mut Vec<Span<'static>>, key: &str, value: &str| {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!("{key}:"), key_style));
        spans.push(Span::styled(value.to_string(), value_style));
    };
    if !image_info.is_empty() {
        title_spans.push(Span::raw(image_info));
    }
    if let Some(lf) = lf_info {
        push_kv(&mut title_spans, "LF", lf);
    }
    if let Some(preproc) = pre_info {
        push_kv(&mut title_spans, "Pre", preproc.as_str());
    }
    if let Some(enc) = enc_info {
        push_kv(&mut title_spans, "Enc", enc);
    }
    if let Some(mask) = mask_info {
        push_kv(&mut title_spans, "Syn", mask);
    }
    if !auto_detected_info.is_empty() {
        title_spans.push(Span::styled(
            format!(" {auto_detected_info}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(canvas) = ansi_canvas_info {
        push_kv(&mut title_spans, "Canvas", canvas);
    }
    if let Some(plugin) = plugin_info {
        push_kv(&mut title_spans, "Plugin", plugin);
    }
    push_kv(&mut title_spans, "Zoom", zoom_info);
    title_spans.push(Span::raw(" "));
    title_spans.push(Span::styled("Autoplay:", key_style));
    title_spans.push(Span::styled(
        autoplay_info,
        if v.autoplay {
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD)
        } else {
            value_style
        },
    ));
    if let Some(col) = col_info {
        push_kv(&mut title_spans, "Col", col.as_str());
    }
    if !match_info.is_empty() {
        title_spans.push(Span::styled(
            match_info,
            Style::default().fg(Color::Black).bg(Color::LightYellow),
        ));
    }
    title_spans.push(Span::raw(" "));
    let title_line = Line::from(title_spans);

    let (border_style, border_type, title_line_for_block) = if let Some(label) = quick_preview_label
    {
        // Quick-preview embedded panel: custom compact title
        if active {
            (
                Style::default()
                    .fg(clr_header_fg())
                    .add_modifier(Modifier::BOLD),
                BorderType::Thick,
                Line::from(Span::styled(
                    format!(" {} ", label),
                    Style::default()
                        .fg(clr_header_fg())
                        .add_modifier(Modifier::BOLD),
                )),
            )
        } else {
            (
                Style::default().fg(clr_panel_border_dim()),
                BorderType::Rounded,
                Line::from(Span::styled(
                    format!(" {} ", label),
                    Style::default().fg(clr_panel_border_dim()),
                )),
            )
        }
    } else if full_width_header {
        (
            Style::default()
                .fg(clr_panel_border())
                .add_modifier(Modifier::BOLD),
            BorderType::Thick,
            Line::from(Span::raw(String::new())),
        )
    } else if active {
        (
            Style::default()
                .fg(clr_panel_border())
                .add_modifier(Modifier::BOLD),
            BorderType::Thick,
            title_line.clone(),
        )
    } else {
        (
            Style::default().fg(clr_panel_border_dim()),
            BorderType::Rounded,
            title_line.clone(),
        )
    };
    if full_width_header {
        render_viewer_full_width_header(f, viewer_host, title_line.clone(), active);
    }
    let block = Block::default()
        .title(title_line_for_block)
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style);
    let inner = block.inner(area);
    f.render_widget(block, area);
    render_solid_bg(f, inner, Color::Black);

    let use_graphics_protocol = if quick_preview_label.is_some() {
        crate::viewer::embedded_graphics_supported()
    } else {
        crate::viewer::kitty_graphics_supported()
    };

    if v.is_image_mode() && use_graphics_protocol {
        let lines = vec![Line::from(Span::styled(
            "",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ))];
        //     // let mut lines = vec![Line::from(Span::styled(
        //     "Image preview",
        //     Style::default()
        //         .fg(Color::White)
        //         .add_modifier(Modifier::BOLD),
        // ))];
        // if let Some(image) = v.image_info() {
        //     let detail = match (image.width, image.height) {
        //         (Some(w), Some(h)) => format!("{} - {}x{}", image.format, w, h),
        //         _ => image.format.to_string(),
        //     };
        //     lines.push(Line::from(Span::styled(
        //         detail,
        //         Style::default().fg(Color::Gray),
        //     )));
        // }
        f.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .style(Style::default().bg(Color::Black)),
            inner,
        );
        if show_footer {
            let shortcuts = viewer_footer_shortcuts(v);
            let items = footer_shortcut_items(&shortcuts);
            render_shortcut_bar(f, footer_area, &items, default_shortcut_bar_style());
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

            let rendered_line = if ln_width > 0 {
                let num_str = format!("{:>width$}\u{2502} ", abs_idx + 1, width = ln_digits);
                let mut spans = vec![Span::styled(
                    num_str,
                    Style::default().fg(clr_panel_border_dim()),
                )];
                spans.extend(content_line.spans);
                Line::from(spans)
            } else {
                content_line
            };

            let rendered_line = if !v.wrap
                || !matches!(v.mode, ViewMode::Text | ViewMode::Markdown | ViewMode::Ansi)
            {
                clip_line_to_width(rendered_line, width)
            } else {
                rendered_line
            };
            line_with_default_viewer_style(rendered_line)
        })
        .collect();

    if v.viewer_plugin.is_none()
        && v.wrap
        && matches!(v.mode, ViewMode::Text | ViewMode::Markdown | ViewMode::Ansi)
    {
        f.render_widget(
            Paragraph::new(items)
                .wrap(Wrap { trim: false })
                .scroll((v.wrap_visual_offset() as u16, 0))
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
        let shortcuts = viewer_footer_shortcuts(v);
        let items = footer_shortcut_items(&shortcuts);
        render_shortcut_bar(f, footer_area, &items, default_shortcut_bar_style());
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
        .border_style(Style::default().fg(clr_panel_border()))
        .style(Style::default().bg(clr_menu_dd_bg()));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    safe_render_widget(
        f,
        Block::default().style(Style::default().bg(clr_menu_dd_bg())),
        inner,
    );

    let list_items = items
        .iter()
        .enumerate()
        .map(|(idx, (shortcut, label))| {
            let style = if idx == state.cursor {
                Style::default()
                    .fg(clr_menu_sel_fg())
                    .bg(clr_menu_sel_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
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
        List::new(list_items).style(Style::default().bg(clr_menu_dd_bg())),
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
            .style(Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())),
        hint_area,
    );
}

pub(super) fn render_viewer_menu(
    f: &mut Frame,
    viewer: &Viewer,
    menu: &ViewerMenuState,
    area: Rect,
) {
    let items: Vec<String> = match menu.kind {
        ViewerMenuKind::Mode => vec![
            "Text: as plain text",
            "Markdown: CommonMark viewer",
            "Binary: as hex dump",
            "Ansi: with ANSI escapes",
            "Image: as inline preview",
            "Audio: as inline preview",
            "Plugins viewer",
            "Audio player",
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
            "Markdown",
            "C / C++",
            "Rust",
            "JavaScript / TS",
            "Python",
            "PHP",
            "HTML / XML",
            "CSS / SCSS",
            "TOML",
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
        .border_style(Style::default().fg(clr_panel_border()))
        .style(Style::default().bg(clr_menu_dd_bg()));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    safe_render_widget(
        f,
        Block::default().style(Style::default().bg(clr_menu_dd_bg())),
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
                Style::default().fg(clr_menu_dd_sep()).bg(clr_menu_dd_bg())
            } else if idx == menu.cursor {
                Style::default()
                    .fg(clr_menu_sel_fg())
                    .bg(clr_menu_sel_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())
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
        List::new(visible_items).style(Style::default().bg(clr_menu_dd_bg())),
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
            Paragraph::new(info).style(Style::default().fg(clr_menu_dd_fg()).bg(clr_menu_dd_bg())),
            info_area,
        );
    }
}

fn viewer_mode_menu_line(idx: usize, item: &str, style: Style) -> Line<'static> {
    if idx == 6 {
        return Line::from(vec![
            Span::styled(" ", style),
            Span::styled("P. ", style.add_modifier(Modifier::BOLD)),
            Span::styled(item.to_string(), style),
        ]);
    }
    if idx == 7 {
        return Line::from(vec![
            Span::styled(" ", style),
            Span::styled("A. ", style.add_modifier(Modifier::BOLD)),
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
            "Auto detect",
            "Markdown",
            "C / C++",
            "Rust",
            "JavaScript / TS",
            "Python",
            "PHP",
            "HTML / XML",
            "CSS / SCSS",
            "TOML",
            "SQL",
            "Shell / Bash",
            "Pascal",
            "Assembler",
            "Ketchup",
            "Syntax OFF",
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
