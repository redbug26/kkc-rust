use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEventKind};
use mlua::{Lua, Table, Value};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::io;
use unicode_width::UnicodeWidthStr;

const CLR_DIALOG_BG: Color = Color::Rgb(125, 107, 92);
const CLR_DIALOG_FG: Color = Color::Rgb(244, 235, 208);
const CLR_DIALOG_BORDER: Color = Color::Rgb(239, 225, 196);
const CLR_DIALOG_TITLE: Color = Color::Rgb(255, 244, 114);
const CLR_DIALOG_SELECTED_BG: Color = Color::Rgb(214, 196, 167);
const CLR_DIALOG_SELECTED_FG: Color = Color::Black;
const CLR_DIALOG_HINT: Color = Color::Rgb(172, 160, 142);

// Match the native command palette visual language.
const CLR_PAL_BG: Color = Color::Rgb(30, 30, 30);
const CLR_PAL_BORDER: Color = Color::Rgb(80, 80, 80);
const CLR_PAL_INPUT_BG: Color = Color::Rgb(58, 58, 58);
const CLR_PAL_INPUT_FG: Color = Color::White;
const CLR_PAL_SEP: Color = Color::Rgb(70, 70, 70);
const CLR_PAL_LIST_FG: Color = Color::Rgb(200, 200, 200);
const CLR_PAL_SEL_BG: Color = Color::Rgb(40, 79, 135);
const CLR_PAL_SEL_FG: Color = Color::White;
const CLR_PAL_HINT: Color = Color::Rgb(130, 130, 130);
const CLR_PAL_TITLE: Color = Color::Rgb(190, 190, 190);
const CLR_PAL_FOOTER_BG: Color = Color::Rgb(52, 52, 52);
const CLR_PAL_FOOTER_FG: Color = Color::Rgb(230, 230, 230);

