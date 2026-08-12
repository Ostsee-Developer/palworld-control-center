use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::native::{
    ADMIN_PASSWORD_FILE, NativeConfig, REST_NETRC_FILE, SettingValue, update_settings,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
pub enum InternalTask {
    Prepare,
    GracefulStop,
    Backup,
    ServerUpdate,
    SelfUpdate,
}

pub fn run_internal(task: InternalTask) -> Result<()> {
    match task {
        InternalTask::SelfUpdate => crate::system_install::self_update(),
        InternalTask::Prepare => prepare(&NativeConfig::load()?),
        InternalTask::GracefulStop => graceful_stop(&NativeConfig::load()?),
        InternalTask::Backup => create_backup(&NativeConfig::load()?).map(|_| ()),
        InternalTask::ServerUpdate => update_server(&NativeConfig::load()?, false),
    }
}

pub fn prepare(config: &NativeConfig) -> Result<()> {
    let admin = fs::read_to_string(ADMIN_PASSWORD_FILE)
        .context("Admin-Passwortdatei fehlt")?
        .trim()
        .to_owned();
    if admin.is_empty() {
        bail!("Admin-Passwort ist leer");
    }
    let mut updates = BTreeMap::new();
    updates.insert("AdminPassword".to_owned(), SettingValue::String(admin));
    updates.insert("RESTAPIEnabled".to_owned(), SettingValue::Bool(true));
    updates.insert(
        "RESTAPIPort".to_owned(),
        SettingValue::Integer(u64::from(config.rest_port)),
    );
    updates.insert(
        "ServerPlayerMaxNum".to_owned(),
        SettingValue::Integer(u64::from(config.max_players)),
    );
    update_settings(&config.settings_file(), &updates)
}

pub fn graceful_stop(config: &NativeConfig) -> Result<()> {
    if service_active() {
        let _ = rest_post(config, "save", None);
        thread::sleep(Duration::from_secs(5));
    }
    Ok(())
}

pub fn create_backup(config: &NativeConfig) -> Result<PathBuf> {
    config.validate()?;
    let saves = config.server_dir.join("Pal/Saved/SaveGames");
    if !saves.is_dir() {
        bail!("SaveGames-Verzeichnis fehlt: {}", saves.display());
    }
    if service_active() {
        rest_post(config, "save", None)?;
        thread::sleep(Duration::from_secs(5));
    }
    fs::create_dir_all(&config.backup_dir)?;
    fs::set_permissions(&config.backup_dir, fs::Permissions::from_mode(0o700))?;
    let timestamp = unix_now();
    let archive = config
        .backup_dir
        .join(format!("palworld-{timestamp}.tar.zst"));
    let partial = config
        .backup_dir
        .join(format!(".palworld-{timestamp}.partial"));
    if archive.exists() || partial.exists() {
        bail!("Backup-Zieldatei existiert bereits");
    }
    if let Err(error) = checked(
        Command::new("tar")
            .args(["--create", "--zstd", "--file"])
            .arg(&partial)
            .args(["--directory"])
            .arg(config.server_dir.join("Pal/Saved"))
            .arg("SaveGames"),
        "Backup-Archiv",
    ) {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    fs::set_permissions(&partial, fs::Permissions::from_mode(0o600))?;
    fs::rename(&partial, &archive)?;
    write_checksum(&archive)?;
    prune_backups(config)?;
    Ok(archive)
}

pub fn verify_backup(config: &NativeConfig, archive: &Path) -> Result<()> {
    validate_backup_path(config, archive)?;
    let checksum = checksum_path(archive);
    let text = fs::read_to_string(&checksum)
        .with_context(|| format!("Prüfsummendatei fehlt: {}", checksum.display()))?;
    let mut fields = text.split_whitespace();
    let expected = fields.next().context("SHA-256 fehlt")?;
    let name = fields.next().context("Backupname in SHA-256-Datei fehlt")?;
    if expected.len() != 64
        || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
        || Some(name.trim_start_matches('*')) != archive.file_name().and_then(|name| name.to_str())
        || fields.next().is_some()
    {
        bail!("Backup-Prüfsummendatei ist ungültig");
    }
    let actual = sha256(archive)?;
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("SHA-256-Prüfung des Backups fehlgeschlagen");
    }
    Ok(())
}

pub fn restore_backup(config: &NativeConfig, archive: &Path) -> Result<()> {
    verify_backup(config, archive)?;
    inspect_archive(archive)?;
    let restore_root = config
        .state_dir
        .join(format!("restore-{}", std::process::id()));
    if restore_root.exists() {
        bail!("Temporäres Restore-Verzeichnis existiert bereits");
    }
    fs::create_dir_all(&restore_root)?;
    fs::set_permissions(&restore_root, fs::Permissions::from_mode(0o700))?;
    checked(
        Command::new("tar")
            .args(["--extract", "--zstd", "--file"])
            .arg(archive)
            .args(["--directory"])
            .arg(&restore_root)
            .args(["--no-same-owner", "--no-same-permissions"]),
        "Backup-Extraktion",
    )?;
    let extracted = restore_root.join("SaveGames");
    if !extracted.is_dir() {
        bail!("Backup enthält kein SaveGames-Verzeichnis");
    }
    let _pre_restore = create_backup(config)?;
    checked(
        Command::new("systemctl").args(["stop", "palworld.service"]),
        "Serverstop vor Restore",
    )?;
    let current = config.server_dir.join("Pal/Saved/SaveGames");
    let rollback = config.state_dir.join(format!("rollback-{}", unix_now()));
    fs::rename(&current, &rollback)
        .context("Aktuelle Welt konnte nicht in Rollback verschoben werden")?;
    if let Err(error) = fs::rename(&extracted, &current) {
        let _ = fs::rename(&rollback, &current);
        return Err(error).context("Wiederhergestellte Welt konnte nicht aktiviert werden");
    }
    chown_recursive(&current, &config.service_user, &config.service_group)?;
    if let Err(error) = checked(
        Command::new("systemctl").args(["start", "palworld.service"]),
        "Serverstart nach Restore",
    ) {
        let failed = config
            .state_dir
            .join(format!("failed-restore-{}", unix_now()));
        let _ = fs::rename(&current, &failed);
        let _ = fs::rename(&rollback, &current);
        let _ = Command::new("systemctl")
            .args(["start", "palworld.service"])
            .status();
        return Err(error).context("Restore wurde zurückgerollt");
    }
    fs::remove_dir_all(&rollback)?;
    fs::remove_dir_all(&restore_root)?;
    Ok(())
}

pub fn update_server(config: &NativeConfig, force: bool) -> Result<()> {
    if !force && (players_online(config).unwrap_or(0) > 0 || !update_required(config)?) {
        return Ok(());
    }
    let _backup = create_backup(config)?;
    checked(
        Command::new("systemctl").args(["stop", "palworld.service"]),
        "Serverstop vor Update",
    )?;
    let update = run_steamcmd(config);
    let start = checked(
        Command::new("systemctl").args(["start", "palworld.service"]),
        "Serverstart nach Update",
    );
    update.and(start)
}

fn update_required(config: &NativeConfig) -> Result<bool> {
    let build = crate::backend::find_installed_build(&config.base_dir)
        .context("Installierte Palworld-Build-ID fehlt")?;
    let build = build
        .parse::<u64>()
        .context("Installierte Palworld-Build-ID ist ungültig")?;
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--connect-timeout",
            "5",
            "--max-time",
            "15",
            "--get",
            "--data-urlencode",
            "appid=2394010",
            "--data-urlencode",
        ])
        .arg(format!("version={build}"))
        .arg("https://api.steampowered.com/ISteamApps/UpToDateCheck/v1/")
        .output()
        .context("Steam-Updateprüfung konnte nicht gestartet werden")?;
    if !output.status.success() {
        bail!(
            "Steam-Updateprüfung fehlgeschlagen: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .context("Steam-Updateprüfung lieferte ungültiges JSON")?;
    let response = value
        .get("response")
        .and_then(Value::as_object)
        .context("Steam-Updateantwort fehlt")?;
    if response.get("success").and_then(Value::as_bool) != Some(true) {
        bail!("Steam konnte die installierte Build-ID nicht prüfen");
    }
    response
        .get("up_to_date")
        .and_then(Value::as_bool)
        .map(|up_to_date| !up_to_date)
        .context("Steam-Updateantwort enthält keinen Versionsstatus")
}

pub fn save_world(config: &NativeConfig) -> Result<()> {
    rest_post(config, "save", None).map(|_| ())
}

pub fn api_action(config: &NativeConfig, endpoint: &str, payload: Option<Value>) -> Result<()> {
    rest_post(config, endpoint, payload).map(|_| ())
}

fn rest_post(config: &NativeConfig, endpoint: &str, payload: Option<Value>) -> Result<String> {
    let mut command = Command::new("curl");
    command.args([
        "--silent",
        "--show-error",
        "--fail",
        "--connect-timeout",
        "2",
        "--max-time",
        "20",
        "--noproxy",
        "*",
        "--netrc-file",
        REST_NETRC_FILE,
        "--request",
        "POST",
    ]);
    if let Some(payload) = payload {
        command
            .args([
                "--header",
                "Content-Type: application/json",
                "--data-binary",
            ])
            .arg(payload.to_string());
    }
    let output = command
        .arg(format!(
            "http://127.0.0.1:{}/v1/api/{endpoint}",
            config.rest_port
        ))
        .output()
        .context("REST-Aufruf konnte nicht gestartet werden")?;
    output_text(output, "Palworld-REST-Aktion")
}

fn run_steamcmd(config: &NativeConfig) -> Result<()> {
    let steamcmd = config.steamcmd_dir.join("steamcmd.sh");
    let mut command = Command::new("runuser");
    command
        .args(["-u", &config.service_user, "--", "env"])
        .arg(format!("HOME={}", config.base_dir.display()))
        .arg(format!("USER={}", config.service_user))
        .arg(format!("LOGNAME={}", config.service_user))
        .arg(steamcmd)
        .arg("+force_install_dir")
        .arg(&config.server_dir)
        .args([
            "+login",
            "anonymous",
            "+app_update",
            "2394010",
            "validate",
            "+quit",
        ]);
    checked(&mut command, "SteamCMD Serverupdate")
}

fn players_online(config: &NativeConfig) -> Option<u64> {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--fail",
            "--connect-timeout",
            "1",
            "--max-time",
            "3",
            "--noproxy",
            "*",
            "--netrc-file",
            REST_NETRC_FILE,
        ])
        .arg(format!(
            "http://127.0.0.1:{}/v1/api/metrics",
            config.rest_port
        ))
        .output()
        .ok()?;
    let value: Value = serde_json::from_slice(&output.stdout).ok()?;
    value.get("currentplayernum").and_then(Value::as_u64)
}

