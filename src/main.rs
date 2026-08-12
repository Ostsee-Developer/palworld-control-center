mod app;
mod backend;
mod dynacat;
mod jobs;
mod metrics;
mod model;
mod theme;
mod ui;

use std::{io, path::PathBuf, time::Duration};

use anyhow::Result;
use app::App;
use clap::Parser;
use crossterm::{
    cursor::Show,
    event::{self, Event, KeyEventKind},
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

    /// Aktiviert mutierende Aktionen für diese Sitzung. Ohne den Schalter bleibt die App read-only.
    #[arg(long)]
    enable_writes: bool,

    /// Alternative Palworld-AIO-Betriebsdatei, hauptsächlich für Tests und Migrationen.
    #[arg(long, value_name = "DATEI")]
    config: Option<PathBuf>,

    /// Stellt eine read-only DynaCat-API auf diesem lokalen Unix-Socket bereit.
    #[arg(long, value_name = "SOCKET")]
    dynacat_socket: Option<PathBuf>,
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

    let mut app = App::new(cli.demo, cli.enable_writes, cli.config);
    let dynacat = cli
        .dynacat_socket
        .as_deref()
        .map(dynacat::DynaCatPublisher::start)
        .transpose()?;
    app.refresh();
    let tick_rate = Duration::from_millis(cli.tick_rate_ms);

    while !app.should_quit {
        if let Some(publisher) = &dynacat {
            let job = app.job_view.as_ref().map(|job| {
                (
                    job.label.as_str(),
                    job.detail.as_str(),
                    job.running,
                    job.success,
                )
            });
            publisher.publish(&app.data, &app.metrics, job);
        }
        terminal.draw(|frame| ui::draw(frame, &app))?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                app.handle_key(key);
            }
        } else {
            app.refresh();
        }
    }

    Ok(())
}
