use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};

use crate::native::{
    ADMIN_PASSWORD_FILE, NativeConfig, REST_NETRC_FILE, SettingValue, atomic_write, update_settings,
};

const STEAMCMD_URL: &str = "https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz";
const INSTALL_MARKER: &str = "/var/lib/palworld-control-center/installing";
const STEAMCMD_INSTALL_ATTEMPTS: u32 = 3;

pub fn needs_first_run() -> bool {
    Path::new(INSTALL_MARKER).is_file() || !crate::native::exists()
}

pub fn run_first_start() -> Result<()> {
    require_root()?;
    if !Path::new("/usr/local/bin/pcc").exists() {
        bail!(
            "PCC ist noch nicht systemweit installiert. Bitte zuerst ausführen: sudo ./palworld-control-center-linux-amd64 install"
        );
    }
    if Path::new(INSTALL_MARKER).is_file()
        && crate::native::exists()
        && NativeConfig::load()
            .is_ok_and(|config| directory_not_empty(&config.server_dir.join("Pal/Saved/SaveGames")))
    {
        let password = fs::read_to_string(ADMIN_PASSWORD_FILE)
            .context("Admin-Passwort konnte nach der Installation nicht gelesen werden")?;
        println!("Eine zuvor unterbrochene Installation ist bereits vollständig gestartet.");
        println!("Admin-Passwort: {}", password.trim());
        println!("Bitte jetzt sicher speichern. Drücke Enter für das Dashboard.");
        let mut ignored = String::new();
        let _ = io::stdin().read_line(&mut ignored);
        fs::remove_file(INSTALL_MARKER)
            .context("Installationsstatus konnte nicht bereinigt werden")?;
        return Ok(());
    }
    if !Path::new(INSTALL_MARKER).is_file()
        && directory_not_empty(Path::new("/opt/palworld/server"))
    {
        bail!(
            "Vorhandene Palworld-Installation unter /opt/palworld/server erkannt. PCC überschreibt sie nicht. Für eine Neuinstallation zuerst Welt und Konfiguration sichern und die alte Installation bewusst entfernen; für die Legacy-Ansicht kann --config /etc/palworld/palworld.env verwendet werden."
        );
    }

    println!("\n╭──────────────────────────────────────────────────────────────╮");
    println!("│ PALWORLD CONTROL CENTER · ERSTEINRICHTUNG                    │");
    println!("╰──────────────────────────────────────────────────────────────╯\n");
    println!("Keine Palworld-Installation gefunden. PCC richtet jetzt den nativen");
    println!("Linux-Server mit SteamCMD, REST, Backups, Updates und systemd ein.\n");

    let server_name = prompt("Servername", "Svennis Palworld")?;
    let description = prompt("Beschreibung", "Privater Palworld Dedicated Server")?;
    let join_password = prompt_secret("Join-Passwort (leer = keines)")?;
    let max_players = prompt_number("Maximale Spieler", 32, 1, 128)? as u32;
    let game_port = prompt_number("Spiel-Port UDP", 8211, 1024, 65535)? as u16;
    let rest_port = loop {
        let port = prompt_number("REST-Port TCP", 8212, 1024, 65535)? as u16;
        if port != game_port {
            break port;
        }
        println!("Spiel- und REST-Port dürfen nicht identisch sein.");
    };
    let public_lobby = prompt_yes_no("In der Community-Liste anzeigen?", false)?;
    let public_ip = if public_lobby {
        prompt("Öffentliche IP (leer = automatisch)", "")?
    } else {
        String::new()
    };
    validate_public_ip(&public_ip)?;
    let configure_ufw = prompt_yes_no(
        "UFW sicher einrichten (SSH erlauben, REST extern sperren)?",
        true,
    )?;
    let ssh_port = if configure_ufw {
        prompt_number("Aktueller SSH-Port TCP", detect_ssh_port(), 1, 65535)? as u16
    } else {
        22
    };

    println!("\nZusammenfassung:");
    println!("  Server:       {server_name}");
    println!("  Spieler:      {max_players}");
    println!("  Spiel-Port:   UDP {game_port}");
    println!("  REST-Port:    TCP {rest_port} (extern gesperrt: {configure_ufw})");
    println!("  Öffentliche Liste: {public_lobby}");
    println!("  Daten:        /opt/palworld");
    println!("  Backups:      /var/backups/palworld\n");
    if !prompt_yes_no("Installation jetzt starten?", true)? {
        bail!("Installation wurde abgebrochen");
    }

    let config = NativeConfig {
        schema_version: 1,
        service_user: "palworld".to_owned(),
        service_group: "palworld".to_owned(),
        base_dir: PathBuf::from("/opt/palworld"),
        server_dir: PathBuf::from("/opt/palworld/server"),
        steamcmd_dir: PathBuf::from("/opt/palworld/steamcmd"),
        backup_dir: PathBuf::from("/var/backups/palworld"),
        state_dir: PathBuf::from("/var/lib/palworld-control-center"),
        game_port,
        rest_port,
        max_players,
        public_lobby,
        public_ip,
        public_port: game_port,
        backup_retention_days: 14,
        backup_max_count: 60,
        ufw_managed: configure_ufw,
    };
    install_server(
        &config,
        &server_name,
        &description,
        &join_password,
        ssh_port,
    )?;
    Ok(())
}

