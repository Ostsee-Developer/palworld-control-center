use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    net::IpAddr,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "/etc/palworld-control-center/server.json";
pub const ADMIN_PASSWORD_FILE: &str = "/etc/palworld-control-center/admin-password";
pub const REST_NETRC_FILE: &str = "/etc/palworld-control-center/rest.netrc";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NativeConfig {
    pub schema_version: u32,
    pub service_user: String,
    pub service_group: String,
    pub base_dir: PathBuf,
    pub server_dir: PathBuf,
    pub steamcmd_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub state_dir: PathBuf,
    pub game_port: u16,
    pub rest_port: u16,
    pub max_players: u32,
    pub public_lobby: bool,
    pub public_ip: String,
    pub public_port: u16,
    pub backup_retention_days: u32,
    pub backup_max_count: u32,
    pub ufw_managed: bool,
}

impl NativeConfig {
    pub fn load() -> Result<Self> {
        reject_symlink_components(Path::new(CONFIG_FILE))?;
        let bytes = fs::read(CONFIG_FILE).context("Native PCC-Serverkonfiguration fehlt")?;
        let config: Self = serde_json::from_slice(&bytes)
            .context("Native PCC-Serverkonfiguration ist ungültig")?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self)?;
        atomic_write(Path::new(CONFIG_FILE), &bytes, 0o640)
    }

    pub fn settings_file(&self) -> PathBuf {
        self.server_dir
            .join("Pal/Saved/Config/LinuxServer/PalWorldSettings.ini")
    }

    pub fn pak_store(&self) -> PathBuf {
        self.base_dir.join("pak-mods/packages")
    }

    pub fn pak_target(&self) -> PathBuf {
        self.server_dir.join("Pal/Content/Paks/~mods")
    }

    pub fn pak_quarantine(&self) -> PathBuf {
        self.base_dir.join("pak-mod-quarantine")
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!("Nicht unterstützte PCC-Konfigurationsversion");
        }
        if !valid_service_name(&self.service_user) || self.service_group != self.service_user {
            bail!("Ungültiger Palworld-Systembenutzer");
        }
        validate_base_path(&self.base_dir)?;
        validate_backup_path(&self.backup_dir, &self.base_dir)?;
        if self.server_dir != self.base_dir.join("server")
            || self.steamcmd_dir != self.base_dir.join("steamcmd")
            || self.state_dir != Path::new("/var/lib/palworld-control-center")
            || self.game_port < 1024
            || self.rest_port < 1024
            || self.game_port == self.rest_port
            || self.public_port != self.game_port
            || !(1..=128).contains(&self.max_players)
            || self.backup_retention_days == 0
            || self.backup_retention_days > 365
            || self.backup_max_count == 0
            || self.backup_max_count > 1_000
        {
            bail!("Native PCC-Serverkonfiguration enthält ungültige Pfade oder Ports");
        }
        if !self.public_ip.is_empty() && self.public_ip.parse::<IpAddr>().is_err() {
            bail!("Native PCC-Serverkonfiguration enthält eine ungültige öffentliche IP");
        }
        Ok(())
    }
}

pub fn exists() -> bool {
    Path::new(CONFIG_FILE).is_file()
}

pub fn read_settings(path: &Path) -> Result<BTreeMap<String, String>> {
    reject_symlink_components(path)?;
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Palworld-Konfiguration fehlt: {}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("Palworld-Konfiguration muss eine reguläre Datei sein");
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("Palworld-Konfiguration fehlt: {}", path.display()))?;
    let tuple = option_settings(&text)?;
    let mut settings = BTreeMap::new();
    for field in split_top_level(tuple)? {
        let Some((key, value)) = split_assignment(&field) else {
            continue;
        };
        settings.insert(key.to_owned(), decode_value(value));
    }
    Ok(settings)
}

pub fn update_settings(path: &Path, updates: &BTreeMap<String, SettingValue>) -> Result<()> {
    reject_symlink_components(path)?;
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Palworld-Konfiguration fehlt: {}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("Palworld-Konfiguration muss eine reguläre Datei sein");
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("Palworld-Konfiguration fehlt: {}", path.display()))?;
    let (start, end) = option_settings_range(&text)?;
    let fields = split_top_level(&text[start..end])?;
    let mut rendered = Vec::with_capacity(fields.len() + updates.len());
    let mut remaining = updates.clone();
    for field in fields {
        if let Some((key, _)) = split_assignment(&field)
            && let Some(value) = remaining.remove(key)
        {
            rendered.push(format!("{key}={}", value.render()));
        } else {
            rendered.push(field);
        }
    }
    for (key, value) in remaining {
        if !valid_setting_key(&key) {
            bail!("Ungültiger Palworld-Einstellungsschlüssel: {key}");
        }
        rendered.push(format!("{key}={}", value.render()));
    }
    let mut output = String::with_capacity(text.len() + 256);
    output.push_str(&text[..start]);
    output.push_str(&rendered.join(","));
    output.push_str(&text[end..]);
    atomic_write(path, output.as_bytes(), 0o640)
}

