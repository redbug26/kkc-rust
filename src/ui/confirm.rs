use super::*;
use crate::app::{AssocInputAction, AssocInputDialog};

struct TextDialogStyle {
    border_fg: Color,
    dialog_bg: Color,
    prompt_fg: Color,
    input_bg: Color,
    input_fg: Color,
    hint_fg: Color,
}

enum TextDialogFooter<'a> {
    Plain(std::borrow::Cow<'a, str>),
    AssocMultiline,
}

pub(crate) fn assoc_input_shortcuts() -> Vec<FooterShortcut> {
    vec![
        FooterShortcut {
            label: "Enter:NewLine",
            key: KeyCode::Enter,
        },
        FooterShortcut {
            label: "F2:Save",
            key: KeyCode::F(2),
        },
        FooterShortcut {
            label: "Esc:Cancel",
            key: KeyCode::Esc,
        },
    ]
}

pub(super) fn render_confirm(f: &mut Frame, dlg: &ConfirmDialog, area: Rect) {
    if dlg.macro_name.is_some() {
        let Some(spec) = crate::lua_dialog::confirm_render_spec(dlg) else {
            return;
        };
        render_confirm_box(f, &spec, area, dlg.active_button);
        return;
    }

    match &dlg.action {
        ConfirmAction::Message | ConfirmAction::MessageThen(_) => {
            render_confirm_message(f, dlg, area)
        }
        ConfirmAction::Quit | ConfirmAction::Delete(_) | ConfirmAction::DeleteRemote(_) => {}
        ConfirmAction::CloseTextEditorUnsaved => render_confirm_text_editor_unsaved(f, area),
        ConfirmAction::SaveEditorBeforeQuit => render_confirm_save_editor_before_quit(f, area),
    }
}

/// Hard-wrap `msg` to fit within `max_width` display columns.
/// Tabs are expanded to 2 spaces. Word-breaks are preferred; hard-breaks
/// are used when a single token exceeds the available width.
fn wrap_message(msg: &str, max_width: usize) -> String {
    if max_width == 0 {
        return msg.to_string();
    }
    let mut result = String::new();
    for raw_line in msg.lines() {
        let line = raw_line.replace('\t', "  ");
        if line.is_empty() {
            result.push('\n');
            continue;
        }
        let mut remaining: &str = &line;
        while !remaining.is_empty() {
            let mut acc = 0usize;
            let mut last_space: Option<usize> = None;
            let mut hard_cut: Option<usize> = None;
            for (i, ch) in remaining.char_indices() {
                let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
                if acc + cw > max_width {
                    hard_cut = Some(i);
                    break;
                }
                acc += cw;
                if ch == ' ' {
                    last_space = Some(i);
                }
            }
            match hard_cut {
                None => {
                    result.push_str(remaining);
                    result.push('\n');
                    break;
                }
                Some(cut) => {
                    let split = last_space.unwrap_or(cut);
                    result.push_str(&remaining[..split]);
                    result.push('\n');
                    remaining = remaining[split..].trim_start_matches(' ');
                }
            }
        }
    }
    result
}

