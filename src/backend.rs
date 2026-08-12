use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use serde_json::Value;

use crate::model::{
    BackupEntry, ChangelogEntry, CheckStatus, DashboardData, ModEntry, Player, SecurityCheck,
    ServerSnapshot, Setting, SettingOption, UpdateSnapshot,
};

const SERVICE: &str = "palworld.service";
const DEFAULT_ENV_FILE: &str = "/etc/palworld/palworld.env";
const OFFICIAL_NEWS_URL: &str = "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid=1623730&count=4&maxlength=0&format=json";

#[derive(Clone, Debug)]
pub struct BackendConfig {
    pub env_file: PathBuf,
    pub config_found: bool,
    pub mode: String,
    pub service_user: String,
    pub service_group: String,
    pub base_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub rest_port: u16,
    pub rest_netrc: PathBuf,
    pub server_config: PathBuf,
    pub config_tool: PathBuf,
    pub settings_catalog_tool: PathBuf,
    pub pak_tool: PathBuf,
    pub pak_store: PathBuf,
    pub pak_target: PathBuf,
    pub pak_quarantine: PathBuf,
    pub mod_inspect_tool: PathBuf,
    pub mod_config_tool: PathBuf,
    pub workshop_dir: PathBuf,
    pub mod_settings_file: PathBuf,
    pub backup_tool: PathBuf,
    pub restore_tool: PathBuf,
    pub update_tool: PathBuf,
    pub diagnostics_tool: PathBuf,
}

impl BackendConfig {
    pub fn discover(env_file: Option<PathBuf>) -> Self {
        let env_file = env_file.unwrap_or_else(|| PathBuf::from(DEFAULT_ENV_FILE));
        let values = read_env_file(&env_file).unwrap_or_default();
        let config_found = !values.is_empty();
        let mode = env_value(&values, "MODE", "native");
        let base_dir = PathBuf::from(env_value(&values, "BASE_DIR", "/opt/palworld"));
        let server_dir = PathBuf::from(env_value(
            &values,
            "SERVER_DIR",
            base_dir.join("server").to_string_lossy().as_ref(),
        ));
        let backup_dir = PathBuf::from(env_value(&values, "BACKUP_DIR", "/var/backups/palworld"));
        let runtime_dir =
            PathBuf::from(env_value(&values, "RUNTIME_DIR", "/usr/local/lib/palworld"));
        let platform = if mode == "wine" {
            "WindowsServer"
        } else {
            "LinuxServer"
        };
        let server_config_default = server_dir
            .join("Pal/Saved/Config")
            .join(platform)
            .join("PalWorldSettings.ini");

        Self {
            env_file,
            config_found,
            mode,
            service_user: env_value(&values, "SERVICE_USER", "palworld"),
            service_group: env_value(&values, "SERVICE_GROUP", "palworld"),
            base_dir: base_dir.clone(),
            backup_dir,
            rest_port: env_value(&values, "REST_PORT", "8212")
                .parse()
                .unwrap_or(8212),
            rest_netrc: PathBuf::from(env_value(&values, "REST_NETRC", "/etc/palworld/rest.netrc")),
            server_config: PathBuf::from(env_value(
                &values,
                "SERVER_CONFIG_FILE",
                server_config_default.to_string_lossy().as_ref(),
            )),
            config_tool: tool_path(&values, "CONFIG_TOOL", &runtime_dir, "palworld_config.py"),
            settings_catalog_tool: tool_path(
                &values,
                "SETTINGS_CATALOG_TOOL",
                &runtime_dir,
                "settings_catalog.py",
            ),
            pak_tool: tool_path(&values, "PAK_MOD_TOOL", &runtime_dir, "pak_mod_manager.py"),
            pak_store: PathBuf::from(env_value(
                &values,
                "PAK_MOD_STORE_DIR",
                base_dir
                    .join("pak-mods/packages")
                    .to_string_lossy()
                    .as_ref(),
            )),
            pak_target: PathBuf::from(env_value(
                &values,
                "PAK_MOD_TARGET_DIR",
                server_dir
                    .join("Pal/Content/Paks/~mods")
                    .to_string_lossy()
                    .as_ref(),
            )),
            pak_quarantine: PathBuf::from(env_value(
                &values,
                "PAK_MOD_QUARANTINE_DIR",
                base_dir
                    .join("pak-mod-quarantine")
                    .to_string_lossy()
                    .as_ref(),
            )),
            mod_inspect_tool: tool_path(
                &values,
                "MOD_INSPECT_TOOL",
                &runtime_dir,
                "mod_inspect.py",
            ),
            mod_config_tool: tool_path(
                &values,
                "MOD_CONFIG_TOOL",
                &runtime_dir,
                "palworld_mod_config.py",
            ),
            workshop_dir: PathBuf::from(env_value(
                &values,
                "WORKSHOP_DIR",
                server_dir.join("Mods/Workshop").to_string_lossy().as_ref(),
            )),
            mod_settings_file: PathBuf::from(env_value(
                &values,
                "MOD_SETTINGS_FILE",
                server_dir
                    .join("Mods/PalModSettings.ini")
                    .to_string_lossy()
                    .as_ref(),
            )),
            backup_tool: runtime_dir.join("backup.sh"),
            restore_tool: runtime_dir.join("restore.sh"),
            update_tool: runtime_dir.join("update.sh"),
            diagnostics_tool: runtime_dir.join("diagnostics.sh"),
        }
    }

