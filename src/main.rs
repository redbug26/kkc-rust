mod about;
mod app;
mod archive;
mod cloud_status;
mod config;
mod copy;
mod events;
mod file_cache;
mod file_icons;
mod file_ops;
mod file_types;
mod gif_recorder;
mod help;
mod idf;
mod matrix_screensaver;
mod panel;
mod plugins;
mod remote;
mod remote_plugins;
mod search;
mod system_info;
mod terminal;
mod tree_mode;
mod ui;
mod viewer;
mod viewer_plugins;

use anyhow::Result;
use app::{App, AppMode, MenuAction, palette_label_for_action};
use config::Config;
use crossterm::{
    cursor::MoveTo,
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, SetTitle, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};
use std::io::{self, Stdout, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

type QuickPreviewImageTask = (PathBuf, (u32, u32), Receiver<Option<Vec<u8>>>);

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

fn teardown_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let _ = viewer::clear_kitty_images(terminal.backend_mut(), None);
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn sanitize_title_component(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_control()).collect()
}

fn short_path_for_title(path: &str) -> String {
    let mut out = path.to_string();
    if let Some(home) = directories::UserDirs::new()
        .map(|u| u.home_dir().to_string_lossy().into_owned())
        .filter(|home| !home.is_empty())
    {
        if out == home {
            out = "~".to_string();
        } else {
            let home_prefix = format!("{home}/");
            if out.starts_with(&home_prefix) {
                out = format!("~/{}", &out[home_prefix.len()..]);
            }
        }
    }

    let visible_len = out.chars().count();
    if visible_len <= 56 {
        return out;
    }

    let parts: Vec<&str> = out.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() < 2 {
        return out;
    }
    let tail = format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]);

    if let Some((scheme, rest)) = out.split_once("://") {
        let host = rest.split('/').next().unwrap_or(rest);
        return format!("{scheme}://{host}/.../{tail}");
    }

    format!(".../{tail}")
}

