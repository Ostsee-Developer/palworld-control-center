use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Row, Table, Tabs, Wrap},
};

use crate::{
    app::{App, Overlay, Tab},
    model::CheckStatus,
    theme,
};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    frame.render_widget(
        Block::default().style(Style::default().bg(theme::BACKGROUND)),
        frame.area(),
    );
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_header(frame, root[0], app);
    draw_tabs(frame, root[1], app);
    match app.current_tab() {
        Tab::Overview => draw_overview(frame, root[2], app),
        Tab::Players => draw_players(frame, root[2], app),
        Tab::Settings => draw_settings(frame, root[2], app),
        Tab::Mods => draw_mods(frame, root[2], app),
        Tab::Backups => draw_backups(frame, root[2], app),
        Tab::Updates => draw_updates(frame, root[2], app),
        Tab::Logs => draw_log_explorer(frame, root[2], app),
        Tab::Security => draw_security(frame, root[2], app),
    }
    draw_job(frame, root[3], app);
    draw_footer(frame, root[4], app);
    if let Some(overlay) = &app.overlay {
        draw_overlay(frame, overlay);
    }
}

fn panel(title: impl Into<String>) -> Block<'static> {
    Block::default()
        .title(Line::from(vec![
            Span::styled(" ◈ ", Style::default().fg(theme::MINT)),
            Span::styled(
                title.into(),
                Style::default()
                    .fg(theme::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(theme::MUTED))
        .style(Style::default().bg(theme::PANEL).fg(theme::TEXT))
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let status_color = if app.data.server.api_connected {
        theme::GREEN
    } else if app.data.server.service_active {
        theme::AMBER
    } else {
        theme::RED
    };
    let mode = if app.demo { "DEMO" } else { "LIVE" };
    let access = if app.writes_enabled {
        "WRITE: FREIGEGEBEN"
    } else {
        "READ-ONLY"
    };
    let line = Line::from(vec![
        Span::styled(
            " PALWORLD ",
            Style::default()
                .fg(theme::BACKGROUND)
                .bg(theme::MINT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " CONTROL CENTER ",
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("// HACKER TERMINAL PRO", Style::default().fg(theme::MUTED)),
        Span::raw("    "),
        Span::styled(
            format!("● {} · {mode} · {access} ", app.data.server.service),
            Style::default().fg(status_color),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(line)
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(theme::MINT)),
            )
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
        .highlight_style(
            Style::default()
                .fg(theme::MINT)
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme::PANEL_ALT)),
        );
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

    draw_overview_logs(frame, left[0], app);
    draw_backup_summary(frame, left[1], app);
    draw_resources(frame, right[0], app);
    draw_quick_settings(frame, right[1], app);
}

fn draw_overview_logs(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let visible = app
        .data
        .logs
        .len()
        .saturating_sub(area.height.saturating_sub(2) as usize);
    let items = app
        .data
        .logs
        .iter()
        .skip(visible)
        .map(|entry| ListItem::new(log_line(entry)))
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items).block(panel("REALTIME SERVER LOGS")), area);
}

fn draw_backup_summary(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let block = panel("REALTIME BACKUP / RESTORE");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .margin(1)
        .split(inner);
    let newest = app
        .data
        .backups
        .first()
        .map_or("Noch kein Backup erkannt", |backup| backup.name.as_str());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "LETZTES  ",
                Style::default()
                    .fg(theme::CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(newest, Style::default().fg(theme::TEXT)),
        ])),
        rows[0],
    );
    let (ratio, label, color) = match &app.job_view {
        Some(job) if job.running => (
            f64::from((app.ticks % 20) as u16) / 20.0,
            "LÄUFT",
            theme::MINT,
        ),
        Some(job) if job.success == Some(true) => (1.0, "FERTIG", theme::GREEN),
        Some(_) => (0.0, "FEHLER", theme::RED),
        None => (0.0, "BEREIT", theme::MUTED),
    };
    frame.render_widget(
        Gauge::default().ratio(ratio).label(label).gauge_style(
            Style::default()
                .fg(color)
                .bg(theme::PANEL_ALT)
                .add_modifier(Modifier::BOLD),
        ),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} Sicherungen · N/Backup-Tab zum Erstellen · Restore prüft SHA-256 und rollt Fehler zurück",
            app.data.backups.len()
        ))
        .style(Style::default().fg(theme::MUTED))
        .wrap(Wrap { trim: true }),
        rows[2],
    );
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

    resource_line(frame, rows[0], "CPU", app.metrics.cpu_percent, "Host");
    resource_line(
        frame,
        rows[1],
        "RAM",
        app.metrics.memory_percent,
        &format!(
            "{:.1}/{:.1}G",
            app.metrics.memory_used_gib, app.metrics.memory_total_gib
        ),
    );
    resource_line(
        frame,
        rows[2],
        "DISK",
        app.metrics.disk_percent,
        &format!("{:.1}G frei", app.metrics.disk_free_gib),
    );
    let players = match (app.data.server.players_online, app.data.server.players_max) {
        (Some(online), Some(max)) => format!("{online}/{max}"),
        _ => "—".to_owned(),
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("PLAYERS  ", Style::default().fg(theme::MUTED)),
            Span::styled(
                players,
                Style::default()
                    .fg(theme::MINT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("   FPS  ", Style::default().fg(theme::MUTED)),
            Span::styled(
                app.data
                    .server
                    .server_fps
                    .map_or_else(|| "—".to_owned(), |fps| fps.to_string()),
                Style::default().fg(theme::CYAN),
            ),
        ])),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("VERSION  ", Style::default().fg(theme::MUTED)),
                Span::styled(
                    app.data
                        .server
                        .palworld_version
                        .as_deref()
                        .unwrap_or("nicht erreichbar"),
                    Style::default().fg(theme::CYAN),
                ),
            ]),
            Line::from(Span::styled(
                &app.data.server.connection_detail,
                Style::default().fg(theme::MUTED),
            )),
        ])
        .wrap(Wrap { trim: true }),
        rows[4],
    );
}

