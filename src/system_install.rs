use std::{
    cmp::Ordering,
    env, fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::Path,
    process::Command,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const INSTALL_DIR: &str = "/usr/local/bin";
const INSTALLED_BINARY: &str = "/usr/local/bin/palworld-control-center";
const SHORT_COMMAND: &str = "/usr/local/bin/pcc";
const CONFIG_DIR: &str = "/etc/palworld-control-center";
const CACHE_DIR: &str = "/var/cache/palworld-control-center";
const UPDATE_CHANNEL_FILE: &str = "/etc/palworld-control-center/update-channel";
const UPDATE_SERVICE: &str = "/etc/systemd/system/palworld-control-center-update.service";
const UPDATE_TIMER: &str = "/etc/systemd/system/palworld-control-center-update.timer";
const RELEASES_URL: &str =
    "https://api.github.com/repos/Ostsee-Developer/palworld-control-center/releases";
const REPOSITORY_RELEASE_PREFIX: &str =
    "https://github.com/Ostsee-Developer/palworld-control-center/releases/download/";
const BINARY_ASSET: &str = "palworld-control-center-linux-amd64";
const CHECKSUM_ASSET: &str = "palworld-control-center-linux-amd64.sha256";

const SERVICE_UNIT: &str = r#"[Unit]
Description=Palworld Control Center Update
Documentation=https://github.com/Ostsee-Developer/palworld-control-center
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/pcc --internal-task self-update
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectHome=true
ProtectHostname=true
ProtectClock=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectKernelLogs=true
ProtectSystem=strict
ReadWritePaths=/usr/local/bin /var/cache/palworld-control-center
LockPersonality=true
RestrictRealtime=true
RestrictSUIDSGID=true
SystemCallArchitectures=native
IOSchedulingClass=idle
TimeoutStartSec=10min
"#;

const TIMER_UNIT: &str = r#"[Unit]
Description=Daily Palworld Control Center update check

[Timer]
OnCalendar=daily
Persistent=true
RandomizedDelaySec=2h
AccuracySec=15min
Unit=palworld-control-center-update.service

[Install]
WantedBy=timers.target
"#;

pub fn install_panel() -> Result<()> {
    require_root("sudo ./palworld-control-center-linux-amd64 install")?;
    require_supported_host()?;
    ensure_panel_dependencies()?;

    let source = env::current_exe().context("Pfad der laufenden PCC-Binary fehlt")?;
    validate_regular_executable(&source, "PCC-Binary")?;
    create_protected_directory(Path::new(INSTALL_DIR), 0o755)
        .context("/usr/local/bin konnte nicht sicher angelegt werden")?;
    atomic_install_binary(&source, Path::new(INSTALLED_BINARY))?;
    install_short_command()?;

    create_protected_directory(Path::new(CONFIG_DIR), 0o750)?;
    create_protected_directory(Path::new(CACHE_DIR), 0o700)?;
    if !Path::new(UPDATE_CHANNEL_FILE).exists() {
        atomic_write(Path::new(UPDATE_CHANNEL_FILE), b"prerelease\n", 0o640)?;
    }
    atomic_write(Path::new(UPDATE_SERVICE), SERVICE_UNIT.as_bytes(), 0o644)?;
    atomic_write(Path::new(UPDATE_TIMER), TIMER_UNIT.as_bytes(), 0o644)?;
    checked_command("systemctl", &["daemon-reload"])?;
    checked_command(
        "systemctl",
        &["enable", "--now", "palworld-control-center-update.timer"],
    )?;

    println!(
        "\nPCC {} wurde systemweit installiert.",
        env!("CARGO_PKG_VERSION")
    );
    println!("Starte jetzt einfach: sudo pcc");
    println!("Beim ersten Start öffnet PCC automatisch den Palworld-Installationsassistenten.");
    Ok(())
}

fn ensure_panel_dependencies() -> Result<()> {
    let curl_available = Command::new("curl")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if curl_available {
        return Ok(());
    }
    checked_command("apt-get", &["update"])?;
    checked_command(
        "apt-get",
        &[
            "install",
            "-y",
            "--no-install-recommends",
            "ca-certificates",
            "curl",
        ],
    )
}

pub fn self_update() -> Result<()> {
    require_root("sudo pcc")?;
    let channel = fs::read_to_string(UPDATE_CHANNEL_FILE)
        .unwrap_or_else(|_| "prerelease".to_owned())
        .trim()
        .to_owned();
    if channel != "stable" && channel != "prerelease" {
        bail!("Ungültiger PCC-Updatekanal: {channel}");
    }

    let temporary = Path::new(CACHE_DIR).join(format!("update-{}", std::process::id()));
    if temporary.exists() {
        bail!(
            "Temporäres Updateverzeichnis existiert bereits: {}",
            temporary.display()
        );
    }
    fs::create_dir_all(&temporary).context("Updateverzeichnis konnte nicht erstellt werden")?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
    let result = perform_update(&temporary, &channel);
    let _ = fs::remove_dir_all(&temporary);
    result
}

fn perform_update(temporary: &Path, channel: &str) -> Result<()> {
    let releases_file = temporary.join("releases.json");
    download(RELEASES_URL, &releases_file)?;
    let releases: Vec<Release> = serde_json::from_slice(
        &fs::read(&releases_file).context("GitHub-Releaseantwort konnte nicht gelesen werden")?,
    )
    .context("GitHub-Releaseantwort ist ungültig")?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .context("Die eingebaute PCC-Version ist ungültig")?;
    let candidate = releases
        .iter()
        .filter(|release| !release.draft && (channel == "prerelease" || !release.prerelease))
        .filter_map(|release| Version::parse(&release.tag_name).map(|version| (release, version)))
        .filter(|(_, version)| version > &current)
        .max_by(|(_, left), (_, right)| left.cmp(right));
    let Some((release, target)) = candidate else {
        return Ok(());
    };

    let binary_url = release.asset_url(BINARY_ASSET)?;
    let checksum_url = release.asset_url(CHECKSUM_ASSET)?;
    let expected_prefix = format!("{REPOSITORY_RELEASE_PREFIX}{}/", release.tag_name);
    if !binary_url.starts_with(&expected_prefix) || !checksum_url.starts_with(&expected_prefix) {
        bail!("Release-Assets stammen nicht vom erwarteten PCC-Release");
    }

    let binary = temporary.join(BINARY_ASSET);
    let checksum = temporary.join(CHECKSUM_ASSET);
    download(binary_url, &binary)?;
    download(checksum_url, &checksum)?;
    verify_download(&binary, &checksum)?;
    validate_amd64_elf(&binary)?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700))
        .context("Update-Binary konnte nicht ausführbar gemacht werden")?;
    validate_reported_version(&binary, target.raw())?;
    atomic_install_binary(&binary, Path::new(INSTALLED_BINARY))?;
    println!("PCC wurde auf {} aktualisiert.", target.raw());
    Ok(())
}