fn install_server(
    config: &NativeConfig,
    server_name: &str,
    description: &str,
    join_password: &str,
    ssh_port: u16,
) -> Result<()> {
    config.validate()?;
    let resuming = Path::new(INSTALL_MARKER).is_file();
    refuse_existing_world(config, resuming)?;
    if !resuming {
        ensure_directory(&config.state_dir, 0o700)?;
        atomic_write(Path::new(INSTALL_MARKER), b"native-rust-v1\n", 0o600)?;
    }
    println!("\n[1/9] Installiere Systemabhängigkeiten …");
    checked(Command::new("apt-get").arg("update"), "apt-get update")?;
    checked(
        Command::new("apt-get").args([
            "install",
            "-y",
            "--no-install-recommends",
            "ca-certificates",
            "curl",
            "tar",
            "gzip",
            "zstd",
            "iproute2",
            "ufw",
            "passwd",
            "util-linux",
            "coreutils",
            "lib32gcc-s1",
            "lib32stdc++6",
        ]),
        "Systemabhängigkeiten",
    )?;

    println!("[2/9] Erstelle isolierten Systembenutzer und Verzeichnisse …");
    ensure_service_user(config)?;
    create_directories(config)?;

    println!("[3/9] Lade und initialisiere SteamCMD direkt von Valves CDN …");
    install_steamcmd(config)?;
    warmup_steamcmd(config)?;

    println!("[4/9] Installiere Palworld Dedicated Server (Steam App 2394010) …");
    run_steamcmd(config, true)?;
    install_steam_sdk(config)?;

    println!("[5/9] Erzeuge geschützte Serverkonfiguration …");
    let admin_password = random_password()?;
    write_credentials(config, &admin_password)?;
    configure_palworld(
        config,
        server_name,
        description,
        join_password,
        &admin_password,
    )?;
    config.save()?;
    chown_path(
        Path::new(crate::native::CONFIG_FILE),
        "root",
        &config.service_group,
        false,
    )?;

    println!("[6/9] Installiere native PCC-Systemdienste und Timer …");
    install_units(config)?;

    println!("[7/9] Konfiguriere Netzwerkgrenze …");
    if config.ufw_managed {
        configure_ufw(config, ssh_port)?;
    } else {
        println!(
            "WARNUNG: TCP {} muss extern durch eine Firewall gesperrt werden.",
            config.rest_port
        );
    }

    println!("[8/9] Starte Palworld …");
    checked(
        Command::new("systemctl").args(["enable", "--now", "palworld.service"]),
        "Palworld-Dienststart",
    )?;

    println!("[9/9] Warte auf die lokale REST-API …");
    let ready = wait_for_api(config, 360);
    println!("\nInstallation abgeschlossen.");
    println!(
        "Palworld REST: {}",
        if ready { "erreichbar" } else { "startet noch" }
    );
    println!("Admin-Passwort: {admin_password}");
    println!("Bitte jetzt sicher speichern; PCC zeigt es später nicht im Dashboard an.");
    println!("\nDrücke Enter, um das Dashboard zu öffnen.");
    let mut ignored = String::new();
    let _ = io::stdin().read_line(&mut ignored);
    fs::remove_file(INSTALL_MARKER)
        .context("Installationsstatus konnte nicht abgeschlossen werden")?;
    Ok(())
}