fn render_confirm_message(f: &mut Frame, dlg: &ConfirmDialog, area: Rect) {
    let max_w = area.width.saturating_sub(4);
    let width = 72u16.min(max_w).max(40);
    // 2 border cols + 2 padding cols = 4 reserved
    let text_w = width.saturating_sub(4) as usize;

    // Pre-wrap so we know the exact row count (and avoid ratatui overflowing
    // long unbreakable tokens like Lua file paths).
    let wrapped = wrap_message(dlg.message.as_deref().unwrap_or(""), text_w);
    let msg_rows = wrapped.lines().count().max(1) as u16;

    // borders(2) + top_pad(1) + text rows + bottom_pad(1) + ok_btn(1) + hint(1)
    let desired_h = msg_rows + 6;
    let height = desired_h.max(8).min(area.height.saturating_sub(2).max(8));

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

    let title_str = if dlg.title.as_deref().unwrap_or("").is_empty() {
        " Notice ".to_string()
    } else {
        format!(" {} ", dlg.title.as_deref().unwrap_or(""))
    };

    let block = Block::default()
        .title(title_str)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER))
        .style(Style::default().bg(CLR_MENU_DD_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    // Available rows for message text (leave room for OK button + hint)
    let msg_h = inner.height.saturating_sub(3).max(1);
    safe_render_widget(
        f,
        Paragraph::new(wrapped.as_str())
            .style(Style::default().fg(CLR_MENU_DD_FG).bg(CLR_MENU_DD_BG)),
        Rect {
            x: inner.x + 1,
            y: inner.y + 1,
            width: inner.width.saturating_sub(2),
            height: msg_h,
        },
    );

    safe_render_widget(
        f,
        Paragraph::new(" [ OK ] ")
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(CLR_MENU_SEL_FG)
                    .bg(CLR_MENU_SEL_BG)
                    .add_modifier(Modifier::BOLD),
            ),
        Rect {
            x: inner.x + inner.width.saturating_sub(8) / 2,
            y: inner.y + inner.height.saturating_sub(2),
            width: 8,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new("Enter / Esc")
            .alignment(Alignment::Center)
            .style(Style::default().fg(CLR_MENU_DD_SEP).bg(CLR_MENU_DD_BG)),
        Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        },
    );
}

// ---------------------------------------------------------------------------
// Lua-backed confirmation boxes
// ---------------------------------------------------------------------------

fn render_confirm_box(
    f: &mut Frame,
    spec: &crate::lua_dialog::ConfirmDialogSpec,
    area: Rect,
    active_button: ConfirmButton,
) {
    let popup = clamp_rect(area, crate::lua_dialog::confirm_dialog_popup_rect(spec, area));

    if spec.shadow_dx > 0 || spec.shadow_dy > 0 {
        let sh = Rect {
            x: popup.x + spec.shadow_dx,
            y: popup.y + spec.shadow_dy,
            width: popup.width,
            height: popup.height,
        };
        if sh.x + sh.width <= area.x + area.width && sh.y + sh.height <= area.y + area.height {
            safe_render_widget(
                f,
                Block::default().style(Style::default().bg(Color::Rgb(20, 15, 10))),
                sh,
            );
        }
    }
    safe_render_widget(f, Clear, popup);

    let palette = DialogButtonPalette::from_lua(spec.palette);
    let block_style = confirm_box_style(palette);
    let block = Block::default()
        .title(spec.title.as_str())
        .borders(Borders::ALL)
        .border_style(block_style.border)
        .title_style(block_style.title)
        .style(block_style.body);
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    let sep: String = std::iter::repeat('─').take(inner.width as usize).collect();
    for sep_y in &spec.separators {
        safe_render_widget(
            f,
            Paragraph::new(sep.clone()).style(
                Style::default()
                    .fg(block_style.separator_fg)
                    .bg(block_style.bg),
            ),
            Rect {
                x: inner.x,
                y: inner.y + *sep_y,
                width: inner.width,
                height: 1,
            },
        );
    }

    if let Some(header) = &spec.header {
        render_confirm_box_text(f, header, inner, block_style.header, Alignment::Center);
    }
    render_confirm_box_text(
        f,
        &spec.message,
        inner,
        block_style.message,
        Alignment::Center,
    );

    render_dialog_buttons(f, spec, area, active_button, palette);
}

struct ConfirmBoxStyle {
    bg: Color,
    body: Style,
    border: Style,
    title: Style,
    separator_fg: Color,
    header: Style,
    message: Style,
}