    pub fn rest_url(&self, endpoint: &str) -> String {
        format!("http://127.0.0.1:{}/v1/api/{endpoint}", self.rest_port)
    }
}

pub struct Backend {
    pub config: BackendConfig,
    cached_news: Vec<ChangelogEntry>,
    last_news_refresh: Option<Instant>,
}

impl Backend {
    pub fn new(config: BackendConfig) -> Self {
        Self {
            config,
            cached_news: Vec::new(),
            last_news_refresh: None,
        }
    }

    pub fn refresh_fast(&self, data: &mut DashboardData) {
        data.server = self.load_server();
        data.players = self.load_players();
        data.logs = self.load_logs(180);
    }

    pub fn force_news_refresh(&mut self) {
        self.last_news_refresh = None;
    }

    pub fn refresh_slow(&mut self, data: &mut DashboardData, writes_enabled: bool) {
        data.settings = self.load_settings();
        data.mods = self.load_mods();
        data.backups = self.load_backups();
        data.updates = self.load_updates();
        data.security = self.load_security(writes_enabled);
        if self
            .last_news_refresh
            .is_none_or(|instant| instant.elapsed() >= Duration::from_secs(900))
        {
            if let Some(news) = load_official_news() {
                self.cached_news = news;
            }
            self.last_news_refresh = Some(Instant::now());
        }
        data.updates.changelog.clone_from(&self.cached_news);
    }

