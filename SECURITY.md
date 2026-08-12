# Security policy

## Supported versions

This repository is in public alpha. Only the latest tagged pre-release and the latest commit on `main` are supported after all required checks pass.

## Reporting a vulnerability

Do not place credentials, world data, private logs or exploit details in a normal issue. Use GitHub's private security advisory workflow for this repository. If that feature is unavailable for the account, contact the repository owner privately.

Please include the affected commit, deployment mode, impact, reproduction steps and whether credentials or world data may have been exposed.

## Security invariants

- PCC accesses Palworld REST only through loopback; host firewall policy must reject external REST traffic when Palworld listens on multiple interfaces.
- No free-form shell execution is exposed through the TUI or DynaCat API.
- Backup and restore paths reject traversal, symlinks and special files where applicable.
- Secrets are read from protected files and are redacted from logs, diagnostics and API responses.
- Every current mutating TUI request is startup-gated, confirmed and audited; the planned daemon additionally authenticates and authorizes each request.
- A failed restore or credential change must leave a recoverable previous state.
- The TTY1 kiosk must start read-only and must never expose a shell, writes or sensitive values without application-owned admin authentication.