fn confirm_box_style(palette: DialogButtonPalette) -> ConfirmBoxStyle {
    match palette {
        DialogButtonPalette::Normal => ConfirmBoxStyle {
            bg: CLR_APP_BG,
            body: Style::default().bg(CLR_APP_BG),
            border: Style::default().fg(CLR_PANEL_BORDER_DIM).bg(CLR_APP_BG),
            title: Style::default()
                .fg(CLR_BUTTON_FG)
                .bg(CLR_APP_BG)
                .add_modifier(Modifier::BOLD),
            separator_fg: CLR_PANEL_BORDER_DIM,
            header: Style::default()
                .fg(CLR_BUTTON_FG)
                .bg(CLR_APP_BG)
                .add_modifier(Modifier::BOLD),
            message: Style::default().fg(Color::Rgb(50, 36, 22)).bg(CLR_APP_BG),
        },
        DialogButtonPalette::Danger => ConfirmBoxStyle {
            bg: Color::Rgb(38, 18, 14),
            body: Style::default().bg(Color::Rgb(38, 18, 14)),
            border: Style::default().fg(Color::Rgb(180, 60, 40)),
            title: Style::default()
                .fg(Color::Rgb(255, 100, 80))
                .add_modifier(Modifier::BOLD),
            separator_fg: Color::Rgb(180, 60, 40),
            header: Style::default()
                .fg(Color::Rgb(255, 160, 60))
                .bg(Color::Rgb(38, 18, 14))
                .add_modifier(Modifier::BOLD),
            message: Style::default()
                .fg(Color::Rgb(240, 200, 180))
                .bg(Color::Rgb(38, 18, 14)),
        },
    }
}

fn render_confirm_box_text(
    f: &mut Frame,
    text: &crate::lua_dialog::ConfirmDialogText,
    inner: Rect,
    style: Style,
    alignment: Alignment,
) {
    let message = if text.message_prefix_blank {
        format!("\n{}", text.message_text)
    } else {
        text.message_text.clone()
    };
    let max_width = inner.width as usize;
    let message = truncate_multiline_to_width(&message, max_width);
    safe_render_widget(
        f,
        Paragraph::new(message).alignment(alignment).style(style),
        Rect {
            x: inner.x,
            y: inner.y + text.message_y,
            width: inner.width,
            height: text.message_height,
        },
    );
}

fn truncate_to_display_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if unicode_width::UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if width + cw > max_width - 1 {
            break;
        }
        out.push(ch);
        width += cw;
    }
    out.push('…');
    out
}