    fn load_server(&self) -> ServerSnapshot {
        let service_active = command_success("systemctl", &["is-active", "--quiet", SERVICE]);
        let installed_build = find_installed_build(&self.config.base_dir);
        let mut snapshot = ServerSnapshot {
            service_active,
            service: if service_active {
                "SERVER ONLINE".to_owned()
            } else if self.config.config_found {
                "SERVER OFFLINE".to_owned()
            } else {
                "NICHT VERBUNDEN".to_owned()
            },
            connection_detail: if self.config.config_found {
                format!(
                    "AIO-Konfiguration erkannt · {} · REST 127.0.0.1:{}",
                    self.config.mode, self.config.rest_port
                )
            } else {
                format!("Konfiguration fehlt: {}", self.config.env_file.display())
            },
            installed_build: installed_build.clone(),
            ..ServerSnapshot::default()
        };

        let info = self.rest_get("info");
        let metrics = self.rest_get("metrics");
        snapshot.api_connected = info.is_some();
        if let Some(info) = info {
            snapshot.server_name = json_string(&info, "servername");
            snapshot.palworld_version = json_string(&info, "version");
            snapshot.service = "VERBUNDEN".to_owned();
            snapshot.connection_detail = "Lokale Palworld-REST-API erreichbar".to_owned();
        } else if service_active {
            snapshot.connection_detail = format!(
                "Dienst aktiv, REST-API auf 127.0.0.1:{} noch nicht erreichbar",
                self.config.rest_port
            );
        }
        if let Some(metrics) = metrics {
            snapshot.players_online =
                json_u64(&metrics, "currentplayernum").map(|value| value as u32);
            snapshot.players_max = json_u64(&metrics, "maxplayernum").map(|value| value as u32);
            snapshot.server_fps = json_u64(&metrics, "serverfps").map(|value| value as u32);
            snapshot.frame_time_ms = metrics.get("serverframetime").and_then(Value::as_f64);
            snapshot.uptime_seconds = json_u64(&metrics, "uptime");
        }

        if let Some(settings) = self.read_current_settings() {
            snapshot.client_mods = settings
                .get("bAllowClientMod")
                .and_then(|value| parse_bool(value));
            snapshot.exp_rate = settings.get("ExpRate").and_then(|value| value.parse().ok());
            snapshot.pvp = settings.get("bIsPvP").and_then(|value| parse_bool(value));
            if snapshot.players_max.is_none() {
                snapshot.players_max = settings
                    .get("ServerPlayerMaxNum")
                    .and_then(|value| value.parse().ok());
            }
        }
        snapshot
    }

    fn load_players(&self) -> Vec<Player> {
        #[derive(Deserialize)]
        struct PlayersResponse {
            #[serde(default)]
            players: Vec<Player>,
        }
        self.rest_get("players")
            .and_then(|value| serde_json::from_value::<PlayersResponse>(value).ok())
            .map_or_else(Vec::new, |response| response.players)
    }

    fn load_settings(&self) -> Vec<Setting> {
        let current = self.read_current_settings().unwrap_or_default();
        let output = Command::new(&self.config.settings_catalog_tool)
            .arg("catalog")
            .output();
        let catalog = output
            .ok()
            .filter(|value| value.status.success())
            .and_then(|value| serde_json::from_slice::<Vec<CatalogCategory>>(&value.stdout).ok());

        let mut result = Vec::new();
        let mut known = HashSet::new();
        if let Some(catalog) = catalog {
            for category in catalog {
                for item in category.settings {
                    known.insert(item.key.clone());
                    let value = if item.secret {
                        String::new()
                    } else {
                        current.get(&item.key).cloned().unwrap_or_default()
                    };
                    result.push(Setting {
                        key: item.key,
                        label: item.label,
                        category: category.label.clone(),
                        kind: item.kind,
                        description: item.description,
                        value,
                        secret: item.secret,
                        protected: item.protected,
                        managed: item.managed,
                        options: item.options,
                    });
                }
            }
        }
        for (key, value) in current {
            if !known.contains(&key) {
                let secret = is_secret_key(&key);
                result.push(Setting {
                    label: key.clone(),
                    key,
                    category: "Weitere / zukünftige Optionen".to_owned(),
                    kind: "raw".to_owned(),
                    description: "Vom installierten Server erkannt; derzeit nur lesbar.".to_owned(),
                    value: if secret { String::new() } else { value },
                    secret,
                    protected: false,
                    managed: Some("unknown".to_owned()),
                    options: Vec::new(),
                });
            }
        }
        result
    }

    fn read_current_settings(&self) -> Option<BTreeMap<String, String>> {
        let output = Command::new(&self.config.config_tool)
            .arg("dump-json")
            .arg(&self.config.server_config)
            .output()
            .ok()?;
        if !output.status.success() {
            return self.rest_get("settings").and_then(json_object_to_strings);
        }
        serde_json::from_slice(&output.stdout).ok()
    }

