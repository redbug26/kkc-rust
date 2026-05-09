use anyhow::{Context, Result, anyhow, bail};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    execute,
    terminal::{self},
};
use mlua::{Lua, String as LuaString, Table, Value};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear as RatatuiClear, Paragraph},
};
use crate::ui::{ShortcutBarItem, ShortcutBarStyle, render_shortcut_bar};
use serde::Deserialize;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

const BUNDLED_TETRIS_APP: &str = include_str!("../assets/applications/tetris/app.lua");
const BUNDLED_TETRIS_MANIFEST: &str = include_str!("../assets/applications/tetris/app.toml");
const BUNDLED_ASCII_APP: &str = include_str!("../assets/applications/ascii/app.lua");
const BUNDLED_ASCII_MANIFEST: &str = include_str!("../assets/applications/ascii/app.toml");
const BUNDLED_CALCULATOR_APP: &str = include_str!("../assets/applications/calculator/app.lua");
const BUNDLED_CALCULATOR_MANIFEST: &str =
    include_str!("../assets/applications/calculator/app.toml");
const BUNDLED_CALENDAR_APP: &str = include_str!("../assets/applications/calendar/app.lua");
const BUNDLED_CALENDAR_MANIFEST: &str = include_str!("../assets/applications/calendar/app.toml");
const BUNDLED_SNAKE_APP: &str = include_str!("../assets/applications/snake/app.lua");
const BUNDLED_SNAKE_MANIFEST: &str = include_str!("../assets/applications/snake/app.toml");
const BUNDLED_GIT_REPO_APP: &str = include_str!("../assets/applications/git_repo/app.lua");
const BUNDLED_GIT_REPO_MANIFEST: &str = include_str!("../assets/applications/git_repo/app.toml");

#[derive(Debug, Clone, Deserialize)]
struct LuaAppManifest {
    app: LuaAppManifestMeta,
}

#[derive(Debug, Clone, Deserialize)]
struct LuaAppManifestMeta {
    id: String,
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    main: Option<String>,
    fps: Option<u32>,
    width: Option<u16>,
    height: Option<u16>,
}

#[derive(Debug, Clone)]
struct LuaAppDescriptor {
    app_dir: PathBuf,
    manifest: LuaAppManifest,
}

#[derive(Debug, Clone)]
pub struct LuaAppInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy)]
struct CellStyle {
    fg: u32,  // RGB color as 0xRRGGBB, 0xFFFFFF = white
    bg: u32,  // RGB color as 0xRRGGBB, 0x000000 = black
}

impl CellStyle {
    fn default() -> Self {
        Self {
            fg: 0xFFFFFF,  // white
            bg: 0x000000,  // black
        }
    }
}

#[derive(Debug, Clone)]
struct GraphicsBuffer {
    width: u16,
    height: u16,
    cells: Vec<char>,
    styles: Vec<CellStyle>,
    current_style: CellStyle,
}

impl GraphicsBuffer {
    fn new(width: u16, height: u16) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let size = w as usize * h as usize;
        Self {
            width: w,
            height: h,
            cells: vec![' '; size],
            styles: vec![CellStyle::default(); size],
            current_style: CellStyle::default(),
        }
    }

    fn resize(&mut self, width: u16, height: u16) {
        let w = width.max(1);
        let h = height.max(1);
        let size = w as usize * h as usize;
        self.width = w;
        self.height = h;
        self.cells = vec![' '; size];
        self.styles = vec![CellStyle::default(); size];
    }

    fn clear(&mut self, ch: char) {
        self.cells.fill(ch);
        self.styles.fill(self.current_style);
    }

    fn put(&mut self, x: i64, y: i64, ch: char) {
        if x < 1 || y < 1 {
            return;
        }
        let xu = (x as u16).saturating_sub(1);
        let yu = (y as u16).saturating_sub(1);
        if xu >= self.width || yu >= self.height {
            return;
        }
        let idx = yu as usize * self.width as usize + xu as usize;
        self.cells[idx] = ch;
        self.styles[idx] = self.current_style;
    }

    fn set_color(&mut self, fg: u32, bg: u32) {
        self.current_style = CellStyle { fg, bg };
    }

    fn reset_color(&mut self) {
        self.current_style = CellStyle::default();
    }

    fn text(&mut self, x: i64, y: i64, text: &str) {
        if y < 1 {
            return;
        }
        let mut col = x;
        for ch in text.chars() {
            self.put(col, y, ch);
            col += 1;
        }
    }

    fn box_rect(&mut self, x: i64, y: i64, w: i64, h: i64, ch: char) {
        if w <= 0 || h <= 0 {
            return;
        }
        for dy in 0..h {
            for dx in 0..w {
                self.put(x + dx, y + dy, ch);
            }
        }
    }

    fn render_lines(&self) -> Vec<(String, Vec<CellStyle>)> {
        let mut out = Vec::with_capacity(self.height as usize);
        for y in 0..self.height {
            let start = y as usize * self.width as usize;
            let end = start + self.width as usize;
            let line_str: String = self.cells[start..end].iter().collect();
            let line_styles: Vec<CellStyle> = self.styles[start..end].to_vec();
            out.push((line_str, line_styles));
        }
        out
    }
}

