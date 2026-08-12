## What changed

<!-- Describe the implementation and user impact. -->

## Security and rollback

<!-- Describe new privileges, external inputs, secrets, filesystem changes and rollback behavior. -->

## Validation

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --locked --all-targets --all-features -- -D warnings`
- [ ] `cargo test --all-features --locked`
- [ ] `cargo build --release --locked`
- [ ] No credentials, world data or private diagnostics included