    fn load_mods(&self) -> Vec<ModEntry> {
        let mut result = Vec::new();
        let output = Command::new(&self.config.pak_tool)
            .args(["--store"])
            .arg(&self.config.pak_store)
            .args(["--target"])
            .arg(&self.config.pak_target)
            .args(["--quarantine"])
            .arg(&self.config.pak_quarantine)
            .arg("list")
            .output();
        if let Ok(output) = output
            && output.status.success()
            && let Ok(entries) = serde_json::from_slice::<Vec<PakStatus>>(&output.stdout)
        {
            for entry in entries {
                result.push(ModEntry {
                    name: entry.name,
                    kind: "PAK".to_owned(),
                    version: None,
                    enabled: entry.enabled,
                    partial: entry.partially_enabled,
                    compatible: None,
                    detail: if entry.files.is_empty() {
                        entry
                            .error
                            .unwrap_or_else(|| "keine Paketdateien".to_owned())
                    } else {
                        entry.files.join(", ")
                    },
                });
            }
        }

        let mut direct_packages = BTreeMap::<String, Vec<String>>::new();
        if let Ok(entries) = fs::read_dir(&self.config.pak_target) {
            for entry in entries.flatten() {
                let path = entry.path();
                let suffix = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if matches!(suffix.as_str(), "pak" | "utoc" | "ucas")
                    && !entry.file_type().is_ok_and(|kind| kind.is_symlink())
                {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let stem = path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or(&name)
                        .to_owned();
                    direct_packages.entry(stem).or_default().push(name);
                }
            }
        }
        for (name, mut files) in direct_packages {
            files.sort();
            let has_pak = files
                .iter()
                .any(|file| file.to_ascii_lowercase().ends_with(".pak"));
            result.push(ModEntry {
                name,
                kind: "PAK direkt".to_owned(),
                version: None,
                enabled: has_pak,
                partial: !has_pak,
                compatible: None,
                detail: format!(
                    "{} · direkt in ~mods erkannt; nicht vom AIO-Paketmanager verwaltet",
                    files.join(", ")
                ),
            });
        }

        if self.config.mode == "wine" {
            let output = Command::new(&self.config.mod_inspect_tool)
                .arg("scan")
                .arg(&self.config.workshop_dir)
                .output();
            let active = Command::new(&self.config.mod_config_tool)
                .arg(&self.config.mod_settings_file)
                .arg("list")
                .output()
                .ok()
                .filter(|value| value.status.success())
                .map(|value| {
                    String::from_utf8_lossy(&value.stdout)
                        .lines()
                        .map(str::to_owned)
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
            if let Ok(output) = output
                && output.status.success()
                && let Ok(entries) = serde_json::from_slice::<Vec<WorkshopStatus>>(&output.stdout)
            {
                for entry in entries {
                    let name = entry.package.unwrap_or_else(|| entry.folder.clone());
                    result.push(ModEntry {
                        enabled: active.contains(&name),
                        name,
                        kind: "Workshop".to_owned(),
                        version: entry.version,
                        partial: false,
                        compatible: Some(entry.server_compatible),
                        detail: entry.error.unwrap_or(entry.folder),
                    });
                }
            }
        }
        result.sort_by_key(|entry| (entry.kind.clone(), entry.name.to_ascii_lowercase()));
        result
    }

    fn load_backups(&self) -> Vec<BackupEntry> {
        let mut result = Vec::new();
        let Ok(entries) = fs::read_dir(&self.config.backup_dir) else {
            return result;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("palworld-") || !name.ends_with(".tar.zst") {
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let modified_unix = metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |value| value.as_secs());
            result.push(BackupEntry {
                checksum_present: path.with_extension("zst.sha256").is_file(),
                path,
                name,
                size_bytes: metadata.len(),
                modified_unix,
            });
        }
        result.sort_by_key(|entry| std::cmp::Reverse(entry.modified_unix));
        result
    }

    fn load_updates(&self) -> UpdateSnapshot {
        let installed_build = find_installed_build(&self.config.base_dir);
        let next_check = command_text(
            "systemctl",
            &[
                "list-timers",
                "palworld-update.timer",
                "--all",
                "--no-pager",
                "--no-legend",
            ],
        )
        .and_then(|text| text.lines().next().map(str::trim).map(str::to_owned));
        let last_result = command_text(
            "journalctl",
            &[
                "--unit=palworld-update.service",
                "--lines=8",
                "--no-pager",
                "--output=cat",
            ],
        )
        .and_then(|text| {
            text.lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(normalize_log_line)
        });
        UpdateSnapshot {
            installed_build,
            next_check,
            last_result,
            changelog: self.cached_news.clone(),
        }
    }

    fn load_logs(&self, lines: usize) -> Vec<String> {
        let count = lines.to_string();
        command_text(
            "journalctl",
            &[
                "--unit=palworld.service",
                "--lines",
                &count,
                "--no-pager",
                "--output=short-iso",
            ],
        )
        .map(|text| {
            let mut normalized = VecDeque::new();
            for line in text.lines().map(normalize_log_line) {
                if line.trim().is_empty() || normalized.back() == Some(&line) {
                    continue;
                }
                normalized.push_back(line);
            }
            normalized.into()
        })
        .unwrap_or_else(|| vec!["journalctl ist nicht verfügbar".to_owned()])
    }

    fn load_security(&self, writes_enabled: bool) -> Vec<SecurityCheck> {
        let mut checks = vec![SecurityCheck {
            label: "Schreibzugriff".to_owned(),
            status: if writes_enabled {
                CheckStatus::Warn
            } else {
                CheckStatus::Pass
            },
            detail: if writes_enabled {
                "Für diese Sitzung explizit aktiviert; jede Aktion verlangt zusätzlich Bestätigung"
                    .to_owned()
            } else {
                "Gesperrt; Start mit --enable-writes erforderlich".to_owned()
            },
        }];
        checks.push(path_check(
            "AIO-Konfiguration",
            &self.config.env_file,
            0o640,
        ));
        checks.push(path_check(
            "REST-Zugangsdaten",
            &self.config.rest_netrc,
            0o600,
        ));
        checks.push(path_check(
            "Admin-Passwortdatei",
            Path::new("/etc/palworld/admin-password"),
            0o600,
        ));
        checks.push(directory_check(
            "Backup-Verzeichnis",
            &self.config.backup_dir,
        ));

        let listening = command_text("ss", &["-ltnH"]).unwrap_or_default();
        let port = format!(":{}", self.config.rest_port);
        let bound = listening.lines().find(|line| line.contains(&port));
        checks.push(match bound {
            Some(line) if line.contains("127.0.0.1:") || line.contains("[::1]:") => SecurityCheck {
                label: "REST-Netzgrenze".to_owned(),
                status: CheckStatus::Pass,
                detail: "Nur auf Loopback gebunden".to_owned(),
            },
            Some(_) => SecurityCheck {
                label: "REST-Netzgrenze".to_owned(),
                status: CheckStatus::Warn,
                detail: format!(
                    "Port {} lauscht nicht ausschließlich auf Loopback; Firewall-Regeln prüfen",
                    self.config.rest_port
                ),
            },
            None => SecurityCheck {
                label: "REST-Netzgrenze".to_owned(),
                status: CheckStatus::Warn,
                detail: "REST-Port lauscht derzeit nicht".to_owned(),
            },
        });
        checks
    }

    fn rest_get(&self, endpoint: &str) -> Option<Value> {
        let output = Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--fail",
                "--connect-timeout",
                "1",
                "--max-time",
                "3",
                "--noproxy",
                "*",
                "--netrc-file",
            ])
            .arg(&self.config.rest_netrc)
            .arg(self.config.rest_url(endpoint))
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| serde_json::from_slice(&output.stdout).ok())
            .flatten()
    }
}

