# Design Doc 82: Distribution Channels — Homebrew Tap & Cargo

## FR Reference

FR-072: 分发渠道扩展 — Docker 镜像与 Homebrew

## Scope Decision

FR-072 originally requested Docker images + Homebrew tap. Analysis determined that Docker distribution is **not viable** for the current architecture:

- `orchestratord` spawns AI agents as local child processes via `tokio::process::Command`
- Agents require host tools (`claude` CLI), credentials (`~/.claude/`), filesystem access, and shell
- Containerization would require mounting the entire host environment, defeating its purpose

Docker support is deferred indefinitely. The FR is closed with **Homebrew tap + cargo install** as distribution channels.

## Design Decisions

### 1. Homebrew Tap

#### Formula Template in Main Repo

The formula template lives at `homebrew/orchestrator.rb` with placeholder values. The release workflow generates the final formula with correct URLs and SHA-256 checksums, then pushes it to `c9r-io/homebrew-tap` under `Formula/orchestrator.rb`.

#### Platform Coverage

| Platform | Homebrew Support | Target Triple |
|----------|-----------------|---------------|
| macOS ARM64 | Yes | `aarch64-apple-darwin` |
| Linux x86_64 | Yes (Linuxbrew) | `x86_64-unknown-linux-gnu` |
| Linux ARM64 | Yes (Linuxbrew) | `aarch64-unknown-linux-gnu` |

#### Release Automation

The `homebrew` job in `release.yml` runs after the `publish` job:

1. `scripts/update-homebrew-formula.sh` fetches the checksum manifest from the just-published release
2. Renders the formula template with correct version, URLs, and SHA-256 values
3. `dmnemec/copy_file_to_another_repo_action` pushes the rendered formula to `c9r-io/homebrew-tap`

Requires `TAP_GITHUB_TOKEN` secret with write access to the tap repo.

### 2. Cargo Install (crates.io)

#### Crate Publishing Strategy

All internal library and binary crates are published to crates.io, enabling:
```bash
cargo install orchestrator-cli      # installs 'orchestrator' binary
cargo install orchestratord         # installs 'orchestratord' binary
```

#### Publishing Order

Crates must be published in strict dependency order (leaf → root):

1. `orchestrator-proto` — no internal dependencies
2. `orchestrator-config` — no internal dependencies
3. `agent-orchestrator` (core) — depends on proto + config
4. `orchestrator-scheduler` — depends on core + config + proto
5. `orchestrator-cli` — depends on core + proto
6. `orchestratord` — depends on core + scheduler + proto

A 30-second delay between publishes allows the crates.io index to propagate.

#### Trusted Publisher (OIDC)

Authentication uses crates.io Trusted Publishers via GitHub Actions OIDC — no manual API tokens needed:

- The `crates-io` job declares `permissions: { id-token: write }` and `environment: release`
- `rust-lang/crates-io-auth-action@v1` exchanges the GitHub OIDC token for a short-lived crates.io publish token
- Each crate must be configured on crates.io with a Trusted Publisher pointing to `c9r-io/orchestrator`, workflow `release.yml`, environment `release`

#### Excluded from Publishing

| Crate | Reason |
|-------|--------|
| `orchestrator-gui` | Depends on Tauri; requires platform-specific build toolchain |
| `orchestrator-integration-tests` | Test-only crate |

#### Path + Version Dependencies

All internal path dependencies now carry a `version` spec:
```toml
orchestrator-proto = { path = "../proto", version = "0.1.0" }
```

This satisfies both local development (path resolution) and crates.io publishing (version resolution).

#### Protobuf Build Compatibility

The `orchestrator-proto` crate uses `protoc-bin-vendored = "3"` as a build dependency, providing pre-compiled `protoc` binaries. Users running `cargo install` do **not** need to install protobuf separately.

#### crates.io Metadata

All publishable crates include: `description`, `repository`, `keywords`, `categories`, and `license`.

## Files Created

| File | Purpose |
|------|---------|
| `homebrew/orchestrator.rb` | Formula template with placeholders |
| `scripts/update-homebrew-formula.sh` | Generates final formula from release checksums |

## Files Modified

| File | Change |
|------|--------|
| `.github/workflows/release.yml` | Added `homebrew` and `crates-io` jobs after `publish` |
| `crates/proto/Cargo.toml` | Added crates.io metadata |
| `crates/orchestrator-config/Cargo.toml` | Added crates.io metadata, removed empty authors |
| `core/Cargo.toml` | Added version specs to path deps, crates.io metadata |
| `crates/orchestrator-scheduler/Cargo.toml` | Added version specs to path deps, crates.io metadata |
| `crates/cli/Cargo.toml` | Added version specs to path deps, crates.io metadata |
| `crates/daemon/Cargo.toml` | Added version specs to path deps, crates.io metadata |
| `crates/gui/Cargo.toml` | Added `publish = false`, version spec to path dep |
| `crates/integration-tests/Cargo.toml` | Added version specs to path deps |

## External Prerequisites

| Resource | Action Required |
|----------|----------------|
| `c9r-io/homebrew-tap` GitHub repo | Must be created manually |
| `TAP_GITHUB_TOKEN` secret | Must be added to orchestrator repo settings |
| crates.io Trusted Publisher | Each crate must be configured on crates.io: owner `c9r-io`, repo `orchestrator`, workflow `release.yml`, environment `release` |
| GitHub environment `release` | Must be created in repo Settings → Environments (optional but recommended for approval gates) |
