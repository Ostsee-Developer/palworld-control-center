use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
};

use serde_json::{Value, json};

use crate::backend::{BackendConfig, normalize_log_line, unix_now};
use crate::{native, runtime};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
}

impl ServiceAction {
    fn verb(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    Service(ServiceAction),
    SaveWorld,
    Broadcast(String),
    Kick { user_id: String, message: String },
    Ban { user_id: String, message: String },
    Unban(String),
    SetSetting { key: String, input: String },
    CreateBackup,
    VerifyBackup(PathBuf),
    RestoreBackup(PathBuf),
    DeleteBackup(PathBuf),
    UpdateServer,
    TogglePak { name: String, enable: bool },
    ImportPak(PathBuf),
    QuarantinePak(String),
    Diagnostics,
}

impl Action {
    pub fn label(&self) -> String {
        match self {
            Self::Service(ServiceAction::Start) => "Server wird gestartet".to_owned(),
            Self::Service(ServiceAction::Stop) => "Server wird sicher gestoppt".to_owned(),
            Self::Service(ServiceAction::Restart) => "Server wird neu gestartet".to_owned(),
            Self::SaveWorld => "Welt wird gespeichert".to_owned(),
            Self::Broadcast(_) => "Servernachricht wird gesendet".to_owned(),
            Self::Kick { .. } => "Spieler wird entfernt".to_owned(),
            Self::Ban { .. } => "Spieler wird gesperrt".to_owned(),
            Self::Unban(_) => "Spielersperre wird aufgehoben".to_owned(),
            Self::SetSetting { key, .. } => format!("Einstellung {key} wird gespeichert"),
            Self::CreateBackup => "Welt-Backup wird erstellt".to_owned(),
            Self::VerifyBackup(path) => format!(
                "Prüfsumme wird verifiziert: {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            Self::RestoreBackup(path) => format!(
                "Welt wird wiederhergestellt: {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            Self::DeleteBackup(path) => format!(
                "Backup wird gelöscht: {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            Self::UpdateServer => "Palworld-Update läuft".to_owned(),
            Self::TogglePak { name, enable } => format!(
                "PAK {name} wird {}",
                if *enable { "aktiviert" } else { "deaktiviert" }
            ),
            Self::ImportPak(path) => format!("PAK wird importiert: {}", path.display()),
            Self::QuarantinePak(name) => format!("PAK {name} wird in Quarantäne verschoben"),
            Self::Diagnostics => "Redigiertes Diagnosepaket wird erstellt".to_owned(),
        }
    }

    pub const fn requires_write(&self) -> bool {
        !matches!(self, Self::VerifyBackup(_) | Self::Diagnostics)
    }
}

#[derive(Clone, Debug)]
pub enum JobEvent {
    Finished { success: bool, detail: String },
}

pub struct JobHandle {
    pub label: String,
    receiver: Receiver<JobEvent>,
}

impl JobHandle {
    pub fn poll(&self) -> Option<JobEvent> {
        self.receiver.try_recv().ok()
    }
}

pub fn spawn(action: Action, config: BackendConfig) -> JobHandle {
    let label = action.label();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let audit_label = action.label();
        let result = run_action(&action, &config);
        let audit_result = if result.is_ok() { "success" } else { "failure" };
        let _ = Command::new("logger")
            .args([
                "--tag",
                "palworld-control-center",
                "--priority",
                "authpriv.notice",
                "--",
            ])
            .arg(format!("result={audit_result} action={audit_label}"))
            .status();
        let event = match result {
            Ok(detail) => JobEvent::Finished {
                success: true,
                detail,
            },
            Err(detail) => JobEvent::Finished {
                success: false,
                detail,
            },
        };
        let _ = sender.send(event);
    });
    JobHandle { label, receiver }
}