fn download(url: &str, destination: &Path) -> Result<()> {
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--connect-timeout",
            "10",
            "--max-time",
            "180",
            "--retry",
            "3",
            "--retry-all-errors",
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "X-GitHub-Api-Version: 2022-11-28",
            "--user-agent",
            "palworld-control-center",
            "--output",
        ])
        .arg(destination)
        .arg(url)
        .status()
        .context("curl konnte nicht gestartet werden")?;
    if !status.success() {
        bail!("Download ist fehlgeschlagen: {url}");
    }
    Ok(())
}

fn verify_download(binary: &Path, checksum_file: &Path) -> Result<()> {
    let checksum = fs::read_to_string(checksum_file).context("Prüfsummendatei fehlt")?;
    let mut fields = checksum.split_whitespace();
    let expected = fields.next().context("SHA-256 fehlt")?;
    let name = fields.next().context("Dateiname in SHA-256-Datei fehlt")?;
    if expected.len() != 64
        || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
        || name.trim_start_matches('*') != BINARY_ASSET
        || fields.next().is_some()
    {
        bail!("Prüfsummendatei ist ungültig");
    }
    let mut file = fs::File::open(binary).context("Update-Binary fehlt")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("SHA-256-Prüfung des PCC-Updates fehlgeschlagen");
    }
    Ok(())
}

fn validate_amd64_elf(path: &Path) -> Result<()> {
    let mut header = [0_u8; 20];
    fs::File::open(path)?.read_exact(&mut header)?;
    if &header[..4] != b"\x7fELF"
        || header[4] != 2
        || header[5] != 1
        || u16::from_le_bytes([header[18], header[19]]) != 62
    {
        bail!("Update ist keine Linux-amd64-ELF-Binary");
    }
    Ok(())
}

fn validate_reported_version(binary: &Path, expected: &str) -> Result<()> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .context("Update-Binary konnte nicht geprüft werden")?;
    if !output.status.success() {
        bail!("Update-Binary beantwortet --version nicht");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if text.split_whitespace().last() != Some(expected) {
        bail!("Binary-Version stimmt nicht mit dem Release-Tag überein");
    }
    Ok(())
}

fn atomic_install_binary(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("Installationsziel ohne Elternpfad")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".pcc-install-{}", std::process::id()));
    if temporary.exists() {
        bail!(
            "Temporäre Installationsdatei existiert bereits: {}",
            temporary.display()
        );
    }
    let result = (|| -> Result<()> {
        fs::copy(source, &temporary).context("PCC-Binary konnte nicht kopiert werden")?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))?;
        fs::File::open(&temporary)?.sync_all()?;
        fs::rename(&temporary, destination)
            .context("PCC-Binary konnte nicht atomar aktiviert werden")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn install_short_command() -> Result<()> {
    let short = Path::new(SHORT_COMMAND);
    match fs::symlink_metadata(short) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if fs::read_link(short)? == Path::new("palworld-control-center") {
                return Ok(());
            }
            bail!("{SHORT_COMMAND} ist bereits ein fremder Symlink");
        }
        Ok(_) => bail!("{SHORT_COMMAND} existiert bereits und wird nicht überschrieben"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    symlink("palworld-control-center", short).context("Kurzbefehl pcc konnte nicht angelegt werden")
}

