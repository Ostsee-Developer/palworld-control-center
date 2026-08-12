# Architecture

## Components

```text
Palworld REST (loopback) ─┐
systemd / journald ───────┼── PCC Rust runtime ── Ratatui dashboard
INI / backups / SteamCMD ─┘          │
                                     └── read-only DynaCat Unix socket
```

- `pcc install` copies the current verified binary to `/usr/local/bin`, creates the short command and installs the PCC update timer.
- The first ordinary `pcc` start runs the interactive Rust server installer when the native server configuration is absent.
- Hidden typed internal tasks are used only as `ExecStart` targets for PCC-generated systemd units. They are not a public administration CLI.
- The Ratatui application translates dashboard operations into typed background jobs so monitoring remains responsive.
- The DynaCat socket exposes only a privacy-reduced read-only view.

## Native ownership

PCC owns the complete lifecycle of a new installation:

- SteamCMD installation and App ID `2394010` updates
- `PalWorldSettings.ini` parsing and atomic mutation
- generated REST credentials
- systemd service, backup timer and update timer
- SHA-256 backup creation and verification
- transactional world restore and rollback
- PCC release self-updates

No AIO shell runtime is installed. A legacy environment reader remains temporarily for migration visibility, but native actions never delegate to it.

## Security boundaries

The TUI has no free-form command interface. Every operation maps to an allow-listed Rust action with fixed executable and argument boundaries. Mutations are disabled by default, require an explicit startup flag, show a confirmation dialog and emit an authpriv audit event.

Secrets remain in `/etc/palworld-control-center/admin-password` and `/etc/palworld-control-center/rest.netrc`. The Palworld REST API is reached through loopback only. Because the game server may listen on more than loopback, the installer can create an explicit UFW loopback allow rule followed by an external TCP deny rule for the REST port.

## State transitions

Backup, restore and update operations run off the rendering thread:

```text
confirmed -> running -> succeeded
                  └──> failed / rolled back
```

A restore verifies the archive checksum and paths, creates a fresh pre-restore backup, stops the service, swaps the world directory, fixes ownership and restarts the service. A failed restart restores the previous world. A server update creates a backup before stopping the service and running SteamCMD.

## Future boundary

The remaining privilege boundary milestone introduces a small local daemon and application-owned authentication so the interactive dashboard no longer needs to run as root. TTY kiosk mode must remain read-only before authentication and must never bypass normal Linux login on other TTYs.
