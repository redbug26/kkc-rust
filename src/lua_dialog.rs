use crate::app::{ConfirmAction, ConfirmDialog};
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
use std::borrow::Cow;
use std::io;
use unicode_width::UnicodeWidthStr;

use crate::theme::theme;

#[inline]
fn clr_dialog_bg() -> Color {
    theme().dialog.background
}
#[inline]
fn clr_dialog_fg() -> Color {
    theme().dialog.foreground
}
#[inline]
fn clr_dialog_border() -> Color {
    theme().dialog.border
}
#[inline]
fn clr_dialog_title() -> Color {
    theme().dialog.title
}
#[inline]
fn clr_dialog_selected_bg() -> Color {
    theme().dialog.selected_background
}
#[inline]
fn clr_dialog_selected_fg() -> Color {
    theme().dialog.selected_foreground
}
#[inline]
fn clr_dialog_hint() -> Color {
    theme().dialog.hint
}

// Match the native command palette visual language.
#[inline]
fn clr_pal_bg() -> Color {
    theme().palette.background
}
#[inline]
fn clr_pal_border() -> Color {
    theme().palette.border
}
#[inline]
fn clr_pal_input_bg() -> Color {
    theme().palette.input_background
}
#[inline]
fn clr_pal_input_fg() -> Color {
    theme().palette.input_foreground
}
#[inline]
fn clr_pal_sep() -> Color {
    theme().palette.separator
}
#[inline]
fn clr_pal_list_fg() -> Color {
    theme().palette.list_foreground
}
#[inline]
fn clr_pal_sel_bg() -> Color {
    theme().palette.selected_background
}
#[inline]
fn clr_pal_sel_fg() -> Color {
    theme().palette.selected_foreground
}
#[inline]
fn clr_pal_hint() -> Color {
    theme().palette.no_match
}
#[inline]
fn clr_pal_title() -> Color {
    theme().palette.title
}
#[inline]
fn clr_pal_footer_bg() -> Color {
    theme().palette.footer_background
}
#[inline]
fn clr_pal_footer_fg() -> Color {
    theme().palette.footer_foreground
}

const CONFIRM_QUIT_MACRO: &str = include_str!("../assets/macros/confirm_quit.lua");
const CONFIRM_DELETE_MACRO: &str = include_str!("../assets/macros/confirm_delete.lua");
const CONFIRM_TEXT_EDITOR_UNSAVED_MACRO: &str =
    include_str!("../assets/macros/confirm_text_editor_unsaved.lua");
const CONFIRM_SAVE_EDITOR_BEFORE_QUIT_MACRO: &str =
    include_str!("../assets/macros/confirm_save_editor_before_quit.lua");
const CONFIRM_NOTIFY_MACRO: &str = include_str!("../assets/macros/confirm_notify.lua");
const INPUT_MKDIR_MACRO: &str = include_str!("../assets/macros/input_mkdir.lua");
const INPUT_RENAME_MACRO: &str = include_str!("../assets/macros/input_rename.lua");
const INPUT_WILDCARD_MACRO: &str = include_str!("../assets/macros/input_wildcard.lua");
const INPUT_GOTO_PATH_MACRO: &str = include_str!("../assets/macros/input_goto_path.lua");
const INPUT_SAVE_SESSION_MACRO: &str = include_str!("../assets/macros/input_save_session.lua");
const INPUT_PLUGIN_ACTION_MACRO: &str = include_str!("../assets/macros/input_plugin_action.lua");

// ---------------------------------------------------------------------------
// Input dialog spec (Lua-backed)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct InputDialogSpec {
    pub title: String,
    pub prompt: String,
    pub shadow_dx: u16,
    pub shadow_dy: u16,
    pub palette: ConfirmDialogPalette,
    pub button_gap: u16,
    pub buttons: Vec<ConfirmDialogButtonSpec>,
}

impl Default for InputDialogSpec {
    fn default() -> Self {
        Self {
            title: " Input ".into(),
            prompt: "Value:".into(),
            shadow_dx: 0,
            shadow_dy: 0,
            palette: ConfirmDialogPalette::Normal,
            button_gap: 4,
            buttons: vec![
                ConfirmDialogButtonSpec {
                    callback: "confirm".into(),
                    label: "▶   OK   ◀".into(),
                    width: 9,
                },
                ConfirmDialogButtonSpec {
                    callback: "cancel".into(),
                    label: "▶ Cancel ◀".into(),
                    width: 12,
                },
            ],
        }
    }
}

pub fn input_render_spec(dlg: &crate::app::InputDialog) -> Option<InputDialogSpec> {
    let macro_name = dlg.macro_name?;
    let default = InputDialogSpec {
        title: format!(" {} ", dlg.title.as_deref().unwrap_or("Input")),
        prompt: dlg.prompt.clone().unwrap_or_default(),
        ..InputDialogSpec::default()
    };
    load_input_dialog_spec(macro_name, default.clone())
        .ok()
        .or(Some(default))
}

