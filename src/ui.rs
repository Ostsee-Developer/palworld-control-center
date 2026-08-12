use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Tabs, Wrap},
};

use crate::{
    app::{App, Tab},
    theme,
};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    frame.render_widget(Block::default().style(Style::default().bg(theme::BACKGROUND)), frame.area());
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_header(frame, root[0], app);
    draw_tabs(frame, root[1], app);
    if app.current_tab() == Tab::Overview {
        draw_overview(frame, root[2], app);
    } else {
        draw_placeholder(frame, root[2], app);
    }
    draw_footer(frame, root[3], app);
}

fn panel(title: &'static str) -> Block<'static> {
    Block::default()
        .title(Line::from(vec![
            Span::styled(" ◈ ", Style::default().fg(theme::MINT)),
            Span::styled(title, Style::default().fg(theme::TEXT).add_modifier(Modifier::BOLD)),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(theme::MUTED))
        .style(Style::default().bg(theme::PANEL).fg(theme::TEXT))
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let status_color = if app.server.connected { theme::GREEN } else { theme::AMBER };
    let mode = if app.demo { "DEMO" } else { "LIVE" };
    let line = Line::from(vec![
        Span::styled(" PALWORLD ", Style::default().fg(theme::BACKGROUND).bg(theme::MINT).add_modifier(Modifier::BOLD)),
        Span::styled(" CONTROL CENTER ", Style::default().fg(theme::CYAN).add_modifier(Modifier::BOLD)),
        Span::styled("// HACKER TERMINAL PRO", Style::default().fg(theme::MUTED)),
        Span::raw("    "),
        Span::styled(format!("● {} · {} ", app.server.service, mode), Style::default().fg(status_color)),
    ]);
    frame.render_widget(
        Paragraph::new(line)
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(theme::MINT)))
            .style(Style::default().bg(theme::BACKGROUND)),
        area,
    );
}

fn draw_tabs(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let titles = Tab::ALL
        .into_iter()
        .map(|tab| Line::from(tab.label()))
        .collect::<Vec<_>>();
    let tabs = Tabs::new(titles)
        .select(app.selected_tab)
        .divider(Span::styled(" │ ", Style::default().fg(theme::MUTED)))
        .style(Style::default().fg(theme::MUTED).bg(theme::BACKGROUND))
        .highlight_style(Style::default().fg(theme::MINT).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(theme::PANEL_ALT)));
    frame.render_widget(tabs, area);
}

fn draw_overview(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
        .split(area);
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(columns[0]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(columns[1]);

    draw_logs(frame, left[0], app);
    draw_backup(frame, left[1], app);
    draw_resources(frame, right[0], app);
    draw_quick_settings(frame, right[1], app);
}

fn draw_logs(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let items = app
        .logs
        .iter()
        .rev()
        .take(area.height.saturating_sub(2) as usize)
        .rev()
        .map(|entry| {
            let color = if entry.contains("ERROR") || entry.contains("FEHLER") {
                theme::RED
            } else if entry.contains("WARN") {
                theme::AMBER
            } else if entry.contains("JOB") {
                theme::CYAN
            } else {
                theme::TEXT
            };
            ListItem::new(Line::from(Span::styled(entry, Style::default().fg(color))))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items).block(panel("REALTIME SERVER LOGS")), area);
}

fn draw_backup(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let inner = panel("REALTIME BACKUP / RESTORE").inner(area);
    frame.render_widget(panel("REALTIME BACKUP / RESTORE"), area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(2), Constraint::Min(1)])
        .margin(1)
        .split(inner);
    let progress = app.server.backup_progress.unwrap_or(0);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("JOB  ", Style::default().fg(theme::CYAN).add_modifier(Modifier::BOLD)),
            Span::styled(app.server.backup_label, Style::default().fg(theme::TEXT)),
        ])),
        rows[0],
    );
    frame.render_widget(
        Gauge::default()
            .ratio(f64::from(progress) / 100.0)
            .label(format!("{progress:>3}%"))
            .gauge_style(Style::default().fg(theme::MINT).bg(theme::PANEL_ALT).add_modifier(Modifier::BOLD)),
        rows[1],
    );
    let note = if app.server.backup_progress.is_some() {
        "Welt gesichert · Zstandard-Kompression läuft · SHA-256 folgt"
    } else {
        "Read-only Alpha: Job-Engine wird in Phase 2 angebunden"
    };
    frame.render_widget(Paragraph::new(note).style(Style::default().fg(theme::MUTED)), rows[2]);
}

fn draw_resources(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let block = panel("RESOURCES / PLAYERS / VERSION");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(3),
        ])
        .margin(1)
        .split(inner);

    resource_line(frame, rows[0], "CPU ", app.metrics.cpu_percent, "Host");
    resource_line(
        frame,
        rows[1],
        "RAM ",
        app.metrics.memory_percent,
        &format!("{:.1}/{:.1} GiB", app.metrics.memory_used_gib, app.metrics.memory_total_gib),
    );
    resource_line(
        frame,
        rows[2],
        "DISK",
        app.metrics.disk_percent,
        &format!("{:.1} GiB frei", app.metrics.disk_free_gib),
    );
    let players = match (app.server.players_online, app.server.players_max) {
        (Some(online), Some(max)) => format!("{online}/{max}"),
        _ => "—".to_owned(),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("PLAYERS  ", Style::default().fg(theme::MUTED)),
            Span::styled(players, Style::default().fg(theme::MINT).add_modifier(Modifier::BOLD)),
            Span::styled("    VERSION  ", Style::default().fg(theme::MUTED)),
            Span::styled(
                app.server.palworld_version.as_deref().unwrap_or("nicht verbunden"),
                Style::default().fg(theme::CYAN),
            ),
        ])),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new("○ ◔ ◑ ◕ ●  Terminal-Kreisindikatoren · Live-Aktualisierung")
            .style(Style::default().fg(theme::MUTED))
            .wrap(Wrap { trim: true }),
        rows[4],
    );
}

