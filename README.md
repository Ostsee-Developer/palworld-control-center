# Palworld Control Center

Private, terminal-first operations application for a self-hosted Palworld Dedicated Server on Linux.

The project replaces the growing Bash/dialog dashboard with a secure Rust architecture while preserving the proven Palworld AIO workflows. The first milestone is intentionally read-only: real host metrics and `journalctl` logs are displayed without changing the server, world, backups or configuration.

## Alpha preview

```text
┌───────────────────────────────────┬──────────────────────┐
│ REALTIME SERVER LOGS              │ RESOURCES            │
│ journald, events and job output   │ CPU / RAM / disk     │
│                                   │ players / version    │
├───────────────────────────────────┼──────────────────────┤
│ REALTIME BACKUP / RESTORE         │ QUICK GAME SETTINGS  │
│ job, progress and verification    │ important values     │
└───────────────────────────────────┴──────────────────────┘
```

Additional functionality is organized in tabs: Overview, Players, Settings, Mods, Backups, Updates, Logs and Security.

## Run

```bash
cargo run --release
```

The application reads real system metrics and the latest entries of `palworld.service`. For a complete design preview without a Palworld installation:

```bash
cargo run --release -- --demo
```

Keyboard controls:

- `←` / `→` or `h` / `l`: change tabs
- `1`–`8`: open a tab directly
- `r`: refresh
- `q` or `Ctrl+C`: exit

## Security baseline

- read-only first milestone
- no shell interpolation; `journalctl` is started with a fixed argument list
- no credentials in the process environment, logs or repository
- Palworld REST remains on `127.0.0.1`
- DynaCat will communicate with a dedicated local daemon, never directly with root-owned files
- restores and credential changes remain transactional
- release binaries are built by GitHub Actions and accompanied by SHA-256 checksums

See [architecture](docs/ARCHITECTURE.md), [TUI layout](docs/TUI-LAYOUT.md), [API contract](docs/DYNACAT-API.md) and [security policy](SECURITY.md).

## Status

`0.1.0-alpha.1` — visual and architectural foundation. Existing Palworld AIO 1.3.2 remains the stable production manager during development.