fn truncate_multiline_to_width(text: &str, max_width: usize) -> String {
    text.lines()
        .map(|line| truncate_to_display_width(line, max_width))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_dialog_buttons(
    f: &mut Frame,
    spec: &crate::lua_dialog::ConfirmDialogSpec,
    area: Rect,
    active_button: ConfirmButton,
    palette: DialogButtonPalette,
) {
    let rects = crate::lua_dialog::confirm_dialog_button_rects(spec, area);
    for (idx, button) in spec.buttons.iter().enumerate() {
        let Some(rect) = rects.get(idx) else {
            continue;
        };
        let active = match idx {
            0 => active_button == ConfirmButton::Primary,
            1 => active_button == ConfirmButton::Secondary,
            _ => false,
        };
        render_dialog_button(f, *rect, button.label.as_str(), active, palette);
    }
}

#[derive(Clone, Copy)]
enum DialogButtonPalette {
    Normal,
    Danger,
}

impl DialogButtonPalette {
    fn from_lua(palette: crate::lua_dialog::ConfirmDialogPalette) -> Self {
        match palette {
            crate::lua_dialog::ConfirmDialogPalette::Normal => Self::Normal,
            crate::lua_dialog::ConfirmDialogPalette::Danger => Self::Danger,
        }
    }
}

fn render_dialog_button(
    f: &mut Frame,
    area: Rect,
    label: &str,
    active: bool,
    palette: DialogButtonPalette,
) {
    let active_bg = match palette {
        DialogButtonPalette::Normal => CLR_PANEL_BORDER,
        DialogButtonPalette::Danger => Color::Rgb(190, 58, 44),
    };
    let inactive_bg = match palette {
        DialogButtonPalette::Normal => Color::Rgb(108, 92, 74),
        DialogButtonPalette::Danger => Color::Rgb(30, 14, 12),
    };
    let inactive_fg = match palette {
        DialogButtonPalette::Normal => Color::Rgb(132, 118, 98),
        DialogButtonPalette::Danger => Color::Rgb(124, 92, 80),
    };
     let shadow_bg = match palette {
        DialogButtonPalette::Normal => CLR_APP_BG,
        DialogButtonPalette::Danger => Color::Rgb(38, 18, 14),
    };
    let shadow_fg = match palette {
        DialogButtonPalette::Normal => Color::Rgb(118, 95, 70),
        DialogButtonPalette::Danger => Color::Rgb(88, 36, 30),
    };
    let style = if active {
        Style::default()
            .fg(if matches!(palette, DialogButtonPalette::Danger) {
                Color::White
            } else {
                Color::Black
            })
            .bg(active_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(inactive_fg).bg(inactive_bg)
    };
    let shadow_style = Style::default().fg(shadow_fg).bg(shadow_bg);
    let bg_style = Style::default().bg(inactive_bg);
    let text = format!("{:^width$}", label, width = area.width as usize);

    safe_render_widget(
        f,
        Paragraph::new(Line::from(vec![
            Span::styled(text, style),
            Span::styled("▖", shadow_style),
        ]))
        .style(bg_style),
        Rect {
            x: area.x,
            y: area.y,
            width: area.width.saturating_add(1),
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new(Line::from(vec![
            Span::styled(" ", shadow_style),
            Span::styled(
                "▀".repeat(area.width.saturating_sub(1) as usize),
                shadow_style,
            ),
            Span::styled("▘", shadow_style),
        ]))
        .style(bg_style),
        Rect {
            x: area.x,
            y: area.y.saturating_add(1),
            width: area.width.saturating_add(1),
            height: 1,
        },
    );
}

fn render_confirm_text_editor_unsaved(f: &mut Frame, area: Rect) {
    const W: u16 = 58;
    const H: u16 = 9;
    let x = (area.width.saturating_sub(W)) / 2 + area.x;
    let y = (area.height.saturating_sub(H)) / 2 + area.y;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width: W,
            height: H,
        },
    );
    safe_render_widget(f, Clear, popup);

    let title = Span::styled(
        " Unsaved Changes ",
        Style::default()
            .fg(Color::Rgb(255, 210, 120))
            .add_modifier(Modifier::BOLD),
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER))
        .style(Style::default().bg(CLR_APP_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    safe_render_widget(
        f,
        Paragraph::new("Save changes before closing the text editor?")
            .alignment(Alignment::Center)
            .style(Style::default().fg(CLR_MENU_DD_FG).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 2,
        },
    );

    let btn_y = inner.y + 4;
    let save_w: u16 = 11;
    let discard_w: u16 = 13;
    let gap: u16 = 4;
    let btn_x = inner.x + (inner.width.saturating_sub(save_w + gap + discard_w)) / 2;

    safe_render_widget(
        f,
        Paragraph::new(" [ Save ] ").style(
            Style::default()
                .fg(Color::Black)
                .bg(CLR_PANEL_BORDER)
                .add_modifier(Modifier::BOLD),
        ),
        Rect {
            x: btn_x,
            y: btn_y,
            width: save_w,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new(" [ Discard ] ").style(Style::default().fg(CLR_TEXT).bg(CLR_APP_BG)),
        Rect {
            x: btn_x + save_w + gap,
            y: btn_y,
            width: discard_w,
            height: 1,
        },
    );

    safe_render_widget(
        f,
        Paragraph::new("Enter/Y=Save  ·  N=Discard  ·  Esc=Cancel")
            .alignment(Alignment::Center)
            .style(Style::default().fg(CLR_MENU_DD_SEP).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: btn_y + 2,
            width: inner.width,
            height: 1,
        },
    );
}

fn render_confirm_save_editor_before_quit(f: &mut Frame, area: Rect) {
    const W: u16 = 58;
    const H: u16 = 9;
    let x = (area.width.saturating_sub(W)) / 2 + area.x;
    let y = (area.height.saturating_sub(H)) / 2 + area.y;
    let popup = clamp_rect(
        area,
        Rect {
            x,
            y,
            width: W,
            height: H,
        },
    );
    safe_render_widget(f, Clear, popup);

    let title = Span::styled(
        " Unsaved Changes ",
        Style::default()
            .fg(Color::Rgb(255, 210, 120))
            .add_modifier(Modifier::BOLD),
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_PANEL_BORDER))
        .style(Style::default().bg(CLR_APP_BG));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    safe_render_widget(
        f,
        Paragraph::new("Save changes before quitting?")
            .alignment(Alignment::Center)
            .style(Style::default().fg(CLR_MENU_DD_FG).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 2,
        },
    );

    let btn_y = inner.y + 4;
    let save_w: u16 = 11;
    let discard_w: u16 = 13;
    let gap: u16 = 4;
    let btn_x = inner.x + (inner.width.saturating_sub(save_w + gap + discard_w)) / 2;

    safe_render_widget(
        f,
        Paragraph::new(" [ Save ] ").style(
            Style::default()
                .fg(Color::Black)
                .bg(CLR_PANEL_BORDER)
                .add_modifier(Modifier::BOLD),
        ),
        Rect {
            x: btn_x,
            y: btn_y,
            width: save_w,
            height: 1,
        },
    );
    safe_render_widget(
        f,
        Paragraph::new(" [ Discard ] ").style(Style::default().fg(CLR_TEXT).bg(CLR_APP_BG)),
        Rect {
            x: btn_x + save_w + gap,
            y: btn_y,
            width: discard_w,
            height: 1,
        },
    );

    safe_render_widget(
        f,
        Paragraph::new("Enter/Y=Save  ·  N=Discard  ·  Esc=Cancel")
            .alignment(Alignment::Center)
            .style(Style::default().fg(CLR_MENU_DD_SEP).bg(CLR_APP_BG)),
        Rect {
            x: inner.x,
            y: btn_y + 2,
            width: inner.width,
            height: 1,
        },
    );
}

// ---------------------------------------------------------------------------
// Input dialog
// ---------------------------------------------------------------------------

pub(super) fn render_input(f: &mut Frame, dlg: &InputDialog, area: Rect) {
    if let Some(spec) = crate::lua_dialog::input_render_spec(dlg) {
        render_input_box(f, &spec, dlg, area);
    }
}

fn render_input_box(
    f: &mut Frame,
    spec: &crate::lua_dialog::InputDialogSpec,
    dlg: &InputDialog,
    area: Rect,
) {
    let popup = clamp_rect(area, crate::lua_dialog::input_dialog_popup_rect(spec, area));

    if spec.shadow_dx > 0 || spec.shadow_dy > 0 {
        let sh = Rect {
            x: popup.x + spec.shadow_dx,
            y: popup.y + spec.shadow_dy,
            width: popup.width,
            height: popup.height,
        };
        if sh.x + sh.width <= area.x + area.width && sh.y + sh.height <= area.y + area.height {
            safe_render_widget(
                f,
                Block::default().style(Style::default().bg(Color::Rgb(20, 15, 10))),
                sh,
            );
        }
    }
    safe_render_widget(f, Clear, popup);

    let palette = DialogButtonPalette::from_lua(spec.palette);
    let block_style = confirm_box_style(palette);
    let block = Block::default()
        .title(spec.title.as_str())
        .borders(Borders::ALL)
        .border_style(block_style.border)
        .title_style(block_style.title)
        .style(block_style.body);
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    // Row 0: blank (visual gap after title border)
    // Row 1: prompt label
    safe_render_widget(
        f,
        Paragraph::new(format!(" {} ", spec.prompt)).style(block_style.message),
        Rect {
            x: inner.x,
            y: inner.y + 1,
            width: inner.width,
            height: 1,
        },
    );

    // Row 2: input field (backed by ratatui-textarea state)
    let input_w = inner.width.saturating_sub(2) as usize;
    let text = dlg.textarea.lines().first().map(|s| s.as_str()).unwrap_or("");
    let cursor_col = dlg.textarea.cursor().1; // char-column
    let hscroll = cursor_col.saturating_sub(input_w.saturating_sub(1));
    let shown: String = text.chars().skip(hscroll).take(input_w).collect();
    let value_display = format!("{:<width$}", shown, width = input_w);
    let input_bg = Color::Rgb(214, 196, 167);
    let input_fg = Color::Rgb(30, 20, 10);
    safe_render_widget(
        f,
        Paragraph::new(format!(" {} ", value_display))
            .style(Style::default().fg(input_fg).bg(input_bg)),
        Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: 1,
        },
    );
    // Show cursor only when input field has focus
    if dlg.focused_button.is_none() {
        let cursor_x = inner.x
            + 1
            + cursor_col
                .saturating_sub(hscroll)
                .min(input_w.saturating_sub(1)) as u16;
        safe_set_cursor_position(f, cursor_x, inner.y + 2);
    }

    // Row 3: buttons — active button determined by focused_button
    let rects = crate::lua_dialog::input_dialog_button_rects(spec, area);
    for (idx, button) in spec.buttons.iter().enumerate() {
        let Some(rect) = rects.get(idx) else {
            continue;
        };
        let is_active = dlg.focused_button.map_or(false, |fb| fb == idx);
        render_dialog_button(f, *rect, button.label.as_str(), is_active, palette);
    }
}

pub(super) fn render_assoc_input(f: &mut Frame, dlg: &AssocInputDialog, area: Rect) {
    let is_multiline = matches!(dlg.action, AssocInputAction::Openers { .. });
    let cursor = dlg.textarea.cursor();
    let (cursor_row, cursor_col) = (cursor.0, cursor.1);
    render_text_input_dialog(
        f,
        &dlg.title,
        &dlg.prompt,
        dlg.textarea.lines(),
        (cursor_row, cursor_col),
        area,
        is_multiline,
        TextDialogStyle {
            border_fg: CLR_QS_BORDER,
            dialog_bg: CLR_QS_BG,
            prompt_fg: CLR_QS_LIST_FG,
            input_bg: CLR_QS_INPUT_BG,
            input_fg: CLR_QS_INPUT_FG,
            hint_fg: CLR_QS_NO_MATCH,
        },
        if is_multiline {
            TextDialogFooter::AssocMultiline
        } else {
            TextDialogFooter::Plain(std::borrow::Cow::Borrowed(" Enter:OK  Esc:Cancel"))
        },
    );
}

fn render_text_input_dialog(
    f: &mut Frame,
    title: &str,
    prompt: &str,
    lines: &[String],
    cursor: (usize, usize),
    area: Rect,
    is_multiline: bool,
    style: TextDialogStyle,
    footer: TextDialogFooter<'_>,
) {
    let popup = text_input_popup_rect(area, is_multiline);
    safe_render_widget(f, Clear, popup);

    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(style.border_fg).bg(style.dialog_bg))
        .style(Style::default().bg(style.dialog_bg));
    let inner = block.inner(popup);
    safe_render_widget(f, block, popup);

    render_text_input_prompt(f, inner, prompt, &style);

    let first_line = lines.first().map(|s| s.as_str()).unwrap_or("");
    if is_multiline {
        render_multiline_text_input(f, inner, lines, cursor.0, cursor.1, &style);
    } else {
        render_singleline_text_input(f, inner, first_line, cursor.1, &style);
    }

    render_text_input_footer(f, inner, footer, &style);
}