fn resource_line(frame: &mut Frame<'_>, area: Rect, label: &str, value: f64, detail: &str) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(6), Constraint::Min(8), Constraint::Length(15)])
        .split(area);
    let color = theme::usage_color(value);
    frame.render_widget(
        Paragraph::new(format!("{} {:>3.0}%", pie(value), value)).style(Style::default().fg(color)),
        columns[0],
    );
    frame.render_widget(
        Gauge::default()
            .ratio((value / 100.0).clamp(0.0, 1.0))
            .gauge_style(Style::default().fg(color).bg(theme::PANEL_ALT)),
        columns[1],
    );
    frame.render_widget(
        Paragraph::new(format!("{label} {detail}")).alignment(Alignment::Right).style(Style::default().fg(theme::MUTED)),
        columns[2],
    );
}

fn pie(value: f64) -> &'static str {
    match value {
        value if value < 12.5 => "○",
        value if value < 37.5 => "◔",
        value if value < 62.5 => "◑",
        value if value < 87.5 => "◕",
        _ => "●",
    }
}

fn draw_quick_settings(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let enabled = |value: Option<bool>| match value {
        Some(true) => "ON",
        Some(false) => "OFF",
        None => "—",
    };
    let lines = vec![
        setting("CLIENT MODS", enabled(app.server.client_mods)),
        setting(
            "EXP RATE",
            app.server.exp_rate.map_or_else(|| "—".to_owned(), |value| format!("{value:.2}x")),
        ),
        setting("PVP", enabled(app.server.pvp)),
        setting(
            "MAX PLAYERS",
            app.server.players_max.map_or_else(|| "—".to_owned(), |value| value.to_string()),
        ),
        Line::from(Span::styled(
            "Read-only Alpha · alle 93 Optionen im SETTINGS-Tab",
            Style::default().fg(theme::MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("QUICK GAME SETTINGS"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn setting(name: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {name:<18}"), Style::default().fg(theme::MUTED)),
        Span::styled(value.into(), Style::default().fg(theme::MINT).add_modifier(Modifier::BOLD)),
    ])
}

fn draw_placeholder(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let text = match app.current_tab() {
        Tab::Players => "Spielerliste, Broadcast, Kick, Ban und Unban werden hier gebündelt.",
        Tab::Settings => "93 Palworld-Einstellungen, Suche, Kategorien, Diff und sichere Übernahme.",
        Tab::Mods => "PAK-, LogicMods- und Workshop-Inventar mit Kompatibilitätsstatus.",
        Tab::Backups => "Backup-Historie, Live-Jobs, Integritätsprüfung und transaktionaler Restore.",
        Tab::Updates => "Buildvergleich, offizielles Changelog, Backup-Plan und Upgrade-Fortschritt.",
        Tab::Logs => "Filterbare Journald-Ansicht mit Leveln, Suche, Export und Ereigniskorrelation.",
        Tab::Security => "REST-Exposition, Dateirechte, Auditlog, API-Tokens und Diagnosezustand.",
        Tab::Overview => "",
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(app.current_tab().title(), Style::default().fg(theme::MINT).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(Span::styled(text, Style::default().fg(theme::TEXT))),
            Line::from(""),
            Line::from(Span::styled("Modul vorbereitet · Implementierung folgt in der nächsten Phase", Style::default().fg(theme::AMBER))),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .block(panel(app.current_tab().title())),
        area,
    );
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let line = Line::from(vec![
        Span::styled(" ←/→ ", Style::default().fg(theme::BACKGROUND).bg(theme::MINT)),
        Span::styled(" Tabs  ", Style::default().fg(theme::MUTED)),
        Span::styled(" 1–8 ", Style::default().fg(theme::BACKGROUND).bg(theme::CYAN)),
        Span::styled(" Direktwahl  ", Style::default().fg(theme::MUTED)),
        Span::styled(" R ", Style::default().fg(theme::BACKGROUND).bg(theme::AMBER)),
        Span::styled(" Refresh  ", Style::default().fg(theme::MUTED)),
        Span::styled(" Q ", Style::default().fg(theme::BACKGROUND).bg(theme::RED)),
        Span::styled(" Quit", Style::default().fg(theme::MUTED)),
        Span::styled(format!("    tick #{:06}", app.ticks), Style::default().fg(theme::MUTED)),
    ]);
    frame.render_widget(Paragraph::new(line).style(Style::default().bg(theme::BACKGROUND)), area);
}

#[cfg(test)]
mod tests {
    use super::pie;

    #[test]
    fn pie_indicator_uses_expected_ranges() {
        assert_eq!(pie(0.0), "○");
        assert_eq!(pie(25.0), "◔");
        assert_eq!(pie(50.0), "◑");
        assert_eq!(pie(75.0), "◕");
        assert_eq!(pie(100.0), "●");
    }
}