pub fn initialize() -> Result<()> {
    let apps_dir = applications_dir()?;
    install_bundled_app(
        &apps_dir,
        "tetris",
        BUNDLED_TETRIS_APP,
        BUNDLED_TETRIS_MANIFEST,
    )?;
    install_bundled_app(
        &apps_dir,
        "ascii",
        BUNDLED_ASCII_APP,
        BUNDLED_ASCII_MANIFEST,
    )?;
    install_bundled_app(
        &apps_dir,
        "calculator",
        BUNDLED_CALCULATOR_APP,
        BUNDLED_CALCULATOR_MANIFEST,
    )?;
    install_bundled_app(
        &apps_dir,
        "calendar",
        BUNDLED_CALENDAR_APP,
        BUNDLED_CALENDAR_MANIFEST,
    )?;
    install_bundled_app(
        &apps_dir,
        "snake",
        BUNDLED_SNAKE_APP,
        BUNDLED_SNAKE_MANIFEST,
    )?;
    install_bundled_app(
        &apps_dir,
        "git_repo",
        BUNDLED_GIT_REPO_APP,
        BUNDLED_GIT_REPO_MANIFEST,
    )?;
    Ok(())
}

pub fn app_id_from_command_token(token: &str) -> Option<String> {
    token
        .strip_prefix("kkc-lua-app:")
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
}

pub fn launch_lua_application_with_cwd(
    app_id: &str,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<()> {
    let descriptor = resolve_app_descriptor(app_id)?;
    let main_name = descriptor
        .manifest
        .app
        .main
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("app.lua");
    let script_path = descriptor.app_dir.join(main_name);
    if !script_path.is_file() {
        bail!(
            "Lua app '{}' is missing main script {}",
            app_id,
            script_path.display()
        );
    }
    run_lua_app(&descriptor, &script_path, args, cwd)
}

pub fn list_installed_apps() -> Result<Vec<LuaAppInfo>> {
    let mut out = Vec::new();
    let mut seen_ids = HashSet::new();
    let roots = app_roots()?;

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("app.toml");
            if !manifest_path.is_file() {
                continue;
            }
            let manifest = match load_manifest(&manifest_path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let app_id = manifest.app.id.trim();
            if app_id.is_empty() || seen_ids.contains(app_id) {
                continue;
            }
            let main_name = manifest
                .app
                .main
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("app.lua");
            if !path.join(main_name).is_file() {
                continue;
            }

            let info = LuaAppInfo {
                id: app_id.to_string(),
                name: manifest
                    .app
                    .name
                    .clone()
                    .unwrap_or_else(|| app_id.to_string()),
                version: manifest
                    .app
                    .version
                    .clone()
                    .unwrap_or_else(|| "0.1.0".to_string()),
                description: manifest.app.description.clone().unwrap_or_default(),
            };
            seen_ids.insert(info.id.clone());
            out.push(info);
        }
    }

    out.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(out)
}

pub fn is_lua_application_dir(dir: &Path) -> Result<bool> {
    let manifest_path = dir.join("app.toml");
    if !manifest_path.is_file() {
        return Ok(false);
    }
    let manifest = load_manifest(&manifest_path)?;
    if manifest.app.id.trim().is_empty() {
        return Ok(false);
    }
    let main_name = manifest
        .app
        .main
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("app.lua");
    Ok(dir.join(main_name).is_file())
}

fn install_bundled_app(apps_dir: &Path, id: &str, script: &str, manifest: &str) -> Result<()> {
    let app_dir = apps_dir.join(id);
    fs::create_dir_all(&app_dir).with_context(|| format!("Creating {}", app_dir.display()))?;
    write_if_changed(&app_dir.join("app.lua"), script)?;
    write_if_changed(&app_dir.join("app.toml"), manifest)?;
    Ok(())
}

fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    if matches!(fs::read_to_string(path), Ok(existing) if existing == content) {
        return Ok(());
    }
    fs::write(path, content).with_context(|| format!("Writing {}", path.display()))
}

fn applications_dir() -> Result<PathBuf> {
    let dir = crate::config::data_dir()?.join("applications");
    fs::create_dir_all(&dir).with_context(|| format!("Creating {}", dir.display()))?;
    Ok(dir)
}