fn text_input_popup_rect(area: Rect, is_multiline: bool) -> Rect {
    let width = if is_multiline {
        84u16.min(area.width.saturating_sub(4)).max(56)
    } else {
        60u16.min(area.width.saturating_sub(4)).max(42)
    };
    let height = if is_multiline {
        12u16.min(area.height.saturating_sub(4)).max(8)
    } else {
        7u16
    };
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    clamp_rect(
        area,
        Rect {
            x,
            y,
            width,
            height,
        },
    )
}

fn render_text_input_prompt(f: &mut Frame, inner: Rect, prompt: &str, style: &TextDialogStyle) {
    safe_render_widget(
        f,
        Paragraph::new(Line::from(Span::styled(
            format!(" {} ", prompt),
            Style::default().fg(style.prompt_fg).bg(style.dialog_bg),
        ))),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );
}

fn render_singleline_text_input(
    f: &mut Frame,
    inner: Rect,
    value: &str,
    cursor_col: usize,
    style: &TextDialogStyle,
) {
    let input_w = inner.width.saturating_sub(2) as usize;
    let hscroll = cursor_col.saturating_sub(input_w.saturating_sub(1));
    let shown = slice_chars(value, hscroll, input_w);
    let value_display = format!("{:<width$}", shown, width = input_w);

    safe_render_widget(
        f,
        Paragraph::new(Line::from(Span::styled(
            format!(" {} ", value_display),
            Style::default().fg(style.input_fg).bg(style.input_bg),
        ))),
        Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: 1,
        },
    );

    let cursor_x = inner.x
        + 1
        + cursor_col
            .saturating_sub(hscroll)
            .min(input_w.saturating_sub(1)) as u16;
    let cursor_y = inner.y + 2;
    if cursor_y < inner.y + inner.height {
        safe_set_cursor_position(f, cursor_x, cursor_y);
    }
}

