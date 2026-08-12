# Contributing

Development uses short branches and pull requests into `main`. Keep the TUI read-only until the corresponding daemon operation has validation, authorization, audit logging and rollback coverage.

Before opening a pull request:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release
```

Never commit real credentials, server environment files, world saves, diagnostic archives or API tokens.