fn run_action(action: &Action, config: &BackendConfig) -> Result<String, String> {
    if let Some(native_config) = &config.native {
        return run_native_action(action, config, native_config);
    }
    match action {
        Action::Service(action) => run_command(
            Command::new("systemctl")
                .arg(action.verb())
                .arg("palworld.service"),
        ),
        Action::SaveWorld => run_api(config, "save", None),
        Action::Broadcast(message) => {
            run_api(config, "announce", Some(json!({ "message": message })))
        }
        Action::Kick { user_id, message } => run_api(
            config,
            "kick",
            Some(json!({ "userid": user_id, "message": message })),
        ),
        Action::Ban { user_id, message } => run_api(
            config,
            "ban",
            Some(json!({ "userid": user_id, "message": message })),
        ),
        Action::Unban(user_id) => run_api(config, "unban", Some(json!({ "userid": user_id }))),
        Action::SetSetting { key, input } => set_setting(config, key, input),
        Action::CreateBackup => run_command(
            Command::new(&config.backup_tool)
                .arg("--label")
                .arg("control-center"),
        ),
        Action::VerifyBackup(path) => verify_backup(config, path),
        Action::RestoreBackup(path) => {
            let archive = validate_backup_path(config, path)?;
            run_command(Command::new(&config.restore_tool).arg(archive).arg("world"))
        }
        Action::DeleteBackup(path) => delete_backup(config, path),
        Action::UpdateServer => run_command(Command::new(&config.update_tool).arg("--force")),
        Action::TogglePak { name, enable } => toggle_pak(config, name, *enable),
        Action::ImportPak(path) => import_pak(config, path),
        Action::QuarantinePak(name) => pak_command(config, "quarantine", Some(name)),
        Action::Diagnostics => {
            let directory = PathBuf::from(format!(
                "/tmp/palworld-control-center-diagnostics-{}-{}",
                std::process::id(),
                unix_now()
            ));
            fs::create_dir(&directory)
                .map_err(|error| format!("Privates Diagnoseverzeichnis fehlt: {error}"))?;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).map_err(
                |error| format!("Diagnoseverzeichnis konnte nicht geschützt werden: {error}"),
            )?;
            let output = directory.join("palworld-diagnostics.tar.gz");
            run_command(Command::new(&config.diagnostics_tool).arg(&output))?;
            Ok(format!("Diagnosepaket erstellt: {}", output.display()))
        }
    }
}

fn run_native_action(
    action: &Action,
    config: &BackendConfig,
    native_config: &native::NativeConfig,
) -> Result<String, String> {
    let result = match action {
        Action::Service(action) => {
            return run_command(
                Command::new("systemctl")
                    .arg(action.verb())
                    .arg("palworld.service"),
            );
        }
        Action::SaveWorld => {
            runtime::save_world(native_config).map(|()| "Welt gespeichert".to_owned())
        }
        Action::Broadcast(message) => runtime::api_action(
            native_config,
            "announce",
            Some(json!({ "message": message })),
        )
        .map(|()| "Servernachricht gesendet".to_owned()),
        Action::Kick { user_id, message } => runtime::api_action(
            native_config,
            "kick",
            Some(json!({ "userid": user_id, "message": message })),
        )
        .map(|()| "Spieler entfernt".to_owned()),
        Action::Ban { user_id, message } => runtime::api_action(
            native_config,
            "ban",
            Some(json!({ "userid": user_id, "message": message })),
        )
        .map(|()| "Spieler gesperrt".to_owned()),
        Action::Unban(user_id) => {
            runtime::api_action(native_config, "unban", Some(json!({ "userid": user_id })))
                .map(|()| "Spielersperre aufgehoben".to_owned())
        }
        Action::SetSetting { key, input } => {
            return set_native_setting(config, native_config, key, input);
        }
        Action::CreateBackup => runtime::create_backup(native_config)
            .map(|archive| format!("Backup erstellt: {}", archive.display())),
        Action::VerifyBackup(path) => runtime::verify_backup(native_config, path)
            .map(|()| format!("Backup geprüft: {}", path.display())),
        Action::RestoreBackup(path) => runtime::restore_backup(native_config, path)
            .map(|()| format!("Welt wiederhergestellt: {}", path.display())),
        Action::DeleteBackup(path) => return delete_backup(config, path),
        Action::UpdateServer => runtime::update_server(native_config, true)
            .map(|()| "Palworld-Update abgeschlossen".to_owned()),
        Action::TogglePak { .. } | Action::ImportPak(_) | Action::QuarantinePak(_) => {
            return Err(
                "Native PAK-Verwaltung ist noch experimentell und in dieser Version gesperrt"
                    .to_owned(),
            );
        }
        Action::Diagnostics => return native_diagnostics(native_config),
    };
    result.map_err(|error| error.to_string())
}

