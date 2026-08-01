## What does this PR do?

Brief description of the change.

## Related Issue

Closes #<!-- issue number -->

<!-- For non-trivial changes, please open an issue first to discuss the approach. -->

## Checklist

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes (CI builds the GUI crate too; locally you may `--exclude orchestrator-gui` if you lack its webkit/frontend prerequisites)
- [ ] `cargo test --workspace` passes
