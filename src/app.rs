use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    backend::{Backend, BackendConfig, demo_data},
    jobs::{self, Action, JobEvent, JobHandle, ServiceAction},
    metrics::SystemMetrics,
    model::{BackupEntry, DashboardData, ModEntry, Player, Setting},
};

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogFilter {
    All,
    Warnings,
    Errors,
}

impl LogFilter {
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "ALLE",
            Self::Warnings => "WARN+ERROR",
            Self::Errors => "NUR ERROR",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::All => Self::Warnings,
            Self::Warnings => Self::Errors,
            Self::Errors => Self::All,
        }
    }
}

#[derive(Clone, Debug)]
pub enum TextSubmit {
    SettingsSearch,
    LogSearch,
    EditSetting { key: String },
    Broadcast,
    KickMessage { user_id: String },
    BanMessage { user_id: String },
    Unban,
    ImportPak,
}

#[derive(Clone, Debug)]
pub enum Overlay {
    Text {
        title: String,
        prompt: String,
        value: String,
        submit: TextSubmit,
    },
    Confirm {
        title: String,
        prompt: String,
        action: Action,
    },
    Message {
        title: String,
        body: String,
        error: bool,
    },
}

#[derive(Clone, Debug)]
pub struct JobView {
    pub label: String,
    pub detail: String,
    pub running: bool,
    pub success: Option<bool>,
}

pub struct App {
    pub selected_tab: usize,
    pub should_quit: bool,
    pub demo: bool,
    pub writes_enabled: bool,
    pub metrics: SystemMetrics,
    pub data: DashboardData,
    pub selection: [usize; 8],
    pub settings_search: String,
    pub log_search: String,
    pub log_filter: LogFilter,
    pub overlay: Option<Overlay>,
    pub job_view: Option<JobView>,
    pub ticks: u64,
    backend: Backend,
    job: Option<JobHandle>,
    force_slow_refresh: bool,
}

impl App {
    pub fn new(demo: bool, writes_enabled: bool, env_file: Option<PathBuf>) -> Self {
        let config = BackendConfig::discover(env_file);
        let data = if demo {
            demo_data()
        } else {
            DashboardData::default()
        };
        Self {
            selected_tab: 0,
            should_quit: false,
            demo,
            writes_enabled: writes_enabled && !demo,
            metrics: SystemMetrics::new(demo),
            data,
            selection: [0; 8],
            settings_search: String::new(),
            log_search: String::new(),
            log_filter: LogFilter::All,
            overlay: None,
            job_view: None,
            ticks: 0,
            backend: Backend::new(config),
            job: None,
            force_slow_refresh: true,
        }
    }

    pub fn current_tab(&self) -> Tab {
        Tab::ALL[self.selected_tab]
    }

    pub fn selected_index(&self) -> usize {
        self.selection[self.selected_tab]
    }

    pub fn next_tab(&mut self) {
        self.selected_tab = (self.selected_tab + 1) % Tab::ALL.len();
        self.clamp_selection();
    }

    pub fn previous_tab(&mut self) {
        self.selected_tab = self
            .selected_tab
            .checked_sub(1)
            .unwrap_or(Tab::ALL.len() - 1);
        self.clamp_selection();
    }

    pub fn select_tab(&mut self, number: char) {
        if let Some(index) = number.to_digit(10).and_then(|value| value.checked_sub(1)) {
            let index = index as usize;
            if index < Tab::ALL.len() {
                self.selected_tab = index;
                self.clamp_selection();
            }
        }
    }