pub fn normalize_log_line(line: &str) -> String {
    let mut value = line.trim().to_owned();
    if let Some(index) = value.find("]: ") {
        let prefix = &value[..index + 2];
        let message = &value[index + 3..];
        if let Ok(json) = serde_json::from_str::<Value>(message) {
            let level = json
                .get("level")
                .or_else(|| json.get("severity"))
                .and_then(Value::as_str)
                .unwrap_or("INFO");
            let event = json
                .get("message")
                .or_else(|| json.get("event"))
                .and_then(Value::as_str)
                .unwrap_or("JSON-Ereignis");
            value = format!("{prefix} {level:<5} {event}");
        }
    }
    redact_log(value)
}

fn redact_log(mut value: String) -> String {
    const SECRETS: [&str; 4] = [
        "AdminPassword",
        "ServerPassword",
        "Authorization:",
        "password=",
    ];
    for marker in SECRETS {
        if let Some(index) = value
            .to_ascii_lowercase()
            .find(&marker.to_ascii_lowercase())
        {
            value.truncate(index + marker.len());
            value.push_str(" [REDACTED]");
        }
    }
    value
}

fn load_official_news() -> Option<Vec<ChangelogEntry>> {
    #[derive(Deserialize)]
    struct Root {
        appnews: AppNews,
    }
    #[derive(Deserialize)]
    struct AppNews {
        #[serde(default)]
        newsitems: Vec<NewsItem>,
    }
    #[derive(Deserialize)]
    struct NewsItem {
        title: String,
        url: String,
        date: u64,
    }
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--connect-timeout",
            "2",
            "--max-time",
            "8",
            "--location",
            OFFICIAL_NEWS_URL,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root: Root = serde_json::from_slice(&output.stdout).ok()?;
    Some(
        root.appnews
            .newsitems
            .into_iter()
            .map(|item| ChangelogEntry {
                title: item.title,
                url: item.url,
                published_unix: item.date,
            })
            .collect(),
    )
}