fn service_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", "palworld.service"])
        .status()
        .is_ok_and(|status| status.success())
}

fn inspect_archive(archive: &Path) -> Result<()> {
    let listing = Command::new("tar")
        .args(["--list", "--zstd", "--quoting-style=escape", "--file"])
        .arg(archive)
        .output()
        .context("Backup-Pfadprüfung konnte nicht gestartet werden")?;
    if !listing.status.success() {
        bail!("Backup-Pfadprüfung fehlgeschlagen");
    }
    let names = String::from_utf8(listing.stdout).context("Backup enthält ungültige Dateinamen")?;
    for name in names.lines() {
        let normalized = name.trim_end_matches('/');
        if normalized.is_empty()
            || normalized.starts_with('/')
            || name.contains('\\')
            || normalized
                .split('/')
                .any(|part| part == ".." || part.is_empty())
            || !(normalized == "SaveGames" || normalized.starts_with("SaveGames/"))
            || !name.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-')
            })
        {
            bail!("Backup enthält unzulässigen Pfad: {name}");
        }
    }
    let verbose = Command::new("tar")
        .args(["--list", "--verbose", "--zstd", "--file"])
        .arg(archive)
        .output()
        .context("Backup-Inhaltsprüfung konnte nicht gestartet werden")?;
    if !verbose.status.success() {
        bail!("Backup-Inhaltsprüfung fehlgeschlagen");
    }
    let text = String::from_utf8_lossy(&verbose.stdout);
    for line in text.lines() {
        let kind = line.as_bytes().first().copied().unwrap_or_default();
        if kind != b'-' && kind != b'd' {
            bail!("Backup enthält Links oder Spezialdateien");
        }
    }
    Ok(())
}

