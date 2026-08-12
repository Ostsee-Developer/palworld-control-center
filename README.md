# Palworld Control Center

Terminal-first operations application for the Palworld AIO server stack on Linux. It connects the Hacker Terminal Pro interface to the existing, hardened AIO runtime instead of duplicating backup, restore, update and mod-management logic.

## Functional alpha

The eight tabs now expose real data and operations:

- **Overview:** host resources, Palworld service/API state, version, players, FPS, logs, latest backup and quick settings.
- **Players:** privacy-reduced player list plus broadcast, kick, ban and unban through the local Palworld REST API.
- **Settings:** searchable catalog merged with every key found in the active `PalWorldSettings.ini`; secret values are never rendered. Supported values are validated and written atomically.
- **Mods:** managed PAK packages, unmanaged direct PAK detection and Windows/Wine Workshop inventory. Native PAK enable/disable/import/quarantine reuses the AIO safety model.
- **Backups:** history, SHA-256 verification, creation, world-only restore and deletion. Restore remains transactional and creates a pre-restore backup.
- **Updates:** installed Steam build, systemd schedule, last result, official Steam news and the proven backup-first SteamCMD update flow.
- **Logs:** normalized journald feed with search and level filters. Repeated lines are collapsed and known credential markers are redacted.
- **Security:** file-mode, regular-file, backup-path and REST-listener checks plus redacted diagnostic collection.

All slow operations run as background jobs, so host metrics and the UI remain responsive.

## Compatibility

The application automatically reads `/etc/palworld/palworld.env` and supports the existing Palworld AIO paths and tools, including:

- `/opt/palworld/server`
- `/var/backups/palworld`
- `/usr/local/lib/palworld/*.sh`
- `/usr/local/lib/palworld/*.py`
- `palworld.service`, `palworld-backup.timer` and `palworld-update.timer`

Use `--config /path/to/palworld.env` for a migrated or test installation.

## Run

Read-only is the secure default:

```bash
palworld-control-center
```

Enable mutating actions for one explicitly started session:

```bash
sudo palworld-control-center --enable-writes
```

The temporary switch will be replaced by the planned application-owned admin login before the TTY1 kiosk mode is introduced. For a complete preview without a Palworld installation:

```bash
palworld-control-center --demo
```

Common controls:

- `←` / `→` or `h` / `l`: change tabs
- `↑` / `↓` or `j` / `k`: select an item
- `1`–`8`: open a tab directly
- `r`: refresh
- `q` or `Ctrl+C`: exit

The footer shows tab-specific operations. Every mutation opens a second confirmation dialog.

## DynaCat API

An optional read-only API can be exposed on a local Unix socket:

```bash
sudo install -d -o root -g dynacat -m 0750 /run/palworld-control-center
sudo palworld-control-center \
  --dynacat-socket /run/palworld-control-center/dynacat.sock
```

The socket is mode `0660`. It never returns passwords, tokens, IP addresses, full player identifiers or raw environment files. See [the API contract](docs/DYNACAT-API.md).

## Security baseline

- Palworld REST is contacted only through `127.0.0.1` with the protected AIO netrc file.
- There is no free-form shell evaluation; child processes receive fixed commands and separate arguments.
- Writes are disabled unless `--enable-writes` is present.
- Every action has an allow-listed implementation, confirmation screen and authpriv audit event.
- Backup targets reject symlinks and paths outside their configured root; managed PAK names cannot escape the mod store and import sources cannot be symlinks.
- Restore and updates delegate to the tested AIO backup-first rollback flows.
- Secret settings are never loaded into the presentation model.
- Player IP addresses and full identifiers are excluded from the normal TUI and DynaCat API.

Pocketpair explicitly warns that its REST API is not designed for direct Internet exposure. This project therefore does not offer a TCP listener or reverse-proxy mode for that API.

See [architecture](docs/ARCHITECTURE.md), [TUI layout](docs/TUI-LAYOUT.md), [DynaCat API](docs/DYNACAT-API.md) and [security policy](SECURITY.md).

## Next security milestone

The Palworld server installer, TTY1 takeover and passwordless boot presentation are intentionally planned together. The kiosk will start in public read-only mode; an application-owned admin login will be required for writes and for revealing sensitive information. It will not create an unauthenticated root shell or bypass the normal Linux login on other TTYs.

## Status

`0.2.0-alpha.1` — functional AIO integration. The original Bash/dialog manager remains the recovery interface while the Rust application matures.
