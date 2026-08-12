# Architecture

## Components

The target architecture separates presentation, orchestration and privileged operations:

```text
Palworld REST (loopback) ─┐
systemd / journald ───────┼── palworld-control-center ── local DynaCat socket
Palworld AIO runtime ─────┘               │
                                          └── background job runner + audit
```

- `palworld-control-center`: SSH/console-first Ratatui interface and temporary orchestration layer.
- Palworld AIO runtime: existing validated scripts for backup, restore, update, configuration and PAK management.
- DynaCat socket: read-only, privacy-reduced local HTTP/JSON view.
- `palworldd`: future local daemon with a narrow allow-listed operation API and application-owned authentication.
- `palworldctl`: future automation and recovery CLI using the same daemon API.
- DynaCat: external presentation and aggregation client. It never edits Palworld files directly.

## Security boundaries

The TUI has no free-form command interface. Every operation maps to a typed action with fixed executable and argument boundaries. Mutations are disabled by default, require an explicit startup flag in this milestone, show a confirmation dialog and emit an authpriv audit event. The DynaCat integration is always read-only. The Palworld REST API is contacted only over loopback and is never forwarded to the public network.

Secrets remain in protected files such as `/etc/palworld/admin-password` and `/etc/palworld/rest.netrc`. API responses, diagnostics and logs must redact them.

## State and jobs

Backup, restore and update operations run off the rendering thread so monitoring stays live. The UI represents their lifecycle as:

```text
confirmed -> running -> succeeded
                  └──> failed (AIO rollback where applicable)
```

Restore is never performed against a running world. The daemon saves and stops the server, validates the selected archive and destination, restores transactionally, verifies the result and only then starts the service again.

## Migration

Palworld AIO remains the production and recovery layer throughout development. The Rust application reads its environment, INI, state, mod and backup formats and delegates high-risk workflows to the same runtime. The next boundary change introduces `palworldd`, application-owned admin authentication and a read-only TTY1 kiosk before the installer is integrated.
