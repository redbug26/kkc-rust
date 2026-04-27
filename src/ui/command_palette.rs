//! Render the Ctrl-P command palette popup.

use super::*;
use crate::app::{CommandPaletteState, PALETTE_DATA};

// Accent colour for shortcuts and dim colour for fn_name.
const CLR_SHORTCUT: Color = Color::Rgb(100, 195, 220);
const CLR_CATEGORY: Color = Color::Rgb(140, 140, 140);
const CLR_FN_NAME: Color = Color::Rgb(90, 90, 90);
const CLR_MARKER: Color = Color::Rgb(255, 220, 80);
// Width reserved for right-aligned shortcut column (e.g. "Ctrl+F1" = 7 + padding)
const SHORT_W: usize = 11;

pub(super) fn render_command_palette(f: &mut Frame, s: &CommandPaletteState, area: Rect) {
    let indices = s.filtered_indices();
    let total = indices.len();

    // ── Popup geometry ────────────────────────────────────────────────────
    let w = area.width.saturating_sub(4).min(72).max(54);
    let visible = (total as u16).min(18).max(3);
    // 1 input + 1 sep + visible items + 1 hint + 2 border
    let h = (visible + 5).min(area.height.saturating_sub(3)).max(8);

    let popup = clamp_rect(
        area,
        Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + 2,
            width: w,
            height: h,
        },
    );

    safe_render_widget(f, Clear, popup);
    let block = Block::default()
        .title("  Command Palette  ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_QS_BORDER))
        .style(Style::default().bg(CLR_QS_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    if inner.height < 3 {
        return;
    }

    // ── Input field ───────────────────────────────────────────────────────
    let count_hint = if total > 0 {
        format!(" {}/{} ", s.match_pos + 1, total)
    } else {
        " 0/0 ".to_string()
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = format!(" \u{2315} {}\u{2581}", s.query);

    let input_row = Line::from(vec![
        Span::styled(
            truncate_str(&input_text, input_inner_w),
            Style::default().fg(CLR_QS_INPUT_FG).bg(CLR_QS_INPUT_BG),
        ),
        Span::styled(
            count_hint,
            Style::default()
                .fg(if total == 0 { Color::Red } else { CLR_QS_NO_MATCH })
                .bg(CLR_QS_INPUT_BG),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(CLR_QS_INPUT_BG)),
        Rect { x: inner.x, y: inner.y, width: inner.width, height: 1 },
    );

    // ── Separator ─────────────────────────────────────────────────────────
    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        Rect { x: inner.x, y: inner.y + 1, width: inner.width, height: 1 },
    );

    // ── List ──────────────────────────────────────────────────────────────
    // Row format:  marker(2) + "Category/Label (fn_name)" fills + shortcut(SHORT_W)
    // e.g.:  "> File/View file (view_file)       F3"
    const MARKER_W: usize = 2;
    let inner_w = inner.width as usize;
    // Space available for the "Category/Label (fn_name)" text
    let label_area_w = inner_w.saturating_sub(MARKER_W + SHORT_W).max(4);

    let list_h = inner.height.saturating_sub(3) as usize; // -input -sep -hint
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: list_h as u16,
    };

    // Scroll offset so the cursor stays visible
    let start = if s.match_pos >= list_h {
        s.match_pos - list_h + 1
    } else {
        0
    };

    for (row_idx, &cmd_idx) in indices.iter().skip(start).take(list_h).enumerate() {
        let vis_idx = start + row_idx;
        let selected = vis_idx == s.match_pos;
        let entry = &PALETTE_DATA[cmd_idx];

        let (row_bg, label_fg, cat_fg, fn_fg, short_fg, marker_fg) = if selected {
            (
                CLR_QS_SEL_BG,
                CLR_QS_SEL_FG,
                Color::Rgb(200, 215, 240),
                Color::Rgb(130, 155, 185),
                Color::Rgb(150, 230, 255),
                CLR_MARKER,
            )
        } else {
            (CLR_QS_BG, CLR_QS_LIST_FG, CLR_CATEGORY, CLR_FN_NAME, CLR_SHORTCUT, Color::DarkGray)
        };

        let marker = if selected { "> " } else { "  " };

        // Build "Category/Label (fn_name)" — truncate to fit label_area_w
        // We split into styled sub-spans: cat (dim), / (dim), label (normal), fn (dim)
        let cat_text = entry.category;
        let slash = "/";
        let label_text = entry.label;
        let fn_text = format!(" ({})", entry.fn_name);

        // Compute how much space each part gets; truncate label if needed
        let fixed_prefix = cat_text.len() + slash.len();
        let fixed_suffix = fn_text.len();
        let avail_for_label = label_area_w
            .saturating_sub(fixed_prefix + fixed_suffix)
            .max(4);
        let label_shown = truncate_str(label_text, avail_for_label);

        // Combined length; pad with spaces to fill label_area_w
        let used = fixed_prefix + label_shown.len() + fixed_suffix;
        let padding = " ".repeat(label_area_w.saturating_sub(used));

        let shortcut_str = format!(
            "{:>width$}",
            entry.shortcut.unwrap_or(""),
            width = SHORT_W
        );

        let spans = vec![
            Span::styled(marker, Style::default().fg(marker_fg).bg(row_bg)),
            Span::styled(cat_text, Style::default().fg(cat_fg).bg(row_bg)),
            Span::styled(slash, Style::default().fg(cat_fg).bg(row_bg)),
            Span::styled(label_shown, Style::default().fg(label_fg).bg(row_bg)),
            Span::styled(fn_text, Style::default().fg(fn_fg).bg(row_bg)),
            Span::styled(padding, Style::default().bg(row_bg)),
            Span::styled(shortcut_str, Style::default().fg(short_fg).bg(row_bg)),
        ];

        safe_render_widget(
            f,
            Paragraph::new(Line::from(spans)).style(Style::default().bg(row_bg)),
            Rect {
                x: list_area.x,
                y: list_area.y + row_idx as u16,
                width: list_area.width,
                height: 1,
            },
        );
    }

    // ── Hint bar ──────────────────────────────────────────────────────────
    let hint_y = inner.y + inner.height.saturating_sub(1);
    safe_render_widget(
        f,
        Paragraph::new("  \u{23ce} Run   Esc Close ")
            .style(Style::default().fg(CLR_BUTTON_FG).bg(CLR_BUTTON_BG)),
        Rect { x: inner.x, y: hint_y, width: inner.width, height: 1 },
    );
}