pub fn input_dialog_popup_rect(spec: &InputDialogSpec, area: Rect) -> Rect {
    let buttons_total = spec
        .buttons
        .iter()
        .fold(0u16, |acc, b| acc.saturating_add(b.width));
    let gap_total = spec
        .button_gap
        .saturating_mul(spec.buttons.len().saturating_sub(1) as u16);
    let buttons_group_w = buttons_total.saturating_add(gap_total).saturating_add(1);
    let prompt_w = display_width(&spec.prompt).saturating_add(4);
    let title_w = display_width(&spec.title);
    let inner_w = title_w.max(prompt_w).max(buttons_group_w).max(30);
    let desired_w = inner_w.saturating_add(2).clamp(36, 80);
    // border + blank + prompt + input + blank + button + button_shadow + border = 8
    let height = 8u16;
    let max_w = area.width.saturating_sub(2 + spec.shadow_dx).max(3);
    let max_h = area.height.saturating_sub(2 + spec.shadow_dy).max(6);
    let width = desired_w.min(max_w);
    let height = height.min(max_h);
    let avail_w = area.width.saturating_sub(width + spec.shadow_dx);
    let avail_h = area.height.saturating_sub(height + spec.shadow_dy);
    Rect {
        x: area.x + avail_w / 2,
        y: area.y + avail_h / 2,
        width,
        height,
    }
}

pub fn input_dialog_button_rects(spec: &InputDialogSpec, area: Rect) -> Vec<Rect> {
    let popup = input_dialog_popup_rect(spec, area);
    let inner = inner_rect(popup);
    let buttons_total = spec
        .buttons
        .iter()
        .fold(0u16, |acc, b| acc.saturating_add(b.width));
    let gap_total = spec
        .button_gap
        .saturating_mul(spec.buttons.len().saturating_sub(1) as u16);
    let group_w = buttons_total.saturating_add(gap_total);
    let btn_x = inner.x + inner.width.saturating_sub(group_w) / 2;
    // blank(0) + prompt(1) + input(2) + blank(3) = buttons at row 4
    let btn_y = inner.y + 4;
    let mut x = btn_x;
    spec.buttons
        .iter()
        .map(|button| {
            let rect = Rect {
                x,
                y: btn_y,
                width: button.width,
                height: 1,
            };
            x = x
                .saturating_add(button.width)
                .saturating_add(spec.button_gap);
            rect
        })
        .collect()
}

fn load_input_dialog_spec(name: &str, default: InputDialogSpec) -> Result<InputDialogSpec> {
    let Some(source) = input_macro_source(name) else {
        anyhow::bail!("unknown input macro: {}", name);
    };
    let lua = Lua::new();
    let package: Table = lua.globals().get("package")?;
    let preload: Table = package.get("preload")?;
    install_lua_dialog_module(&lua, &preload)?;
    let ctx_table = lua.create_table()?;
    ctx_table.set("title", default.title.clone())?;
    ctx_table.set("prompt", default.prompt.clone())?;
    lua.globals().set("ctx", ctx_table)?;
    let spec: Table = lua.load(source.as_ref()).set_name(name).eval()?;
    parse_input_dialog_spec(&spec, default)
}

fn input_macro_source(name: &str) -> Option<Cow<'static, str>> {
    if !is_safe_macro_name(name) {
        return None;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("macros")
        .join(format!("{}.lua", name));
    if let Ok(source) = std::fs::read_to_string(path) {
        return Some(Cow::Owned(source));
    }
    match name {
        "input_mkdir" => Some(Cow::Borrowed(INPUT_MKDIR_MACRO)),
        "input_rename" => Some(Cow::Borrowed(INPUT_RENAME_MACRO)),
        "input_wildcard" => Some(Cow::Borrowed(INPUT_WILDCARD_MACRO)),
        "input_goto_path" => Some(Cow::Borrowed(INPUT_GOTO_PATH_MACRO)),
        "input_save_session" => Some(Cow::Borrowed(INPUT_SAVE_SESSION_MACRO)),
        "input_plugin_action" => Some(Cow::Borrowed(INPUT_PLUGIN_ACTION_MACRO)),
        _ => None,
    }
}

fn parse_input_dialog_spec(spec: &Table, default: InputDialogSpec) -> Result<InputDialogSpec> {
    let palette = match spec
        .get::<Option<String>>("palette")?
        .unwrap_or_else(|| "normal".into())
        .as_str()
    {
        "danger" => ConfirmDialogPalette::Danger,
        _ => ConfirmDialogPalette::Normal,
    };
    let buttons_table = spec.get::<Option<Table>>("buttons")?;
    let button_gap = buttons_table
        .as_ref()
        .map(|bt| table_u16(bt, "gap", default.button_gap))
        .unwrap_or(default.button_gap);
    let buttons = buttons_table
        .as_ref()
        .and_then(|bt| bt.get::<Option<Table>>("items").ok().flatten())
        .map(parse_confirm_buttons)
        .transpose()?
        .filter(|b| !b.is_empty())
        .unwrap_or(default.buttons);
    Ok(InputDialogSpec {
        title: table_string(spec, "title", &default.title)?,
        prompt: table_string(spec, "prompt", &default.prompt)?,
        shadow_dx: table_u16(spec, "shadow_dx", default.shadow_dx).clamp(0, 8),
        shadow_dy: table_u16(spec, "shadow_dy", default.shadow_dy).clamp(0, 4),
        palette,
        button_gap,
        buttons,
    })
}

