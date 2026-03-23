# Contributing to Agent Orchestrator

Thank you for your interest in the Agent Orchestrator project!

## About This Project

This is an **AI-native development** project — the codebase is primarily developed and maintained using AI-assisted workflows (Claude Code with orchestrator skills). This means our development model differs from traditional open-source projects, and we're actively exploring how external contributions best fit into this paradigm.

## How to Contribute

### Feature Requests (Preferred)

The most impactful way to contribute is by sharing your use cases and ideas:

1. Open a [Feature Request](https://github.com/c9r-io/orchestrator/issues/new?template=feature_request.md) issue
2. Describe your scenario and the problem you're trying to solve
3. We'll evaluate and track it as an internal FR document

### Bug Reports

Found a bug? Please [report it](https://github.com/c9r-io/orchestrator/issues/new?template=bug_report.md) with:

- Your OS and architecture
- `orchestrator --version` / `orchestratord --version` output
- Steps to reproduce
- Expected vs. actual behavior
- Relevant logs (if applicable)

### Pull Requests

PRs are welcome with the following guidance:

- **Small fixes** (typos, doc improvements): submit directly
- **Non-trivial changes**: please open an issue first to discuss the approach — this avoids duplicated effort since the AI-native workflow may already have the change in progress
- All PRs must pass CI: `cargo fmt`, `cargo clippy -D warnings`, `cargo test`

## Development Setup

### Prerequisites

- **Rust** 1.77+ (`rustup` recommended)
- **protoc** (Protocol Buffers compiler)
  - macOS: `brew install protobuf`
  - Linux: `sudo apt-get install -y protobuf-compiler`
  - Or let the build system use the vendored protoc automatically

### Build & Test

```bash
# Build all crates
cargo build --workspace

# Run tests (excludes GUI crate which needs Tauri deps)
cargo test --workspace --exclude orchestrator-gui

# Lint
cargo clippy --workspace --exclude orchestrator-gui --all-targets -- -D warnings

# Format check
cargo fmt --all -- --check

# Async lock governance (CI enforced)
./scripts/check-async-lock-governance.sh
```

### Running Locally

```bash
# Start daemon in foreground
orchestratord --foreground --workers 2

# In another terminal
orchestrator init
orchestrator apply -f fixtures/capability-test.yaml
orchestrator task create --goal "test run"
orchestrator task list
```

## Code Style

- **Formatting**: `cargo fmt` (enforced in CI)
- **Linting**: `cargo clippy` with `-D warnings` (zero warnings policy)
- **Async safety**: `std::sync::RwLock` restricted to approved files (see `scripts/check-async-lock-governance.sh`)
- **Commits**: conventional format — `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
