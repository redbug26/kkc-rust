use super::*;
use crate::app::{PALETTE_DATA, ShortcutPanelState};

const SHORTCUT_W: usize = 16;
const CLR_SHORTCUT: Color = Color::Rgb(100, 195, 220);
const CLR_FN_NAME: Color = Color::Rgb(90, 90, 90);
const CLR_MARKER: Color = Color::Rgb(255, 220, 80);

pub(crate) fn shortcut_panel_shortcuts() -> Vec<FooterShortcut> {
    vec![
        FooterShortcut {
            label: "\u{23ce}:Set",
            key: KeyCode::Enter,
        },
        FooterShortcut {
            label: "\u{232B}:Clear",
            key: KeyCode::Delete,
        },
        FooterShortcut {
            label: "R:Default",
            key: KeyCode::Char('r'),
        },
        FooterShortcut {
            label: "\u{238B}:Close",
            key: KeyCode::Esc,
        },
    ]
}

pub(super) fn render_shortcut_panel(
    f: &mut Frame,
    app: &App,
    state: &ShortcutPanelState,
    area: Rect,
) {
    let indices = state.filtered_indices();
    let total = indices.len();
    let w = area.width.saturating_sub(4).min(86).max(58);
    let visible = (total as u16).min(20).max(4);
    let h = (visible + 5).min(area.height.saturating_sub(3)).max(9);
    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + area.width.saturating_sub(w) / 2,
            y: area.y + 2,
            width: w,
            height: h,
        },
    );

    safe_render_widget(f, Clear, popup);
    let title = if state.capture {
        " Shortcuts - press a key "
    } else {
        " Shortcuts "
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_QS_BORDER))
        .style(Style::default().bg(CLR_QS_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);
    if inner.height < 4 {
        return;
    }

    let count_hint = if total > 0 {
        format!(" {}/{} ", state.cursor.saturating_add(1).min(total), total)
    } else {
        " 0/0 ".to_string()
    };
    let input_w = inner.width.saturating_sub(count_hint.len() as u16) as usize;
    let prompt = if state.capture {
        " Press shortcut, Backspace/Delete clears, R default, Esc cancels".to_string()
    } else {
        format!(" Search {}", state.query)
    };
    safe_render_widget(
        f,
        Paragraph::new(Line::from(vec![
            Span::styled(
                truncate_str(&prompt, input_w),
                Style::default().fg(CLR_QS_INPUT_FG).bg(CLR_QS_INPUT_BG),
            ),
            Span::styled(
                count_hint,
                Style::default().fg(CLR_QS_NO_MATCH).bg(CLR_QS_INPUT_BG),
            ),
        ]))
        .style(Style::default().bg(CLR_QS_INPUT_BG)),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    safe_render_widget(
        f,
        Paragraph::new("-".repeat(inner.width as usize))
            .style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );

    let list_h = inner.height.saturating_sub(3) as usize;
    let start = if state.cursor >= list_h {
        state.cursor - list_h + 1
    } else {
        0
    };
    let label_w = (inner.width as usize)
        .saturating_sub(SHORTCUT_W + 4)
        .max(12);

    for (row_idx, palette_idx) in indices.iter().skip(start).take(list_h).enumerate() {
        let row_y = inner.y + 2 + row_idx as u16;
        let selected = start + row_idx == state.cursor;
        let entry = &PALETTE_DATA[*palette_idx];
        let shortcut = app
            .effective_shortcut_for(entry.fn_name, entry.shortcut)
            .unwrap_or_default();
        let default_shortcut = entry
            .shortcut
            .map(crate::app::normalize_shortcut)
            .unwrap_or_default();
        let changed = shortcut != default_shortcut;

        let (bg, fg, dim, shortcut_fg) = if selected {
            (
                CLR_QS_SEL_BG,
                CLR_QS_SEL_FG,
                Color::Rgb(170, 190, 220),
                Color::Rgb(150, 230, 255),
            )
        } else {
            (CLR_QS_BG, CLR_QS_LIST_FG, CLR_FN_NAME, CLR_SHORTCUT)
        };
        let marker = if selected { "> " } else { "  " };
        let title = format!("{}/{} ({})", entry.category, entry.label, entry.fn_name);
        let title = truncate_str(&title, label_w);
        let pad = " ".repeat(label_w.saturating_sub(title.len()));
        let shortcut_text = if changed && !default_shortcut.is_empty() {
            format!("{}*", shortcut)
        } else {
            shortcut
        };
        let shortcut_text = format!("{:>width$}", shortcut_text, width = SHORTCUT_W);

        safe_render_widget(
            f,
            Paragraph::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(CLR_MARKER).bg(bg)),
                Span::styled(title, Style::default().fg(fg).bg(bg)),
                Span::styled(pad, Style::default().bg(bg)),
                Span::styled(shortcut_text, Style::default().fg(shortcut_fg).bg(bg)),
                Span::styled(" ", Style::default().fg(dim).bg(bg)),
            ]))
            .style(Style::default().bg(bg)),
            Rect {
                x: inner.x,
                y: row_y,
                width: inner.width,
                height: 1,
            },
        );
    }

    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    let hint_items = footer_shortcut_items(&shortcut_panel_shortcuts());
    render_shortcut_bar(f, hint_area, &hint_items, default_shortcut_bar_style());
}