#[derive(Clone, Debug)]
pub enum SettingValue {
    String(String),
    Bool(bool),
    Integer(u64),
    Raw(String),
}

impl SettingValue {
    fn render(&self) -> String {
        match self {
            Self::String(value) => format!("\"{}\"", escape_ini_string(value)),
            Self::Bool(value) => if *value { "True" } else { "False" }.to_owned(),
            Self::Integer(value) => value.to_string(),
            Self::Raw(value) => value.clone(),
        }
    }
}

pub fn parsed_input(current: &str, input: &str) -> Result<SettingValue> {
    let trimmed = input.trim();
    if current.eq_ignore_ascii_case("true") || current.eq_ignore_ascii_case("false") {
        return match trimmed.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "ja" | "on" => Ok(SettingValue::Bool(true)),
            "false" | "0" | "no" | "nein" | "off" => Ok(SettingValue::Bool(false)),
            _ => bail!("Boolean erwartet: true oder false"),
        };
    }
    if current.parse::<u64>().is_ok() {
        return trimmed
            .parse::<u64>()
            .map(SettingValue::Integer)
            .context("Positive Ganzzahl erwartet");
    }
    if current.parse::<f64>().is_ok() {
        let value = trimmed.parse::<f64>().context("Zahl erwartet")?;
        if !value.is_finite() {
            bail!("Endliche Zahl erwartet");
        }
        return Ok(SettingValue::Raw(value.to_string()));
    }
    if trimmed.contains(['\n', '\r', '\0']) || trimmed.len() > 512 {
        bail!("Ungültiger Textwert");
    }
    Ok(SettingValue::String(trimmed.to_owned()))
}

fn option_settings(text: &str) -> Result<&str> {
    let (start, end) = option_settings_range(text)?;
    Ok(&text[start..end])
}

fn option_settings_range(text: &str) -> Result<(usize, usize)> {
    let marker = "OptionSettings=(";
    let start = text.find(marker).context("OptionSettings fehlt")? + marker.len();
    let bytes = text.as_bytes();
    let mut depth = 1_i32;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, byte) in bytes[start..].iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && quoted {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((start, start + offset));
                }
            }
            _ => {}
        }
    }
    bail!("OptionSettings ist nicht vollständig geschlossen")
}