fn path_check(label: &str, path: &Path, expected_max: u32) -> SecurityCheck {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return SecurityCheck {
            label: label.to_owned(),
            status: CheckStatus::Fail,
            detail: format!("Fehlt oder ist nicht lesbar: {}", path.display()),
        };
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return SecurityCheck {
            label: label.to_owned(),
            status: CheckStatus::Fail,
            detail: "Muss eine reguläre Datei sein; Symlinks sind unzulässig".to_owned(),
        };
    }
    let mode = metadata.permissions().mode() & 0o777;
    let too_open = mode & !expected_max != 0;
    SecurityCheck {
        label: label.to_owned(),
        status: if too_open {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        },
        detail: format!("{} · Modus {mode:04o}", path.display()),
    }
}

fn directory_check(label: &str, path: &Path) -> SecurityCheck {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return SecurityCheck {
            label: label.to_owned(),
            status: CheckStatus::Fail,
            detail: format!("Fehlt: {}", path.display()),
        };
    };
    SecurityCheck {
        label: label.to_owned(),
        status: if metadata.is_dir() && !metadata.file_type().is_symlink() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: if metadata.file_type().is_symlink() {
            "Symlink ist für Backups nicht zulässig".to_owned()
        } else {
            path.display().to_string()
        },
    }
}

fn read_env_file(path: &Path) -> Option<HashMap<String, String>> {
    let text = fs::read_to_string(path).ok()?;
    let mut values = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        if key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            values.insert(key.to_owned(), unquote_env(raw));
        }
    }
    Some(values)
}