fn set_native_setting(
    config: &BackendConfig,
    native_config: &native::NativeConfig,
    key: &str,
    input: &str,
) -> Result<String, String> {
    if matches!(key, "AdminPassword" | "RESTAPIEnabled" | "RESTAPIPort") {
        return Err("Diese Einstellung wird von PCC verwaltet".to_owned());
    }
    let next_config = if key == "ServerPlayerMaxNum" {
        let max_players = input
            .trim()
            .parse::<u32>()
            .map_err(|_| "Spielerzahl muss eine Ganzzahl sein".to_owned())?;
        if !(1..=128).contains(&max_players) {
            return Err("Spielerzahl muss zwischen 1 und 128 liegen".to_owned());
        }
        let mut next = native_config.clone();
        next.max_players = max_players;
        Some(next)
    } else {
        None
    };
    let settings =
        native::read_settings(&config.server_config).map_err(|error| error.to_string())?;
    let current = settings
        .get(key)
        .ok_or_else(|| "Unbekannte Palworld-Einstellung".to_owned())?;
    let value = native::parsed_input(current, input).map_err(|error| error.to_string())?;
    let updates = [(key.to_owned(), value)].into_iter().collect();
    native::update_settings(&config.server_config, &updates).map_err(|error| error.to_string())?;
    fix_config_ownership(config)?;
    if let Some(next) = next_config {
        next.save().map_err(|error| error.to_string())?;
        run_command(
            Command::new("chown")
                .arg(format!("root:{}", config.service_group))
                .arg(native::CONFIG_FILE),
        )?;
    }
    Ok(format!("{key} gespeichert · Neustart erforderlich"))
}

fn native_diagnostics(config: &native::NativeConfig) -> Result<String, String> {
    let base_name = format!(
        "palworld-control-center-diagnostics-{}-{}",
        std::process::id(),
        unix_now()
    );
    let directory = PathBuf::from("/tmp").join(&base_name);
    let output = PathBuf::from("/tmp").join(format!("{base_name}.tar.gz"));
    fs::create_dir(&directory)
        .map_err(|error| format!("Privates Diagnoseverzeichnis fehlt: {error}"))?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Diagnoseverzeichnis konnte nicht geschützt werden: {error}"))?;
    let mut config_summary = serde_json::to_value(config)
        .map_err(|error| format!("Diagnosekonfiguration ist ungültig: {error}"))?;
    if let Some(object) = config_summary.as_object_mut() {
        object.insert(
            "public_ip".to_owned(),
            Value::String("[REDACTED]".to_owned()),
        );
    }
    let config_summary = serde_json::to_vec_pretty(&config_summary)
        .map_err(|error| format!("Diagnosekonfiguration ist ungültig: {error}"))?;
    fs::write(directory.join("server.json"), config_summary).map_err(|error| {
        format!("Diagnosekonfiguration konnte nicht geschrieben werden: {error}")
    })?;
    for (name, program, arguments) in [
        (
            "service.txt",
            "systemctl",
            vec!["status", "palworld.service", "--no-pager"],
        ),
        (
            "journal.txt",
            "journalctl",
            vec!["--unit=palworld.service", "--lines=500", "--no-pager"],
        ),
        ("version.txt", "/usr/local/bin/pcc", vec!["--version"]),
    ] {
        let command_output = Command::new(program).args(arguments).output();
        let raw = command_output.map_or_else(
            |error| format!("{program} konnte nicht gestartet werden: {error}").into_bytes(),
            |result| [result.stdout, result.stderr].concat(),
        );
        let bytes = String::from_utf8_lossy(&raw)
            .lines()
            .map(normalize_log_line)
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes();
        fs::write(directory.join(name), bytes)
            .map_err(|error| format!("Diagnosedatei konnte nicht geschrieben werden: {error}"))?;
    }
    let result = run_command(
        Command::new("tar")
            .args(["--create", "--gzip", "--file"])
            .arg(&output)
            .args(["--directory", "/tmp", &base_name]),
    );
    let _ = fs::remove_dir_all(&directory);
    result.map(|_| format!("Diagnosepaket erstellt: {}", output.display()))
}