fn atomic_write(path: &Path, content: &[u8], mode: u32) -> Result<()> {
    let parent = path.parent().context("Zieldatei ohne Elternpfad")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".pcc-write-{}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| {
                format!(
                    "Temporäre Datei konnte nicht erstellt werden: {}",
                    temporary.display()
                )
            })?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn create_protected_directory(path: &Path, mode: u32) -> Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "Geschützter Pfad ist kein reguläres Verzeichnis: {}",
            path.display()
        );
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn validate_regular_executable(path: &Path, label: &str) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("{label} fehlt: {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.mode() & 0o111 == 0 {
        bail!(
            "{label} muss eine reguläre ausführbare Datei sein: {}",
            path.display()
        );
    }
    Ok(())
}

fn require_supported_host() -> Result<()> {
    let os = fs::read_to_string("/etc/os-release").context("/etc/os-release fehlt")?;
    let id = os_release_value(&os, "ID").unwrap_or_default();
    let version = os_release_value(&os, "VERSION_ID").unwrap_or_default();
    if !matches!(
        (id.as_str(), version.as_str()),
        ("debian", "13") | ("ubuntu", "26.04")
    ) {
        bail!("Unterstützt werden Debian 13 und Ubuntu Server 26.04 LTS; erkannt: {id} {version}");
    }
    if env::consts::ARCH != "x86_64" || !Path::new("/run/systemd/system").is_dir() {
        bail!("PCC benötigt Linux amd64 mit systemd");
    }
    Ok(())
}

fn os_release_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (candidate, value) = line.split_once('=')?;
        (candidate == key).then(|| value.trim_matches(['\'', '"']).to_owned())
    })
}

fn require_root(hint: &str) -> Result<()> {
    if fs::metadata("/proc/self")?.uid() != 0 {
        bail!("Root-Rechte erforderlich. Bitte ausführen: {hint}");
    }
    Ok(())
}

fn checked_command(program: &str, arguments: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(arguments)
        .status()
        .with_context(|| format!("{program} konnte nicht gestartet werden"))?;
    if !status.success() {
        bail!("{program} wurde mit Status {status} beendet");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

impl Release {
    fn asset_url(&self, name: &str) -> Result<&str> {
        self.assets
            .iter()
            .find(|asset| asset.name == name)
            .map(|asset| asset.browser_download_url.as_str())
            .with_context(|| format!("Release-Asset fehlt: {name}"))
    }
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Version {
    raw: String,
    core: [u64; 3],
    prerelease: Option<Vec<String>>,
}

impl Version {
    fn parse(value: &str) -> Option<Self> {
        let raw = value.trim_start_matches('v').to_owned();
        let without_build = raw.split('+').next()?;
        let (core, prerelease) = without_build
            .split_once('-')
            .map_or((without_build, None), |(core, pre)| {
                (core, Some(pre.split('.').map(str::to_owned).collect()))
            });
        let values = core
            .split('.')
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        if values.len() != 3
            || prerelease.as_ref().is_some_and(|parts: &Vec<String>| {
                parts.is_empty() || parts.iter().any(String::is_empty)
            })
        {
            return None;
        }
        Some(Self {
            raw,
            core: [values[0], values[1], values[2]],
            prerelease,
        })
    }

    fn raw(&self) -> &str {
        &self.raw
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core
            .cmp(&other.core)
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => compare_prerelease(left, right),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_prerelease(left: &[String], right: &[String]) -> Ordering {
    for (left, right) in left.iter().zip(right) {
        let ordering = match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => left.cmp(right),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

#[cfg(test)]
mod tests {
    use super::{Version, compare_prerelease, os_release_value};
    use std::cmp::Ordering;

    #[test]
    fn versions_follow_semver_release_order() -> Result<(), &'static str> {
        let alpha = Version::parse("v0.2.0-alpha.1").ok_or("invalid alpha fixture")?;
        let beta = Version::parse("0.2.0-beta.1").ok_or("invalid beta fixture")?;
        let stable = Version::parse("0.2.0").ok_or("invalid stable fixture")?;
        assert!(alpha < beta);
        assert!(beta < stable);
        assert_eq!(
            compare_prerelease(
                &["rc".to_owned(), "2".to_owned()],
                &["rc".to_owned(), "10".to_owned()]
            ),
            Ordering::Less
        );
        Ok(())
    }

    #[test]
    fn os_release_parser_avoids_prefix_matches() {
        let fixture = "ID=debian\nVERSION_ID=\"13\"\n";
        assert_eq!(os_release_value(fixture, "ID").as_deref(), Some("debian"));
        assert_eq!(
            os_release_value(fixture, "VERSION_ID").as_deref(),
            Some("13")
        );
    }
}