fn render_multiline_text_input(
    f: &mut Frame,
    inner: Rect,
    lines: &[String],
    cursor_line: usize,
    cursor_col: usize,
    style: &TextDialogStyle,
) {
    let line_w = inner.width.saturating_sub(2) as usize;
    let input_top = inner.y + 1;
    let input_h = inner.height.saturating_sub(3) as usize;

    let vscroll = if cursor_line < input_h {
        0
    } else {
        cursor_line.saturating_sub(input_h - 1)
    };
    let active_hscroll = cursor_col.saturating_sub(line_w.saturating_sub(1));

    for draw_row in 0..input_h {
        let line_idx = vscroll + draw_row;
        let line: &str = lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");
        let hscroll = if line_idx == cursor_line {
            active_hscroll
        } else {
            0
        };
        let shown = slice_chars(line, hscroll, line_w);
        let padded = format!("{:<width$}", shown, width = line_w);
        safe_render_widget(
            f,
            Paragraph::new(padded).style(Style::default().fg(style.input_fg).bg(style.input_bg)),
            Rect {
                x: inner.x + 1,
                y: input_top + draw_row as u16,
                width: inner.width.saturating_sub(2),
                height: 1,
            },
        );
    }

    let cursor_y = input_top + cursor_line.saturating_sub(vscroll) as u16;
    let cursor_x = inner.x
        + 1
        + cursor_col
            .saturating_sub(active_hscroll)
            .min(line_w.saturating_sub(1)) as u16;
    if cursor_y < inner.y + inner.height.saturating_sub(1) {
        safe_set_cursor_position(f, cursor_x, cursor_y);
    }
}

fn render_text_input_footer(
    f: &mut Frame,
    inner: Rect,
    footer: TextDialogFooter<'_>,
    style: &TextDialogStyle,
) {
    let hint_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };

    match footer {
        TextDialogFooter::Plain(hint) => {
            safe_render_widget(
                f,
                Paragraph::new(Line::from(Span::styled(
                    hint.as_ref(),
                    Style::default().fg(style.hint_fg).bg(style.dialog_bg),
                ))),
                hint_area,
            );
        }
        TextDialogFooter::AssocMultiline => {
            let hint_items = footer_shortcut_items(&assoc_input_shortcuts());
            render_shortcut_bar(f, hint_area, &hint_items, secondary_shortcut_bar_style());
        }
    }
}

fn slice_chars(value: &str, start: usize, width: usize) -> String {
    value.chars().skip(start).take(width).collect()
}