fn run_api(
    config: &BackendConfig,
    endpoint: &str,
    payload: Option<Value>,
) -> Result<String, String> {
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
    ]);
    command.arg(&config.rest_netrc).args(["--request", "POST"]);
    if payload.is_some() {
        command.args([
            "--header",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
        ]);
        command.stdin(Stdio::piped());
    }
    command
        .arg(config.rest_url(endpoint))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("REST-Aufruf konnte nicht gestartet werden: {error}"))?;
    if let Some(payload) = payload {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "REST-Eingabekanal fehlt".to_owned())?;
        serde_json::to_writer(&mut stdin, &payload)
            .map_err(|error| format!("REST-Nutzlast ist ungültig: {error}"))?;
        stdin
            .flush()
            .map_err(|error| format!("REST-Nutzlast konnte nicht gesendet werden: {error}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("REST-Aufruf ist fehlgeschlagen: {error}"))?;
    output_result(output, "REST-Aktion abgeschlossen")
}

fn set_setting(config: &BackendConfig, key: &str, input: &str) -> Result<String, String> {
    let validation = Command::new(&config.settings_catalog_tool)
        .arg("validate")
        .arg(key)
        .arg(input)
        .output()
        .map_err(|error| format!("Eingabeprüfung konnte nicht gestartet werden: {error}"))?;
    if !validation.status.success() {
        return Err(trim_output(&validation.stderr, &validation.stdout));
    }
    let value: Value = serde_json::from_slice(&validation.stdout)
        .map_err(|error| format!("Eingabeprüfung lieferte ungültiges JSON: {error}"))?;
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "Eingabeprüfung lieferte keinen Typ".to_owned())?;
    let normalized = value
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| "Eingabeprüfung lieferte keinen Wert".to_owned())?;
    run_command(
        Command::new(&config.config_tool)
            .arg("set")
            .arg(&config.server_config)
            .arg(key)
            .arg(kind)
            .arg(normalized),
    )?;
    let ownership = format!("{}:{}", config.service_user, config.service_group);
    run_command(
        Command::new("chown")
            .arg(ownership)
            .arg(&config.server_config),
    )?;
    Ok(format!(
        "{key} = {normalized} gespeichert · Neustart erforderlich"
    ))
}

fn toggle_pak(config: &BackendConfig, name: &str, enable: bool) -> Result<String, String> {
    let detail = pak_command(
        config,
        if enable { "enable" } else { "disable" },
        Some(name),
    )?;
    if enable {
        run_command(
            Command::new(&config.config_tool)
                .arg("set")
                .arg(&config.server_config)
                .args(["bAllowClientMod", "bool", "true"]),
        )?;
        fix_config_ownership(config)?;
    }
    Ok(format!("{detail} · Neustart erforderlich"))
}

fn import_pak(config: &BackendConfig, source: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("Importquelle ist nicht erreichbar: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err("Importquelle darf kein Symlink sein".to_owned());
    }
    if !metadata.is_file() && !metadata.is_dir() {
        return Err("Importquelle muss eine reguläre Datei oder ein Verzeichnis sein".to_owned());
    }
    let source = source
        .canonicalize()
        .map_err(|error| format!("Importquelle ist ungültig: {error}"))?;
    let mut command = pak_base_command(config);
    command.arg("import").arg(source).arg("--enable");
    let detail = run_command(&mut command)?;
    run_command(
        Command::new(&config.config_tool)
            .arg("set")
            .arg(&config.server_config)
            .args(["bAllowClientMod", "bool", "true"]),
    )?;
    fix_config_ownership(config)?;
    fix_pak_ownership(config)?;
    Ok(format!(
        "{detail} · Client-Mods aktiviert · Neustart erforderlich"
    ))
}

fn pak_command(
    config: &BackendConfig,
    command_name: &str,
    name: Option<&str>,
) -> Result<String, String> {
    let mut command = pak_base_command(config);
    command.arg(command_name);
    if let Some(name) = name {
        validate_pak_name(name)?;
        command.arg(name);
    }
    let result = run_command(&mut command)?;
    fix_pak_ownership(config)?;
    Ok(result)
}

fn validate_pak_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err("PAK-Name enthält unzulässige Pfadbestandteile".to_owned());
    }
    Ok(())
}

fn fix_pak_ownership(config: &BackendConfig) -> Result<(), String> {
    let owner = format!("root:{}", config.service_group);
    run_command(
        Command::new("chown")
            .args(["--recursive"])
            .arg(&owner)
            .arg(&config.pak_store)
            .arg(&config.pak_quarantine),
    )?;
    for directory in [&config.pak_store, &config.pak_quarantine] {
        run_command(
            Command::new("find")
                .arg(directory)
                .args(["-type", "d", "-exec", "chmod", "0750", "{}", "+"]),
        )?;
        run_command(
            Command::new("find")
                .arg(directory)
                .args(["-type", "f", "-exec", "chmod", "0640", "{}", "+"]),
        )?;
    }
    let target_owner = format!("{}:{}", config.service_user, config.service_group);
    run_command(
        Command::new("chown")
            .arg(target_owner)
            .arg(&config.pak_target),
    )?;
    Ok(())
}

fn fix_config_ownership(config: &BackendConfig) -> Result<(), String> {
    let ownership = format!("{}:{}", config.service_user, config.service_group);
    run_command(
        Command::new("chown")
            .arg(ownership)
            .arg(&config.server_config),
    )?;
    Ok(())
}

