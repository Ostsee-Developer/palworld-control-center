mod app;
mod metrics;
mod theme;
mod ui;

use std::{io, time::Duration};

use anyhow::Result;
use app::App;
use clap::Parser;
use crossterm::{
    cursor::Show,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    /// Zeigt eine vollständige Designvorschau ohne Palworld-Installation.
    #[arg(long)]
    demo: bool,

    /// Aktualisierungsintervall in Millisekunden.
    #[arg(long, default_value_t = 1_000, value_parser = clap::value_parser!(u64).range(100..=10_000))]
    tick_rate_ms: u64,
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(cli.demo);
    app.refresh();
    let tick_rate = Duration::from_millis(cli.tick_rate_ms);

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &app))?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match (key.code, key.modifiers) {
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Char('q'), _) => {
                            app.should_quit = true;
                        }
                        (KeyCode::Left | KeyCode::Char('h'), _) => app.previous_tab(),
                        (KeyCode::Right | KeyCode::Char('l'), _) => app.next_tab(),
                        (KeyCode::Char('r'), _) => app.refresh(),
                        (KeyCode::Char(number @ '1'..='8'), _) => app.select_tab(number),
                        _ => {}
                    }
                }
            }
        } else {
            app.refresh();
        }
    }

    Ok(())
}