fn resolve_app_descriptor(app_id: &str) -> Result<LuaAppDescriptor> {
    let roots = app_roots()?;

    for root in &roots {
        let by_dir = root.join(app_id);
        let manifest_path = by_dir.join("app.toml");
        if manifest_path.is_file() {
            let manifest = load_manifest(&manifest_path)?;
            if manifest.app.id == app_id {
                return Ok(LuaAppDescriptor {
                    app_dir: by_dir,
                    manifest,
                });
            }
        }
    }

    for root in &roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("app.toml");
            if !manifest_path.is_file() {
                continue;
            }
            let manifest = load_manifest(&manifest_path)?;
            if manifest.app.id == app_id {
                return Ok(LuaAppDescriptor {
                    app_dir: path,
                    manifest,
                });
            }
        }
    }

    bail!(
        "Lua app '{}' not found. Expected app.toml in data/applications/<id> or data/plugins/<id>",
        app_id
    )
}

fn app_roots() -> Result<Vec<PathBuf>> {
    let data = crate::config::data_dir()?;
    let apps = data.join("applications");
    let plugins = data.join("plugins");
    Ok(vec![apps, plugins])
}

fn load_manifest(path: &Path) -> Result<LuaAppManifest> {
    let text = fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
    let manifest: LuaAppManifest =
        toml::from_str(&text).with_context(|| format!("Parsing {}", path.display()))?;
    Ok(manifest)
}