fn refuse_existing_world(config: &NativeConfig, resuming: bool) -> Result<()> {
    let saves = config.server_dir.join("Pal/Saved/SaveGames");
    if directory_not_empty(&saves) {
        bail!(
            "Vorhandene Welt erkannt: {}. PCC überschreibt keine SaveGames.",
            saves.display()
        );
    }
    if !resuming && directory_not_empty(&config.server_dir) {
        bail!(
            "Serververzeichnis ist nicht leer: {}. Bitte alte Installation zuerst sichern und bewusst entfernen.",
            config.server_dir.display()
        );
    }
    Ok(())
}

fn ensure_service_user(config: &NativeConfig) -> Result<()> {
    let group = Command::new("getent")
        .args(["group", &config.service_group])
        .output()
        .context("Systemgruppe konnte nicht geprüft werden")?;
    if !group.status.success() {
        checked(
            Command::new("groupadd").args(["--system", &config.service_group]),
            "Palworld-Systemgruppe",
        )?;
    } else {
        let text = String::from_utf8_lossy(&group.stdout);
        let gid = text
            .trim()
            .split(':')
            .nth(2)
            .and_then(|value| value.parse::<u32>().ok())
            .context("Palworld-Systemgruppe hat keine gültige GID")?;
        if gid == 0 || gid >= 1_000 {
            bail!("Vorhandene Gruppe palworld ist keine sichere Systemgruppe");
        }
    }
    let user = Command::new("id")
        .args(["--user", &config.service_user])
        .output()
        .context("Systembenutzer konnte nicht geprüft werden")?;
    if !user.status.success() {
        checked(
            Command::new("useradd").args([
                "--system",
                "--gid",
                &config.service_group,
                "--home-dir",
                config.base_dir.to_string_lossy().as_ref(),
                "--create-home",
                "--shell",
                "/usr/sbin/nologin",
                &config.service_user,
            ]),
            "Palworld-Systembenutzer",
        )?;
    } else {
        let uid = String::from_utf8_lossy(&user.stdout)
            .trim()
            .parse::<u32>()
            .context("Palworld-Systembenutzer hat keine gültige UID")?;
        if uid == 0 || uid >= 1_000 {
            bail!("Vorhandener Benutzer palworld ist kein sicherer Systembenutzer");
        }
        checked(
            Command::new("usermod")
                .args(["--gid", &config.service_group, "--home"])
                .arg(&config.base_dir)
                .args([
                    "--shell",
                    "/usr/sbin/nologin",
                    "--lock",
                    &config.service_user,
                ]),
            "Palworld-Systembenutzer absichern",
        )?;
    }
    Ok(())
}

fn create_directories(config: &NativeConfig) -> Result<()> {
    for path in [
        &config.base_dir,
        &config.server_dir,
        &config.steamcmd_dir,
        &config.state_dir,
        &config.pak_store(),
        &config.pak_target(),
        &config.pak_quarantine(),
    ] {
        ensure_directory(path, 0o750)?;
    }
    ensure_directory(&config.backup_dir, 0o700)?;
    fs::set_permissions(
        Path::new("/etc/palworld-control-center"),
        fs::Permissions::from_mode(0o750),
    )?;
    chown_path(
        Path::new("/etc/palworld-control-center"),
        "root",
        &config.service_group,
        false,
    )?;
    chown_path(
        &config.base_dir,
        &config.service_user,
        &config.service_group,
        true,
    )?;
    chown_path(&config.state_dir, "root", &config.service_group, true)?;
    Ok(())
}