fn pak_base_command(config: &BackendConfig) -> Command {
    let mut command = Command::new(&config.pak_tool);
    command
        .arg("--store")
        .arg(&config.pak_store)
        .arg("--target")
        .arg(&config.pak_target)
        .arg("--quarantine")
        .arg(&config.pak_quarantine);
    command
}

fn verify_backup(config: &BackendConfig, path: &Path) -> Result<String, String> {
    let archive = validate_backup_path(config, path)?;
    let checksum = archive.with_extension("zst.sha256");
    let metadata =
        fs::symlink_metadata(&checksum).map_err(|_| "Prüfsummendatei fehlt".to_owned())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Prüfsumme muss eine reguläre Datei sein".to_owned());
    }
    let checksum = checksum
        .canonicalize()
        .map_err(|_| "Prüfsummendatei fehlt".to_owned())?;
    let backup_dir = config
        .backup_dir
        .canonicalize()
        .map_err(|error| format!("Backup-Verzeichnis ist ungültig: {error}"))?;
    if checksum.parent() != Some(backup_dir.as_path()) {
        return Err("Prüfsumme liegt außerhalb des Backup-Verzeichnisses".to_owned());
    }
    let checksum_name = checksum
        .file_name()
        .ok_or_else(|| "Ungültiger Prüfsummenname".to_owned())?;
    run_command(
        Command::new("sha256sum")
            .current_dir(&config.backup_dir)
            .arg("--check")
            .arg(checksum_name),
    )
}

fn delete_backup(config: &BackendConfig, path: &Path) -> Result<String, String> {
    let archive = validate_backup_path(config, path)?;
    let checksum = archive.with_extension("zst.sha256");
    fs::remove_file(&archive)
        .map_err(|error| format!("Backup konnte nicht gelöscht werden: {error}"))?;
    if checksum.is_file() {
        fs::remove_file(checksum)
            .map_err(|error| format!("Prüfsummendatei konnte nicht gelöscht werden: {error}"))?;
    }
    Ok(format!(
        "Backup gelöscht: {}",
        archive.file_name().unwrap_or_default().to_string_lossy()
    ))
}

fn validate_backup_path(config: &BackendConfig, path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Backup ist nicht erreichbar: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Backup muss eine reguläre Datei sein".to_owned());
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("Backup-Pfad ist ungültig: {error}"))?;
    let backup_dir = config
        .backup_dir
        .canonicalize()
        .map_err(|error| format!("Backup-Verzeichnis ist ungültig: {error}"))?;
    if canonical.parent() != Some(backup_dir.as_path()) {
        return Err("Backup liegt außerhalb des konfigurierten Verzeichnisses".to_owned());
    }
    let name = canonical
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !name.starts_with("palworld-") || !name.ends_with(".tar.zst") {
        return Err("Dateiname entspricht nicht dem Palworld-Backupformat".to_owned());
    }
    Ok(canonical)
}

fn run_command(command: &mut Command) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("Prozess konnte nicht gestartet werden: {error}"))?;
    output_result(output, "Aktion erfolgreich abgeschlossen")
}

fn output_result(output: std::process::Output, fallback: &str) -> Result<String, String> {
    let detail = trim_output(&output.stdout, &output.stderr);
    if output.status.success() {
        Ok(if detail.is_empty() {
            fallback.to_owned()
        } else {
            detail
        })
    } else {
        Err(if detail.is_empty() {
            format!("Prozess endete mit {}", output.status)
        } else {
            detail
        })
    }
}

fn trim_output(primary: &[u8], secondary: &[u8]) -> String {
    let primary = String::from_utf8_lossy(primary);
    let secondary = String::from_utf8_lossy(secondary);
    let mut lines = primary
        .lines()
        .chain(secondary.lines())
        .filter(|line| !line.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>();
    if lines.len() > 8 {
        lines = lines.split_off(lines.len() - 8);
    }
    lines.join(" · ")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Action, validate_pak_name};

    #[test]
    fn verification_is_read_only_but_restore_is_not() {
        let path = PathBuf::from("/var/backups/palworld/test.tar.zst");
        assert!(!Action::VerifyBackup(path.clone()).requires_write());
        assert!(Action::RestoreBackup(path).requires_write());
    }

    #[test]
    fn pak_names_cannot_escape_the_managed_store() {
        assert!(validate_pak_name("CreativeMenu").is_ok());
        assert!(validate_pak_name("../CreativeMenu").is_err());
        assert!(validate_pak_name("folder/CreativeMenu").is_err());
    }
}