fn run_lua_app(
    descriptor: &LuaAppDescriptor,
    script_path: &Path,
    args: &[String],
    cwd: Option<&Path>,
) -> Result<()> {
    let lua = Lua::new();
    let (width, height) = terminal::size().unwrap_or((80, 24));
    let graphics = Rc::new(RefCell::new(GraphicsBuffer::new(width, height)));
    let should_quit = Rc::new(Cell::new(false));
    let start_time = Instant::now();
    let launch_cwd = cwd
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| descriptor.app_dir.clone()));

    install_lua_app_modules(
        &lua,
        &descriptor.manifest,
        &descriptor.app_dir,
        args,
        Rc::clone(&graphics),
        Rc::clone(&should_quit),
        start_time,
        launch_cwd,
    )?;

    let script = fs::read_to_string(script_path)
        .with_context(|| format!("Reading {}", script_path.display()))?;
    let value: Value = lua
        .load(&script)
        .set_name(script_path.to_string_lossy().as_ref())
        .eval()
        .with_context(|| format!("Running {}", script_path.display()))?;

    let app_table = match value {
        Value::Table(t) => t,
        _ => lua
            .globals()
            .get::<Table>("app")
            .map_err(|_| anyhow!("Lua app must return a table with callbacks"))?,
    };

    let init_ctx = lua.create_table()?;
    init_ctx.set("width", width)?;
    init_ctx.set("height", height)?;
    init_ctx.set("id", descriptor.manifest.app.id.clone())?;
    init_ctx.set(
        "name",
        descriptor
            .manifest
            .app
            .name
            .clone()
            .unwrap_or_else(|| descriptor.manifest.app.id.clone()),
    )?;
    init_ctx.set(
        "version",
        descriptor
            .manifest
            .app
            .version
            .clone()
            .unwrap_or_else(|| "0.1.0".to_string()),
    )?;
    init_ctx.set(
        "description",
        descriptor
            .manifest
            .app
            .description
            .clone()
            .unwrap_or_default(),
    )?;
    let lua_args = lua.create_table()?;
    for (idx, arg) in args.iter().enumerate() {
        lua_args.set(idx + 1, arg.clone())?;
    }
    init_ctx.set("args", lua_args)?;

    call_table_function_if_exists(&app_table, "init", init_ctx)?;

    let fps = descriptor.manifest.app.fps.unwrap_or(30).clamp(10, 120);
    let frame_time = Duration::from_secs_f64(1.0 / fps as f64);

    // ── Setup ratatui terminal ──────────────────────────────────────────────
    // KKC already has raw mode enabled. Render directly on main screen.
    // Hide the cursor but don't enter alternate screen so KKC content is visible behind.
    let mut stdout = io::stdout();
    execute!(stdout, Hide)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let app_title = descriptor
        .manifest
        .app
        .name
        .as_deref()
        .unwrap_or(&descriptor.manifest.app.id)
        .to_string();

    // zoomed: false = floating window showing KKC behind, true = full-screen
    let mut zoomed: bool = false;

    // App-requested window size (from manifest; defaults if not specified)
    let app_width = descriptor.manifest.app.width.unwrap_or(40);
    let app_height = descriptor.manifest.app.height.unwrap_or(20);

    let mut last_frame = Instant::now();
    let mut resized = false;

    // Notify Lua of its initial inner content area
    {
        let sz = terminal.size()?;
        let term_size = Rect::new(0, 0, sz.width, sz.height);
        let inner = lua_app_inner_area(term_size, zoomed, app_width, app_height);
        graphics.borrow_mut().resize(inner.width, inner.height);
        call_table_function_if_exists(
            &app_table,
            "resize",
            (inner.width as i64, inner.height as i64),
        )?;
    }

    while !should_quit.get() {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(last_frame);
        if elapsed < frame_time {
            let timeout = frame_time - elapsed;
            if event::poll(timeout)? {
                match event::read()? {
                    Event::Key(key) => {
                        // F5 to toggle zoom (full-screen vs floating window)
                        if matches!(key.code, KeyCode::F(5)) {
                            zoomed = !zoomed;
                            resized = true;
                        } else if is_quit_key(&key) {
                            should_quit.set(true);
                            continue;
                        } else if let Some(name) = key_name(&key) {
                            call_table_function_if_exists(&app_table, "keypressed", name)?;
                        }
                    }
                    Event::Mouse(mouse) => {
                        dispatch_mouse_event(&app_table, mouse)?;
                    }
                    Event::Resize(new_w, new_h) => {
                        let inner =
                            lua_app_inner_area(Rect::new(0, 0, new_w, new_h), zoomed, app_width, app_height);
                        graphics.borrow_mut().resize(inner.width, inner.height);
                        resized = true;
                    }
                    _ => {}
                }
            }
            continue;
        }

        let dt = elapsed.as_secs_f64();
        last_frame = now;

        if resized {
            let sz = terminal.size()?;
            let term_size = Rect::new(0, 0, sz.width, sz.height);
            let inner = lua_app_inner_area(term_size, zoomed, app_width, app_height);
            graphics.borrow_mut().resize(inner.width, inner.height);
            call_table_function_if_exists(
                &app_table,
                "resize",
                (inner.width as i64, inner.height as i64),
            )?;
            resized = false;
        }

        call_table_function_if_exists(&app_table, "update", dt)?;
        call_table_function_if_exists(&app_table, "draw", ())?;

        let lua_lines = graphics.borrow().render_lines();
        let shortcut_items = lua_shortcut_items(&app_table)?;
        let title_str = app_title.clone();

        terminal.draw(|f| {
            let term_area = f.area();

            let win_area = lua_app_window_area(term_area, zoomed, app_width, app_height);

            // Window border + title  (viewer style)
            let border_style = Style::default()
                .fg(Color::Rgb(239, 225, 196))
                .add_modifier(Modifier::BOLD);
            let title_line = Line::from(vec![
                Span::styled(
                    format!(" {} ", title_str),
                    Style::default()
                        .fg(Color::Rgb(255, 244, 114))
                        .bg(Color::Rgb(54, 42, 30))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  F5 zoom  Esc quit ",
                    Style::default()
                        .fg(Color::Rgb(160, 140, 110))
                        .bg(Color::Rgb(54, 42, 30)),
                ),
            ]);
            let block = Block::default()
                .title(title_line)
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .border_style(border_style)
                .style(Style::default().bg(Color::Black));

            let inner = block.inner(win_area);
            f.render_widget(RatatuiClear, win_area);
            f.render_widget(block, win_area);

            // Render Lua buffer lines into the inner area
            for (row_idx, (line_str, line_styles)) in lua_lines.iter().enumerate() {
                let y = inner.y + row_idx as u16;
                if y >= inner.y + inner.height {
                    break;
                }
                // Clip to inner width
                let chars: Vec<char> = line_str.chars().collect();
                let visible_w = inner.width as usize;
                let mut spans = Vec::new();
                for (col, ch) in chars.iter().take(visible_w).enumerate() {
                    let style = if col < line_styles.len() {
                        let s = line_styles[col];
                        let fg = Color::Rgb(
                            ((s.fg >> 16) & 0xFF) as u8,
                            ((s.fg >> 8) & 0xFF) as u8,
                            (s.fg & 0xFF) as u8,
                        );
                        let bg = Color::Rgb(
                            ((s.bg >> 16) & 0xFF) as u8,
                            ((s.bg >> 8) & 0xFF) as u8,
                            (s.bg & 0xFF) as u8,
                        );
                        Style::default().fg(fg).bg(bg)
                    } else {
                        Style::default().fg(Color::White).bg(Color::Black)
                    };
                    spans.push(Span::styled(ch.to_string(), style));
                }
                f.render_widget(
                    Paragraph::new(Line::from(spans)),
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: 1,
                    },
                );
            }

            // Generic Lua footer shortcuts rendered with KKC native shortcut bar renderer.
            if !shortcut_items.is_empty() && inner.height > 0 {
                let footer_area = Rect {
                    x: inner.x,
                    y: inner.y + inner.height.saturating_sub(1),
                    width: inner.width,
                    height: 1,
                };
                let style = ShortcutBarStyle {
                    key_fg: Color::Rgb(230, 238, 255),
                    key_bg: Color::Rgb(52, 73, 110),
                    label_fg: Color::Rgb(198, 212, 238),
                    label_bg: Color::Rgb(30, 36, 52),
                    bar_bg: Color::Rgb(22, 26, 40),
                    sep_fg: Color::Rgb(88, 104, 136),
                };
                render_shortcut_bar(f, footer_area, &shortcut_items, style);
            }
        })?;
    }

    // ── Teardown ───────────────────────────────────────────────────────────
    // Just restore the cursor; raw mode and screen content handled by KKC.
    execute!(terminal.backend_mut(), Show)?;
    Ok(())
}