fn ensure_directory(path: &Path, mode: u32) -> Result<()> {
    reject_symlink_components(path)?;
    fs::create_dir_all(path)?;
    reject_symlink_components(path)?;
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Verzeichnis ist nicht erreichbar: {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "Installationspfad muss ein echtes Verzeichnis sein: {}",
            path.display()
        );
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("Installationspfad muss absolut sein: {}", path.display());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push("/"),
            Component::Normal(part) => current.push(part),
            _ => bail!("Installationspfad enthält unzulässige Komponenten"),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("Symlink im Installationspfad: {}", current.display());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn install_steamcmd(config: &NativeConfig) -> Result<()> {
    let archive = PathBuf::from(format!("/tmp/pcc-steamcmd-{}.tar.gz", std::process::id()));
    if archive.exists() {
        bail!(
            "Temporäres SteamCMD-Archiv existiert bereits: {}",
            archive.display()
        );
    }
    checked(
        Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--proto",
                "=https",
                "--tlsv1.2",
                "--retry",
                "5",
                "--retry-all-errors",
                "--output",
            ])
            .arg(&archive)
            .arg(STEAMCMD_URL),
        "SteamCMD-Download",
    )?;
    checked(
        Command::new("runuser")
            .args(["-u", &config.service_user, "--", "tar"])
            .args(["--extract", "--gzip", "--file"])
            .arg(&archive)
            .args(["--directory"])
            .arg(&config.steamcmd_dir)
            .args(["--no-same-owner", "--no-same-permissions"]),
        "SteamCMD-Extraktion",
    )?;
    fs::remove_file(&archive)?;
    chown_path(
        &config.steamcmd_dir,
        &config.service_user,
        &config.service_group,
        true,
    )
}

fn steamcmd_base_command(config: &NativeConfig) -> Command {
    let steamcmd = config.steamcmd_dir.join("steamcmd.sh");
    let mut command = Command::new("runuser");
    command
        .args(["-u", &config.service_user, "--", "env"])
        .arg(format!("HOME={}", config.base_dir.display()))
        .arg(format!("USER={}", config.service_user))
        .arg(format!("LOGNAME={}", config.service_user))
        .arg("LC_ALL=C.UTF-8")
        .arg(&steamcmd);
    command
}

fn warmup_steamcmd(config: &NativeConfig) -> Result<()> {
    let mut command = steamcmd_base_command(config);
    command.args(["+login", "anonymous", "+quit"]);
    checked(&mut command, "SteamCMD-Initialisierung")
}

fn run_steamcmd(config: &NativeConfig, validate: bool) -> Result<()> {
    let mut last_status = None;
    for attempt in 1..=STEAMCMD_INSTALL_ATTEMPTS {
        let mut command = steamcmd_base_command(config);
        command
            .args([
                "+@sSteamCmdForcePlatformType",
                "linux",
                "+@sSteamCmdForcePlatformBitness",
                "64",
                "+force_install_dir",
            ])
            .arg(&config.server_dir)
            .args(["+login", "anonymous", "+app_update", "2394010"]);
        if validate {
            command.arg("validate");
        }
        command.arg("+quit");

        let status = command
            .status()
            .context("SteamCMD Palworld-Installation konnte nicht gestartet werden")?;
        if status.success() {
            return Ok(());
        }
        last_status = Some(status);
        if attempt < STEAMCMD_INSTALL_ATTEMPTS {
            let delay = Duration::from_secs(u64::from(attempt) * 5);
            println!(
                "WARNUNG: SteamCMD-Versuch {attempt}/{STEAMCMD_INSTALL_ATTEMPTS} ist fehlgeschlagen. Neuer Versuch in {} Sekunden …",
                delay.as_secs()
            );
            thread::sleep(delay);
            warmup_steamcmd(config)?;
        }
    }

    let status = last_status.context("SteamCMD lieferte keinen Exit-Status")?;
    bail!(
        "SteamCMD Palworld-Installation ist nach {STEAMCMD_INSTALL_ATTEMPTS} Versuchen mit Status {status} fehlgeschlagen"
    )
}

fn install_steam_sdk(config: &NativeConfig) -> Result<()> {
    let sdk = config.base_dir.join(".steam/sdk64");
    ensure_directory(&sdk, 0o750)?;
    let source = config.steamcmd_dir.join("linux64/steamclient.so");
    if source.is_file() {
        let metadata = fs::symlink_metadata(&source)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("Steam-Runtimebibliothek ist keine reguläre Datei");
        }
        atomic_write(&sdk.join("steamclient.so"), &fs::read(source)?, 0o640)?;
    }
    chown_path(
        &config.base_dir.join(".steam"),
        &config.service_user,
        &config.service_group,
        true,
    )
}

