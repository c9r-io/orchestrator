---
self_referential_safe: false
self_referential_safe_scenarios:
  - S1
  - S3
  - S4
  - S5
  - S6
  - S7
  - S8
# S2 不安全：需要已发布的 release 和 checksum manifest
# S9 不安全：端到端 Homebrew 安装，需要外部 tap repo 和 release tag
# S10 不安全：端到端 cargo install，需要 crates.io 发布
# S1/S3-S8 安全：文件读取、grep 检查、cargo check
---

# QA 124: Distribution Channels — Homebrew Tap & Cargo

## FR Reference

FR-072

## Verification Scenarios

### Scenario 1: Formula template syntax

**Steps:**
1. `cat homebrew/orchestrator.rb`
2. Verify placeholder tokens: `PLACEHOLDER_VERSION`, `PLACEHOLDER_SHA256_MACOS_ARM64`, `PLACEHOLDER_SHA256_LINUX_AMD64`, `PLACEHOLDER_SHA256_LINUX_ARM64`
3. Verify `on_macos` / `on_linux` blocks with correct target triples in URLs

**Expected:** Valid Ruby formula with platform-specific download blocks and SHA-256 placeholders.

### Scenario 2: Update script renders formula

**Steps:**
1. `scripts/update-homebrew-formula.sh v0.1.0` (requires a published release with checksum manifest)
2. Verify output contains no `PLACEHOLDER_` tokens
3. Verify SHA-256 values are 64-character hex strings
4. Verify URLs point to correct release tag

**Expected:** Fully rendered formula with real checksums and URLs printed to stdout.

### Scenario 3: Release workflow includes homebrew job

**Steps:**
1. `grep -A 20 'homebrew:' .github/workflows/release.yml`
2. Verify `needs: publish` dependency
3. Verify `TAP_GITHUB_TOKEN` secret usage
4. Verify destination repo is `c9r-io/homebrew-tap`

**Expected:** Homebrew job runs after publish, pushes formula to tap repo.

### Scenario 4: Path dependencies carry version specs

**Steps:**
1. Search workspace Cargo.toml files for path dependencies missing version specs:
   ```bash
   grep -rn 'path = "' crates/ core/ Cargo.toml --include='Cargo.toml' \
     | grep -v 'version =' \
     | grep -v 'src/main.rs' \
     | grep -v 'src/lib.rs' \
     | grep -v 'tests/'
   ```

**Expected:** Zero results — all path dependencies must also include `version = "..."`.

> **Note:** The grep excludes `path = "src/..."` entries (internal `[[bin]]`/`[[lib]]` targets, not external dependencies) and scopes to workspace directories to avoid stale `target/package/` artifacts.

### Scenario 5: crates.io metadata completeness

**Steps:**
1. For each publishable crate (proto, orchestrator-config, agent-orchestrator, orchestrator-scheduler, orchestrator-cli, orchestratord):
   - `grep -E '^(description|repository|keywords|categories|license)' <crate>/Cargo.toml`

**Expected:** All five fields present in each publishable crate.

### Scenario 6: Non-publishable crates excluded

**Steps:**
1. `grep 'publish = false' crates/gui/Cargo.toml crates/integration-tests/Cargo.toml`

**Expected:** Both files contain `publish = false`.

### Scenario 7: Release workflow cargo publish job

**Steps:**
1. `grep -A 25 'crates-io:' .github/workflows/release.yml`
2. Verify publishing order: proto → config → core → scheduler → cli → daemon
3. Verify `CARGO_REGISTRY_TOKEN` secret usage
4. Verify `needs: publish` dependency

**Expected:** Crates published in dependency order with propagation delay.

### Scenario 8: Workspace compiles with version specs

**Steps:**
1. `cargo check`

**Expected:** Clean compilation, no errors.

### Scenario 9: End-to-end Homebrew install (manual, post-release)

**Steps:**
1. Create `c9r-io/homebrew-tap` repo with empty `Formula/` directory
2. Add `TAP_GITHUB_TOKEN` secret to orchestrator repo
3. Push a release tag
4. Wait for release workflow to complete
5. `brew tap c9r-io/tap && brew install c9r-io/tap/orchestrator`
6. `orchestrator --version` && `orchestratord --version`

**Expected:** Both binaries install successfully and print matching version.

### Scenario 10: End-to-end cargo install (manual, post-release)

**Steps:**
1. Add `CARGO_REGISTRY_TOKEN` secret to orchestrator repo
2. Push a release tag and wait for crates-io job to complete
3. `cargo install orchestrator-cli`
4. `cargo install orchestratord`
5. `orchestrator --version` && `orchestratord --version`

**Expected:** Both binaries compile and install successfully, print matching version.

## Checklist

- [x] S1: Formula template syntax ✅
- [ ] S2: Update script renders formula (requires published release — manual post-release)
- [x] S3: Release workflow includes homebrew job ✅
- [x] S4: Path dependencies carry version specs ✅
- [x] S5: crates.io metadata completeness ✅
- [x] S6: Non-publishable crates excluded ✅
- [x] S7: Release workflow cargo publish job ✅
- [x] S8: Workspace compiles with version specs ✅
- [ ] S9: End-to-end Homebrew install (manual, post-release)
- [ ] S10: End-to-end cargo install (manual, post-release)