/// Compute the window area for a Lua app (floating when !zoomed, full-screen when zoomed).
fn lua_app_window_area(term: Rect, zoomed: bool, req_width: u16, req_height: u16) -> Rect {
    if zoomed {
        return term;  // Full screen
    }
    // Floating window: use app-requested size, constrained to terminal
    let w = req_width.min(term.width);
    let h = req_height.min(term.height);
    let x = term.x + term.width.saturating_sub(w) / 2;
    let y = term.y + term.height.saturating_sub(h) / 2;
    Rect { x, y, width: w, height: h }
}

/// Inner content area (after borders) inside the window.
fn lua_app_inner_area(term: Rect, zoomed: bool, req_width: u16, req_height: u16) -> Rect {
    let win = lua_app_window_area(term, zoomed, req_width, req_height);
    Rect {
        x: win.x + 1,
        y: win.y + 1,
        width: win.width.saturating_sub(2),
        height: win.height.saturating_sub(2),
    }
}

fn install_lua_app_modules(
    lua: &Lua,
    manifest: &LuaAppManifest,
    app_dir: &Path,
    args: &[String],
    graphics: Rc<RefCell<GraphicsBuffer>>,
    should_quit: Rc<Cell<bool>>,
    start_time: Instant,
    launch_cwd: PathBuf,
) -> Result<()> {
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let current_path: String = package.get("path")?;
    let app_path = format!(
        "{}/?.lua;{}/?/init.lua;{}",
        app_dir.display(),
        app_dir.display(),
        current_path
    );
    package.set("path", app_path)?;

    let preload: Table = package.get("preload")?;
    let app_root = app_dir.to_path_buf();

    let quit_cell = Rc::clone(&should_quit);
    let app_id = manifest.app.id.clone();
    let app_name = manifest
        .app
        .name
        .clone()
        .unwrap_or_else(|| app_id.clone());
    let app_version = manifest
        .app
        .version
        .clone()
        .unwrap_or_else(|| "0.1.0".to_string());
    let app_args = args.to_vec();
    let launch_cwd_text = launch_cwd.to_string_lossy().into_owned();

    let kkc_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;
        let quit_cell = Rc::clone(&quit_cell);
        t.set(
            "quit",
            lua.create_function(move |_, ()| {
                quit_cell.set(true);
                Ok(())
            })?,
        )?;

        let start_time = start_time;
        t.set(
            "time",
            lua.create_function(move |_, ()| Ok(start_time.elapsed().as_secs_f64()))?,
        )?;

        let args_for_fn = app_args.clone();
        t.set(
            "args",
            lua.create_function(move |lua, ()| {
                let tbl = lua.create_table()?;
                for (idx, arg) in args_for_fn.iter().enumerate() {
                    tbl.set(idx + 1, arg.clone())?;
                }
                Ok(tbl)
            })?,
        )?;

        t.set("id", app_id.clone())?;
        t.set("name", app_name.clone())?;
        t.set("version", app_version.clone())?;
        t.set("cwd", launch_cwd_text.clone())?;
        let cwd_fn_value = launch_cwd_text.clone();
        t.set(
            "get_cwd",
            lua.create_function(move |_, ()| Ok(cwd_fn_value.clone()))?,
        )?;
        Ok(t)
    })?;
    preload.set("kkc", kkc_mod)?;

    let gfx = Rc::clone(&graphics);
    let graphics_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;

        let g_size = Rc::clone(&gfx);
        t.set(
            "size",
            lua.create_function(move |_, ()| {
                let g = g_size.borrow();
                Ok((g.width, g.height))
            })?,
        )?;

        let g_clear = Rc::clone(&gfx);
        t.set(
            "clear",
            lua.create_function(move |_, ch: Option<String>| {
                let c = ch
                    .as_deref()
                    .and_then(|s| s.chars().next())
                    .unwrap_or(' ');
                g_clear.borrow_mut().clear(c);
                Ok(())
            })?,
        )?;

        let g_put = Rc::clone(&gfx);
        t.set(
            "put",
            lua.create_function(move |_, (x, y, ch): (i64, i64, LuaString)| {
                let bytes = ch.as_bytes();
                let text = String::from_utf8_lossy(&bytes);
                let c = text.chars().next().unwrap_or(' ');
                g_put.borrow_mut().put(x, y, c);
                Ok(())
            })?,
        )?;

        let g_text = Rc::clone(&gfx);
        t.set(
            "text",
            lua.create_function(move |_, (x, y, text): (i64, i64, LuaString)| {
                let bytes = text.as_bytes();
                let rendered = String::from_utf8_lossy(&bytes).into_owned();
                g_text.borrow_mut().text(x, y, &rendered);
                Ok(())
            })?,
        )?;

        // print(row, col, text) — row-first convenience alias for text()
        let g_print = Rc::clone(&gfx);
        t.set(
            "print",
            lua.create_function(move |_, (row, col, text): (i64, i64, LuaString)| {
                let bytes = text.as_bytes();
                let rendered = String::from_utf8_lossy(&bytes).into_owned();
                g_print.borrow_mut().text(col, row, &rendered);
                Ok(())
            })?,
        )?;

        let g_box = Rc::clone(&gfx);
        t.set(
            "box",
            lua.create_function(move |_, (x, y, w, h, ch): (i64, i64, i64, i64, Option<String>)| {
                let c = ch
                    .as_deref()
                    .and_then(|s| s.chars().next())
                    .unwrap_or(' ');
                g_box.borrow_mut().box_rect(x, y, w, h, c);
                Ok(())
            })?,
        )?;

        // color(fg, bg) - set foreground and background colors in hex (0xRRGGBB)
        let g_color = Rc::clone(&gfx);
        t.set(
            "color",
            lua.create_function(move |_, (fg, bg): (i64, i64)| {
                g_color.borrow_mut().set_color(fg as u32, bg as u32);
                Ok(())
            })?,
        )?;

        // set_fg(color) - set foreground color, keep background
        let g_set_fg = Rc::clone(&gfx);
        t.set(
            "set_fg",
            lua.create_function(move |_, fg: i64| {
                let mut g = g_set_fg.borrow_mut();
                let bg = g.current_style.bg;
                g.set_color(fg as u32, bg);
                Ok(())
            })?,
        )?;

        // set_bg(color) - set background color, keep foreground
        let g_set_bg = Rc::clone(&gfx);
        t.set(
            "set_bg",
            lua.create_function(move |_, bg: i64| {
                let mut g = g_set_bg.borrow_mut();
                let fg = g.current_style.fg;
                g.set_color(fg, bg as u32);
                Ok(())
            })?,
        )?;

        // reset() - reset to default colors (white text on black background)
        let g_reset = Rc::clone(&gfx);
        t.set(
            "reset",
            lua.create_function(move |_, ()| {
                g_reset.borrow_mut().reset_color();
                Ok(())
            })?,
        )?;

        Ok(t)
    })?;
    preload.set("kkc-graphics", graphics_mod)?;

    let key_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;
        t.set("LEFT", "left")?;
        t.set("RIGHT", "right")?;
        t.set("UP", "up")?;
        t.set("DOWN", "down")?;
        t.set("SPACE", "space")?;
        t.set("ENTER", "enter")?;
        t.set("ESC", "esc")?;
        Ok(t)
    })?;
    preload.set("kkc-key", key_mod)?;

    let mouse_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;
        t.set("LEFT", "left")?;
        t.set("RIGHT", "right")?;
        t.set("MIDDLE", "middle")?;
        t.set("UP", "up")?;
        t.set("DOWN", "down")?;
        t.set("DRAG", "drag")?;
        t.set("MOVE", "move")?;
        t.set("SCROLL_UP", "scroll_up")?;
        t.set("SCROLL_DOWN", "scroll_down")?;
        t.set("SCROLL_LEFT", "scroll_left")?;
        t.set("SCROLL_RIGHT", "scroll_right")?;
        Ok(t)
    })?;
    preload.set("kkc-mouse", mouse_mod)?;

    // Lightweight pseudo-random helper module for terminal games.
    let rand_state = Rc::new(RefCell::new({
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        nanos ^ 0x9E37_79B9_7F4A_7C15
    }));

    let rand_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;

        let state_int = Rc::clone(&rand_state);
        t.set(
            "int",
            lua.create_function(move |_, (min, max): (i64, i64)| {
                let low = min.min(max);
                let high = min.max(max);
                let span = (high - low + 1).max(1) as u64;
                let mut s = state_int.borrow_mut();
                *s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
                let val = (*s % span) as i64 + low;
                Ok(val)
            })?,
        )?;

        let state_float = Rc::clone(&rand_state);
        t.set(
            "float",
            lua.create_function(move |_, ()| {
                let mut s = state_float.borrow_mut();
                *s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
                let top = (*s >> 11) as f64;
                Ok(top / ((1u64 << 53) as f64))
            })?,
        )?;

        let state_seed = Rc::clone(&rand_state);
        t.set(
            "seed",
            lua.create_function(move |_, seed: i64| {
                let mut s = state_seed.borrow_mut();
                *s = seed as u64;
                Ok(())
            })?,
        )?;

        Ok(t)
    })?;
    preload.set("kkc-rand", rand_mod)?;

    // Controlled FS module anchored to the app directory for relative paths.
    let fs_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;

        let app_root_resolve = app_root.clone();
        let resolve = move |path: &str| -> PathBuf {
            let candidate = PathBuf::from(path);
            if candidate.is_absolute() {
                candidate
            } else {
                app_root_resolve.join(candidate)
            }
        };

        let app_root_exists = app_root.clone();
        let resolve_exists = resolve.clone();
        t.set(
            "exists",
            lua.create_function(move |_, path: String| Ok(resolve_exists(&path).exists()))?,
        )?;

        let resolve_is_dir = resolve.clone();
        t.set(
            "is_dir",
            lua.create_function(move |_, path: String| Ok(resolve_is_dir(&path).is_dir()))?,
        )?;

        let resolve_read = resolve.clone();
        t.set(
            "read_text",
            lua.create_function(move |_, path: String| {
                let p = resolve_read(&path);
                let text = fs::read_to_string(&p)
                    .map_err(|e| mlua::Error::external(anyhow!("Reading {}: {}", p.display(), e)))?;
                Ok(text)
            })?,
        )?;

        let resolve_write = resolve.clone();
        t.set(
            "write_text",
            lua.create_function(move |_, (path, text): (String, String)| {
                let p = resolve_write(&path);
                if let Some(parent) = p.parent() {
                    fs::create_dir_all(parent).map_err(|e| {
                        mlua::Error::external(anyhow!("Creating {}: {}", parent.display(), e))
                    })?;
                }
                fs::write(&p, text)
                    .map_err(|e| mlua::Error::external(anyhow!("Writing {}: {}", p.display(), e)))?;
                Ok(())
            })?,
        )?;

        let resolve_list = resolve.clone();
        t.set(
            "list_dir",
            lua.create_function(move |lua, path: Option<String>| {
                let p = path
                    .as_deref()
                    .map(resolve_list.clone())
                    .unwrap_or_else(|| app_root_exists.clone());
                let entries = fs::read_dir(&p)
                    .map_err(|e| mlua::Error::external(anyhow!("Reading {}: {}", p.display(), e)))?;
                let out = lua.create_table()?;
                for (idx, entry) in entries.flatten().enumerate() {
                    if let Some(name) = entry.file_name().to_str() {
                        out.set(idx + 1, name.to_string())?;
                    }
                }
                Ok(out)
            })?,
        )?;

        let resolve_mkdir = resolve.clone();
        t.set(
            "mkdir_all",
            lua.create_function(move |_, path: String| {
                let p = resolve_mkdir(&path);
                fs::create_dir_all(&p)
                    .map_err(|e| mlua::Error::external(anyhow!("Creating {}: {}", p.display(), e)))?;
                Ok(())
            })?,
        )?;

        t.set(
            "join",
            lua.create_function(move |_, (a, b): (String, String)| {
                Ok(Path::new(&a).join(&b).to_string_lossy().into_owned())
            })?,
        )?;

        Ok(t)
    })?;
    preload.set("kkc-fs", fs_mod)?;

    // Shell facade for terminal apps. Useful for Git-oriented tools.
    let shell_default_cwd = launch_cwd.clone();
    let shell_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;

        let run_default_cwd = shell_default_cwd.clone();
        t.set(
            "run",
            lua.create_function(
                move |lua, (program, args_tbl, cwd): (String, Option<Table>, Option<String>)| {
                    let mut args = Vec::new();
                    if let Some(tbl) = args_tbl {
                        for value in tbl.sequence_values::<String>() {
                            args.push(value?);
                        }
                    }

                    let mut cmd = Command::new(&program);
                    cmd.args(&args);
                    cmd.stdin(Stdio::null());
                    cmd.env("GIT_TERMINAL_PROMPT", "0");
                    match cwd {
                        Some(path) if !path.trim().is_empty() => {
                            cmd.current_dir(path);
                        }
                        _ => {
                            cmd.current_dir(&run_default_cwd);
                        }
                    }

                    let output = cmd.output().map_err(|e| {
                        mlua::Error::external(anyhow!("Running '{}': {}", program, e))
                    })?;

                    let result = lua.create_table()?;
                    result.set("ok", output.status.success())?;
                    result.set("code", output.status.code().unwrap_or(-1))?;
                    result.set("stdout", String::from_utf8_lossy(&output.stdout).into_owned())?;
                    result.set("stderr", String::from_utf8_lossy(&output.stderr).into_owned())?;
                    Ok(result)
                },
            )?,
        )?;

        let cwd_value = shell_default_cwd.to_string_lossy().into_owned();
        t.set("cwd", lua.create_function(move |_, ()| Ok(cwd_value.clone()))?)?;

        Ok(t)
    })?;
    preload.set("kkc-shell", shell_mod)?;

    // Audio facade for terminal apps. Currently limited to terminal bell.
    let audio_mod = lua.create_function(move |lua, ()| {
        let t = lua.create_table()?;
        t.set(
            "beep",
            lua.create_function(move |_, ()| {
                print!("\x07");
                io::stdout()
                    .flush()
                    .map_err(|e| mlua::Error::external(anyhow!("Flushing stdout: {}", e)))?;
                Ok(())
            })?,
        )?;
        Ok(t)
    })?;
    preload.set("kkc-audio", audio_mod)?;

    Ok(())
}