fn write_credentials(config: &NativeConfig, password: &str) -> Result<()> {
    atomic_write(Path::new(ADMIN_PASSWORD_FILE), password.as_bytes(), 0o640)?;
    let netrc = format!("machine 127.0.0.1 login admin password {password}\n");
    atomic_write(Path::new(REST_NETRC_FILE), netrc.as_bytes(), 0o640)?;
    for path in [ADMIN_PASSWORD_FILE, REST_NETRC_FILE] {
        chown_path(Path::new(path), "root", &config.service_group, false)?;
    }
    Ok(())
}

fn configure_palworld(
    config: &NativeConfig,
    server_name: &str,
    description: &str,
    join_password: &str,
    admin_password: &str,
) -> Result<()> {
    let destination = config.settings_file();
    let parent = destination
        .parent()
        .context("Palworld-Konfigurationspfad fehlt")?;
    ensure_directory(parent, 0o750)?;
    let default = config.server_dir.join("DefaultPalWorldSettings.ini");
    let metadata = fs::symlink_metadata(&default)
        .with_context(|| format!("DefaultPalWorldSettings.ini fehlt: {}", default.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("DefaultPalWorldSettings.ini muss eine reguläre Datei sein");
    }
    atomic_write(&destination, &fs::read(&default)?, 0o640)?;
    let mut updates = BTreeMap::new();
    updates.insert(
        "ServerName".to_owned(),
        SettingValue::String(server_name.to_owned()),
    );
    updates.insert(
        "ServerDescription".to_owned(),
        SettingValue::String(description.to_owned()),
    );
    updates.insert(
        "ServerPassword".to_owned(),
        SettingValue::String(join_password.to_owned()),
    );
    updates.insert(
        "AdminPassword".to_owned(),
        SettingValue::String(admin_password.to_owned()),
    );
    updates.insert(
        "ServerPlayerMaxNum".to_owned(),
        SettingValue::Integer(u64::from(config.max_players)),
    );
    updates.insert("RESTAPIEnabled".to_owned(), SettingValue::Bool(true));
    updates.insert(
        "RESTAPIPort".to_owned(),
        SettingValue::Integer(u64::from(config.rest_port)),
    );
    updates.insert("RCONEnabled".to_owned(), SettingValue::Bool(false));
    updates.insert(
        "PublicPort".to_owned(),
        SettingValue::Integer(u64::from(config.public_port)),
    );
    updates.insert(
        "PublicIP".to_owned(),
        SettingValue::String(config.public_ip.clone()),
    );
    updates.insert(
        "CrossplayPlatforms".to_owned(),
        SettingValue::Raw("(Steam,Xbox,PS5,Mac)".to_owned()),
    );
    updates.insert("bAllowClientMod".to_owned(), SettingValue::Bool(false));
    updates.insert("bIsUseBackupSaveData".to_owned(), SettingValue::Bool(true));
    updates.insert(
        "bIsShowJoinLeftMessage".to_owned(),
        SettingValue::Bool(true),
    );
    updates.insert(
        "LogFormatType".to_owned(),
        SettingValue::Raw("Json".to_owned()),
    );
    update_settings(&destination, &updates)?;
    chown_path(
        &destination,
        &config.service_user,
        &config.service_group,
        false,
    )
}

fn install_units(config: &NativeConfig) -> Result<()> {
    let public_args = if config.public_lobby {
        format!(
            " -publiclobby{} -publicport={}",
            if config.public_ip.is_empty() {
                String::new()
            } else {
                format!(" -publicip={}", config.public_ip)
            },
            config.public_port
        )
    } else {
        String::new()
    };
    let service = format!(
        "[Unit]\nDescription=Palworld Dedicated Server (PCC)\nDocumentation=https://docs.palworldgame.com/\nWants=network-online.target\nAfter=network-online.target\nStartLimitIntervalSec=600\nStartLimitBurst=5\n\n[Service]\nType=simple\nUser={}\nGroup={}\nWorkingDirectory={}\nUMask=0027\nExecStartPre=/usr/local/bin/pcc --internal-task prepare\nExecStart={}/PalServer.sh -port={}{}\nExecStop=/usr/local/bin/pcc --internal-task graceful-stop\nRestart=always\nRestartSec=12\nTimeoutStartSec=300\nTimeoutStopSec=90\nKillMode=mixed\nLimitNOFILE=1048576\nTasksMax=infinity\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nProtectHome=true\nProtectHostname=true\nProtectClock=true\nProtectKernelTunables=true\nProtectKernelModules=true\nProtectKernelLogs=true\nReadWritePaths={}\n\n[Install]\nWantedBy=multi-user.target\n",
        config.service_user,
        config.service_group,
        config.server_dir.display(),
        config.server_dir.display(),
        config.game_port,
        public_args,
        config.base_dir.display(),
    );
    let backup_service = format!(
        "[Unit]\nDescription=Palworld PCC Backup\nConditionPathExists=/etc/palworld-control-center/server.json\n\n[Service]\nType=oneshot\nExecStart=/usr/local/bin/pcc --internal-task backup\nUMask=0077\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nProtectHome=true\nProtectKernelTunables=true\nProtectKernelModules=true\nProtectKernelLogs=true\nReadWritePaths={}\nTimeoutStartSec=30min\n",
        config.backup_dir.display()
    );
    let backup_timer = "[Unit]\nDescription=Palworld PCC Backup Timer\n\n[Timer]\nOnBootSec=15min\nOnUnitActiveSec=3h\nPersistent=true\nRandomizedDelaySec=10min\nUnit=palworld-backup.service\n\n[Install]\nWantedBy=timers.target\n";
    let update_service = format!(
        "[Unit]\nDescription=Palworld PCC Server Update\nConditionPathExists=/etc/palworld-control-center/server.json\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=oneshot\nExecStart=/usr/local/bin/pcc --internal-task server-update\nUMask=0077\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nProtectHome=true\nProtectKernelTunables=true\nProtectKernelModules=true\nProtectKernelLogs=true\nReadWritePaths={} {}\nTimeoutStartSec=0\n",
        config.base_dir.display(),
        config.backup_dir.display()
    );
    let update_timer = "[Unit]\nDescription=Palworld PCC Server Update Timer\n\n[Timer]\nOnBootSec=30min\nOnUnitActiveSec=3h\nPersistent=true\nRandomizedDelaySec=20min\nUnit=palworld-update.service\n\n[Install]\nWantedBy=timers.target\n";

    for (path, content) in [
        ("/etc/systemd/system/palworld.service", service.as_str()),
        (
            "/etc/systemd/system/palworld-backup.service",
            backup_service.as_str(),
        ),
        ("/etc/systemd/system/palworld-backup.timer", backup_timer),
        (
            "/etc/systemd/system/palworld-update.service",
            update_service.as_str(),
        ),
        ("/etc/systemd/system/palworld-update.timer", update_timer),
    ] {
        atomic_write(Path::new(path), content.as_bytes(), 0o644)?;
    }
    checked(
        Command::new("systemctl").arg("daemon-reload"),
        "systemd reload",
    )?;
    checked(
        Command::new("systemctl").args([
            "enable",
            "--now",
            "palworld-backup.timer",
            "palworld-update.timer",
        ]),
        "Palworld-Timer",
    )
}

fn configure_ufw(config: &NativeConfig, ssh_port: u16) -> Result<()> {
    checked(
        Command::new("ufw").args(["allow", &format!("{ssh_port}/tcp"), "comment", "SSH"]),
        "UFW SSH-Regel",
    )?;
    checked(
        Command::new("ufw").args([
            "allow",
            &format!("{}/udp", config.game_port),
            "comment",
            "Palworld game",
        ]),
        "UFW Spielregel",
    )?;
    checked(
        Command::new("ufw").args([
            "insert",
            "1",
            "allow",
            "in",
            "on",
            "lo",
            "to",
            "any",
            "port",
            &config.rest_port.to_string(),
            "proto",
            "tcp",
            "comment",
            "Palworld REST local",
        ]),
        "UFW REST-Loopbackregel",
    )?;
    checked(
        Command::new("ufw").args([
            "insert",
            "2",
            "deny",
            "in",
            "to",
            "any",
            "port",
            &config.rest_port.to_string(),
            "proto",
            "tcp",
            "comment",
            "Palworld REST protected",
        ]),
        "UFW REST-Sperre",
    )?;
    checked(
        Command::new("ufw").args(["--force", "enable"]),
        "UFW-Aktivierung",
    )
}

fn wait_for_api(config: &NativeConfig, seconds: u64) -> bool {
    for _ in 0..seconds {
        let ready = Command::new("curl")
            .args([
                "--silent",
                "--fail",
                "--connect-timeout",
                "1",
                "--max-time",
                "2",
                "--noproxy",
                "*",
                "--netrc-file",
                REST_NETRC_FILE,
            ])
            .arg(format!("http://127.0.0.1:{}/v1/api/info", config.rest_port))
            .status()
            .is_ok_and(|status| status.success());
        if ready {
            return true;
        }
        thread::sleep(Duration::from_secs(1));
    }
    false
}

fn random_password() -> Result<String> {
    let mut bytes = [0_u8; 20];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn prompt(label: &str, default: &str) -> Result<String> {
    print!(
        "{label} [{}]: ",
        if default.is_empty() { "leer" } else { default }
    );
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim();
    let result = if value.is_empty() { default } else { value };
    if result.len() > 128 || result.chars().any(char::is_control) {
        bail!("Ungültige Eingabe für {label}");
    }
    Ok(result.to_owned())
}

fn prompt_secret(label: &str) -> Result<String> {
    struct EchoGuard;
    impl Drop for EchoGuard {
        fn drop(&mut self) {
            let _ = Command::new("stty").arg("echo").status();
            println!();
        }
    }

    print!("{label}: ");
    io::stdout().flush()?;
    checked(Command::new("stty").arg("-echo"), "Terminal-Echoschutz")?;
    let guard = EchoGuard;
    let mut value = String::new();
    let read = io::stdin().read_line(&mut value);
    drop(guard);
    read?;
    let value = value.trim();
    if value.len() > 128 || value.chars().any(char::is_control) {
        bail!("Ungültige Eingabe für {label}");
    }
    Ok(value.to_owned())
}

fn prompt_number(label: &str, default: u64, minimum: u64, maximum: u64) -> Result<u64> {
    loop {
        let input = prompt(label, &default.to_string())?;
        if let Ok(value) = input.parse::<u64>()
            && (minimum..=maximum).contains(&value)
        {
            return Ok(value);
        }
        println!("Bitte eine Zahl zwischen {minimum} und {maximum} eingeben.");
    }
}

fn prompt_yes_no(label: &str, default: bool) -> Result<bool> {
    let marker = if default { "J/n" } else { "j/N" };
    print!("{label} [{marker}]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    match input.trim().to_ascii_lowercase().as_str() {
        "" => Ok(default),
        "j" | "ja" | "y" | "yes" => Ok(true),
        "n" | "nein" | "no" => Ok(false),
        _ => {
            println!("Bitte j oder n eingeben.");
            prompt_yes_no(label, default)
        }
    }
}

fn detect_ssh_port() -> u64 {
    let output = Command::new("sshd").arg("-T").output();
    output
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .find_map(|line| line.strip_prefix("port ")?.parse().ok())
        })
        .unwrap_or(22)
}

fn validate_public_ip(value: &str) -> Result<()> {
    if !value.is_empty() && value.parse::<std::net::IpAddr>().is_err() {
        bail!("Öffentliche IP ist keine gültige IPv4- oder IPv6-Adresse");
    }
    Ok(())
}

fn directory_not_empty(path: &Path) -> bool {
    fs::read_dir(path)
        .ok()
        .and_then(|mut entries| entries.next())
        .is_some()
}

fn chown_path(path: &Path, user: &str, group: &str, recursive: bool) -> Result<()> {
    let mut command = Command::new("chown");
    if recursive {
        command.arg("--recursive");
    }
    command.arg(format!("{user}:{group}")).arg(path);
    checked(&mut command, "Besitzrechte")
}

fn checked(command: &mut Command, label: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("{label} konnte nicht gestartet werden"))?;
    if !status.success() {
        bail!("{label} ist mit Status {status} fehlgeschlagen");
    }
    Ok(())
}

fn require_root() -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    if fs::metadata("/proc/self")?.uid() != 0 {
        bail!("Die erste Serverinstallation benötigt Root-Rechte: sudo pcc");
    }
    Ok(())
}
