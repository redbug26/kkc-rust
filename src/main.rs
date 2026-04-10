mod app;
mod archive;
mod config;
mod events;
mod file_ops;
mod file_types;
mod help;
mod idf;
mod panel;
mod search;
mod ui;
mod viewer;

use anyhow::Result;
use app::{App, AppMode};
use config::Config;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};
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
    let mut app = App::new(config);

    loop {
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
        }

        terminal.draw(|f| ui::render(f, &app))?;

        match app.mode {
            AppMode::Input(_) | AppMode::ViewerSearching(_) => terminal.show_cursor()?,
            _ => terminal.hide_cursor()?,
        }

        // Poll for input (~60 fps)
        if event::poll(Duration::from_millis(16))? {
            let ev = event::read()?;
            match events::handle_event(&mut app, ev) {
                Ok(true) => break,
                Ok(false) => {}
                Err(e) => {
                    app.status.text = format!("Error: {}", e);
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