fn resource_line(frame: &mut Frame<'_>, area: Rect, label: &str, value: f64, detail: &str) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(5),
            Constraint::Length(14),
        ])
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
        Paragraph::new(format!("{label} {detail}"))
            .alignment(Alignment::Right)
            .style(Style::default().fg(theme::MUTED)),
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
        setting("CLIENT MODS", enabled(app.data.server.client_mods)),
        setting(
            "EXP RATE",
            app.data
                .server
                .exp_rate
                .map_or_else(|| "—".to_owned(), |value| format!("{value:.2}x")),
        ),
        setting("PVP", enabled(app.data.server.pvp)),
        setting(
            "MAX PLAYERS",
            app.data
                .server
                .players_max
                .map_or_else(|| "—".to_owned(), |value| value.to_string()),
        ),
        setting("PAK MODS", app.data.mods.len().to_string()),
        Line::from(Span::styled(
            format!("{} Optionen im SETTINGS-Tab", app.data.settings.len()),
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
        Span::styled(
            value.into(),
            Style::default()
                .fg(theme::MINT)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn draw_players(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(area);
    let rows = app
        .data
        .players
        .iter()
        .enumerate()
        .map(|(index, player)| {
            Row::new(vec![
                player.name.clone(),
                player.level.to_string(),
                format!("{:.0} ms", player.ping),
                short_id(&player.user_id),
                player.building_count.to_string(),
            ])
            .style(selected_style(index == app.selected_index()))
        })
        .collect::<Vec<_>>();
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Percentage(38),
            Constraint::Length(8),
        ],
    )
    .header(
        Row::new(["NAME", "LEVEL", "PING", "USER-ID (GEKÜRZT)", "BUILDS"]).style(
            Style::default()
                .fg(theme::CYAN)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .column_spacing(1)
    .block(panel(format!(
        "ONLINE PLAYERS · {}",
        app.data.players.len()
    )));
    frame.render_widget(table, columns[0]);

    let detail = app.data.players.get(app.selected_index());
    let lines = detail.map_or_else(
        || {
            vec![
                Line::from(Span::styled("Keine Spieler online", Style::default().fg(theme::MUTED))),
                Line::from(""),
                Line::from("Die Liste wird über GET /players live aktualisiert."),
            ]
        },
        |player| {
            vec![
                setting("NAME", player.name.clone()),
                setting("ACCOUNT", player.account_name.clone()),
                setting("LEVEL", player.level.to_string()),
                setting("PING", format!("{:.0} ms", player.ping)),
                setting("GEBÄUDE", player.building_count.to_string()),
                Line::from(""),
                Line::from(Span::styled(
                    "IP-Adressen und vollständige IDs werden in der normalen Ansicht nicht angezeigt.",
                    Style::default().fg(theme::MUTED),
                )),
            ]
        },
    );
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("SELECTED PLAYER / PRIVACY"))
            .wrap(Wrap { trim: true }),
        columns[1],
    );
}

fn draw_settings(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
        .split(area);
    let settings = app.visible_settings();
    let selected = app.selected_index();
    let (start, end) = list_window(
        settings.len(),
        selected,
        area.height.saturating_sub(4) as usize,
    );
    let rows = settings[start..end]
        .iter()
        .enumerate()
        .map(|(offset, setting)| {
            let index = start + offset;
            Row::new(vec![
                setting.category.clone(),
                setting.label.clone(),
                setting.display_value().to_owned(),
                if setting.is_editable() {
                    "EDIT"
                } else {
                    "LOCK"
                }
                .to_owned(),
            ])
            .style(selected_style(index == selected))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(25),
                Constraint::Percentage(38),
                Constraint::Percentage(27),
                Constraint::Length(6),
            ],
        )
        .header(
            Row::new(["KATEGORIE", "EINSTELLUNG", "WERT", "MODUS"]).style(
                Style::default()
                    .fg(theme::CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .column_spacing(1)
        .block(panel(format!(
            "ALL GAME SETTINGS · {} · FILTER: {}",
            settings.len(),
            if app.settings_search.is_empty() {
                "—"
            } else {
                &app.settings_search
            }
        ))),
        columns[0],
    );
    let lines = settings.get(selected).map_or_else(
        || vec![Line::from("Keine passende Einstellung gefunden.")],
        |setting| {
            vec![
                Line::from(Span::styled(
                    &setting.label,
                    Style::default().fg(theme::MINT).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(&setting.key, Style::default().fg(theme::CYAN))),
                Line::from(""),
                Line::from(setting.description.as_str()),
                Line::from(""),
                setting_line("TYP", &setting.kind),
                setting_line("AKTUELL", setting.display_value()),
                setting_line(
                    "ZUGRIFF",
                    if setting.is_editable() {
                        "mit --enable-writes"
                    } else {
                        "geschützt / synchronisiert"
                    },
                ),
                Line::from(""),
                Line::from(Span::styled(
                    "Änderungen werden validiert und atomar geschrieben. Neustart wird nie automatisch erzwungen.",
                    Style::default().fg(theme::MUTED),
                )),
            ]
        },
    );
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("SETTING DETAILS"))
            .wrap(Wrap { trim: true }),
        columns[1],
    );
}

fn draw_mods(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);
    let selected = app.selected_index();
    let (start, end) = list_window(
        app.data.mods.len(),
        selected,
        area.height.saturating_sub(4) as usize,
    );
    let rows = app.data.mods[start..end]
        .iter()
        .enumerate()
        .map(|(offset, entry)| {
            let state = if entry.partial {
                "TEILWEISE"
            } else if entry.enabled {
                "AKTIV"
            } else {
                "INAKTIV"
            };
            Row::new(vec![
                entry.kind.clone(),
                entry.name.clone(),
                entry.version.clone().unwrap_or_else(|| "—".to_owned()),
                state.to_owned(),
            ])
            .style(selected_style(start + offset == selected))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Percentage(50),
                Constraint::Length(14),
                Constraint::Length(12),
            ],
        )
        .header(
            Row::new(["TYP", "PAKET", "VERSION", "STATUS"]).style(
                Style::default()
                    .fg(theme::CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .column_spacing(1)
        .block(panel(format!(
            "MOD INVENTORY · {} ERKANNT",
            app.data.mods.len()
        ))),
        columns[0],
    );
    let lines = app.data.mods.get(selected).map_or_else(
        || vec![Line::from("Keine Mods erkannt.")],
        |entry| {
            vec![
                setting("NAME", entry.name.clone()),
                setting("TYP", entry.kind.clone()),
                setting("AKTIV", if entry.enabled { "JA" } else { "NEIN" }),
                setting(
                    "KOMPATIBEL",
                    entry.compatible.map_or("nicht angegeben", |value| {
                        if value { "JA" } else { "NEIN" }
                    }),
                ),
                Line::from(""),
                Line::from(entry.detail.as_str()),
                Line::from(""),
                Line::from(Span::styled(
                    "Native PAKs werden anhand der verwalteten Paketdateien und ihrer aktiven Links geprüft. Direkte Fremddateien bleiben unangetastet.",
                    Style::default().fg(theme::MUTED),
                )),
            ]
        },
    );
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("MOD STATUS / SAFETY"))
            .wrap(Wrap { trim: true }),
        columns[1],
    );
}

fn draw_backups(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);
    let selected = app.selected_index();
    let (start, end) = list_window(
        app.data.backups.len(),
        selected,
        area.height.saturating_sub(4) as usize,
    );
    let rows = app.data.backups[start..end]
        .iter()
        .enumerate()
        .map(|(offset, backup)| {
            Row::new(vec![
                backup.name.clone(),
                human_bytes(backup.size_bytes),
                if backup.checksum_present {
                    "SHA-256"
                } else {
                    "FEHLT"
                }
                .to_owned(),
            ])
            .style(selected_style(start + offset == selected))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(68),
                Constraint::Length(12),
                Constraint::Length(12),
            ],
        )
        .header(
            Row::new(["BACKUP", "GRÖSSE", "INTEGRITÄT"]).style(
                Style::default()
                    .fg(theme::CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .column_spacing(1)
        .block(panel(format!(
            "BACKUP HISTORY · {}",
            app.data.backups.len()
        ))),
        columns[0],
    );
    let lines = app.data.backups.get(selected).map_or_else(
        || vec![Line::from("Noch keine Backups vorhanden.")],
        |backup| {
            vec![
                Line::from(Span::styled(
                    &backup.name,
                    Style::default().fg(theme::MINT).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                setting_line("GRÖSSE", &human_bytes(backup.size_bytes)),
                setting_line(
                    "PRÜFSUMME",
                    if backup.checksum_present {
                        "vorhanden"
                    } else {
                        "fehlt"
                    },
                ),
                Line::from(""),
                Line::from(Span::styled(
                    "Restore: SHA-256 → sichere Extraktion → Serverstop → Pre-Restore-Backup → atomarer Tausch → Rollback bei Fehler.",
                    Style::default().fg(theme::MUTED),
                )),
            ]
        },
    );
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("RESTORE SAFETY"))
            .wrap(Wrap { trim: true }),
        columns[1],
    );
}

fn draw_updates(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(8)])
        .split(area);
    let status_lines = vec![
        setting(
            "INSTALLIERT",
            app.data
                .updates
                .installed_build
                .clone()
                .unwrap_or_else(|| "unbekannt".to_owned()),
        ),
        setting(
            "NÄCHSTER CHECK",
            app.data
                .updates
                .next_check
                .clone()
                .unwrap_or_else(|| "kein Timer erkannt".to_owned()),
        ),
        setting(
            "LETZTES ERGEBNIS",
            app.data
                .updates
                .last_result
                .clone()
                .unwrap_or_else(|| "—".to_owned()),
        ),
        Line::from(Span::styled(
            "Update-Ablauf: Spielerwarnung → Pre-Update-Backup → SteamCMD validate → Dienststart",
            Style::default().fg(theme::MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(status_lines)
            .block(panel("SERVER BUILD / UPDATE PLAN"))
            .wrap(Wrap { trim: true }),
        rows[0],
    );
    let news = app
        .data
        .updates
        .changelog
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            Row::new(vec![
                entry.title.clone(),
                entry.published_unix.to_string(),
                entry.url.clone(),
            ])
            .style(selected_style(index == app.selected_index()))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Table::new(
            news,
            [
                Constraint::Percentage(45),
                Constraint::Length(14),
                Constraint::Percentage(45),
            ],
        )
        .header(
            Row::new(["OFFIZIELLER STEAM-POST", "UNIX-ZEIT", "URL"]).style(
                Style::default()
                    .fg(theme::CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .column_spacing(1)
        .block(panel("OFFICIAL PALWORLD CHANGELOG")),
        rows[1],
    );
}

fn draw_log_explorer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let logs = app.visible_logs();
    let start = logs
        .len()
        .saturating_sub(area.height.saturating_sub(3) as usize);
    let items = logs[start..]
        .iter()
        .map(|entry| ListItem::new(log_line(entry)))
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(panel(format!(
            "LOG EXPLORER · FILTER {} · SUCHE {} · {}/{} ZEILEN",
            app.log_filter.label(),
            if app.log_search.is_empty() {
                "—"
            } else {
                &app.log_search
            },
            logs.len(),
            app.data.logs.len()
        ))),
        area,
    );
}

fn draw_security(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let rows = app
        .data
        .security
        .iter()
        .map(|check| {
            let (status, color) = match check.status {
                CheckStatus::Pass => ("PASS", theme::GREEN),
                CheckStatus::Warn => ("WARN", theme::AMBER),
                CheckStatus::Fail => ("FAIL", theme::RED),
            };
            Row::new(vec![
                status.to_owned(),
                check.label.clone(),
                check.detail.clone(),
            ])
            .style(Style::default().fg(color))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(8),
                Constraint::Percentage(25),
                Constraint::Percentage(65),
            ],
        )
        .header(
            Row::new(["STATUS", "PRÜFUNG", "DETAIL"]).style(
                Style::default()
                    .fg(theme::CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .column_spacing(1)
        .block(panel("SECURITY BASELINE / REDACTED DIAGNOSTICS")),
        area,
    );
}

fn draw_job(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let (line, color) = match &app.job_view {
        Some(job) if job.running => (
            format!(" ◔ JOB  {}  ·  {}", job.label, job.detail),
            theme::CYAN,
        ),
        Some(job) if job.success == Some(true) => (
            format!(" ● OK   {}  ·  {}", job.label, job.detail),
            theme::GREEN,
        ),
        Some(job) => (
            format!(" ● FAIL {}  ·  {}", job.label, job.detail),
            theme::RED,
        ),
        None => (
            format!(" ○ READY  {}", app.data.server.connection_detail),
            theme::MUTED,
        ),
    };
    frame.render_widget(
        Paragraph::new(line)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(color)),
            )
            .style(Style::default().fg(color).bg(theme::BACKGROUND))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let line = Line::from(vec![
        Span::styled(
            " ←/→ ",
            Style::default().fg(theme::BACKGROUND).bg(theme::MINT),
        ),
        Span::styled(" Tabs  ", Style::default().fg(theme::MUTED)),
        Span::styled(
            " ↑/↓ ",
            Style::default().fg(theme::BACKGROUND).bg(theme::CYAN),
        ),
        Span::styled(" Auswahl  ", Style::default().fg(theme::MUTED)),
        Span::styled(app.footer_help(), Style::default().fg(theme::TEXT)),
        Span::styled("   Q Quit", Style::default().fg(theme::MUTED)),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme::BACKGROUND)),
        area,
    );
}

fn draw_overlay(frame: &mut Frame<'_>, overlay: &Overlay) {
    let area = centered_rect(68, 13, frame.area());
    frame.render_widget(Clear, area);
    let (title, color) = match overlay {
        Overlay::Text { title, .. } | Overlay::Confirm { title, .. } => {
            (title.as_str(), theme::MINT)
        }
        Overlay::Message { title, error, .. } => (
            title.as_str(),
            if *error { theme::RED } else { theme::GREEN },
        ),
    };
    let block = Block::default()
        .title(Line::from(Span::styled(
            format!(" ◈ {title} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )))
        .borders(Borders::ALL)
        .border_set(symbols::border::DOUBLE)
        .border_style(Style::default().fg(color))
        .style(Style::default().bg(theme::PANEL_ALT).fg(theme::TEXT));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = match overlay {
        Overlay::Text { prompt, value, .. } => vec![
            Line::from(prompt.as_str()),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    " > ",
                    Style::default().fg(theme::BACKGROUND).bg(theme::MINT),
                ),
                Span::styled(
                    value,
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("█", Style::default().fg(theme::MINT)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Enter bestätigen · Esc abbrechen",
                Style::default().fg(theme::MUTED),
            )),
        ],
        Overlay::Confirm { prompt, .. } => vec![
            Line::from(prompt.as_str()),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    " ENTER/J ",
                    Style::default().fg(theme::BACKGROUND).bg(theme::GREEN),
                ),
                Span::styled(" Ausführen   ", Style::default().fg(theme::TEXT)),
                Span::styled(
                    " N/ESC ",
                    Style::default().fg(theme::BACKGROUND).bg(theme::RED),
                ),
                Span::styled(" Abbrechen", Style::default().fg(theme::TEXT)),
            ]),
        ],
        Overlay::Message { body, .. } => vec![
            Line::from(body.as_str()),
            Line::from(""),
            Line::from(Span::styled(
                "Enter / Leertaste / Esc zum Schließen",
                Style::default().fg(theme::MUTED),
            )),
        ],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .style(Style::default().bg(theme::PANEL_ALT).fg(theme::TEXT)),
        inner,
    );
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height.min(area.height)),
            Constraint::Fill(1),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn selected_style(selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(theme::BACKGROUND)
            .bg(theme::MINT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT)
    }
}

fn log_line(entry: &str) -> Line<'static> {
    let upper = entry.to_ascii_uppercase();
    let color = if upper.contains("ERROR") || upper.contains("FEHLER") {
        theme::RED
    } else if upper.contains("WARN") {
        theme::AMBER
    } else if upper.contains("JOB") {
        theme::CYAN
    } else {
        theme::TEXT
    };
    Line::from(Span::styled(entry.to_owned(), Style::default().fg(color)))
}

fn setting_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<14}"), Style::default().fg(theme::MUTED)),
        Span::styled(value.to_owned(), Style::default().fg(theme::MINT)),
    ])
}

