//! Render the Ctrl-P command palette popup.

use super::*;
use crate::app::{App, CommandPaletteState, PALETTE_DATA, PALETTE_SEP};
use unicode_width::UnicodeWidthStr;

const LUA_APP_CATEGORY: &str = "Apps";

// Accent colour for shortcuts and dim colour for fn_name.
const CLR_SHORTCUT: Color = Color::Rgb(100, 195, 220);
const CLR_SHORTCUT_CHANGED: Color = Color::Rgb(255, 196, 92);
const CLR_CATEGORY: Color = Color::Rgb(140, 140, 140);
const CLR_FN_NAME: Color = Color::Rgb(90, 90, 90);
const CLR_MARKER: Color = Color::Rgb(255, 220, 80);
const CLR_RECENT_STAR: Color = Color::Rgb(255, 190, 60);
// Width reserved for right-aligned shortcut column (e.g. "Ctrl+F1" = 7 + padding)
const SHORT_W: usize = 11;

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub(crate) fn command_palette_shortcuts(capture: bool) -> Vec<FooterShortcut> {
    if capture {
        vec![
            FooterShortcut {
                label: "Press key",
                key: KeyCode::Null,
            },
            FooterShortcut {
                label: " \u{232B} :Clear",
                key: KeyCode::Delete,
            },
            FooterShortcut {
                label: "R:Default",
                key: KeyCode::Char('r'),
            },
            FooterShortcut {
                label: " \u{238B} :Cancel",
                key: KeyCode::Esc,
            },
        ]
    } else {
        vec![
            FooterShortcut {
                label: " \u{23ce} :Run",
                key: KeyCode::Enter,
            },
            FooterShortcut {
                label: "F4:Set Shortcut",
                key: KeyCode::F(4),
            },
        ]
    }
}

