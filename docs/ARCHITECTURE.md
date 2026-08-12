# Architecture

## Components

The target architecture separates presentation, orchestration and privileged operations:

```text
palworld-tui ─┐
palworldctl ──┼── Unix socket ── palworldd ── systemd / REST / files / jobs
DynaCat ──────┘                       │
                                     └── event stream + SQLite history
```

- `palworld-control-center`: SSH/console-first Ratatui interface.
- `palworldd`: future local daemon with a narrow allow-listed operation API.
- `palworldctl`: future automation and recovery CLI using the same daemon API.
- DynaCat: external presentation and aggregation client. It never edits Palworld files directly.

## Security boundaries

The TUI and DynaCat integration do not receive unrestricted root or shell access. Privileged operations are implemented as typed daemon methods with validation, audit records and explicit job states. The Palworld REST API stays bound to loopback and is not forwarded to the public network.

Secrets remain in protected files such as `/etc/palworld/admin-password` and `/etc/palworld/rest.netrc`. API responses, diagnostics and logs must redact them.

## State and jobs

Long-running backup, restore and update operations become persistent jobs. Each transition is written before an external side effect:

```text
queued -> preparing -> running -> verifying -> succeeded
                                  └──────────> failed -> rollback
```

Restore is never performed against a running world. The daemon saves and stops the server, validates the selected archive and destination, restores transactionally, verifies the result and only then starts the service again.

## Migration

Palworld AIO 1.3.2 remains supported throughout development. The Rust application initially reads its existing environment, INI, state and backup formats. Mutating functionality is enabled only after compatibility and rollback tests exist.

