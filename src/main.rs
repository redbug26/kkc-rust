mod app;
mod archive;
mod config;
mod copy;
mod events;
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
use std::time::Duration;

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
    let config = Config::load().unwrap_or_default();
    // Initialise the viewer debug logger before any Viewer is created.
    {
        let log_path = config::project_dirs()
            .ok()
            .map(|d| d.cache_dir().join("debug.log"))
            .unwrap_or_else(|| PathBuf::from("/tmp/kkc_debug.log"));
        viewer::init_debug_log(config.debug_log, log_path);
    }
    let mut app = App::new(config);
    let mut last_kitty_image: Option<(PathBuf, ratatui::layout::Rect, bool)> = None;

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

        let completed = terminal.draw(|f| ui::render(f, &app))?;

        // Ctrl+G GIF capture: append the just-rendered frame to <data_dir>/screen.gif
        if app.capture_gif {
            app.capture_gif = false;
            let gif_path = gif_recorder::gif_path();
            let frame_count = if gif_path.exists() {
                // Count existing frames to use in the notification.
                std::fs::File::open(&gif_path)
                    .ok()
                    .and_then(|f| {
                        use image::{AnimationDecoder, codecs::gif::GifDecoder};
                        use std::io::BufReader;
                        GifDecoder::new(BufReader::new(f)).ok().map(|d| d.into_frames().count())
                    })
                    .unwrap_or(0)
            } else {
                0
            };
            match gif_recorder::capture_frame(completed.buffer, &gif_path) {
                Ok(()) => {
                    app.status.text =
                        format!("GIF: frame {} → {}", frame_count + 1, gif_path.display()).into();
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
        let next_kitty_image = match &app.mode {
            AppMode::Viewer(v) | AppMode::ViewerSearching(v) | AppMode::ViewerMenu(v, _) => {
                ui::kitty_image_area(v, term_area).map(|rect| (v.path.clone(), rect, v.zoomed))
            }
            _ => None,
        };

        if next_kitty_image != last_kitty_image {
            if last_kitty_image.is_some() {
                viewer::clear_kitty_images(terminal.backend_mut())?;
            }
            if let Some((_, rect, _)) = &next_kitty_image {
                if let AppMode::Viewer(v) | AppMode::ViewerSearching(v) | AppMode::ViewerMenu(v, _) =
                    &app.mode
                    && viewer::kitty_graphics_supported()
                {
                    viewer::render_kitty_image(terminal.backend_mut(), v, *rect)?;
                    execute!(
                        terminal.backend_mut(),
                        MoveTo(0, term_area.height.saturating_sub(1))
                    )?;
                    terminal.backend_mut().flush()?;
                }
            }
            last_kitty_image = next_kitty_image;
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