pub(super) fn render_command_palette(
    f: &mut Frame,
    app: &App,
    s: &CommandPaletteState,
    area: Rect,
) {
    let indices = s.filtered_indices();
    // Total selectable (non-separator) items.
    let total = indices.iter().filter(|&&i| i != PALETTE_SEP).count();

    // ── Popup geometry ────────────────────────────────────────────────────
    let w = area.width.saturating_sub(4).min(72).max(54);
    let visible = (indices.len() as u16).min(18).max(3);
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
        .title(if s.capture {
            "  Command Palette - press a shortcut key  "
        } else {
            "  Command Palette  "
        })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_QS_BORDER))
        .style(Style::default().bg(CLR_QS_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    if inner.height < 3 {
        return;
    }

    // ── Input field ───────────────────────────────────────────────────────
    // Compute which selectable position match_pos corresponds to.
    let match_selectable_pos = indices
        .iter()
        .take(s.match_pos.saturating_add(1))
        .filter(|&&i| i != PALETTE_SEP)
        .count();

    let count_hint = if total > 0 {
        format!(" {}/{} ", match_selectable_pos, total)
    } else {
        " 0/0 ".to_string()
    };
    let hint_w = count_hint.len() as u16;
    let input_inner_w = inner.width.saturating_sub(hint_w) as usize;
    let input_text = if s.capture {
        " Shortcut capture active\u{2581}".to_string()
    } else {
        format!(" \u{2315} {}\u{2581}", s.query)
    };

    let input_row = Line::from(vec![
        Span::styled(
            truncate_str(&input_text, input_inner_w),
            Style::default().fg(CLR_QS_INPUT_FG).bg(CLR_QS_INPUT_BG),
        ),
        Span::styled(
            count_hint,
            Style::default()
                .fg(if total == 0 {
                    Color::Red
                } else {
                    CLR_QS_NO_MATCH
                })
                .bg(CLR_QS_INPUT_BG),
        ),
    ]);
    safe_render_widget(
        f,
        Paragraph::new(input_row).style(Style::default().bg(CLR_QS_INPUT_BG)),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    // ── Separator ─────────────────────────────────────────────────────────
    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    safe_render_widget(
        f,
        Paragraph::new(sep).style(Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );

    // ── List ──────────────────────────────────────────────────────────────
    // Row format:  marker(2) + "Category/Label (fn_name)" fills + shortcut(SHORT_W)
    const MARKER_W: usize = 2;
    let inner_w = inner.width as usize;
    let label_area_w = inner_w.saturating_sub(MARKER_W + SHORT_W).max(4);

    let list_h = inner.height.saturating_sub(3) as usize; // -input -sep -hint
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: list_h as u16,
    };

    // Scroll offset so the cursor stays visible.
    let start = if s.match_pos >= list_h {
        s.match_pos - list_h + 1
    } else {
        0
    };

    // Position of the section separator in the combined indices list, if any.
    let sep_pos: Option<usize> = indices.iter().position(|&i| i == PALETTE_SEP);

    for (row_idx, &cmd_idx) in indices.iter().skip(start).take(list_h).enumerate() {
        let vis_idx = start + row_idx;
        let row_y = list_area.y + row_idx as u16;

        // ── Section separator row ─────────────────────────────────────────
        if cmd_idx == PALETTE_SEP {
            let sep_line: String = std::iter::repeat('─').take(inner_w).collect();
            safe_render_widget(
                f,
                Paragraph::new(Line::from(vec![Span::styled(
                    sep_line,
                    Style::default().fg(CLR_QS_SEP).bg(CLR_QS_BG),
                )])),
                Rect {
                    x: list_area.x,
                    y: row_y,
                    width: list_area.width,
                    height: 1,
                },
            );
            continue;
        }

        let selected = vis_idx == s.match_pos;
        // An entry is "recent" when it appears before the separator.
        let is_recent = sep_pos.map_or(false, |sp| vis_idx < sp);

        // ── Determine if this is a static or dynamic (Lua app) entry ─────
        let lua_base = PALETTE_DATA.len();
        let is_lua = cmd_idx >= lua_base;

        let (cat_text, label_text, fn_text, shortcut_str) = if is_lua {
            let info = &s.lua_apps[cmd_idx - lua_base];
            let fn_name = format!("lua_app_{}", info.id);
            let fn_display = format!(" (lua_app_{})", info.id);
            let shortcut = app
                .effective_shortcut_for(&fn_name, None)
                .unwrap_or_default();
            let shortcut_fmt = format!("{:>width$}", shortcut, width = SHORT_W);
            (
                LUA_APP_CATEGORY,
                info.name.as_str(),
                fn_display,
                shortcut_fmt,
            )
        } else {
            let entry = &PALETTE_DATA[cmd_idx];
            let shortcut = app
                .effective_shortcut_for(entry.fn_name, entry.shortcut)
                .unwrap_or_default();
            let default_shortcut = entry
                .shortcut
                .map(crate::app::normalize_shortcut)
                .unwrap_or_default();
            let shortcut_changed = shortcut != default_shortcut;
            let _ = shortcut_changed; // used below
            let shortcut_fmt = format!("{:>width$}", shortcut, width = SHORT_W);
            (entry.category, entry.label, format!(" ({})", entry.fn_name), shortcut_fmt)
        };

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
            (
                CLR_QS_BG,
                CLR_QS_LIST_FG,
                CLR_CATEGORY,
                CLR_FN_NAME,
                CLR_SHORTCUT,
                Color::DarkGray,
            )
        };

        // Marker column: ">" selected, "★" recent (unselected), "  " otherwise.
        let (marker_str, marker_color) = if selected {
            ("> ", marker_fg)
        } else if is_recent {
            ("\u{2605} ", CLR_RECENT_STAR) // ★
        } else {
            ("  ", marker_fg)
        };

        let slash = "/";

        let fixed_prefix = display_width(cat_text) + display_width(slash);
        let fixed_suffix = display_width(&fn_text);
        let avail_for_label = label_area_w
            .saturating_sub(fixed_prefix + fixed_suffix)
            .max(4);
        let label_shown = truncate_str(label_text, avail_for_label);

        let used = fixed_prefix + display_width(&label_shown) + fixed_suffix;
        let padding = " ".repeat(label_area_w.saturating_sub(used));

        // For Lua apps: no shortcut colour customization; for static: detect overrides.
        let (fn_fg, shortcut_fg) = if !is_lua && !selected {
            let entry = &PALETTE_DATA[cmd_idx];
            let shortcut = app
                .effective_shortcut_for(entry.fn_name, entry.shortcut)
                .unwrap_or_default();
            let default_shortcut = entry
                .shortcut
                .map(crate::app::normalize_shortcut)
                .unwrap_or_default();
            let changed = shortcut != default_shortcut;
            if changed {
                (CLR_SHORTCUT_CHANGED, CLR_SHORTCUT_CHANGED)
            } else {
                (fn_fg, short_fg)
            }
        } else {
            (fn_fg, short_fg)
        };

        let spans = vec![
            Span::styled(marker_str, Style::default().fg(marker_color).bg(row_bg)),
            Span::styled(cat_text, Style::default().fg(cat_fg).bg(row_bg)),
            Span::styled(slash, Style::default().fg(cat_fg).bg(row_bg)),
            Span::styled(label_shown, Style::default().fg(label_fg).bg(row_bg)),
            Span::styled(fn_text, Style::default().fg(fn_fg).bg(row_bg)),
            Span::styled(padding, Style::default().bg(row_bg)),
            Span::styled(shortcut_str, Style::default().fg(shortcut_fg).bg(row_bg)),
        ];

        safe_render_widget(
            f,
            Paragraph::new(Line::from(spans)).style(Style::default().bg(row_bg)),
            Rect {
                x: list_area.x,
                y: row_y,
                width: list_area.width,
                height: 1,
            },
        );
    }

    // ── Hint bar ──────────────────────────────────────────────────────────
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    let hint_items = footer_shortcut_items(&command_palette_shortcuts(s.capture));
    render_shortcut_bar(f, hint_area, &hint_items, secondary_shortcut_bar_style());
}
