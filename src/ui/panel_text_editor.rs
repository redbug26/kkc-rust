use super::*;
use unicode_width::UnicodeWidthChar;

struct WrappedEditorLine {
    source_idx: usize,
    start_col: usize,
    end_col: usize,
    text: String,
    first_segment: bool,
}

pub(super) fn render_panel_text_editor(
    f: &mut Frame,
    editor: &crate::app::PanelTextEditorState,
    area: Rect,
    active: bool,
) {
    let title = if active {
        format!(
            " {}  Ctrl+S=save  Alt+Z=wrap:{}  Tab=exit ",
            editor.title(),
            if editor.wrap { "on" } else { "off" }
        )
    } else {
        format!(
            " {}  wrap:{}  Tab=focus ",
            editor.title(),
            if editor.wrap { "on" } else { "off" }
        )
    };
    let border_style = if active {
        Style::default()
            .fg(CLR_HEADER_FG)
            .bg(CLR_APP_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_PANEL_BORDER).bg(CLR_APP_BG)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(border_style)
        .style(Style::default().bg(CLR_PANEL_BG))
        .title(Span::styled(
            title,
            Style::default().fg(CLR_PANEL_TITLE).bg(CLR_APP_BG),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let text_lines = editor.textarea.lines();
    let line_number_width = text_lines.len().max(1).to_string().len().max(2);
    let gutter_width = (line_number_width + 1) as u16;
    let cursor = editor.textarea.cursor();
    let (cursor_row, cursor_col) = (cursor.0, cursor.1);

    let text_width = inner.width.saturating_sub(gutter_width).max(1) as usize;
    if editor.wrap {
        render_wrapped_editor(
            f,
            text_lines,
            inner,
            line_number_width,
            gutter_width,
            text_width,
            cursor_row,
            cursor_col,
            active,
        );
    } else {
        render_nowrap_editor(
            f,
            text_lines,
            inner,
            line_number_width,
            gutter_width,
            text_width,
            cursor_row,
            cursor_col,
            active,
        );
    }
}

fn render_nowrap_editor(
    f: &mut Frame,
    text_lines: &[String],
    inner: Rect,
    line_number_width: usize,
    gutter_width: u16,
    text_width: usize,
    cursor_row: usize,
    cursor_col: usize,
    active: bool,
) {
    let scroll_y = cursor_row.saturating_sub(inner.height.saturating_sub(1) as usize);
    let hscroll = cursor_col.saturating_sub(text_width.saturating_sub(1));
    let lines = text_lines
        .iter()
        .enumerate()
        .skip(scroll_y)
        .take(inner.height as usize)
        .map(|(idx, line)| {
            editor_line(
                line_number_width,
                idx,
                cursor_row,
                active,
                line,
                hscroll,
                text_width,
            )
        })
        .collect::<Vec<_>>();

    f.render_widget(Paragraph::new(lines), inner);

    if active {
        let visible_row = cursor_row.saturating_sub(scroll_y);
        if visible_row < inner.height as usize {
            let cursor_x = inner.x.saturating_add(gutter_width).saturating_add(
                cursor_col
                    .saturating_sub(hscroll)
                    .min(text_width.saturating_sub(1)) as u16,
            );
            let cursor_y = inner.y.saturating_add(visible_row as u16);
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

fn render_wrapped_editor(
    f: &mut Frame,
    text_lines: &[String],
    inner: Rect,
    line_number_width: usize,
    gutter_width: u16,
    text_width: usize,
    cursor_row: usize,
    cursor_col: usize,
    active: bool,
) {
    let wrapped_lines = wrap_editor_lines(text_lines, text_width);
    let cursor_visual_row = wrapped_lines
        .iter()
        .rposition(|line| {
            line.source_idx == cursor_row
                && cursor_col >= line.start_col
                && cursor_col <= line.end_col
        })
        .unwrap_or_else(|| {
            wrapped_lines
                .iter()
                .rposition(|line| line.source_idx == cursor_row)
                .unwrap_or(0)
        });
    let scroll_y = cursor_visual_row.saturating_sub(inner.height.saturating_sub(1) as usize);

    let lines = wrapped_lines
        .iter()
        .skip(scroll_y)
        .take(inner.height as usize)
        .map(|wrapped| {
            let number = if wrapped.first_segment {
                format!(
                    "{:>width$} ",
                    wrapped.source_idx + 1,
                    width = line_number_width
                )
            } else {
                " ".repeat(line_number_width + 1)
            };
            let number_style = line_number_style(wrapped.source_idx, cursor_row, active);
            Line::from(vec![
                Span::styled(number, number_style),
                Span::styled(
                    wrapped.text.clone(),
                    Style::default().fg(CLR_TEXT).bg(CLR_PANEL_BG),
                ),
            ])
        })
        .collect::<Vec<_>>();

    f.render_widget(Paragraph::new(lines), inner);

    if active {
        let visible_row = cursor_visual_row.saturating_sub(scroll_y);
        if visible_row < inner.height as usize {
            let cursor_line = wrapped_lines.get(cursor_visual_row);
            let cursor_segment_col = cursor_line
                .map(|line| {
                    visual_width_between(
                        &text_lines[cursor_row],
                        line.start_col,
                        cursor_col.min(line.end_col),
                    )
                })
                .unwrap_or(0);
            let text_width = text_width as u16;
            let cursor_x = inner
                .x
                .saturating_add(gutter_width)
                .saturating_add((cursor_segment_col as u16).min(text_width.saturating_sub(1)));
            let cursor_y = inner.y.saturating_add(visible_row as u16);
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

fn editor_line(
    line_number_width: usize,
    idx: usize,
    cursor_row: usize,
    active: bool,
    line: &str,
    hscroll: usize,
    text_width: usize,
) -> Line<'static> {
    let number = format!("{:>width$} ", idx + 1, width = line_number_width);
    Line::from(vec![
        Span::styled(number, line_number_style(idx, cursor_row, active)),
        Span::styled(
            slice_chars(line, hscroll, text_width),
            Style::default().fg(CLR_TEXT).bg(CLR_PANEL_BG),
        ),
    ])
}

fn line_number_style(idx: usize, cursor_row: usize, active: bool) -> Style {
    if idx == cursor_row && active {
        Style::default()
            .fg(CLR_PANEL_TITLE)
            .bg(CLR_PANEL_BG)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(CLR_MENU_DD_SEP).bg(CLR_PANEL_BG)
    }
}

fn wrap_editor_lines(lines: &[String], width: usize) -> Vec<WrappedEditorLine> {
    let width = width.max(1);
    let mut out = Vec::new();
    for (source_idx, line) in lines.iter().enumerate() {
        let chars = line.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            out.push(WrappedEditorLine {
                source_idx,
                start_col: 0,
                end_col: 0,
                text: String::new(),
                first_segment: true,
            });
            continue;
        }

        let mut start_col = 0usize;
        let mut first_segment = true;
        while start_col < chars.len() {
            let (text, end_col) = wrap_segment(&chars, start_col, width);
            out.push(WrappedEditorLine {
                source_idx,
                start_col,
                end_col,
                text,
                first_segment,
            });
            start_col = end_col;
            first_segment = false;
        }
    }
    out
}

fn wrap_segment(chars: &[char], start_col: usize, width: usize) -> (String, usize) {
    let mut text = String::new();
    let mut used = 0usize;
    let mut end_col = start_col;
    for (offset, ch) in chars[start_col..].iter().copied().enumerate() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
        if used > 0 && used + ch_width > width {
            break;
        }
        if used == 0 && ch_width > width {
            text.push(ch);
            end_col = start_col + offset + 1;
            break;
        }
        text.push(ch);
        used += ch_width;
        end_col = start_col + offset + 1;
        if used >= width {
            break;
        }
    }
    (text, end_col.max(start_col + 1).min(chars.len()))
}

fn visual_width_between(line: &str, start_col: usize, end_col: usize) -> usize {
    line.chars()
        .skip(start_col)
        .take(end_col.saturating_sub(start_col))
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(1).max(1))
        .sum()
}

fn slice_chars(line: &str, start_col: usize, width: usize) -> String {
    line.chars().skip(start_col).take(width).collect()
}

#[cfg(test)]
mod tests {
    use super::{slice_chars, wrap_editor_lines};

    #[test]
    fn wraps_long_editor_lines_to_text_width() {
        let lines = vec!["abcdef".to_string()];
        let wrapped = wrap_editor_lines(&lines, 3);

        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped[0].text, "abc");
        assert_eq!(wrapped[1].text, "def");
        assert!(wrapped[0].first_segment);
        assert!(!wrapped[1].first_segment);
    }

    #[test]
    fn nowrap_slice_scrolls_to_cursor_window() {
        assert_eq!(slice_chars("abcdef", 3, 3), "def");
    }
}
