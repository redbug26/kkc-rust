mod app;
mod config;
mod events;
mod file_ops;
mod file_types;
mod help;
mod panel;
mod search;
mod ui;
mod viewer;

use anyhow::Result;
use app::App;
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
    Ok(Terminal::new(backend)?)
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
            let panel_h = area.height.saturating_sub(3) as usize;
            let inner_h = panel_h.saturating_sub(2);

            app.left.clamp_scroll(inner_h.max(1));
            app.right.clamp_scroll(inner_h.max(1));
        }

        terminal.draw(|f| ui::render(f, &app))?;

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