#[derive(Debug, Clone)]
pub struct ConfirmDialogSpec {
    pub width: u16,
    pub height: u16,
    pub shadow_dx: u16,
    pub shadow_dy: u16,
    pub title: String,
    pub palette: ConfirmDialogPalette,
    pub separators: Vec<u16>,
    pub header: Option<ConfirmDialogText>,
    pub message: ConfirmDialogText,
    pub buttons_y: u16,
    pub button_gap: u16,
    pub buttons: Vec<ConfirmDialogButtonSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmDialogPalette {
    Normal,
    Danger,
}

#[derive(Debug, Clone)]
pub struct ConfirmDialogText {
    pub message_text: String,
    pub message_y: u16,
    pub message_height: u16,
    pub message_prefix_blank: bool,
}

#[derive(Debug, Clone)]
pub struct ConfirmDialogButtonSpec {
    pub callback: String,
    pub label: String,
    pub width: u16,
}

impl Default for ConfirmDialogSpec {
    fn default() -> Self {
        Self {
            width: 38,
            height: 0,
            shadow_dx: 0,
            shadow_dy: 0,
            title: " KK Commander ".into(),
            palette: ConfirmDialogPalette::Normal,
            separators: vec![],
            header: None,
            message: ConfirmDialogText {
                message_text: "Do you really want to quit?".into(),
                message_y: 1,
                message_height: 3,
                message_prefix_blank: true,
            },
            buttons_y: 5,
            button_gap: 3,
            buttons: vec![
                ConfirmDialogButtonSpec {
                    callback: "confirm".into(),
                    label: "▶  Yes  ◀".into(),
                    width: 11,
                },
                ConfirmDialogButtonSpec {
                    callback: "cancel".into(),
                    label: "▶   No   ◀".into(),
                    width: 11,
                },
            ],
        }
    }
}

pub fn confirm_render_spec(dlg: &ConfirmDialog) -> Option<ConfirmDialogSpec> {
    let macro_name = dlg.macro_name?;
    let fallback = default_confirm_dialog_spec(macro_name, dlg);
    load_confirm_dialog_spec(
        macro_name,
        ConfirmDialogContext::from_dialog(dlg),
        fallback.clone(),
    )
    .ok()
    .or(Some(fallback))
}

pub fn confirm_button_callback(dlg: &ConfirmDialog, button_idx: usize) -> Option<String> {
    confirm_render_spec(dlg).and_then(|spec| {
        spec.buttons
            .get(button_idx)
            .map(|button| button.callback.clone())
    })
}

fn default_confirm_dialog_spec(_name: &str, dlg: &ConfirmDialog) -> ConfirmDialogSpec {
    let title = match dlg.title.as_deref().unwrap_or("") {
        "" => " Confirm ".to_string(),
        t => format!(" {} ", t),
    };
    ConfirmDialogSpec {
        title,
        message: ConfirmDialogText {
            message_text: dlg.message.clone().unwrap_or_default(),
            message_y: 1,
            message_height: 2,
            message_prefix_blank: false,
        },
        ..ConfirmDialogSpec::default()
    }
}

#[derive(Debug, Default, Clone)]
pub struct ConfirmDialogContext {
    pub message: Option<String>,
    pub count: Option<usize>,
}

impl ConfirmDialogContext {
    fn from_dialog(dlg: &ConfirmDialog) -> Self {
        Self {
            message: dlg.message.clone(),
            count: match &dlg.action {
                ConfirmAction::Delete(paths) => Some(paths.len()),
                ConfirmAction::DeleteRemote(targets) => Some(targets.len()),
                _ => None,
            },
        }
    }
}

pub fn confirm_dialog_button_rects(spec: &ConfirmDialogSpec, area: Rect) -> Vec<Rect> {
    let popup = confirm_dialog_popup_rect(spec, area);
    let inner = inner_rect(popup);
    let buttons_total = spec
        .buttons
        .iter()
        .fold(0u16, |acc, button| acc.saturating_add(button.width));
    let gap_total = spec
        .button_gap
        .saturating_mul(spec.buttons.len().saturating_sub(1) as u16);
    let group_w = buttons_total.saturating_add(gap_total);
    let btn_x = inner.x + inner.width.saturating_sub(group_w) / 2;
    let btn_y = inner.y + spec.buttons_y;
    let mut x = btn_x;
    spec.buttons
        .iter()
        .map(|button| {
            let rect = Rect {
                x,
                y: btn_y,
                width: button.width,
                height: 1,
            };
            x = x
                .saturating_add(button.width)
                .saturating_add(spec.button_gap);
            rect
        })
        .collect()
}

pub fn confirm_dialog_popup_rect(spec: &ConfirmDialogSpec, area: Rect) -> Rect {
    let buttons_total = spec
        .buttons
        .iter()
        .fold(0u16, |acc, button| acc.saturating_add(button.width));
    let gap_total = spec
        .button_gap
        .saturating_mul(spec.buttons.len().saturating_sub(1) as u16);
    let buttons_group_w = buttons_total.saturating_add(gap_total).saturating_add(1) + 1;

    let header_w = spec
        .header
        .as_ref()
        .map(|h| max_line_width(&h.message_text))
        .unwrap_or(0);
    let message_w = max_line_width(&spec.message.message_text);
    let title_w = display_width(&spec.title);

    let inner_w = title_w.max(header_w).max(message_w).max(buttons_group_w);
    let desired_w = inner_w.saturating_add(2).clamp(20, 120);

    let max_w = area.width.saturating_sub(2 + spec.shadow_dx).max(3);
    let max_h = area.height.saturating_sub(2 + spec.shadow_dy).max(6);
    let width = desired_w.min(max_w);
    let height = spec.height.clamp(6, 40).min(max_h);

    // Centre the popup accounting for the shadow offset so that the dialog
    // plus its shadow appear visually centred on screen.
    let avail_w = area.width.saturating_sub(width + spec.shadow_dx);
    let avail_h = area.height.saturating_sub(height + spec.shadow_dy);
    Rect {
        x: area.x + avail_w / 2,
        y: area.y + avail_h / 2,
        width,
        height,
    }
}

pub fn confirm_dialog_button_rect_pair(spec: &ConfirmDialogSpec, area: Rect) -> (Rect, Rect) {
    let rects = confirm_dialog_button_rects(spec, area);
    (
        rects.first().copied().unwrap_or(Rect {
            x: area.x,
            y: area.y,
            width: 0,
            height: 0,
        }),
        rects.get(1).copied().unwrap_or(Rect {
            x: area.x,
            y: area.y,
            width: 0,
            height: 0,
        }),
    )
}

fn load_confirm_dialog_spec(
    name: &str,
    ctx: ConfirmDialogContext,
    default: ConfirmDialogSpec,
) -> Result<ConfirmDialogSpec> {
    let Some(source) = confirm_macro_source(name) else {
        anyhow::bail!("unknown confirm macro: {}", name);
    };
    let lua = Lua::new();
    let package: Table = lua.globals().get("package")?;
    let preload: Table = package.get("preload")?;
    install_lua_dialog_module(&lua, &preload)?;
    let ctx_table = lua.create_table()?;
    if let Some(message) = ctx.message {
        ctx_table.set("message", message)?;
    }
    if let Some(count) = ctx.count {
        ctx_table.set("count", count)?;
    }
    lua.globals().set("ctx", ctx_table)?;
    let spec: Table = lua.load(source.as_ref()).set_name(name).eval()?;
    parse_confirm_dialog_spec(&spec, default)
}

fn confirm_macro_source(name: &str) -> Option<Cow<'static, str>> {
    if !is_safe_macro_name(name) {
        return None;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("macros")
        .join(format!("{}.lua", name));
    if let Ok(source) = std::fs::read_to_string(path) {
        return Some(Cow::Owned(source));
    }
    match name {
        "confirm_quit" => Some(Cow::Borrowed(CONFIRM_QUIT_MACRO)),
        "confirm_delete" => Some(Cow::Borrowed(CONFIRM_DELETE_MACRO)),
        "confirm_text_editor_unsaved" => Some(Cow::Borrowed(CONFIRM_TEXT_EDITOR_UNSAVED_MACRO)),
        "confirm_save_editor_before_quit" => {
            Some(Cow::Borrowed(CONFIRM_SAVE_EDITOR_BEFORE_QUIT_MACRO))
        }
        "confirm_notify" => Some(Cow::Borrowed(CONFIRM_NOTIFY_MACRO)),
        _ => None,
    }
}

fn is_safe_macro_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn parse_confirm_dialog_spec(
    spec: &Table,
    default: ConfirmDialogSpec,
) -> Result<ConfirmDialogSpec> {
    let palette = match spec
        .get::<Option<String>>("palette")?
        .unwrap_or_else(|| "normal".into())
        .as_str()
    {
        "danger" => ConfirmDialogPalette::Danger,
        _ => ConfirmDialogPalette::Normal,
    };
    let message_table = spec.get::<Option<Table>>("message")?;
    let header_table = spec.get::<Option<Table>>("header")?;
    let buttons_table = spec.get::<Option<Table>>("buttons")?;

    // Parse text content first — needed to auto-compute layout.
    let message_text = message_table
        .as_ref()
        .map(|t| table_string(t, "text", &default.message.message_text))
        .transpose()?
        .unwrap_or_else(|| default.message.message_text.clone());
    let message_prefix_blank = message_table
        .as_ref()
        .and_then(|t| t.get::<Option<bool>>("prefix_blank").ok().flatten())
        .unwrap_or(default.message.message_prefix_blank);
    let header_text: Option<String> = header_table
        .as_ref()
        .map(|t| {
            table_string(
                t,
                "text",
                default
                    .header
                    .as_ref()
                    .map(|h| h.message_text.as_str())
                    .unwrap_or(""),
            )
        })
        .transpose()?;

    // Auto-layout: derive y/height/buttons_y/height from content when not
    // explicitly set in the Lua script.
    let header_line_count = header_text
        .as_deref()
        .map(|t| t.lines().count().max(1) as u16);
    let message_line_count = message_text.lines().count().max(1) as u16;
    let extra_blank = if message_prefix_blank { 1u16 } else { 0 };

    let header_y_auto = 0u16;
    let header_height_auto = header_line_count.unwrap_or(1);
    let message_y_auto = if header_text.is_some() {
        header_y_auto + header_height_auto + 1
    } else {
        1
    };
    let message_height_auto = (message_line_count + extra_blank).max(1);
    let buttons_y_auto = message_y_auto + message_height_auto + 1;
    let height_auto = buttons_y_auto + 4; // button row + shadow row + 2 borders

    // Parse layout fields, falling back to auto values when absent.
    let message_y = message_table
        .as_ref()
        .and_then(|t| t.get::<Option<u16>>("y").ok().flatten())
        .unwrap_or(message_y_auto);
    let message_height = message_table
        .as_ref()
        .and_then(|t| t.get::<Option<u16>>("height").ok().flatten())
        .unwrap_or(message_height_auto)
        .clamp(1, 10);
    let header = header_table
        .as_ref()
        .zip(header_text)
        .map(|(t, text)| {
            let h_y = t
                .get::<Option<u16>>("y")
                .ok()
                .flatten()
                .unwrap_or(header_y_auto);
            let h_height = t
                .get::<Option<u16>>("height")
                .ok()
                .flatten()
                .unwrap_or(header_height_auto)
                .clamp(1, 10);
            let prefix_blank = t
                .get::<Option<bool>>("prefix_blank")
                .ok()
                .flatten()
                .unwrap_or(false);
            Ok::<_, anyhow::Error>(ConfirmDialogText {
                message_text: text,
                message_y: h_y,
                message_height: h_height,
                message_prefix_blank: prefix_blank,
            })
        })
        .transpose()?;
    let buttons_y = buttons_table
        .as_ref()
        .and_then(|bt| bt.get::<Option<u16>>("y").ok().flatten())
        .unwrap_or(buttons_y_auto);
    let button_gap = buttons_table
        .as_ref()
        .map(|buttons| table_u16(buttons, "gap", default.button_gap))
        .unwrap_or(default.button_gap);
    let buttons = buttons_table
        .as_ref()
        .and_then(|buttons| buttons.get::<Option<Table>>("items").ok().flatten())
        .map(parse_confirm_buttons)
        .transpose()?
        .filter(|buttons| !buttons.is_empty())
        .unwrap_or(default.buttons);
    let height = spec
        .get::<Option<u16>>("height")
        .ok()
        .flatten()
        .unwrap_or(height_auto)
        .clamp(6, 40);

    Ok(ConfirmDialogSpec {
        width: table_u16(&spec, "width", default.width).clamp(24, 120),
        height,
        shadow_dx: table_u16(&spec, "shadow_dx", default.shadow_dx).clamp(0, 8),
        shadow_dy: table_u16(&spec, "shadow_dy", default.shadow_dy).clamp(0, 4),
        title: table_string(&spec, "title", &default.title)?,
        palette,
        separators: table_u16_list(&spec, "separators")?.unwrap_or(default.separators),
        header,
        message: ConfirmDialogText {
            message_text,
            message_y,
            message_height,
            message_prefix_blank,
        },
        buttons_y,
        button_gap,
        buttons,
    })
}

fn parse_confirm_buttons(items: Table) -> Result<Vec<ConfirmDialogButtonSpec>> {
    let mut buttons = Vec::new();
    for item in items.sequence_values::<Table>() {
        let item = item?;
        let id = table_string(&item, "id", "confirm")?;
        let callback = table_string(&item, "callback", id.as_str())?;
        let label = table_string(&item, "label", id.as_str())?;
        let width = table_u16(&item, "width", label.chars().count() as u16).clamp(4, 30);
        buttons.push(ConfirmDialogButtonSpec {
            callback,
            label,
            width,
        });
    }
    Ok(buttons)
}

fn table_string(table: &Table, key: &str, default: &str) -> Result<String> {
    Ok(table
        .get::<Option<String>>(key)?
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string()))
}

