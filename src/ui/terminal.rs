use super::*;

fn clr_term_bg() -> Color {
    clr_menu_dd_bg()
}

fn clr_term_fg() -> Color {
    clr_menu_dd_fg()
}

fn clr_term_border() -> Color {
    clr_exec()
}

fn clr_term_prompt() -> Color {
    clr_exec()
}

fn clr_term_input() -> Color {
    clr_qs_input_fg()
}

pub(super) fn render_terminal(f: &mut Frame, app: &App, area: Rect) {
    let ts = &app.terminal;
    let running = app.running_cmd.is_some();

    f.render_widget(Clear, area);
    let title = format!(
        " KKC Terminal — {}{}— Ctrl-U/Esc to close ",
        app.active_panel().path.display(),
        if running { " [running…] " } else { " " },
    );
    f.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if running {
                clr_archive()
            } else {
                clr_term_border()
            }))
            .style(Style::default().bg(clr_term_bg()))
            .title(Span::styled(title, Style::default().fg(clr_term_prompt()))),
        area,
    );

    let inner = Block::default().borders(Borders::ALL).inner(area);
    if inner.height < 2 {
        return;
    }

    // Split: scrollback lines + prompt input line at the bottom.
    let prompt_y = inner.y + inner.height - 1;
    let log_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height - 1,
    };

    // Scrollback — bottom-aligned
    let visible_lines = log_area.height as usize;
    let start = ts.output.len().saturating_sub(visible_lines);
    let lines: Vec<Line> = ts.output[start..]
        .iter()
        .map(|l| {
            // Lines emitted by the prompt itself get a fixed style
            if let Some(prompt_line) = l.strip_prefix(crate::terminal::PROMPT_LINE_MARKER) {
                return Line::from(Span::styled(
                    prompt_line.to_string(),
                    Style::default()
                        .fg(clr_term_prompt())
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if l.starts_with('[') {
                return Line::from(Span::styled(
                    l.clone(),
                    Style::default().fg(clr_qs_no_match()),
                ));
            }
            // For all other lines parse embedded ANSI escape codes
            let mut line = crate::terminal::ansi_line_to_line(l);
            // If the line has no spans with any explicit fg colour we fall back
            // to the default terminal foreground so plain text matches the theme.
            if line.spans.iter().all(|s| s.style.fg.is_none()) {
                line = line.style(Style::default().fg(clr_term_fg()));
            }
            line
        })
        .collect();

    safe_render_widget(
        f,
        Paragraph::new(lines).style(Style::default().bg(clr_term_bg())),
        log_area,
    );

    // Prompt line (blocked while running)
    let prompt = crate::terminal::terminal_prompt(app, running);
    let prompt_len = prompt.chars().count() as u16;
    let input_x = inner.x + prompt_len;

    safe_render_widget(
        f,
        Paragraph::new(Line::from(vec![
            Span::styled(
                prompt,
                Style::default()
                    .fg(if running {
                        clr_archive()
                    } else {
                        clr_term_prompt()
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(ts.input.clone(), Style::default().fg(clr_term_input())),
        ]))
        .style(Style::default().bg(clr_term_bg())),
        Rect {
            x: inner.x,
            y: prompt_y,
            width: inner.width,
            height: 1,
        },
    );

    // Show cursor only when not running
    if !running {
        let cursor_col = ts.input[..ts.cursor].chars().count() as u16;
        let cx = input_x + cursor_col;
        if cx < inner.x + inner.width {
            f.set_cursor_position((cx, prompt_y));
        }
    }
}
