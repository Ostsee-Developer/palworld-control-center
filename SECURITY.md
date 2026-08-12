# Security policy

## Supported versions

This repository is in private alpha. Only the latest commit on `main` is supported after all required checks pass.

## Reporting a vulnerability

Do not place credentials, world data, private logs or exploit details in a normal issue. Use GitHub's private security advisory workflow for this repository. If that feature is unavailable for the account, contact the repository owner privately.

Please include the affected commit, deployment mode, impact, reproduction steps and whether credentials or world data may have been exposed.

## Security invariants

- Palworld REST is loopback-only.
- No free-form shell execution is exposed through the TUI or DynaCat API.
- Backup and restore paths reject traversal, symlinks and special files where applicable.
- Secrets are read from protected files and are redacted from logs, diagnostics and API responses.
- Every mutating daemon request will be authenticated, authorized and audited.
- A failed restore or credential change must leave a recoverable previous state.

