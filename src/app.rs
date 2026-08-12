use std::{collections::VecDeque, process::Command};

use crate::metrics::SystemMetrics;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    Overview,
    Players,
    Settings,
    Mods,
    Backups,
    Updates,
    Logs,
    Security,
}

impl Tab {
    pub const ALL: [Self; 8] = [
        Self::Overview,
        Self::Players,
        Self::Settings,
        Self::Mods,
        Self::Backups,
        Self::Updates,
        Self::Logs,
        Self::Security,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Overview => "01 OVERVIEW",
            Self::Players => "02 PLAYERS",
            Self::Settings => "03 SETTINGS",
            Self::Mods => "04 MODS",
            Self::Backups => "05 BACKUPS",
            Self::Updates => "06 UPDATES",
            Self::Logs => "07 LOGS",
            Self::Security => "08 SECURITY",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Overview => "OPERATIONS OVERVIEW",
            Self::Players => "PLAYER CONTROL",
            Self::Settings => "GAME SETTINGS",
            Self::Mods => "MOD INVENTORY",
            Self::Backups => "BACKUP & RESTORE",
            Self::Updates => "SERVER UPDATES & CHANGELOG",
            Self::Logs => "LOG EXPLORER",
            Self::Security => "SECURITY & AUDIT",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServerSnapshot {
    pub connected: bool,
    pub service: &'static str,
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
    pub palworld_version: Option<String>,
    pub client_mods: Option<bool>,
    pub exp_rate: Option<f32>,
    pub pvp: Option<bool>,
    pub backup_progress: Option<u16>,
    pub backup_label: &'static str,
}

impl ServerSnapshot {
    fn disconnected() -> Self {
        Self {
            connected: false,
            service: "NICHT VERBUNDEN",
            players_online: None,
            players_max: None,
            palworld_version: None,
            client_mods: None,
            exp_rate: None,
            pvp: None,
            backup_progress: None,
            backup_label: "Kein aktiver Backup-Job",
        }
    }

    fn demo() -> Self {
        Self {
            connected: true,
            service: "ONLINE",
            players_online: Some(7),
            players_max: Some(32),
            palworld_version: Some("v1.0.1 / build 20260812".to_owned()),
            client_mods: Some(true),
            exp_rate: Some(1.15),
            pvp: Some(false),
            backup_progress: Some(72),
            backup_label: "world-20260812-150401.tar.zst",
        }
    }
}

pub struct App {
    pub selected_tab: usize,
    pub should_quit: bool,
    pub demo: bool,
    pub metrics: SystemMetrics,
    pub server: ServerSnapshot,
    pub logs: VecDeque<String>,
    pub ticks: u64,
}

impl App {
    pub fn new(demo: bool) -> Self {
        Self {
            selected_tab: 0,
            should_quit: false,
            demo,
            metrics: SystemMetrics::new(demo),
            server: if demo {
                ServerSnapshot::demo()
            } else {
                ServerSnapshot::disconnected()
            },
            logs: VecDeque::new(),
            ticks: 0,
        }
    }

    pub fn current_tab(&self) -> Tab {
        Tab::ALL[self.selected_tab]
    }

    pub fn next_tab(&mut self) {
        self.selected_tab = (self.selected_tab + 1) % Tab::ALL.len();
    }

    pub fn previous_tab(&mut self) {
        self.selected_tab = self
            .selected_tab
            .checked_sub(1)
            .unwrap_or(Tab::ALL.len() - 1);
    }

    pub fn select_tab(&mut self, number: char) {
        if let Some(index) = number.to_digit(10).and_then(|value| value.checked_sub(1)) {
            let index = index as usize;
            if index < Tab::ALL.len() {
                self.selected_tab = index;
            }
        }
    }

    pub fn refresh(&mut self) {
        self.ticks = self.ticks.saturating_add(1);
        self.metrics.refresh();
        if self.demo {
            self.load_demo_logs();
        } else if self.ticks == 1 || self.ticks.is_multiple_of(3) {
            self.load_journal();
        }
    }

    fn load_demo_logs(&mut self) {
        const DEMO: [&str; 8] = [
            "15:04:01 INFO  palworld.service ist aktiv",
            "15:04:02 INFO  REST-API auf 127.0.0.1:8212 erreichbar",
            "15:04:04 INFO  7/32 Spieler online · 60 FPS",
            "15:04:08 INFO  CreativeMenu.pak dateiseitig erkannt",
            "15:04:10 INFO  Welt-Speicherung über REST ausgelöst",
            "15:04:13 JOB   Backup: Metadaten werden erfasst",
            "15:04:17 JOB   Backup: 72 % · 1.8 GiB / 2.5 GiB",
            "15:04:19 INFO  Nächste Update-Prüfung in 2h 41m",
        ];
        self.logs = DEMO.into_iter().map(str::to_owned).collect();
    }

    fn load_journal(&mut self) {
        let output = Command::new("journalctl")
            .args([
                "--unit=palworld.service",
                "--lines=14",
                "--no-pager",
                "--output=short-iso",
            ])
            .output();

        self.logs = match output {
            Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::to_owned)
                .collect(),
            Ok(output) => VecDeque::from([format!(
                "journalctl nicht verfügbar: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )]),
            Err(error) => {
                VecDeque::from([format!("journalctl konnte nicht gestartet werden: {error}")])
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{App, Tab};

    #[test]
    fn tab_navigation_wraps_in_both_directions() {
        let mut app = App::new(true);
        app.previous_tab();
        assert_eq!(app.current_tab(), Tab::Security);
        app.next_tab();
        assert_eq!(app.current_tab(), Tab::Overview);
    }

    #[test]
    fn numeric_shortcut_selects_tab() {
        let mut app = App::new(true);
        app.select_tab('4');
        assert_eq!(app.current_tab(), Tab::Mods);
    }
}
