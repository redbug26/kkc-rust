use super::*;

const CLR_TERM_BG: Color = Color::Rgb(10, 10, 10);
const CLR_TERM_FG: Color = Color::Rgb(200, 200, 200);
const CLR_TERM_BORDER: Color = Color::Rgb(80, 180, 80);
const CLR_TERM_PROMPT: Color = Color::Rgb(100, 220, 100);
const CLR_TERM_INPUT: Color = Color::White;

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
                Color::Rgb(220, 160, 60)
            } else {
                CLR_TERM_BORDER
            }))
            .style(Style::default().bg(CLR_TERM_BG))
            .title(Span::styled(title, Style::default().fg(CLR_TERM_PROMPT))),
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
                        .fg(CLR_TERM_PROMPT)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if l.starts_with('[') {
                return Line::from(Span::styled(
                    l.clone(),
                    Style::default().fg(Color::Rgb(160, 160, 160)),
                ));
            }
            // For all other lines parse embedded ANSI escape codes
            let mut line = crate::terminal::ansi_line_to_line(l);
            // If the line has no spans with any explicit fg colour we fall back
            // to the default terminal foreground so plain text matches the theme.
            if line.spans.iter().all(|s| s.style.fg.is_none()) {
                line = line.style(Style::default().fg(CLR_TERM_FG));
            }
            line
        })
        .collect();

    safe_render_widget(
        f,
        Paragraph::new(lines).style(Style::default().bg(CLR_TERM_BG)),
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
                        Color::Rgb(220, 160, 60)
                    } else {
                        CLR_TERM_PROMPT
                    })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(ts.input.clone(), Style::default().fg(CLR_TERM_INPUT)),
        ]))
        .style(Style::default().bg(CLR_TERM_BG)),
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