fn unquote_env(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('\'') && trimmed.ends_with('\''))
            || (trimmed.starts_with('"') && trimmed.ends_with('"')))
    {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    let mut result = String::new();
    let mut escaped = false;
    for character in trimmed.chars() {
        if escaped {
            result.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            result.push(character);
        }
    }
    if escaped {
        result.push('\\');
    }
    result
}

fn env_value(values: &HashMap<String, String>, key: &str, default: &str) -> String {
    values
        .get(key)
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| default.to_owned())
}

fn tool_path(
    values: &HashMap<String, String>,
    key: &str,
    runtime_dir: &Path,
    default_name: &str,
) -> PathBuf {
    PathBuf::from(env_value(
        values,
        key,
        runtime_dir.join(default_name).to_string_lossy().as_ref(),
    ))
}

fn command_success(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

fn command_text(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn find_installed_build(base: &Path) -> Option<String> {
    let manifest = find_named_file(base, "appmanifest_2394010.acf", 6)?;
    let text = fs::read_to_string(manifest).ok()?;
    for line in text.lines() {
        if line.contains("\"buildid\"") {
            let fields = line.split('"').collect::<Vec<_>>();
            if let Some(value) = fields.get(3).filter(|value| !value.is_empty()) {
                return Some((*value).to_owned());
            }
        }
    }
    None
}

fn find_named_file(root: &Path, name: &str, max_depth: usize) -> Option<PathBuf> {
    let mut queue = VecDeque::from([(root.to_path_buf(), 0_usize)]);
    while let Some((directory, depth)) = queue.pop_front() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_file() && entry.file_name() == name {
                return Some(path);
            }
            if file_type.is_dir() && depth < max_depth {
                queue.push_back((path, depth + 1));
            }
        }
    }
    None
}

fn json_object_to_strings(value: Value) -> Option<BTreeMap<String, String>> {
    let object = value.as_object()?;
    Some(
        object
            .iter()
            .map(|(key, value)| {
                let value = match value {
                    Value::String(value) => value.clone(),
                    Value::Bool(value) => if *value { "True" } else { "False" }.to_owned(),
                    Value::Number(value) => value.to_string(),
                    _ => value.to_string(),
                };
                (key.clone(), value)
            })
            .collect(),
    )
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn json_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("password") || lower.contains("secret") || lower.contains("token")
}

#[derive(Deserialize)]
struct CatalogCategory {
    label: String,
    #[serde(default)]
    settings: Vec<CatalogItem>,
}

#[derive(Deserialize)]
struct CatalogItem {
    key: String,
    label: String,
    kind: String,
    description: String,
    #[serde(default)]
    secret: bool,
    #[serde(default)]
    protected: bool,
    #[serde(default)]
    managed: Option<String>,
    #[serde(default)]
    options: Vec<SettingOption>,
}