fn short_file_for_title(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn compose_window_title(app: &App) -> String {
    match &app.mode {
        AppMode::Browse | AppMode::QuickSearch => {
            let panel_path = short_path_for_title(&app.active_panel().display_path());
            format!("KKC - {}", sanitize_title_component(&panel_path))
        }
        AppMode::Viewer(v)
        | AppMode::ViewerSearching(v)
        | AppMode::ViewerGotoLine(v, _)
        | AppMode::ViewerGoto(v, _)
        | AppMode::ViewerMenu(v, _)
        | AppMode::ViewerPluginPalette(v, _) => {
            let file = short_file_for_title(&v.path);
            let label = palette_label_for_action(MenuAction::ViewFile);
            format!("KKC - {} - {}", label, sanitize_title_component(&file))
        }
        AppMode::Opener(state) => {
            let file = short_file_for_title(&state.path);
            let label = palette_label_for_action(MenuAction::OpenInOs);
            format!("KKC - {} - {}", label, sanitize_title_component(&file))
        }
        _ => {
            let label = app
                .last_menu_action
                .map(palette_label_for_action)
                .filter(|label| !label.is_empty())
                .unwrap_or("KKC");
            format!("KKC - {}", sanitize_title_component(label))
        }
    }
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let startup_start = Instant::now();
    let config_load_start = Instant::now();
    let config = Config::load().unwrap_or_default();
    let config_load_elapsed = config_load_start.elapsed();
    // Initialise the viewer debug logger before any Viewer is created.
    {
        let log_path = config::project_dirs()
            .ok()
            .map(|d| d.cache_dir().join("debug.log"))
            .unwrap_or_else(|| PathBuf::from("/tmp/kkc_debug.log"));
        viewer::init_debug_log(config.debug_log, log_path);
    }
    viewer::debug_log(&format!(
        "startup: config loaded in {:.3} ms",
        config_load_elapsed.as_secs_f64() * 1000.0
    ));
    let app_new_start = Instant::now();
    let mut app = App::new(config);
    viewer::debug_log(&format!(
        "startup: App::new completed in {:.3} ms",
        app_new_start.elapsed().as_secs_f64() * 1000.0
    ));
    let mut last_kitty_image: Option<(PathBuf, ratatui::layout::Rect, bool, Option<String>)> = None;
    let mut first_draw_logged = false;
    let mut startup_ready_logged = false;
    let mut last_user_activity = Instant::now();
    let mut last_window_title = String::new();
    let mut quick_preview_image_task: Option<QuickPreviewImageTask> = None;

    loop {
        app.poll_background_tasks();

        if let Some((path, cache_key, rx)) = &quick_preview_image_task {
            match rx.try_recv() {
                Ok(payload) => {
                    if let Some(viewer) = app
                        .quick_preview
                        .as_ref()
                        .filter(|viewer| viewer.path == *path)
                    {
                        viewer::insert_kitty_png_payload(viewer, *cache_key, payload);
                    }
                    quick_preview_image_task = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    quick_preview_image_task = None;
                }
            }
        }

        let window_title = compose_window_title(&app);
        if window_title != last_window_title {
            execute!(terminal.backend_mut(), SetTitle(window_title.clone()))?;
            last_window_title = window_title;
        }

        if app.config.screensaver_idle_minutes > 0
            && !matches!(
                app.mode,
                AppMode::MatrixScreensaver(_)
                    | AppMode::Terminal
                    | AppMode::Input(_)
                    | AppMode::AssocInput(_)
                    | AppMode::CopyProgress(_)
                    | AppMode::RemoteConnecting(_)
            )
        {
            let timeout = Duration::from_secs(app.config.screensaver_idle_minutes * 60);
            if last_user_activity.elapsed() >= timeout {
                app.mode = AppMode::MatrixScreensaver(app::MatrixScreensaverState::new());
            }
        }

        // Draw (clamp_scroll first)
        {
            let area = terminal.size()?;
            let reserved_rows = if app.config.show_fkey_bar { 7 } else { 6 };
            let visible_rows = area.height.saturating_sub(reserved_rows) as usize;

            app.left.clamp_scroll(visible_rows.max(1));
            app.right.clamp_scroll(visible_rows.max(1));

            if let AppMode::MatrixScreensaver(ref mut state) = app.mode {
                state.step(area.width as usize, area.height as usize);
            }
        }

        // After spawning an external program the alternate screen is blank and
        // ratatui's buffer is stale — clear it so the next draw is unconditional.
        if app.needs_clear {
            app.needs_clear = false;
            terminal.clear()?;
            last_kitty_image = None;
        }

        // Advance About animation tick every frame (~60 fps)
        if let AppMode::About(ref mut state) = app.mode {
            state.tick = state.tick.wrapping_add(1);
            state.step_worm();
        }

        // Compute next kitty image state before drawing so we can clear the old
        // image *before* the TUI draw (required for iTerm2 where clearing writes
        // spaces that would otherwise erase freshly drawn TUI content).
        let term_size = terminal.size()?;
        let term_area = Rect {
            x: 0,
            y: 0,
            width: term_size.width,
            height: term_size.height,
        };
        let next_kitty_image = if viewer::kitty_graphics_supported() {
            match &app.mode {
                AppMode::Viewer(v) | AppMode::ViewerSearching(v) | AppMode::ViewerMenu(v, _) => {
                    ui::kitty_image_area(v, term_area).map(|rect| {
                        (
                            v.path.clone(),
                            rect,
                            v.zoomed,
                            v.plugin_state.get("page").cloned(),
                        )
                    })
                }
                AppMode::Browse => {
                    // Quick-preview: only render image when no modal overlay is shown
                    if viewer::embedded_graphics_supported() {
                        app.quick_preview.as_ref().and_then(|v| {
                            ui::kitty_image_area_quick_preview(&app, term_area).and_then(|rect| {
                                if viewer::cached_kitty_png_payload(v, rect).is_none()
                                    && let Some(request) =
                                        viewer::kitty_image_payload_request(v, rect)
                                {
                                    let already_running = quick_preview_image_task
                                        .as_ref()
                                        .map(|(path, cache_key, _)| {
                                            *path == request.path && *cache_key == request.cache_key
                                        })
                                        .unwrap_or(false);
                                    if !already_running {
                                        let path = request.path.clone();
                                        let cache_key = request.cache_key;
                                        let (tx, rx) = mpsc::channel();
                                        std::thread::spawn(move || {
                                            let payload =
                                                viewer::build_kitty_png_payload_for_request(
                                                    &request,
                                                );
                                            let _ = tx.send(payload);
                                        });
                                        quick_preview_image_task = Some((path, cache_key, rx));
                                    }
                                    return None;
                                }
                                Some((
                                    v.path.clone(),
                                    rect,
                                    v.zoomed,
                                    v.plugin_state.get("page").cloned(),
                                ))
                            })
                        })
                    } else {
                        None
                    }
                }
                _ => {
                    // Any other mode (DirBookmarks, QuickSearch, CommandPalette, etc.)
                    // is a modal overlay — suppress the image so it doesn't bleed
                    // through the overlay drawn by the TUI.
                    None
                }
            }
        } else {
            None
        };

        // PRE-DRAW: clear the old image only when leaving image rendering entirely.
        // For image-to-image transitions, the Kitty renderer reuses the same image
        // id and replacing it in post-draw avoids a one-frame black flash.
        if next_kitty_image.is_none()
            && next_kitty_image != last_kitty_image
            && last_kitty_image.is_some()
        {
            let last_rect = last_kitty_image.as_ref().map(|(_, rect, _, _)| *rect);
            viewer::clear_kitty_images(terminal.backend_mut(), last_rect)?;
        }

        let draw_start = Instant::now();
        let completed = terminal.draw(|f| ui::render(f, &app))?;
        if !first_draw_logged {
            first_draw_logged = true;
            viewer::debug_log(&format!(
                "startup: first terminal draw completed in {:.3} ms",
                draw_start.elapsed().as_secs_f64() * 1000.0
            ));
        }

        // Ctrl+G GIF capture: append the just-rendered frame to <data_dir>/screen.gif
        if app.capture_gif {
            app.capture_gif = false;
            let gif_path = gif_recorder::gif_path();
            match gif_recorder::capture_frame(completed.buffer, &gif_path) {
                Ok(frame_count) => {
                    app.set_status(format!(
                        "GIF: frame {} -> {}",
                        frame_count,
                        gif_path.display()
                    ));
                }
                Err(e) => app.notify(format!("GIF capture failed: {e}")),
            }
        }

        // POST-DRAW: for image-to-image transitions, clear the previous Kitty
        // image here so the new one can replace it without a pre-draw black
        // flash. Leaving image mode entirely is still handled in pre-draw.
        if next_kitty_image != last_kitty_image {
            if next_kitty_image.is_some() && last_kitty_image.is_some() {
                let last_rect = last_kitty_image.as_ref().map(|(_, rect, _, _)| *rect);
                viewer::clear_kitty_images(terminal.backend_mut(), last_rect)?;
            }
            if let Some((path, rect, _, _)) = &next_kitty_image {
                if viewer::kitty_graphics_supported() {
                    // Find the viewer to render (full viewer or quick_preview)
                    let v_opt: Option<&viewer::Viewer> = match &app.mode {
                        AppMode::Viewer(v)
                        | AppMode::ViewerSearching(v)
                        | AppMode::ViewerMenu(v, _)
                            if v.path == *path =>
                        {
                            Some(v)
                        }
                        _ => app.quick_preview.as_ref().filter(|v| v.path == *path),
                    };
                    if let Some(v) = v_opt {
                        let rendered =
                            if matches!(app.mode, AppMode::Browse) && v.image_info().is_some() {
                                viewer::render_cached_kitty_image(terminal.backend_mut(), v, *rect)?
                            } else {
                                viewer::render_kitty_image(terminal.backend_mut(), v, *rect)?;
                                true
                            };
                        if rendered {
                            execute!(
                                terminal.backend_mut(),
                                MoveTo(0, term_area.height.saturating_sub(1))
                            )?;
                            terminal.backend_mut().flush()?;
                            if !startup_ready_logged {
                                viewer::debug_log("startup: kitty image rendered after first draw");
                            }
                        }
                    }
                }
            }
            last_kitty_image = next_kitty_image;
        }

        if first_draw_logged && !startup_ready_logged {
            startup_ready_logged = true;
            viewer::debug_log(&format!(
                "startup: first loop ready in {:.3} ms",
                startup_start.elapsed().as_secs_f64() * 1000.0
            ));
        }

        match app.mode {
            AppMode::Input(_)
            | AppMode::AssocInput(_)
            | AppMode::ViewerSearching(_)
            | AppMode::Terminal => terminal.show_cursor()?,
            _ => terminal.hide_cursor()?,
        }

        // Poll for input (~60 fps)
        if event::poll(Duration::from_millis(16))? {
            let ev = event::read()?;
            last_user_activity = Instant::now();
            match events::handle_event(&mut app, ev) {
                Ok(true) => break,
                Ok(false) => {}
                Err(e) => {
                    app.notify(format!("Error: {}", e));
                }
            }
        }
    }

    // Preferences are saved on change; runtime state is saved on shutdown.
    match app.save_state() {
        Ok(()) => viewer::debug_log("shutdown: runtime state saved"),
        Err(e) => viewer::debug_log(&format!("shutdown: runtime state save failed: {e}")),
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal);
    let _ = execute!(
        terminal.backend_mut(),
        SetTitle("See you soon, space cowboy..."),
    );
    teardown_terminal(&mut terminal)?;

    if let Err(ref e) = result {
        eprintln!("KKC error: {e}");
    }

    result
}
