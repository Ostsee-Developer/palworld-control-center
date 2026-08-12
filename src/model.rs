use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize)]
pub struct DashboardData {
    pub server: ServerSnapshot,
    pub players: Vec<Player>,
    pub settings: Vec<Setting>,
    pub mods: Vec<ModEntry>,
    pub backups: Vec<BackupEntry>,
    pub updates: UpdateSnapshot,
    pub logs: Vec<String>,
    pub security: Vec<SecurityCheck>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ServerSnapshot {
    pub service_active: bool,
    pub api_connected: bool,
    pub service: String,
    pub connection_detail: String,
    pub server_name: Option<String>,
    pub palworld_version: Option<String>,
    pub installed_build: Option<String>,
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
    pub server_fps: Option<u32>,
    pub frame_time_ms: Option<f64>,
    pub uptime_seconds: Option<u64>,
    pub client_mods: Option<bool>,
    pub exp_rate: Option<f64>,
    pub pvp: Option<bool>,
}

impl Default for ServerSnapshot {
    fn default() -> Self {
        Self {
            service_active: false,
            api_connected: false,
            service: "NICHT VERBUNDEN".to_owned(),
            connection_detail: "Palworld-Installation wird gesucht".to_owned(),
            server_name: None,
            palworld_version: None,
            installed_build: None,
            players_online: None,
            players_max: None,
            server_fps: None,
            frame_time_ms: None,
            uptime_seconds: None,
            client_mods: None,
            exp_rate: None,
            pvp: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Player {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "accountName")]
    pub account_name: String,
    #[serde(default, rename = "playerId")]
    pub player_id: String,
    #[serde(default, rename = "userId")]
    pub user_id: String,
    #[serde(default)]
    pub ping: f64,
    #[serde(default)]
    pub level: u32,
    #[serde(default)]
    pub building_count: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Setting {
    pub key: String,
    pub label: String,
    pub category: String,
    pub kind: String,
    pub description: String,
    pub value: String,
    pub secret: bool,
    pub protected: bool,
    pub managed: Option<String>,
    pub options: Vec<SettingOption>,
}

impl Setting {
    pub fn display_value(&self) -> &str {
        if self.secret && !self.value.is_empty() {
            "••••••••"
        } else {
            &self.value
        }
    }

    pub fn is_editable(&self) -> bool {
        !self.secret && !self.protected && self.managed.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SettingOption {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModEntry {
    pub name: String,
    pub kind: String,
    pub version: Option<String>,
    pub enabled: bool,
    pub partial: bool,
    pub compatible: Option<bool>,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BackupEntry {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
    pub modified_unix: u64,
    pub checksum_present: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct UpdateSnapshot {
    pub installed_build: Option<String>,
    pub next_check: Option<String>,
    pub last_result: Option<String>,
    pub changelog: Vec<ChangelogEntry>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChangelogEntry {
    pub title: String,
    pub url: String,
    pub published_unix: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Clone, Debug, Serialize)]
pub struct SecurityCheck {
    pub label: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[cfg(test)]
mod tests {
    use super::Setting;

    #[test]
    fn secret_setting_is_always_redacted() {
        let setting = Setting {
            key: "AdminPassword".to_owned(),
            label: "Admin".to_owned(),
            category: "Server".to_owned(),
            kind: "string".to_owned(),
            description: String::new(),
            value: "do-not-render".to_owned(),
            secret: true,
            protected: false,
            managed: Some("identity".to_owned()),
            options: Vec::new(),
        };
        assert_eq!(setting.display_value(), "••••••••");
        assert!(!setting.is_editable());
    }
}