pub fn install_lua_dialog_module(lua: &Lua, preload: &Table) -> Result<()> {
    let dialog_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;

        t.set(
            "message",
            lua.create_function(move |_, text: String| {
                run_in_tui(|terminal| {
                    let hint = "Enter:OK  Esc:Cancel";
                    loop {
                        terminal.draw(|f| {
                            let content_w = max_line_width(&text)
                                .max(display_width(hint))
                                .max(display_width("Lua Message"));
                            let content_h = line_count(&text).max(1).saturating_add(1);
                            let area = popup_rect(
                                f.area(),
                                content_w,
                                content_h,
                                38,
                                96,
                                7,
                                28,
                            );
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([Constraint::Min(1), Constraint::Length(1)])
                                .split(inner_rect(area));

                            f.render_widget(Clear, area);
                            f.render_widget(
                                Block::default()
                                    .title("Lua Message")
                                    .title_style(
                                        Style::default()
                                            .fg(CLR_DIALOG_TITLE)
                                            .add_modifier(Modifier::BOLD),
                                    )
                                    .style(Style::default().bg(CLR_DIALOG_BG))
                                    .border_style(Style::default().fg(CLR_DIALOG_BORDER))
                                    .borders(Borders::ALL),
                                area,
                            );
                            f.render_widget(
                                Paragraph::new(text.as_str())
                                    .style(Style::default().fg(CLR_DIALOG_FG).bg(CLR_DIALOG_BG)),
                                chunks[0],
                            );
                            f.render_widget(
                                Paragraph::new(hint)
                                    .alignment(ratatui::layout::Alignment::Center)
                                    .style(Style::default().fg(CLR_DIALOG_HINT).bg(CLR_DIALOG_BG)),
                                chunks[1],
                            );
                        })?;

                        if let Event::Key(key) = event::read()?
                            && key.kind != KeyEventKind::Release
                            && matches!(key.code, KeyCode::Enter | KeyCode::Esc)
                        {
                            break;
                        }
                    }
                    Ok(())
                })
                .map_err(mlua::Error::external)
            })?,
        )?;

        t.set(
            "input",
            lua.create_function(move |_, (prompt, default): (String, Option<String>)| {
                run_in_tui(|terminal| {
                    let mut value = default.clone().unwrap_or_default();
                    let hint = "Type text  Backspace/Delete  Enter:OK  Esc:Cancel";
                    loop {
                        terminal.draw(|f| {
                            let value_line = if value.is_empty() {
                                "Value: "
                            } else {
                                "Value:"
                            };
                            let content_w = max_line_width(&prompt)
                                .max(display_width(hint))
                                .max(display_width(value_line).saturating_add(max_line_width(&value)))
                                .max(display_width("Lua Input"));
                            let content_h = line_count(&prompt)
                                .max(1)
                                .saturating_add(2);
                            let area = popup_rect(
                                f.area(),
                                content_w,
                                content_h,
                                42,
                                104,
                                8,
                                30,
                            );
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([
                                    Constraint::Min(1),
                                    Constraint::Length(1),
                                    Constraint::Length(1),
                                ])
                                .split(inner_rect(area));

                            f.render_widget(Clear, area);
                            f.render_widget(
                                Block::default()
                                    .title("Lua Input")
                                    .title_style(
                                        Style::default()
                                            .fg(CLR_DIALOG_TITLE)
                                            .add_modifier(Modifier::BOLD),
                                    )
                                    .style(Style::default().bg(CLR_DIALOG_BG))
                                    .border_style(Style::default().fg(CLR_DIALOG_BORDER))
                                    .borders(Borders::ALL),
                                area,
                            );
                            f.render_widget(
                                Paragraph::new(prompt.as_str())
                                    .style(Style::default().fg(CLR_DIALOG_FG).bg(CLR_DIALOG_BG)),
                                chunks[0],
                            );
                            f.render_widget(
                                Paragraph::new(format!("Value: {}", value))
                                    .style(
                                        Style::default()
                                            .fg(CLR_DIALOG_SELECTED_FG)
                                            .bg(CLR_DIALOG_SELECTED_BG),
                                    ),
                                chunks[1],
                            );
                            f.render_widget(
                                Paragraph::new(hint)
                                    .alignment(ratatui::layout::Alignment::Center)
                                    .style(Style::default().fg(CLR_DIALOG_HINT).bg(CLR_DIALOG_BG)),
                                chunks[2],
                            );
                        })?;

                        if let Event::Key(key) = event::read()?
                            && key.kind != KeyEventKind::Release
                        {
                            match key.code {
                                KeyCode::Enter => break,
                                KeyCode::Esc => return Ok(default.unwrap_or_default()),
                                KeyCode::Backspace => {
                                    value.pop();
                                }
                                KeyCode::Delete => {
                                    value.clear();
                                }
                                KeyCode::Char(ch) => {
                                    value.push(ch);
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(if value.is_empty() {
                        default.unwrap_or_default()
                    } else {
                        value
                    })
                })
                .map_err(mlua::Error::external)
            })?,
        )?;

        t.set(
            "confirm",
            lua.create_function(move |_, (prompt, default_yes): (String, Option<bool>)| {
                let default_yes = default_yes.unwrap_or(true);
                let hint = if default_yes {
                    "[Y]es / [N]o  Enter:default (yes)  Esc:default"
                } else {
                    "[Y]es / [N]o  Enter:default (no)  Esc:default"
                };
                run_in_tui(|terminal| {
                    loop {
                        terminal.draw(|f| {
                            let content_w = max_line_width(&prompt)
                                .max(display_width(hint))
                                .max(display_width("Lua Confirm"));
                            let area = popup_rect(f.area(), content_w, 3, 38, 96, 7, 20);
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([Constraint::Min(1), Constraint::Length(1)])
                                .split(inner_rect(area));

                            f.render_widget(Clear, area);
                            f.render_widget(
                                Block::default()
                                    .title("Lua Confirm")
                                    .title_style(
                                        Style::default()
                                            .fg(CLR_DIALOG_TITLE)
                                            .add_modifier(Modifier::BOLD),
                                    )
                                    .style(Style::default().bg(CLR_DIALOG_BG))
                                    .border_style(Style::default().fg(CLR_DIALOG_BORDER))
                                    .borders(Borders::ALL),
                                area,
                            );
                            f.render_widget(
                                Paragraph::new(prompt.as_str())
                                    .style(Style::default().fg(CLR_DIALOG_FG).bg(CLR_DIALOG_BG)),
                                chunks[0],
                            );
                            f.render_widget(
                                Paragraph::new(hint)
                                    .alignment(ratatui::layout::Alignment::Center)
                                    .style(Style::default().fg(CLR_DIALOG_HINT).bg(CLR_DIALOG_BG)),
                                chunks[1],
                            );
                        })?;

                        if let Event::Key(key) = event::read()?
                            && key.kind != KeyEventKind::Release
                        {
                            match key.code {
                                KeyCode::Enter | KeyCode::Esc => return Ok(default_yes),
                                KeyCode::Char('y') | KeyCode::Char('Y') => return Ok(true),
                                KeyCode::Char('n') | KeyCode::Char('N') => return Ok(false),
                                _ => {}
                            }
                        }
                    }
                })
                .map_err(mlua::Error::external)
            })?,
        )?;

        t.set(
            "select",
            lua.create_function(
                move |_, (prompt, options, default_idx): (String, Table, Option<usize>)| {
                    let mut choices = Vec::new();
                    for value in options.sequence_values::<String>() {
                        choices.push(value?);
                    }
                    if choices.is_empty() {
                        return Ok(None::<usize>);
                    }

                    let default_zero_based = default_idx.unwrap_or(1).clamp(1, choices.len()) - 1;
                    let (selected, _) = run_palette_dialog(
                        prompt,
                        choices,
                        default_zero_based,
                        Vec::new(),
                        PaletteTheme::CommandPalette,
                    )
                    .map_err(mlua::Error::external)?;
                    Ok(selected.map(|idx| idx + 1))
                },
            )?,
        )?;

        t.set(
            "select_with_checks",
            lua.create_function(
                move |lua,
                      (prompt, options, default_idx, checkboxes, theme_name): (
                    String,
                    Table,
                    Option<usize>,
                    Option<Table>,
                    Option<String>,
                )| {
                    let mut choices = Vec::new();
                    for value in options.sequence_values::<String>() {
                        choices.push(value?);
                    }
                    if choices.is_empty() {
                        let out = lua.create_table()?;
                        out.set("checks", lua.create_table()?)?;
                        return Ok(out);
                    }

                    let mut checks = Vec::new();
                    if let Some(checkboxes) = checkboxes {
                        for entry in checkboxes.sequence_values::<Value>() {
                            match entry? {
                                Value::String(s) => {
                                    checks.push(DialogCheckbox {
                                        label: s.to_str()?.to_string(),
                                        checked: false,
                                    });
                                }
                                Value::Table(t) => {
                                    let label = t
                                        .get::<Option<String>>("label")?
                                        .or_else(|| t.get::<Option<String>>(1).ok().flatten())
                                        .unwrap_or_default();
                                    if !label.is_empty() {
                                        checks.push(DialogCheckbox {
                                            label,
                                            checked: t.get::<Option<bool>>("checked")?.unwrap_or(false),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    let default_zero_based = default_idx.unwrap_or(1).clamp(1, choices.len()) - 1;
                    let theme = PaletteTheme::from_name(theme_name.as_deref());
                    let (selected, check_states) = run_palette_dialog(
                        prompt,
                        choices,
                        default_zero_based,
                        checks,
                        theme,
                    )
                    .map_err(mlua::Error::external)?;

                    let out = lua.create_table()?;
                    if let Some(idx) = selected {
                        out.set("index", idx + 1)?;
                    }
                    let lua_checks = lua.create_table()?;
                    for (i, state) in check_states.into_iter().enumerate() {
                        lua_checks.set(i + 1, state)?;
                    }
                    out.set("checks", lua_checks)?;
                    Ok(out)
                },
            )?,
        )?;

        Ok(t)
    })?;

    preload.set("kkc-dialog", dialog_mod)?;
    Ok(())
}

fn run_in_tui<T, F>(f: F) -> Result<T>
where
    F: FnOnce(&mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<T>,
{
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.hide_cursor()?;
    let result = f(&mut terminal);
    let _ = terminal.clear();
    let _ = terminal.draw(|f| {
        let area = f.area();
        f.render_widget(Clear, area);
    });
    let _ = terminal.hide_cursor();
    result
}

fn popup_rect(
    area: Rect,
    content_width: u16,
    content_height: u16,
    min_w: u16,
    max_w: u16,
    min_h: u16,
    max_h: u16,
) -> Rect {
    let avail_w = area.width.saturating_sub(2).max(20);
    let avail_h = area.height.saturating_sub(2).max(6);
    let w = content_width
        .saturating_add(2)
        .clamp(min_w, max_w)
        .min(avail_w)
        .max(20);
    let h = content_height
        .saturating_add(2)
        .clamp(min_h, max_h)
        .min(avail_h)
        .max(6);
    Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    }
}

fn inner_rect(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

fn display_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text).min(u16::MAX as usize) as u16
}

fn max_line_width(text: &str) -> u16 {
    text.lines().map(display_width).max().unwrap_or(0)
}

fn line_count(text: &str) -> u16 {
    text.lines().count().max(1).min(u16::MAX as usize) as u16
}

#[derive(Clone)]
struct DialogCheckbox {
    label: String,
    checked: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteFocus {
    List,
    Checkboxes,
    Buttons,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteTheme {
    CommandPalette,
    RemoteConnections,
}

impl PaletteTheme {
    fn from_name(name: Option<&str>) -> Self {
        match name
            .map(|s| s.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("remote") | Some("remote_connections") | Some("ctrlf") => {
                Self::RemoteConnections
            }
            _ => Self::CommandPalette,
        }
    }
}

#[derive(Clone, Copy)]
struct PaletteColors {
    bg: Color,
    border: Color,
    input_bg: Color,
    input_fg: Color,
    sep: Color,
    list_fg: Color,
    sel_bg: Color,
    sel_fg: Color,
    hint: Color,
    title: Color,
    footer_bg: Color,
    footer_fg: Color,
    footer_shadow: Color,
}

fn palette_colors(theme: PaletteTheme) -> PaletteColors {
    match theme {
        PaletteTheme::CommandPalette => PaletteColors {
            bg: CLR_PAL_BG,
            border: CLR_PAL_BORDER,
            input_bg: CLR_PAL_INPUT_BG,
            input_fg: CLR_PAL_INPUT_FG,
            sep: CLR_PAL_SEP,
            list_fg: CLR_PAL_LIST_FG,
            sel_bg: CLR_PAL_SEL_BG,
            sel_fg: CLR_PAL_SEL_FG,
            hint: CLR_PAL_HINT,
            title: CLR_PAL_TITLE,
            footer_bg: CLR_PAL_FOOTER_BG,
            footer_fg: CLR_PAL_FOOTER_FG,
            footer_shadow: Color::Rgb(34, 34, 34),
        },
        PaletteTheme::RemoteConnections => PaletteColors {
            bg: CLR_DIALOG_BG,
            border: CLR_DIALOG_BORDER,
            input_bg: CLR_DIALOG_SELECTED_BG,
            input_fg: CLR_DIALOG_SELECTED_FG,
            sep: CLR_DIALOG_HINT,
            list_fg: CLR_DIALOG_FG,
            sel_bg: CLR_DIALOG_SELECTED_BG,
            sel_fg: CLR_DIALOG_SELECTED_FG,
            hint: CLR_DIALOG_HINT,
            title: CLR_DIALOG_TITLE,
            footer_bg: CLR_DIALOG_BORDER,
            footer_fg: CLR_DIALOG_SELECTED_FG,
            footer_shadow: Color::Rgb(140, 122, 102),
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogButton {
    Ok,
    Cancel,
}

fn run_palette_dialog(
    prompt: String,
    choices: Vec<String>,
    default_cursor: usize,
    mut checkboxes: Vec<DialogCheckbox>,
    theme: PaletteTheme,
) -> Result<(Option<usize>, Vec<bool>)> {
    run_in_tui(|terminal| {
        let mut filter = String::new();
        let mut cursor = default_cursor.min(choices.len().saturating_sub(1));
        let mut checks_cursor = 0usize;
        let mut focus = PaletteFocus::List;
        let mut active_button = DialogButton::Ok;

        loop {
            let filtered = filtered_indices(&choices, &filter);
            if filtered.is_empty() {
                cursor = 0;
            } else {
                cursor = cursor.min(filtered.len().saturating_sub(1));
            }
            if checks_cursor >= checkboxes.len() {
                checks_cursor = checkboxes.len().saturating_sub(1);
            }

            let colors = palette_colors(theme);
            let title = if prompt.trim().is_empty() {
                "  Select  ".to_string()
            } else {
                format!("  {}  ", truncate_to_width(prompt.trim(), 48))
            };
            let longest_choice = choices
                .iter()
                .enumerate()
                .map(|(idx, item)| display_width(&format!("{:>2}. {}", idx + 1, item)))
                .max()
                .unwrap_or(0);
            let checkbox_w = checkboxes
                .iter()
                .map(|c| display_width(&format!("[ ] {}", c.label)))
                .max()
                .unwrap_or(0);
            let content_w = max_line_width(&prompt)
                .max(longest_choice.saturating_add(2))
                .max(checkbox_w)
                .max(display_width(" \u{2315} ").saturating_add(display_width(&filter)))
                .max(display_width(" ▶  OK  ◀   ▶ Cancel ◀ "))
                .max(display_width("Command Palette"));
            let visible_list = filtered.len().clamp(1, 12) as u16;
            let options_rows = if checkboxes.is_empty() {
                0
            } else {
                checkboxes.len() as u16 + 1
            };
            let content_h = visible_list.saturating_add(options_rows).saturating_add(3);
            let term_size = terminal.size()?;
            let term_area = Rect {
                x: 0,
                y: 0,
                width: term_size.width,
                height: term_size.height,
            };
            let area = popup_rect(term_area, content_w, content_h, 52, 112, 9, 40);
            let inner = inner_rect(area);

            let list_area = Rect {
                x: inner.x,
                y: inner.y + 2,
                width: inner.width,
                height: inner.height.saturating_sub(4),
            };
            let ok_label = "▶  OK  ◀";
            let cancel_label = "▶ Cancel ◀";
            let ok_w = display_width(ok_label);
            let cancel_w = display_width(cancel_label);
            let buttons_gap = 3u16;
            let buttons_group_w = ok_w
                .saturating_add(1)
                .saturating_add(buttons_gap)
                .saturating_add(cancel_w)
                .saturating_add(1);
            let buttons_group_x = inner.x + inner.width.saturating_sub(buttons_group_w) / 2;
            let ok_x = buttons_group_x;
            let cancel_x = ok_x
                .saturating_add(ok_w)
                .saturating_add(1)
                .saturating_add(buttons_gap);
            let footer_y = inner.y + inner.height.saturating_sub(2);
            let footer_shadow_y = inner.y + inner.height.saturating_sub(1);
            let mut rows = Vec::new();
            if filtered.is_empty() {
                rows.push(Row::NoMatch);
            } else {
                for idx in &filtered {
                    rows.push(Row::Choice(*idx));
                }
            }
            if !checkboxes.is_empty() {
                rows.push(Row::Separator);
                for idx in 0..checkboxes.len() {
                    rows.push(Row::Checkbox(idx));
                }
            }
            let selected_row = if focus == PaletteFocus::Checkboxes && !checkboxes.is_empty() {
                let base = if filtered.is_empty() { 1 } else { filtered.len() + 1 };
                base + checks_cursor
            } else if filtered.is_empty() {
                0
            } else {
                cursor
            };
            let list_h = list_area.height as usize;
            let start = if selected_row >= list_h {
                selected_row - list_h + 1
            } else {
                0
            };

            if focus == PaletteFocus::Buttons {
                terminal.show_cursor()?;
            } else {
                terminal.hide_cursor()?;
            }

            terminal.draw(|f| {
                f.render_widget(Clear, area);
                f.render_widget(
                    Block::default()
                        .title(title)
                        .title_style(Style::default().fg(colors.title).add_modifier(Modifier::BOLD))
                        .style(Style::default().bg(colors.bg))
                        .border_style(Style::default().fg(colors.border))
                        .borders(Borders::ALL),
                    area,
                );

                if inner.height < 4 {
                    return;
                }

                let selected_pos = if filtered.is_empty() { 0 } else { cursor + 1 };
                let count_hint = format!(" {}/{} ", selected_pos, filtered.len());
                let hint_w = display_width(&count_hint);
                let input_w = inner.width.saturating_sub(hint_w) as usize;
                let input_text = format!(" \u{2315} {}\u{2581}", filter);
                let input_line = Line::from(vec![
                    Span::styled(
                        truncate_to_width(&input_text, input_w),
                        Style::default().fg(colors.input_fg).bg(colors.input_bg),
                    ),
                    Span::styled(
                        count_hint,
                        Style::default().fg(colors.hint).bg(colors.input_bg),
                    ),
                ]);
                f.render_widget(
                    Paragraph::new(input_line).style(Style::default().bg(colors.input_bg)),
                    Rect {
                        x: inner.x,
                        y: inner.y,
                        width: inner.width,
                        height: 1,
                    },
                );

                let sep = "─".repeat(inner.width as usize);
                f.render_widget(
                    Paragraph::new(sep.clone()).style(Style::default().fg(colors.sep).bg(colors.bg)),
                    Rect {
                        x: inner.x,
                        y: inner.y + 1,
                        width: inner.width,
                        height: 1,
                    },
                );

                for (row_idx, row) in rows.iter().skip(start).take(list_h).enumerate() {
                    let y = list_area.y + row_idx as u16;
                    match row {
                        Row::Separator => {
                            f.render_widget(
                                Paragraph::new(sep.clone())
                                    .style(Style::default().fg(colors.sep).bg(colors.bg)),
                                Rect {
                                    x: list_area.x,
                                    y,
                                    width: list_area.width,
                                    height: 1,
                                },
                            );
                        }
                        Row::NoMatch => {
                            f.render_widget(
                                Paragraph::new(Line::styled(
                                    "No match",
                                    Style::default().fg(colors.hint).bg(colors.bg),
                                )),
                                Rect {
                                    x: list_area.x,
                                    y,
                                    width: list_area.width,
                                    height: 1,
                                },
                            );
                        }
                        Row::Choice(choice_idx) => {
                            let is_sel = !filtered.is_empty() && filtered[cursor] == *choice_idx;
                            let list_is_active = focus == PaletteFocus::List;
                            let (bg, fg, marker) = if is_sel && list_is_active {
                                // Active selection: highlighted with marker
                                (colors.sel_bg, colors.sel_fg, "> ")
                            } else if is_sel && !list_is_active {
                                // Inactive selection: still visible but dimmed, no marker
                                (colors.bg, colors.hint, "  ")
                            } else {
                                // Not selected
                                (colors.bg, colors.list_fg, "  ")
                            };
                            let row_text =
                                format!("{}{:>2}. {}", marker, choice_idx + 1, choices[*choice_idx]);
                            f.render_widget(
                                Paragraph::new(Line::styled(
                                    truncate_to_width(&row_text, list_area.width as usize),
                                    Style::default().fg(fg).bg(bg),
                                )),
                                Rect {
                                    x: list_area.x,
                                    y,
                                    width: list_area.width,
                                    height: 1,
                                },
                            );
                        }
                        Row::Checkbox(check_idx) => {
                            let item = &checkboxes[*check_idx];
                            let is_sel =
                                focus == PaletteFocus::Checkboxes && checks_cursor == *check_idx;
                            let (bg, fg, marker) = if is_sel {
                                (colors.sel_bg, colors.sel_fg, "> ")
                            } else {
                                (colors.bg, colors.list_fg, "  ")
                            };
                            let mark = if item.checked { "x" } else { " " };
                            let row_text = format!("{}[{}] {}", marker, mark, item.label);
                            f.render_widget(
                                Paragraph::new(Line::styled(
                                    truncate_to_width(&row_text, list_area.width as usize),
                                    Style::default().fg(fg).bg(bg),
                                )),
                                Rect {
                                    x: list_area.x,
                                    y,
                                    width: list_area.width,
                                    height: 1,
                                },
                            );
                        }
                    }
                }

                let ok_selected = active_button == DialogButton::Ok;
                let cancel_selected = active_button == DialogButton::Cancel;
                let ok_style = if ok_selected {
                    Style::default()
                        .fg(colors.sel_fg)
                        .bg(colors.sel_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(colors.footer_fg).bg(colors.footer_bg)
                };
                let cancel_style = if cancel_selected {
                    Style::default()
                        .fg(colors.sel_fg)
                        .bg(colors.sel_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(colors.footer_fg).bg(colors.footer_bg)
                };
                let shadow_side_style =
                    Style::default().fg(colors.footer_shadow).bg(colors.bg);
                let base_bg_style = Style::default().bg(colors.bg);
                let ok_rest = "  OK  ◀";
                let cancel_rest = " Cancel ◀";

                let buttons_line = Line::from(vec![
                    Span::styled(
                        " ".repeat(ok_x.saturating_sub(inner.x) as usize),
                        base_bg_style,
                    ),
                    Span::styled("▶", ok_style),
                    Span::styled(ok_rest, ok_style),
                    Span::styled("▖", shadow_side_style),
                    Span::styled(" ".repeat(buttons_gap as usize), base_bg_style),
                    Span::styled("▶", cancel_style),
                    Span::styled(cancel_rest, cancel_style),
                    Span::styled("▖", shadow_side_style),
                ]);
                f.render_widget(
                    Paragraph::new(buttons_line).style(Style::default().bg(colors.bg)),
                    Rect {
                        x: inner.x,
                        y: footer_y,
                        width: inner.width,
                        height: 1,
                    },
                );

                let shadow_line = Line::from(vec![
                    Span::styled(
                        " ".repeat((ok_x.saturating_sub(inner.x) + 1) as usize),
                        Style::default().bg(colors.bg),
                    ),
                    Span::styled(
                        "▀".repeat((ok_w-1) as usize),
                        Style::default().fg(colors.footer_shadow).bg(colors.bg),
                    ),
                    Span::styled("▘", Style::default().fg(colors.footer_shadow).bg(colors.bg)),
                    Span::styled(
                        " ".repeat((buttons_gap + 1) as usize),
                        Style::default().bg(colors.bg),
                    ),
                    Span::styled(
                        "▀".repeat((cancel_w - 1) as usize),
                        Style::default().fg(colors.footer_shadow).bg(colors.bg),
                    ),
                    Span::styled("▘", Style::default().fg(colors.footer_shadow).bg(colors.bg)),
                ]);
                f.render_widget(
                    Paragraph::new(shadow_line).style(Style::default().bg(colors.bg)),
                    Rect {
                        x: inner.x,
                        y: footer_shadow_y,
                        width: inner.width,
                        height: 1,
                    },
                );

                if focus == PaletteFocus::Buttons {
                    let cursor_x = if ok_selected { ok_x } else { cancel_x };
                    f.set_cursor_position((cursor_x, footer_y));
                }
            })?;

            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Esc => {
                        let states = checkboxes.iter().map(|c| c.checked).collect();
                        return Ok((None, states));
                    }
                    KeyCode::Enter => {
                        let states = checkboxes.iter().map(|c| c.checked).collect();
                        if active_button == DialogButton::Cancel {
                            return Ok((None, states));
                        }
                        let filtered = filtered_indices(&choices, &filter);
                        if let Some(selected) = filtered.get(cursor).copied() {
                            return Ok((Some(selected), states));
                        }
                        return Ok((None, states));
                    }
                    KeyCode::Tab => {
                        focus = match focus {
                            PaletteFocus::List => {
                                if checkboxes.is_empty() {
                                    PaletteFocus::Buttons
                                } else {
                                    PaletteFocus::Checkboxes
                                }
                            }
                            PaletteFocus::Checkboxes => PaletteFocus::Buttons,
                            PaletteFocus::Buttons => PaletteFocus::List,
                        };
                    }
                    KeyCode::Left => {
                        if focus == PaletteFocus::Buttons {
                            active_button = DialogButton::Ok;
                        }
                    }
                    KeyCode::Right => {
                        if focus == PaletteFocus::Buttons {
                            active_button = DialogButton::Cancel;
                        }
                    }
                    KeyCode::Up => {
                        if focus == PaletteFocus::Checkboxes && !checkboxes.is_empty() {
                            checks_cursor = checks_cursor.saturating_sub(1);
                        } else if focus == PaletteFocus::List {
                            cursor = cursor.saturating_sub(1);
                        } else {
                            focus = if checkboxes.is_empty() {
                                PaletteFocus::List
                            } else {
                                PaletteFocus::Checkboxes
                            };
                        }
                    }
                    KeyCode::Down => {
                        if focus == PaletteFocus::Checkboxes && !checkboxes.is_empty() {
                            checks_cursor =
                                (checks_cursor + 1).min(checkboxes.len().saturating_sub(1));
                        } else if focus == PaletteFocus::List {
                            let filtered = filtered_indices(&choices, &filter);
                            if !filtered.is_empty() {
                                cursor = (cursor + 1).min(filtered.len().saturating_sub(1));
                            }
                        } else {
                            focus = PaletteFocus::List;
                        }
                    }
                    KeyCode::Backspace => {
                        if focus == PaletteFocus::List {
                            filter.pop();
                            cursor = 0;
                        }
                    }
                    KeyCode::Char(' ') => {
                        if focus == PaletteFocus::Checkboxes
                            && let Some(item) = checkboxes.get_mut(checks_cursor)
                        {
                            item.checked = !item.checked;
                        } else if focus == PaletteFocus::Buttons {
                            active_button = if active_button == DialogButton::Ok {
                                DialogButton::Cancel
                            } else {
                                DialogButton::Ok
                            };
                        } else {
                            filter.push(' ');
                            cursor = 0;
                        }
                    }
                    KeyCode::Char(ch) => {
                        if focus == PaletteFocus::List {
                            filter.push(ch);
                            cursor = 0;
                            active_button = DialogButton::Ok;
                        }
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        let button_y = footer_y;
                        if mouse.row == button_y {
                            if mouse.column >= ok_x && mouse.column < ok_x + ok_w {
                                let states = checkboxes.iter().map(|c| c.checked).collect();
                                let filtered = filtered_indices(&choices, &filter);
                                if let Some(selected) = filtered.get(cursor).copied() {
                                    return Ok((Some(selected), states));
                                }
                                return Ok((None, states));
                            }
                            if mouse.column >= cancel_x && mouse.column < cancel_x + cancel_w {
                                let states = checkboxes.iter().map(|c| c.checked).collect();
                                return Ok((None, states));
                            }
                        }

                        if mouse.row >= list_area.y
                            && mouse.row < list_area.y + list_area.height
                            && mouse.column >= list_area.x
                            && mouse.column < list_area.x + list_area.width
                        {
                            let row_idx = start + (mouse.row - list_area.y) as usize;
                            if let Some(row) = rows.get(row_idx).copied() {
                                match row {
                                    Row::Choice(choice_idx) => {
                                        if let Some(pos) =
                                            filtered.iter().position(|idx| *idx == choice_idx)
                                        {
                                            cursor = pos;
                                            focus = PaletteFocus::List;
                                            active_button = DialogButton::Ok;
                                        }
                                    }
                                    Row::Checkbox(check_idx) => {
                                        checks_cursor = check_idx;
                                        focus = PaletteFocus::Checkboxes;
                                        if let Some(item) = checkboxes.get_mut(check_idx) {
                                            item.checked = !item.checked;
                                        }
                                    }
                                    Row::Separator | Row::NoMatch => {}
                                }
                            }
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        if focus == PaletteFocus::Checkboxes && !checkboxes.is_empty() {
                            checks_cursor = checks_cursor.saturating_sub(1);
                        } else {
                            cursor = cursor.saturating_sub(1);
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        if focus == PaletteFocus::Checkboxes && !checkboxes.is_empty() {
                            checks_cursor =
                                (checks_cursor + 1).min(checkboxes.len().saturating_sub(1));
                        } else {
                            let filtered = filtered_indices(&choices, &filter);
                            if !filtered.is_empty() {
                                cursor = (cursor + 1).min(filtered.len().saturating_sub(1));
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    })
}

fn filtered_indices(choices: &[String], filter: &str) -> Vec<usize> {
    let trimmed = filter.trim();
    if trimmed.is_empty() {
        return (0..choices.len()).collect();
    }

    let tokens = trimmed
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return (0..choices.len()).collect();
    }

    let first = &tokens[0];
    let rest = &tokens[1..];
    let mut starts = Vec::new();
    let mut contains = Vec::new();

    for (idx, item) in choices.iter().enumerate() {
        let lowered = item.to_lowercase();
        if !rest.iter().all(|token| lowered.contains(token.as_str())) {
            continue;
        }
        if lowered.starts_with(first.as_str()) {
            starts.push(idx);
        } else if lowered.contains(first.as_str()) {
            contains.push(idx);
        }
    }

    starts.extend(contains);
    starts
}

#[derive(Clone, Copy)]
enum Row {
    Choice(usize),
    Separator,
    Checkbox(usize),
    NoMatch,
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if width + cw > max_width.saturating_sub(1) {
            break;
        }
        out.push(ch);
        width += cw;
    }
    out.push('…');
    out
}