fn call_table_function_if_exists<A>(table: &Table, name: &str, args: A) -> Result<()>
where
    A: mlua::IntoLuaMulti,
{
    let value: Value = table.get(name)?;
    if let Value::Function(func) = value {
        func.call::<()>(args)?;
    }
    Ok(())
}

fn lua_shortcut_items(app_table: &Table) -> Result<Vec<ShortcutBarItem>> {
    let value: Value = app_table.get("shortcuts")?;
    let Value::Function(func) = value else {
        return Ok(Vec::new());
    };

    let returned: Value = func.call(())?;
    let Value::Table(tbl) = returned else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for value in tbl.sequence_values::<String>() {
        let raw = value?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, label)) = trimmed.split_once(':') {
            out.push(ShortcutBarItem {
                key: key.trim().to_string(),
                label: label.trim().to_string(),
            });
        } else {
            out.push(ShortcutBarItem {
                key: trimmed.to_string(),
                label: String::new(),
            });
        }
    }
    Ok(out)
}

fn is_quit_key(key: &KeyEvent) -> bool {
    key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn key_name(key: &KeyEvent) -> Option<String> {
    match key.code {
        KeyCode::Up => Some("up".to_string()),
        KeyCode::Down => Some("down".to_string()),
        KeyCode::Left => Some("left".to_string()),
        KeyCode::Right => Some("right".to_string()),
        KeyCode::Enter => Some("enter".to_string()),
        KeyCode::Esc => Some("esc".to_string()),
        KeyCode::Backspace => Some("backspace".to_string()),
        KeyCode::Delete => Some("delete".to_string()),
        KeyCode::Tab => Some("tab".to_string()),
        KeyCode::PageUp => Some("pageup".to_string()),
        KeyCode::PageDown => Some("pagedown".to_string()),
        KeyCode::Home => Some("home".to_string()),
        KeyCode::End => Some("end".to_string()),
        KeyCode::Insert => Some("insert".to_string()),
        KeyCode::F(n) => Some(format!("f{}", n)),
        KeyCode::Char(' ') => Some("space".to_string()),
        KeyCode::Char(c) => Some(format!("char:{}", c)),
        _ => None,
    }
}

fn dispatch_mouse_event(app_table: &Table, mouse: MouseEvent) -> Result<()> {
    let x = mouse.column as i64 + 1;
    let y = mouse.row as i64 + 1;
    match mouse.kind {
        MouseEventKind::Down(button) => {
            let name = mouse_button_name(button);
            call_table_function_if_exists(app_table, "mousepressed", (name, x, y))?;
        }
        MouseEventKind::Up(button) => {
            let name = mouse_button_name(button);
            call_table_function_if_exists(app_table, "mousereleased", (name, x, y))?;
        }
        MouseEventKind::Drag(button) => {
            let name = mouse_button_name(button);
            call_table_function_if_exists(app_table, "mousedragged", (name, x, y))?;
        }
        MouseEventKind::Moved => {
            call_table_function_if_exists(app_table, "mousemoved", (x, y))?;
        }
        MouseEventKind::ScrollUp => {
            call_table_function_if_exists(app_table, "mousewheel", (0i64, 1i64, x, y))?;
        }
        MouseEventKind::ScrollDown => {
            call_table_function_if_exists(app_table, "mousewheel", (0i64, -1i64, x, y))?;
        }
        MouseEventKind::ScrollLeft => {
            call_table_function_if_exists(app_table, "mousewheel", (-1i64, 0i64, x, y))?;
        }
        MouseEventKind::ScrollRight => {
            call_table_function_if_exists(app_table, "mousewheel", (1i64, 0i64, x, y))?;
        }
    }
    Ok(())
}

fn mouse_button_name(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_token_parses_app_id() {
        assert_eq!(
            app_id_from_command_token("kkc-lua-app:tetris").as_deref(),
            Some("tetris")
        );
        assert!(app_id_from_command_token("kkc-lua-app:").is_none());
        assert!(app_id_from_command_token("bat").is_none());
    }

    #[test]
    fn detects_valid_lua_app_directory() {
        let root = std::env::temp_dir().join(format!(
            "kkc-lua-app-dir-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp app dir");
        fs::write(
            root.join("app.toml"),
            "[app]\nid=\"demo\"\nname=\"Demo\"\nversion=\"0.1.0\"\nmain=\"app.lua\"\n",
        )
        .expect("write app.toml");
        fs::write(root.join("app.lua"), "return {}").expect("write app.lua");

        let ok = is_lua_application_dir(&root).expect("validation should work");
        assert!(ok);

        let _ = fs::remove_dir_all(&root);
    }
}