#[derive(Deserialize)]
struct PakStatus {
    name: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    partially_enabled: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct WorkshopStatus {
    folder: String,
    #[serde(default)]
    package: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    server_compatible: bool,
    #[serde(default)]
    error: Option<String>,
}

pub fn demo_data() -> DashboardData {
    DashboardData {
        server: ServerSnapshot {
            service_active: true,
            api_connected: true,
            service: "VERBUNDEN".to_owned(),
            connection_detail: "Demo · lokale REST-API".to_owned(),
            server_name: Some("Palworld Demo Server".to_owned()),
            palworld_version: Some("v1.0.1".to_owned()),
            installed_build: Some("24466863".to_owned()),
            players_online: Some(7),
            players_max: Some(32),
            server_fps: Some(60),
            frame_time_ms: Some(16.7),
            uptime_seconds: Some(86_400),
            client_mods: Some(true),
            exp_rate: Some(1.15),
            pvp: Some(false),
        },
        players: vec![
            Player {
                name: "Demo Player 1".to_owned(),
                account_name: String::new(),
                player_id: String::new(),
                user_id: String::new(),
                ping: 18.0,
                level: 56,
                building_count: 119,
            },
            Player {
                name: "Demo Player 2".to_owned(),
                account_name: String::new(),
                player_id: String::new(),
                user_id: String::new(),
                ping: 31.0,
                level: 42,
                building_count: 74,
            },
        ],
        settings: vec![
            demo_setting(
                "ExpRate",
                "Erfahrungsrate",
                "Spielbalance",
                "float",
                "1.150000",
            ),
            demo_setting("bIsPvP", "PvP", "Spielregeln & Funktionen", "bool", "False"),
            demo_setting(
                "bAllowClientMod",
                "Modifizierte Clients erlauben",
                "Server & Zugang",
                "bool",
                "True",
            ),
            demo_setting(
                "ServerPlayerMaxNum",
                "Maximale Spieler",
                "Server & Zugang",
                "int",
                "32",
            ),
        ],
        mods: vec![ModEntry {
            name: "CreativeMenu".to_owned(),
            kind: "PAK".to_owned(),
            version: None,
            enabled: true,
            partial: false,
            compatible: None,
            detail: "CreativeMenu.pak".to_owned(),
        }],
        backups: vec![BackupEntry {
            path: PathBuf::from("/var/backups/palworld/palworld-demo.tar.zst"),
            name: "palworld-20260812-190000-manual.tar.zst".to_owned(),
            size_bytes: 2_147_483_648,
            modified_unix: 1_786_557_600,
            checksum_present: true,
        }],
        updates: UpdateSnapshot {
            installed_build: Some("24466863".to_owned()),
            next_check: Some("morgen 08:26".to_owned()),
            last_result: Some("Palworld ist aktuell".to_owned()),
            changelog: vec![ChangelogEntry {
                title: "Palworld Server Update".to_owned(),
                url: "https://store.steampowered.com/news/app/1623730".to_owned(),
                published_unix: 1_786_557_600,
            }],
        },
        logs: vec![
            "19:04:01 INFO  palworld.service ist aktiv".to_owned(),
            "19:04:02 INFO  REST-API auf 127.0.0.1:8212 erreichbar".to_owned(),
            "19:04:04 INFO  7/32 Spieler online · 60 FPS".to_owned(),
            "19:04:08 INFO  CreativeMenu.pak aktiv erkannt".to_owned(),
        ],
        security: vec![SecurityCheck {
            label: "REST-Netzgrenze".to_owned(),
            status: CheckStatus::Pass,
            detail: "Nur auf Loopback gebunden".to_owned(),
        }],
    }
}

fn demo_setting(key: &str, label: &str, category: &str, kind: &str, value: &str) -> Setting {
    Setting {
        key: key.to_owned(),
        label: label.to_owned(),
        category: category.to_owned(),
        kind: kind.to_owned(),
        description: "Demo-Einstellung".to_owned(),
        value: value.to_owned(),
        secret: false,
        protected: false,
        managed: None,
        options: Vec::new(),
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_secs())
}

#[cfg(test)]
mod tests {
    use super::{normalize_log_line, unquote_env};

    #[test]
    fn shell_escaped_environment_values_are_decoded() {
        assert_eq!(unquote_env(r"Debian\ 13\ \(Trixie\)"), "Debian 13 (Trixie)");
        assert_eq!(unquote_env("'native'"), "native");
    }

    #[test]
    fn json_journal_message_is_compacted() {
        let line = r#"2026-08-12 Palworld run.sh[1]: {"level":"INFO","event":"command"}"#;
        let normalized = normalize_log_line(line);
        assert!(normalized.contains("INFO"));
        assert!(normalized.contains("command"));
        assert!(!normalized.contains("timestamp"));
    }

    #[test]
    fn log_secrets_are_redacted() {
        let normalized = normalize_log_line("AdminPassword=super-secret");
        assert!(!normalized.contains("super-secret"));
        assert!(normalized.contains("[REDACTED]"));
    }
}