fn table_u16(table: &Table, key: &str, default: u16) -> u16 {
    table
        .get::<Option<u16>>(key)
        .ok()
        .flatten()
        .unwrap_or(default)
}

fn table_u16_list(table: &Table, key: &str) -> Result<Option<Vec<u16>>> {
    let Some(values) = table.get::<Option<Table>>(key)? else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for value in values.sequence_values::<u16>() {
        out.push(value?);
    }
    Ok(Some(out))
}

pub fn install_lua_dialog_module(lua: &Lua, preload: &Table) -> Result<()> {
    let dialog_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;

        t.set(
            "confirm_box",
            lua.create_function(move |lua, spec: Table| {
                if let Some(callback) = spec.get::<Option<mlua::Function>>("callback")?
                    && let Some(buttons) = spec.get::<Option<Table>>("buttons")?
                    && let Some(items) = buttons.get::<Option<Table>>("items")?
                {
                    for item in items.sequence_values::<Table>() {
                        let item = item?;
                        let result: Value = callback.call(item.clone())?;
                        let callback_id = match result {
                            Value::String(value) => value.to_str()?.to_string(),
                            Value::Table(table) => table
                                .get::<Option<String>>("id")?
                                .or_else(|| table.get::<Option<String>>("callback").ok().flatten())
                                .unwrap_or_default(),
                            _ => String::new(),
                        };
                        if !callback_id.is_empty() {
                            item.set("callback", callback_id)?;
                        }
                    }
                }
                spec.set("callback", lua.create_table()?)?;
                Ok(spec)
            })?,
        )?;

        t.set(
            "message",
            lua.create_function(move |_, text: String| {
                run_in_tui(|terminal| {
                    loop {
                        terminal.draw(|f| {
                            let content_w = max_line_width(&text).max(display_width("Lua Message"));
                            let content_h = line_count(&text).max(1);
                            let area = popup_rect(f.area(), content_w, content_h, 38, 96, 7, 28);
                            let inner = inner_rect(area);

                            f.render_widget(Clear, area);
                            f.render_widget(
                                Block::default()
                                    .title("Lua Message")
                                    .title_style(
                                        Style::default()
                                            .fg(clr_dialog_title())
                                            .add_modifier(Modifier::BOLD),
                                    )
                                    .style(Style::default().bg(clr_dialog_bg()))
                                    .border_style(Style::default().fg(clr_dialog_border()))
                                    .borders(Borders::ALL),
                                area,
                            );
                            f.render_widget(
                                Paragraph::new(text.as_str()).style(
                                    Style::default().fg(clr_dialog_fg()).bg(clr_dialog_bg()),
                                ),
                                inner,
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
                    loop {
                        terminal.draw(|f| {
                            let value_line = if value.is_empty() {
                                "Value: "
                            } else {
                                "Value:"
                            };
                            let content_w = max_line_width(&prompt)
                                .max(
                                    display_width(value_line)
                                        .saturating_add(max_line_width(&value)),
                                )
                                .max(display_width("Lua Input"));
                            let content_h = line_count(&prompt).max(1).saturating_add(2);
                            let area = popup_rect(f.area(), content_w, content_h, 42, 104, 8, 30);
                            let chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([Constraint::Min(1), Constraint::Length(1)])
                                .split(inner_rect(area));

                            f.render_widget(Clear, area);
                            f.render_widget(
                                Block::default()
                                    .title("Lua Input")
                                    .title_style(
                                        Style::default()
                                            .fg(clr_dialog_title())
                                            .add_modifier(Modifier::BOLD),
                                    )
                                    .style(Style::default().bg(clr_dialog_bg()))
                                    .border_style(Style::default().fg(clr_dialog_border()))
                                    .borders(Borders::ALL),
                                area,
                            );
                            f.render_widget(
                                Paragraph::new(prompt.as_str()).style(
                                    Style::default().fg(clr_dialog_fg()).bg(clr_dialog_bg()),
                                ),
                                chunks[0],
                            );
                            f.render_widget(
                                Paragraph::new(format!("Value: {}", value)).style(
                                    Style::default()
                                        .fg(clr_dialog_selected_fg())
                                        .bg(clr_dialog_selected_bg()),
                                ),
                                chunks[1],
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
                run_in_tui(|terminal| {
                    loop {
                        terminal.draw(|f| {
                            let content_w =
                                max_line_width(&prompt).max(display_width("Lua Confirm"));
                            let area = popup_rect(f.area(), content_w, 1, 38, 96, 7, 20);
                            let inner = inner_rect(area);

                            f.render_widget(Clear, area);
                            f.render_widget(
                                Block::default()
                                    .title("Lua Confirm")
                                    .title_style(
                                        Style::default()
                                            .fg(clr_dialog_title())
                                            .add_modifier(Modifier::BOLD),
                                    )
                                    .style(Style::default().bg(clr_dialog_bg()))
                                    .border_style(Style::default().fg(clr_dialog_border()))
                                    .borders(Borders::ALL),
                                area,
                            );
                            f.render_widget(
                                Paragraph::new(prompt.as_str()).style(
                                    Style::default().fg(clr_dialog_fg()).bg(clr_dialog_bg()),
                                ),
                                inner,
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
                                            checked: t
                                                .get::<Option<bool>>("checked")?
                                                .unwrap_or(false),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    let mut choices = Vec::new();
                    for value in options.sequence_values::<String>() {
                        choices.push(value?);
                    }
                    if choices.is_empty() {
                        let out = lua.create_table()?;
                        let lua_checks = lua.create_table()?;
                        for (i, item) in checks.into_iter().enumerate() {
                            lua_checks.set(i + 1, item.checked)?;
                        }
                        out.set("checks", lua_checks)?;
                        return Ok(out);
                    }

                    let default_zero_based = default_idx.unwrap_or(1).clamp(1, choices.len()) - 1;
                    let theme = PaletteTheme::from_name(theme_name.as_deref());
                    let (selected, check_states) =
                        run_palette_dialog(prompt, choices, default_zero_based, checks, theme)
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

        t.set(
            "input_box",
            lua.create_function(move |lua, spec: Table| {
                if let Some(callback) = spec.get::<Option<mlua::Function>>("callback")?
                    && let Some(buttons) = spec.get::<Option<Table>>("buttons")?
                    && let Some(items) = buttons.get::<Option<Table>>("items")?
                {
                    for item in items.sequence_values::<Table>() {
                        let item = item?;
                        let result: Value = callback.call(item.clone())?;
                        let callback_id = match result {
                            Value::String(value) => value.to_str()?.to_string(),
                            Value::Table(table) => table
                                .get::<Option<String>>("id")?
                                .or_else(|| table.get::<Option<String>>("callback").ok().flatten())
                                .unwrap_or_default(),
                            _ => String::new(),
                        };
                        if !callback_id.is_empty() {
                            item.set("callback", callback_id)?;
                        }
                    }
                }
                spec.set("callback", lua.create_table()?)?;
                Ok(spec)
            })?,
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
        match name.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("remote") | Some("remote_connections") | Some("ctrlf") => Self::RemoteConnections,
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
            bg: clr_pal_bg(),
            border: clr_pal_border(),
            input_bg: clr_pal_input_bg(),
            input_fg: clr_pal_input_fg(),
            sep: clr_pal_sep(),
            list_fg: clr_pal_list_fg(),
            sel_bg: clr_pal_sel_bg(),
            sel_fg: clr_pal_sel_fg(),
            hint: clr_pal_hint(),
            title: clr_pal_title(),
            footer_bg: clr_pal_footer_bg(),
            footer_fg: clr_pal_footer_fg(),
            footer_shadow: clr_pal_bg(),
        },
        PaletteTheme::RemoteConnections => PaletteColors {
            bg: clr_dialog_bg(),
            border: clr_dialog_border(),
            input_bg: clr_dialog_selected_bg(),
            input_fg: clr_dialog_selected_fg(),
            sep: clr_dialog_hint(),
            list_fg: clr_dialog_fg(),
            sel_bg: clr_dialog_selected_bg(),
            sel_fg: clr_dialog_selected_fg(),
            hint: clr_dialog_hint(),
            title: clr_dialog_title(),
            footer_bg: clr_dialog_border(),
            footer_fg: clr_dialog_selected_fg(),
            footer_shadow: clr_dialog_hint(),
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
            // Exact rule:
            // - <= 8 items: show full list (no scrolling)
            // - > 8 items: list becomes scrollable with 8 visible rows
            let visible_list = filtered.len().clamp(1, 8) as u16;
            let options_rows = if checkboxes.is_empty() {
                0
            } else {
                checkboxes.len() as u16 + 1
            };
            let spacer_rows = if !checkboxes.is_empty() { 1 } else { 0 };
            let content_h = visible_list
                .saturating_add(options_rows)
                .saturating_add(4)
                .saturating_add(spacer_rows);
            let term_size = terminal.size()?;
            let term_area = Rect {
                x: 0,
                y: 0,
                width: term_size.width,
                height: term_size.height,
            };
            let area = popup_rect(term_area, content_w, content_h, 52, 112, 9, 120);
            let inner = inner_rect(area);

            let body_area = Rect {
                x: inner.x,
                y: inner.y + 2,
                width: inner.width,
                // input(1) + sep(1) + body + optional spacer + buttons(1) + shadow(1)
                height: inner.height.saturating_sub(4 + spacer_rows),
            };
            let checks_reserved = if checkboxes.is_empty() {
                0
            } else {
                (checkboxes.len() as u16).saturating_add(1)
            };
            let body_h = body_area.height;
            let desired_choices_h = visible_list.min(body_h).max(1);
            let checks_h = if checkboxes.is_empty() {
                0
            } else {
                checks_reserved.min(body_h.saturating_sub(desired_choices_h))
            };
            let choices_h = body_h.saturating_sub(checks_h);
            let choices_area = Rect {
                x: body_area.x,
                y: body_area.y,
                width: body_area.width,
                height: choices_h,
            };
            let checks_area = Rect {
                x: body_area.x,
                y: body_area.y + choices_h,
                width: body_area.width,
                height: checks_h,
            };
            let ok_label = "▶   OK   ◀";
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
            let choices_h_usize = choices_area.height as usize;
            let checks_visible_rows = checks_area.height.saturating_sub(1) as usize;
            let checks_focusable = !checkboxes.is_empty() && checks_visible_rows > 0;
            if focus == PaletteFocus::Checkboxes && !checks_focusable {
                focus = PaletteFocus::List;
            }
            let choice_start = if choices_h_usize == 0 {
                0
            } else if cursor >= choices_h_usize {
                cursor - choices_h_usize + 1
            } else {
                0
            };
            let checks_start = if checks_visible_rows == 0 {
                0
            } else if checks_cursor >= checks_visible_rows {
                checks_cursor - checks_visible_rows + 1
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
                        .title_style(
                            Style::default()
                                .fg(colors.title)
                                .add_modifier(Modifier::BOLD),
                        )
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
                    Paragraph::new(sep.clone())
                        .style(Style::default().fg(colors.sep).bg(colors.bg)),
                    Rect {
                        x: inner.x,
                        y: inner.y + 1,
                        width: inner.width,
                        height: 1,
                    },
                );

                if choices_area.height > 0 {
                    if filtered.is_empty() {
                        f.render_widget(
                            Paragraph::new(Line::styled(
                                "No match",
                                Style::default().fg(colors.hint).bg(colors.bg),
                            )),
                            Rect {
                                x: choices_area.x,
                                y: choices_area.y,
                                width: choices_area.width,
                                height: 1,
                            },
                        );
                    } else {
                        for (row_idx, choice_idx) in filtered
                            .iter()
                            .skip(choice_start)
                            .take(choices_h_usize)
                            .enumerate()
                        {
                            let y = choices_area.y + row_idx as u16;
                            let is_sel = filtered[cursor] == *choice_idx;
                            let list_is_active = focus == PaletteFocus::List;
                            let (bg, fg, marker) = if is_sel && list_is_active {
                                (colors.sel_bg, colors.sel_fg, "> ")
                            } else if is_sel {
                                (colors.bg, colors.hint, "  ")
                            } else {
                                (colors.bg, colors.list_fg, "  ")
                            };
                            let row_text = format!(
                                "{}{:>2}. {}",
                                marker,
                                choice_idx + 1,
                                choices[*choice_idx]
                            );
                            f.render_widget(
                                Paragraph::new(Line::styled(
                                    truncate_to_width(&row_text, choices_area.width as usize),
                                    Style::default().fg(fg).bg(bg),
                                )),
                                Rect {
                                    x: choices_area.x,
                                    y,
                                    width: choices_area.width,
                                    height: 1,
                                },
                            );
                        }
                    }
                }

                if checks_area.height > 0 {
                    f.render_widget(
                        Paragraph::new(sep.clone())
                            .style(Style::default().fg(colors.sep).bg(colors.bg)),
                        Rect {
                            x: checks_area.x,
                            y: checks_area.y,
                            width: checks_area.width,
                            height: 1,
                        },
                    );

                    for (row_idx, check_idx) in (checks_start..checkboxes.len())
                        .take(checks_visible_rows)
                        .enumerate()
                    {
                        let y = checks_area.y + 1 + row_idx as u16;
                        let item = &checkboxes[check_idx];
                        let is_current = checks_cursor == check_idx;
                        let is_active = focus == PaletteFocus::Checkboxes;
                        let (bg, fg, marker) = if is_current && is_active {
                            (colors.sel_bg, colors.sel_fg, "> ")
                        } else if is_current {
                            (colors.bg, colors.hint, "  ")
                        } else {
                            (colors.bg, colors.list_fg, "  ")
                        };
                        let mark = if item.checked { "x" } else { " " };
                        let row_text = format!("{}[{}] {}", marker, mark, item.label);
                        f.render_widget(
                            Paragraph::new(Line::styled(
                                truncate_to_width(&row_text, checks_area.width as usize),
                                Style::default().fg(fg).bg(bg),
                            )),
                            Rect {
                                x: checks_area.x,
                                y,
                                width: checks_area.width,
                                height: 1,
                            },
                        );
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
                let shadow_side_style = Style::default().fg(colors.footer_shadow).bg(colors.bg);
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
                        "▀".repeat((ok_w - 1) as usize),
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
                        if focus == PaletteFocus::Checkboxes {
                            if let Some(item) = checkboxes.get_mut(checks_cursor) {
                                item.checked = !item.checked;
                            }
                            continue;
                        }
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
                                if checks_focusable {
                                    PaletteFocus::Checkboxes
                                } else {
                                    PaletteFocus::Buttons
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
                        if focus == PaletteFocus::Checkboxes && checks_focusable {
                            checks_cursor = checks_cursor.saturating_sub(1);
                        } else if focus == PaletteFocus::List {
                            cursor = cursor.saturating_sub(1);
                        } else {
                            focus = if checks_focusable {
                                PaletteFocus::Checkboxes
                            } else {
                                PaletteFocus::List
                            };
                        }
                    }
                    KeyCode::Down => {
                        if focus == PaletteFocus::Checkboxes && checks_focusable {
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

                        if mouse.row >= choices_area.y
                            && mouse.row < choices_area.y + choices_area.height
                            && mouse.column >= choices_area.x
                            && mouse.column < choices_area.x + choices_area.width
                        {
                            if !filtered.is_empty() {
                                let row_idx = choice_start + (mouse.row - choices_area.y) as usize;
                                if row_idx < filtered.len() {
                                    cursor = row_idx;
                                    focus = PaletteFocus::List;
                                    active_button = DialogButton::Ok;
                                }
                            }
                        }

                        if checks_area.height > 1
                            && mouse.row > checks_area.y
                            && mouse.row < checks_area.y + checks_area.height
                            && mouse.column >= checks_area.x
                            && mouse.column < checks_area.x + checks_area.width
                        {
                            let check_idx = checks_start + (mouse.row - checks_area.y - 1) as usize;
                            if check_idx < checkboxes.len() {
                                checks_cursor = check_idx;
                                focus = PaletteFocus::Checkboxes;
                                if let Some(item) = checkboxes.get_mut(check_idx) {
                                    item.checked = !item.checked;
                                }
                            }
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        if focus == PaletteFocus::Checkboxes && checks_focusable {
                            checks_cursor = checks_cursor.saturating_sub(1);
                        } else {
                            cursor = cursor.saturating_sub(1);
                        }
                    }
                    MouseEventKind::ScrollDown => {
                        if focus == PaletteFocus::Checkboxes && checks_focusable {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_quit_macro_builds_generic_spec_with_callbacks() {
        let dlg = ConfirmDialog {
            title: None,
            message: None,
            action: ConfirmAction::Quit,
            macro_name: Some("confirm_quit"),
            active_button: crate::app::ConfirmButton::Primary,
        };
        let spec = confirm_render_spec(&dlg).expect("confirm quit spec");

        assert_eq!(spec.title, " KK Commander ");
        assert_eq!(spec.palette, ConfirmDialogPalette::Normal);
        assert_eq!(spec.buttons.len(), 2);
        assert_eq!(spec.buttons[0].callback, "confirm");
        assert_eq!(spec.buttons[1].callback, "cancel");
    }

    #[test]
    fn confirm_delete_macro_uses_context() {
        let dlg = ConfirmDialog {
            title: None,
            message: Some("Delete foo.txt?".into()),
            action: ConfirmAction::Delete(vec![std::path::PathBuf::from("foo.txt")]),
            macro_name: Some("confirm_delete"),
            active_button: crate::app::ConfirmButton::Primary,
        };
        let spec = confirm_render_spec(&dlg).expect("confirm delete spec");

        assert_eq!(spec.title, " Delete ");
        assert_eq!(spec.palette, ConfirmDialogPalette::Danger);
        assert_eq!(spec.header.unwrap().message_text, "⚠  Delete this item?");
        assert_eq!(spec.message.message_text, "Delete foo.txt?");
        assert_eq!(spec.buttons[0].callback, "confirm");
        assert_eq!(spec.buttons[1].callback, "cancel");
    }

    #[test]
    fn confirm_text_editor_unsaved_macro_uses_generic_dialog_flow() {
        let dlg = ConfirmDialog {
            title: None,
            message: None,
            action: ConfirmAction::CloseTextEditorUnsaved,
            macro_name: Some("confirm_text_editor_unsaved"),
            active_button: crate::app::ConfirmButton::Primary,
        };
        let spec = confirm_render_spec(&dlg).expect("confirm text editor unsaved spec");

        assert_eq!(spec.title, " Unsaved Changes ");
        assert_eq!(spec.buttons[0].callback, "confirm");
        assert_eq!(spec.buttons[1].callback, "cancel");
        assert_eq!(spec.buttons[0].label, "▶ Save ◀");
        assert_eq!(spec.buttons[1].label, "▶ Discard ◀");
    }
}