fn validate_backup_path(config: &NativeConfig, archive: &Path) -> Result<()> {
    let root = fs::canonicalize(&config.backup_dir)?;
    let canonical = fs::canonicalize(archive)?;
    let metadata = fs::symlink_metadata(archive)?;
    if canonical.parent() != Some(root.as_path())
        || metadata.file_type().is_symlink()
        || !metadata.is_file()
        || !archive
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("palworld-") && name.ends_with(".tar.zst"))
    {
        bail!("Unzulässiger Backup-Pfad");
    }
    Ok(())
}

fn write_checksum(archive: &Path) -> Result<()> {
    let name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .context("Backup-Dateiname ist ungültig")?;
    let content = format!("{}  {name}\n", sha256(archive)?);
    crate::native::atomic_write(&checksum_path(archive), content.as_bytes(), 0o600)
}

fn checksum_path(archive: &Path) -> PathBuf {
    PathBuf::from(format!("{}.sha256", archive.display()))
}

fn sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn prune_backups(config: &NativeConfig) -> Result<()> {
    let mut entries = fs::read_dir(&config.backup_dir)?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = entry.metadata().ok()?;
            (metadata.is_file() && name.starts_with("palworld-") && name.ends_with(".tar.zst"))
                .then_some((entry.path(), metadata.modified().ok()?))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(
            u64::from(config.backup_retention_days) * 86_400,
        ))
        .unwrap_or(UNIX_EPOCH);
    for (index, (archive, modified)) in entries.into_iter().enumerate() {
        if index >= config.backup_max_count as usize || modified < cutoff {
            fs::remove_file(&archive)?;
            let checksum = checksum_path(&archive);
            if checksum.is_file() {
                fs::remove_file(checksum)?;
            }
        }
    }
    Ok(())
}

fn chown_recursive(path: &Path, user: &str, group: &str) -> Result<()> {
    checked(
        Command::new("chown")
            .arg("--recursive")
            .arg(format!("{user}:{group}"))
            .arg(path),
        "Restore-Besitzrechte",
    )
}

fn checked(command: &mut Command, label: &str) -> Result<()> {
    let status = command
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("{label} konnte nicht gestartet werden"))?;
    if !status.success() {
        bail!("{label} ist mit Status {status} fehlgeschlagen");
    }
    Ok(())
}

fn output_text(output: Output, label: &str) -> Result<String> {
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        bail!("{label} fehlgeschlagen: {error}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