fn short_id(value: &str) -> String {
    if value.chars().count() <= 14 {
        return value.to_owned();
    }
    let start = value.chars().take(8).collect::<String>();
    let end = value
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{start}…{end}")
}

fn human_bytes(bytes: u64) -> String {
    const GIB: f64 = 1_073_741_824.0;
    const MIB: f64 = 1_048_576.0;
    if bytes >= 1_073_741_824 {
        format!("{:.2} GiB", bytes as f64 / GIB)
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB)
    }
}

fn list_window(len: usize, selected: usize, height: usize) -> (usize, usize) {
    if len == 0 || height == 0 {
        return (0, 0);
    }
    let height = height.min(len);
    let start = selected
        .saturating_sub(height / 2)
        .min(len.saturating_sub(height));
    (start, start + height)
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use crate::app::App;

    use super::{list_window, pie, short_id};

    #[test]
    fn pie_indicator_uses_expected_ranges() {
        assert_eq!(pie(0.0), "○");
        assert_eq!(pie(25.0), "◔");
        assert_eq!(pie(50.0), "◑");
        assert_eq!(pie(75.0), "◕");
        assert_eq!(pie(100.0), "●");
    }

    #[test]
    fn long_identifiers_are_not_fully_exposed() {
        assert_eq!(short_id("steam_12345678901234567"), "steam_12…4567");
    }

    #[test]
    fn list_window_follows_selection() {
        assert_eq!(list_window(100, 50, 10), (45, 55));
        assert_eq!(list_window(3, 0, 10), (0, 3));
    }

    #[test]
    fn every_tab_renders_in_a_standard_terminal() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(170, 50);
        let mut terminal = Terminal::new(backend)?;
        let mut app = App::new(true, false, None);
        for number in '1'..='8' {
            app.select_tab(number);
            terminal.draw(|frame| super::draw(frame, &app))?;
        }
        Ok(())
    }
}