fn split_top_level(value: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut start = 0_usize;
    let mut depth = 0_i32;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in value.as_bytes().iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && quoted {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth < 0 {
                    bail!("Ungültige verschachtelte Palworld-Einstellung");
                }
            }
            b',' if depth == 0 => {
                let field = value[start..index].trim();
                if !field.is_empty() {
                    result.push(field.to_owned());
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if quoted || depth != 0 {
        bail!("Ungültige Palworld-Einstellungsstruktur");
    }
    let tail = value[start..].trim();
    if !tail.is_empty() {
        result.push(tail.to_owned());
    }
    Ok(result)
}

fn split_assignment(field: &str) -> Option<(&str, &str)> {
    let (key, value) = field.split_once('=')?;
    let key = key.trim();
    valid_setting_key(key).then_some((key, value.trim()))
}

fn decode_value(value: &str) -> String {
    let value = value.trim();
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        value[1..value.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        value.to_owned()
    }
}

fn escape_ini_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn valid_setting_key(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_service_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte == b'_')
        && bytes
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
        && value.len() <= 31
}

fn validate_base_path(path: &Path) -> Result<()> {
    let text = path.to_string_lossy();
    if !(text.starts_with("/opt/") || text.starts_with("/srv/"))
        || text.contains("..")
        || !safe_path_text(&text)
    {
        bail!("Serverpfad muss ein sicherer Pfad unter /opt oder /srv sein");
    }
    Ok(())
}

fn validate_backup_path(path: &Path, base: &Path) -> Result<()> {
    let text = path.to_string_lossy();
    let allowed = ["/var/backups/", "/srv/", "/mnt/", "/media/", "/opt/"];
    if !allowed.iter().any(|prefix| text.starts_with(prefix))
        || text.contains("..")
        || !safe_path_text(&text)
        || path.starts_with(base)
        || base.starts_with(path)
    {
        bail!("Backup-Pfad muss sicher und vom Serverpfad getrennt sein");
    }
    Ok(())
}

fn safe_path_text(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
}

pub fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = path.parent().context("Zieldatei ohne Elternpfad")?;
    reject_symlink_components(parent)?;
    fs::create_dir_all(parent)?;
    reject_symlink_components(parent)?;
    let temporary = parent.join(format!(".pcc-native-{}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| {
                format!("Temporäre Datei existiert bereits: {}", temporary.display())
            })?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn reject_symlink_components(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!(
            "Sicherheitskritischer Pfad muss absolut sein: {}",
            path.display()
        );
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push("/"),
            Component::Normal(part) => current.push(part),
            _ => bail!("Pfad enthält unzulässige Komponenten: {}", path.display()),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "Symlink in sicherheitskritischem Pfad: {}",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{NativeConfig, SettingValue, parsed_input, read_settings, update_settings};
    use std::{collections::BTreeMap, fs, os::unix::fs::symlink, path::PathBuf};

    fn config_fixture() -> NativeConfig {
        NativeConfig {
            schema_version: 1,
            service_user: "palworld".to_owned(),
            service_group: "palworld".to_owned(),
            base_dir: PathBuf::from("/opt/palworld"),
            server_dir: PathBuf::from("/opt/palworld/server"),
            steamcmd_dir: PathBuf::from("/opt/palworld/steamcmd"),
            backup_dir: PathBuf::from("/var/backups/palworld"),
            state_dir: PathBuf::from("/var/lib/palworld-control-center"),
            game_port: 8211,
            rest_port: 8212,
            max_players: 32,
            public_lobby: false,
            public_ip: String::new(),
            public_port: 8211,
            backup_retention_days: 14,
            backup_max_count: 60,
            ufw_managed: true,
        }
    }

    #[test]
    fn settings_round_trip_preserves_nested_unknown_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!("pcc-settings-{}", std::process::id()));
        fs::write(
            &path,
            "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(ExpRate=1.0,Unknown=(A=\"x,y\",B=2),ServerName=\"Old\")\n",
        )?;
        let mut updates = BTreeMap::new();
        updates.insert("ExpRate".to_owned(), SettingValue::Raw("1.25".to_owned()));
        updates.insert(
            "ServerName".to_owned(),
            SettingValue::String("New".to_owned()),
        );
        update_settings(&path, &updates)?;
        let values = read_settings(&path)?;
        assert_eq!(values.get("ExpRate").map(String::as_str), Some("1.25"));
        assert_eq!(values.get("ServerName").map(String::as_str), Some("New"));
        assert_eq!(
            values.get("Unknown").map(String::as_str),
            Some("(A=\"x,y\",B=2)")
        );
        let _ = fs::remove_file(path);
        Ok(())
    }

    #[test]
    fn native_configuration_rejects_unsafe_values() {
        let mut config = config_fixture();
        assert!(config.validate().is_ok());
        config.backup_dir = PathBuf::from("/var/backups/pal world");
        assert!(config.validate().is_err());
        config.backup_dir = PathBuf::from("/var/backups/palworld");
        config.public_ip = "not-an-ip".to_owned();
        assert!(config.validate().is_err());
    }

    #[test]
    fn setting_input_follows_existing_scalar_type() {
        assert!(matches!(
            parsed_input("True", "nein"),
            Ok(SettingValue::Bool(false))
        ));
        assert!(matches!(
            parsed_input("32", "64"),
            Ok(SettingValue::Integer(64))
        ));
        assert!(parsed_input("32", "many").is_err());
    }

    #[test]
    fn settings_reader_rejects_symlinks() -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("pcc-settings-link-{}", std::process::id()));
        fs::create_dir_all(&root)?;
        let target = root.join("target.ini");
        let link = root.join("link.ini");
        fs::write(&target, "OptionSettings=(ExpRate=1.0)\n")?;
        symlink(&target, &link)?;
        assert!(read_settings(&link).is_err());
        let _ = fs::remove_file(link);
        let _ = fs::remove_file(target);
        let _ = fs::remove_dir(root);
        Ok(())
    }
}
