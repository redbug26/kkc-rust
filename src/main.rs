mod about;
mod app;
mod archive;
mod cloud_status;
mod config;
mod copy;
mod events;
mod file_icons;
mod file_ops;
mod file_types;
mod gif_recorder;
mod help;
mod idf;
mod panel;
mod plugins;
mod remote;
mod search;
mod terminal;
mod tree_mode;
mod ui;
mod viewer;

use anyhow::Result;
use app::{App, AppMode};
use config::Config;
use crossterm::{
    cursor::MoveTo,
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};
use std::io::{self, Stdout, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

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
    let _ = viewer::clear_kitty_images(terminal.backend_mut());
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
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
    let mut last_kitty_image: Option<(PathBuf, ratatui::layout::Rect, bool)> = None;
    let mut first_draw_logged = false;
    let mut startup_ready_logged = false;

    loop {
        app.poll_background_tasks();

        // Draw (clamp_scroll first)
        {
            let area = terminal.size()?;
            let reserved_rows = if app.config.show_fkey_bar { 7 } else { 6 };
            let visible_rows = area.height.saturating_sub(reserved_rows) as usize;

            app.left.clamp_scroll(visible_rows.max(1));
            app.right.clamp_scroll(visible_rows.max(1));
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
                    ui::kitty_image_area(v, term_area).map(|rect| (v.path.clone(), rect, v.zoomed))
                }
                _ => {
                    // Quick-preview: use the inactive panel area
                    app.quick_preview.as_ref().and_then(|v| {
                        ui::kitty_image_area_quick_preview(&app, term_area)
                            .map(|rect| (v.path.clone(), rect, v.zoomed))
                    })
                }
            }
        } else {
            None
        };

        if next_kitty_image != last_kitty_image {
            if last_kitty_image.is_some() {
                viewer::clear_kitty_images(terminal.backend_mut())?;
            }
            if let Some((path, rect, _)) = &next_kitty_image {
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
                        viewer::render_kitty_image(terminal.backend_mut(), v, *rect)?;
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
            AppMode::Input(_) | AppMode::ViewerSearching(_) | AppMode::Terminal => {
                terminal.show_cursor()?
            }
            _ => terminal.hide_cursor()?,
        }

        // Poll for input (~60 fps)
        if event::poll(Duration::from_millis(16))? {
            let ev = event::read()?;
            match events::handle_event(&mut app, ev) {
                Ok(true) => break,
                Ok(false) => {}
                Err(e) => {
                    app.notify(format!("Error: {}", e));
                }
            }
        }
    }

    app.save_config().ok();
    Ok(())
}

fn main() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal);
    teardown_terminal(&mut terminal)?;

    if let Err(ref e) = result {
        eprintln!("KKC error: {e}");
    }

    result
}