    pub fn refresh(&mut self) {
        self.ticks = self.ticks.saturating_add(1);
        self.metrics.refresh();
        self.poll_job();
        if self.demo {
            return;
        }
        if self.ticks == 1 || self.ticks.is_multiple_of(3) {
            self.backend.refresh_fast(&mut self.data);
        }
        if self.force_slow_refresh || self.ticks == 1 || self.ticks.is_multiple_of(60) {
            self.backend
                .refresh_slow(&mut self.data, self.writes_enabled);
            self.force_slow_refresh = false;
        }
        self.clamp_selection();
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.handle_overlay_key(key) {
            return;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Char('q'), _) => {
                self.should_quit = true;
            }
            (KeyCode::Left | KeyCode::Char('h'), _) => self.previous_tab(),
            (KeyCode::Right | KeyCode::Char('l'), _) => self.next_tab(),
            (KeyCode::Up | KeyCode::Char('k'), _) => self.move_selection(-1),
            (KeyCode::Down | KeyCode::Char('j'), _) => self.move_selection(1),
            (KeyCode::Char('r'), _) => {
                self.force_slow_refresh = true;
                self.refresh();
            }
            (KeyCode::Char(number @ '1'..='8'), _) => self.select_tab(number),
            _ => self.handle_tab_key(key),
        }
    }

    pub fn visible_settings(&self) -> Vec<&Setting> {
        let search = self.settings_search.to_ascii_lowercase();
        self.data
            .settings
            .iter()
            .filter(|setting| {
                search.is_empty()
                    || setting.key.to_ascii_lowercase().contains(&search)
                    || setting.label.to_ascii_lowercase().contains(&search)
                    || setting.category.to_ascii_lowercase().contains(&search)
            })
            .collect()
    }

    pub fn visible_logs(&self) -> Vec<&str> {
        let search = self.log_search.to_ascii_lowercase();
        self.data
            .logs
            .iter()
            .filter(|line| {
                let upper = line.to_ascii_uppercase();
                let level_matches = match self.log_filter {
                    LogFilter::All => true,
                    LogFilter::Warnings => {
                        upper.contains("WARN")
                            || upper.contains("ERROR")
                            || upper.contains("FEHLER")
                    }
                    LogFilter::Errors => upper.contains("ERROR") || upper.contains("FEHLER"),
                };
                level_matches && (search.is_empty() || line.to_ascii_lowercase().contains(&search))
            })
            .map(String::as_str)
            .collect()
    }

    pub fn footer_help(&self) -> &'static str {
        match self.current_tab() {
            Tab::Overview => "S Save · B Backup · O Start/Stop · X Restart · U Update",
            Tab::Players => "A Broadcast · X Kick · B Ban · U Unban",
            Tab::Settings => "/ Suche · Enter Bearbeiten · Space Umschalten · X Restart",
            Tab::Mods => "I Import · Space Aktivieren · D Quarantäne · X Restart",
            Tab::Backups => "N Neu · V Prüfen · R Restore · D Löschen",
            Tab::Updates => "C Changelog laden · U Update · X Restart",
            Tab::Logs => "/ Suche · F Level-Filter · R Refresh",
            Tab::Security => "D Diagnosepaket · R Neu prüfen",
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) -> bool {
        let Some(mut overlay) = self.overlay.take() else {
            return false;
        };
        match &mut overlay {
            Overlay::Text { value, submit, .. } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let value = value.trim().to_owned();
                    let submit = submit.clone();
                    self.submit_text(submit, value);
                }
                KeyCode::Backspace => {
                    value.pop();
                    self.overlay = Some(overlay);
                }
                KeyCode::Char(character)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    if value.chars().count() < 512 {
                        value.push(character);
                    }
                    self.overlay = Some(overlay);
                }
                _ => self.overlay = Some(overlay),
            },
            Overlay::Confirm { action, .. } => match key.code {
                KeyCode::Enter | KeyCode::Char('y' | 'j') => {
                    self.start_action(action.clone());
                }
                KeyCode::Esc | KeyCode::Char('n') => {}
                _ => self.overlay = Some(overlay),
            },
            Overlay::Message { .. } => match key.code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => {}
                _ => self.overlay = Some(overlay),
            },
        }
        true
    }

    fn handle_tab_key(&mut self, key: KeyEvent) {
        match self.current_tab() {
            Tab::Overview => match key.code {
                KeyCode::Char('s') => self.confirm(
                    "Welt speichern",
                    "Aktuellen Weltstand jetzt über die lokale REST-API speichern?",
                    Action::SaveWorld,
                ),
                KeyCode::Char('b') => self.confirm(
                    "Backup erstellen",
                    "Verifiziertes, komprimiertes Welt-Backup jetzt erstellen?",
                    Action::CreateBackup,
                ),
                KeyCode::Char('o') => {
                    let action = if self.data.server.service_active {
                        ServiceAction::Stop
                    } else {
                        ServiceAction::Start
                    };
                    self.confirm(
                        "Serverdienst",
                        if action == ServiceAction::Stop {
                            "Welt speichern und Server sicher stoppen?"
                        } else {
                            "Palworld-Server jetzt starten?"
                        },
                        Action::Service(action),
                    );
                }
                KeyCode::Char('x') => self.confirm_restart(),
                KeyCode::Char('u') => self.confirm_update(),
                _ => {}
            },
            Tab::Players => match key.code {
                KeyCode::Char('a') => self.text(
                    "Servernachricht",
                    "Nachricht an alle Online-Spieler:",
                    String::new(),
                    TextSubmit::Broadcast,
                ),
                KeyCode::Char('x') => {
                    if let Some(player) = self.selected_player().cloned() {
                        self.text(
                            "Spieler kicken",
                            &format!("Nachricht an {}:", player.name),
                            "Entfernt durch Serververwaltung".to_owned(),
                            TextSubmit::KickMessage {
                                user_id: player.user_id,
                            },
                        );
                    }
                }
                KeyCode::Char('b') => {
                    if let Some(player) = self.selected_player().cloned() {
                        self.text(
                            "Spieler bannen",
                            &format!("Begründung für {}:", player.name),
                            "Gesperrt durch Serververwaltung".to_owned(),
                            TextSubmit::BanMessage {
                                user_id: player.user_id,
                            },
                        );
                    }
                }
                KeyCode::Char('u') => self.text(
                    "Spieler entbannen",
                    "Vollständige Palworld-User-ID:",
                    String::new(),
                    TextSubmit::Unban,
                ),
                _ => {}
            },
            Tab::Settings => match key.code {
                KeyCode::Char('/') => self.text(
                    "Settings durchsuchen",
                    "Name, Schlüssel oder Kategorie:",
                    self.settings_search.clone(),
                    TextSubmit::SettingsSearch,
                ),
                KeyCode::Enter => self.edit_selected_setting(),
                KeyCode::Char(' ') => self.toggle_selected_setting(),
                KeyCode::Char('x') => self.confirm_restart(),
                _ => {}
            },
            Tab::Mods => match key.code {
                KeyCode::Char('i') => self.text(
                    "PAK importieren",
                    "Absoluter Pfad zu .pak/.utoc/.ucas oder Paketordner:",
                    String::new(),
                    TextSubmit::ImportPak,
                ),
                KeyCode::Char(' ') => self.toggle_selected_mod(),
                KeyCode::Char('d') => self.quarantine_selected_mod(),
                KeyCode::Char('x') => self.confirm_restart(),
                _ => {}
            },
            Tab::Backups => match key.code {
                KeyCode::Char('n') => self.confirm(
                    "Backup erstellen",
                    "Neues Welt-Backup mit SHA-256-Prüfsumme erstellen?",
                    Action::CreateBackup,
                ),
                KeyCode::Char('v') => {
                    if let Some(backup) = self.selected_backup() {
                        self.start_action(Action::VerifyBackup(backup.path.clone()));
                    }
                }
                KeyCode::Char('r') => {
                    if let Some(backup) = self.selected_backup() {
                        self.confirm(
                            "Restore bestätigen",
                            &format!(
                                "{} als Welt-only wiederherstellen? Vorher entsteht automatisch ein Pre-Restore-Backup.",
                                backup.name
                            ),
                            Action::RestoreBackup(backup.path.clone()),
                        );
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(backup) = self.selected_backup() {
                        self.confirm(
                            "Backup löschen",
                            &format!("{} samt Prüfsumme endgültig löschen?", backup.name),
                            Action::DeleteBackup(backup.path.clone()),
                        );
                    }
                }
                _ => {}
            },
            Tab::Updates => match key.code {
                KeyCode::Char('u') => self.confirm_update(),
                KeyCode::Char('x') => self.confirm_restart(),
                KeyCode::Char('c') => {
                    self.backend.force_news_refresh();
                    self.force_slow_refresh = true;
                    self.refresh();
                }
                _ => {}
            },
            Tab::Logs => match key.code {
                KeyCode::Char('/') => self.text(
                    "Logs durchsuchen",
                    "Suchtext (leer = kein Filter):",
                    self.log_search.clone(),
                    TextSubmit::LogSearch,
                ),
                KeyCode::Char('f') => self.log_filter = self.log_filter.next(),
                _ => {}
            },
            Tab::Security => {
                if key.code == KeyCode::Char('d') {
                    self.start_action(Action::Diagnostics);
                }
            }
        }
    }

    fn submit_text(&mut self, submit: TextSubmit, value: String) {
        match submit {
            TextSubmit::SettingsSearch => {
                self.settings_search = value;
                self.selection[Tab::Settings as usize] = 0;
            }
            TextSubmit::LogSearch => {
                self.log_search = value;
                self.selection[Tab::Logs as usize] = 0;
            }
            TextSubmit::EditSetting { key } => {
                if !value.is_empty() {
                    self.confirm(
                        "Einstellung übernehmen",
                        &format!("{key} auf „{value}“ setzen? Wirksam nach einem Neustart."),
                        Action::SetSetting { key, input: value },
                    );
                }
            }
            TextSubmit::Broadcast => {
                if !value.is_empty() {
                    self.confirm(
                        "Nachricht senden",
                        &format!("Nachricht an alle Spieler senden?\n\n{value}"),
                        Action::Broadcast(value),
                    );
                }
            }
            TextSubmit::KickMessage { user_id } => self.confirm(
                "Kick bestätigen",
                "Ausgewählten Spieler jetzt vom Server entfernen?",
                Action::Kick {
                    user_id,
                    message: value,
                },
            ),
            TextSubmit::BanMessage { user_id } => self.confirm(
                "Ban bestätigen",
                "Ausgewählten Spieler sperren und entfernen?",
                Action::Ban {
                    user_id,
                    message: value,
                },
            ),
            TextSubmit::Unban => {
                if !value.is_empty() {
                    self.confirm(
                        "Unban bestätigen",
                        "Diese User-ID wieder zulassen?",
                        Action::Unban(value),
                    );
                }
            }
            TextSubmit::ImportPak => {
                if !value.is_empty() {
                    self.confirm(
                        "Experimentellen PAK-Import bestätigen",
                        "PAK-Mods können Abstürze oder Save-Probleme verursachen. Der AIO-Manager prüft Dateitypen und überschreibt keine fremden Dateien. Fortfahren?",
                        Action::ImportPak(PathBuf::from(value)),
                    );
                }
            }
        }
    }

    fn edit_selected_setting(&mut self) {
        let Some(setting) = self.selected_setting().cloned() else {
            return;
        };
        if !setting.is_editable() {
            self.message(
                "Geschützte Einstellung",
                "Dieser Wert ist geheim, betriebsrelevant oder wird mit der AIO-Betriebsdatei synchronisiert. Er folgt später der eigenen Admin-Anmeldung.",
                false,
            );
            return;
        }
        if setting.kind == "bool" || !setting.options.is_empty() {
            self.toggle_selected_setting();
            return;
        }
        self.text(
            &setting.label,
            &setting.description,
            setting.value,
            TextSubmit::EditSetting { key: setting.key },
        );
    }

    fn toggle_selected_setting(&mut self) {
        let Some(setting) = self.selected_setting().cloned() else {
            return;
        };
        if !setting.is_editable() {
            self.message(
                "Nur lesbar",
                "Diese Einstellung wird später über die Admin-Anmeldung oder einen spezialisierten sicheren Dialog geändert.",
                false,
            );
            return;
        }
        let next = if setting.kind == "bool" {
            if setting.value.eq_ignore_ascii_case("true") {
                "false".to_owned()
            } else {
                "true".to_owned()
            }
        } else if !setting.options.is_empty() {
            let current = setting
                .options
                .iter()
                .position(|option| option.value == setting.value)
                .unwrap_or(0);
            setting.options[(current + 1) % setting.options.len()]
                .value
                .clone()
        } else {
            return;
        };
        self.confirm(
            "Einstellung umschalten",
            &format!("{} auf {} setzen?", setting.label, next),
            Action::SetSetting {
                key: setting.key,
                input: next,
            },
        );
    }

    fn toggle_selected_mod(&mut self) {
        let Some(entry) = self.selected_mod().cloned() else {
            return;
        };
        if entry.kind != "PAK" {
            self.message(
                "Nicht verwaltet",
                "Direkt platzierte oder offizielle Workshop-Dateien werden in dieser Ansicht nicht blind verändert.",
                false,
            );
            return;
        }
        self.confirm(
            "PAK-Mod umschalten",
            &format!(
                "{} {}? Die Änderung greift nach einem Neustart.",
                entry.name,
                if entry.enabled {
                    "deaktivieren"
                } else {
                    "aktivieren"
                }
            ),
            Action::TogglePak {
                name: entry.name,
                enable: !entry.enabled,
            },
        );
    }

    fn quarantine_selected_mod(&mut self) {
        let Some(entry) = self.selected_mod().cloned() else {
            return;
        };
        if entry.kind != "PAK" {
            self.message(
                "Nicht verwaltet",
                "Nur sicher verwaltete PAK-Pakete können in Quarantäne verschoben werden.",
                false,
            );
            return;
        }
        self.confirm(
            "PAK in Quarantäne",
            &format!(
                "{} deaktivieren und wiederherstellbar in Quarantäne verschieben?",
                entry.name
            ),
            Action::QuarantinePak(entry.name),
        );
    }

    fn confirm_restart(&mut self) {
        self.confirm(
            "Server neu starten",
            "Welt speichern und Palworld sicher neu starten?",
            Action::Service(ServiceAction::Restart),
        );
    }

    fn confirm_update(&mut self) {
        self.confirm(
            "Palworld aktualisieren",
            "Vor dem Update wird ein Backup erstellt. Online-Spieler werden nach AIO-Richtlinie gewarnt. Update jetzt erzwingen?",
            Action::UpdateServer,
        );
    }

    fn confirm(&mut self, title: &str, prompt: &str, action: Action) {
        self.overlay = Some(Overlay::Confirm {
            title: title.to_owned(),
            prompt: prompt.to_owned(),
            action,
        });
    }

    fn text(&mut self, title: &str, prompt: &str, value: String, submit: TextSubmit) {
        self.overlay = Some(Overlay::Text {
            title: title.to_owned(),
            prompt: prompt.to_owned(),
            value,
            submit,
        });
    }

    fn message(&mut self, title: &str, body: &str, error: bool) {
        self.overlay = Some(Overlay::Message {
            title: title.to_owned(),
            body: body.to_owned(),
            error,
        });
    }

    fn start_action(&mut self, action: Action) {
        if self.job.is_some() {
            self.message(
                "Job läuft",
                "Es kann immer nur eine schreibende oder prüfende Aktion gleichzeitig laufen.",
                true,
            );
            return;
        }
        if action.requires_write() && !self.writes_enabled {
            self.message(
                "Schreibzugriff gesperrt",
                "Starte das Control Center bewusst mit --enable-writes. Beim späteren TTY1-Betrieb ersetzt eine eigene Admin-Anmeldung diesen temporären Schalter.",
                true,
            );
            return;
        }
        let handle = jobs::spawn(action, self.backend.config.clone());
        self.job_view = Some(JobView {
            label: handle.label.clone(),
            detail: "Aktion läuft im Hintergrund · Oberfläche bleibt live".to_owned(),
            running: true,
            success: None,
        });
        self.job = Some(handle);
    }

    fn poll_job(&mut self) {
        let event = self.job.as_ref().and_then(JobHandle::poll);
        let Some(JobEvent::Finished { success, detail }) = event else {
            return;
        };
        if let Some(view) = &mut self.job_view {
            view.detail = detail.clone();
            view.running = false;
            view.success = Some(success);
        }
        self.job = None;
        self.force_slow_refresh = true;
        self.message(
            if success {
                "Aktion abgeschlossen"
            } else {
                "Aktion fehlgeschlagen"
            },
            &detail,
            !success,
        );
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.current_item_count();
        if count == 0 {
            self.selection[self.selected_tab] = 0;
            return;
        }
        let current = self.selection[self.selected_tab];
        self.selection[self.selected_tab] = if delta < 0 {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current.saturating_add(delta as usize).min(count - 1)
        };
    }

    fn clamp_selection(&mut self) {
        let count = self.current_item_count();
        self.selection[self.selected_tab] = if count == 0 {
            0
        } else {
            self.selection[self.selected_tab].min(count - 1)
        };
    }

    fn current_item_count(&self) -> usize {
        match self.current_tab() {
            Tab::Players => self.data.players.len(),
            Tab::Settings => self.visible_settings().len(),
            Tab::Mods => self.data.mods.len(),
            Tab::Backups => self.data.backups.len(),
            Tab::Updates => self.data.updates.changelog.len(),
            Tab::Logs => self.visible_logs().len(),
            Tab::Overview | Tab::Security => 0,
        }
    }

    fn selected_player(&self) -> Option<&Player> {
        self.data.players.get(self.selection[Tab::Players as usize])
    }

    fn selected_setting(&self) -> Option<&Setting> {
        self.visible_settings()
            .get(self.selection[Tab::Settings as usize])
            .copied()
    }

    fn selected_mod(&self) -> Option<&ModEntry> {
        self.data.mods.get(self.selection[Tab::Mods as usize])
    }

    fn selected_backup(&self) -> Option<&BackupEntry> {
        self.data.backups.get(self.selection[Tab::Backups as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::{App, LogFilter, Tab};

    #[test]
    fn tab_navigation_wraps_in_both_directions() {
        let mut app = App::new(true, false, None);
        app.previous_tab();
        assert_eq!(app.current_tab(), Tab::Security);
        app.next_tab();
        assert_eq!(app.current_tab(), Tab::Overview);
    }

    #[test]
    fn numeric_shortcut_selects_tab() {
        let mut app = App::new(true, false, None);
        app.select_tab('4');
        assert_eq!(app.current_tab(), Tab::Mods);
    }

    #[test]
    fn log_filter_cycles() {
        assert_eq!(LogFilter::All.next(), LogFilter::Warnings);
        assert_eq!(LogFilter::Warnings.next(), LogFilter::Errors);
        assert_eq!(LogFilter::Errors.next(), LogFilter::All);
    }
}
