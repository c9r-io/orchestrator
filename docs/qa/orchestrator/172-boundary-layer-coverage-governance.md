---
self_referential_safe: true
---

# Orchestrator - Boundary Layer Coverage Governance

**Module**: Orchestrator

**Scope**: Machine-readable coverage, baseline enforcement, boundary matrices, adapter seams, and traceability

**Scenarios**: 5
**Priority**: High

---

## Background

This QA uses only repository tests, temporary SQLite state, a local in-process
tonic server, and deterministic frontend fixtures. It does not connect to the
developer daemon, Slack, a paid model, or a workflow under `docs/workflow/`.

The approved baseline is a non-regression starting point. A passing percentage
does not replace success, rejection, conflict, and unavailable assertions.

---

## Scenario 1: Coverage Policy Fixtures And Cross-Platform Paths

### Preconditions

- Node.js 22 or a compatible version is installed.
- The repository is checked out on Linux or macOS.

### Goal

Verify normalization, baseline comparison, and unsupported branch semantics
without compiling the workspace.

### Steps

1. Run:

   ```bash
   ./scripts/coverage-governance.sh --fixture-test
   ```

2. Inspect `scripts/coverage/test-coverage-governance.mjs`.
3. Confirm it tests POSIX and Windows source paths, an accepted baseline, a
   three-cause regression, and explicit branch `unsupported`.

### Expected

- The command prints `coverage governance fixtures: PASS`.
- A lower daemon line percentage, lower React function percentage, and lower
  Playwright scenario count all fail comparison.
- Unsupported branches contain `null` counts/percentage and never `0`.

---

## Scenario 2: Unified Report, Approved Baseline, And CI Artifact

### Preconditions

- Rust stable with `llvm-tools-preview` and `cargo-llvm-cov 0.8.5` is installed.
- Run `cd gui && npm ci && npx playwright install chromium` once.
- No external provider credentials are required.

### Goal

Generate and enforce every coverage layer with the canonical command.

### Steps

1. Run:

   ```bash
   ./scripts/coverage-governance.sh
   ```

2. Inspect `target/coverage-governance/summary.json`.
3. Compare it with `coverage/boundary-baseline.json`.
4. Inspect the `boundary-coverage` job in `.github/workflows/ci.yml`.

### Expected

- JSON/LCOV, React JSON, Playwright JSON, and the normalized summary exist.
- Rust components and key modules contain line/function counts and percentages.
- Stable Rust reports branch status as `unsupported`.
- React contains real line/function/branch data.
- Playwright reports executed scenario counts and zero failures.
- CI uploads the artifact even when baseline enforcement fails.

---

## Scenario 3: Daemon Boundary Risk Matrix

### Preconditions

- Rust dependencies are available.
- Tests may create only temporary state.

### Goal

Verify the five production daemon adapter boundaries assert semantic outcomes,
not only line execution.

### Steps

1. Run:

   ```bash
   cargo test -p orchestratord server::boundary_contract_tests -- --nocapture
   ```

2. Review `crates/daemon/src/server/boundary_contract_tests.rs`.
3. Confirm Attention, Handoff, Session, SourceConnection, and Action Audit each
   cross a real production server function.

### Expected

- Attention proves success, invalid snooze, read-only denial, and stale version.
- Handoff proves generation, invalid cursor, and read-only denial; repository
  tests retain stale resume-plan enforcement.
- Session proves list, invalid mode, and fail-closed control policy; existing
  lease tests retain fencing/conflict enforcement.
- SourceConnection proves list, invalid scope, admin denial, and missing-Gateway
  failure; owning FR QA retains optimistic lifecycle conflicts.
- Action Audit proves query, invalid ID, denied-attempt recording, changed
  idempotency conflict, and request-ID metadata.

---

## Scenario 4: CLI And Tauri Real gRPC Adapter Templates

### Preconditions

- The host can compile the CLI and Tauri test target.
- No desktop window or daemon process is running.

### Goal

Verify reusable client adapters cross real tonic serialization and preserve
parameters and errors.

### Steps

1. Run:

   ```bash
   cargo test -p orchestrator-cli
   cargo test -p orchestrator-gui \
     task_create_crosses_real_tauri_handler_and_in_process_grpc_adapter
   ```

2. Inspect `crates/cli/src/grpc_adapter_tests.rs` and the Tauri test in
   `crates/gui/src/lib.rs`.
3. Confirm CLI client tests include missing UDS, invalid TLS material, and
   invalid TCP address cases.

### Expected

- CLI task creation preserves name, goal, project, workspace, workflow, and
  no-start behavior through real gRPC.
- A missing task remains a tonic `NotFound` error.
- UDS/TLS/TCP failures do not silently fall back.
- Tauri invoke creates and reads a real task through the injected channel.
- A missing task is rendered through the safe localized error boundary.

---

## Scenario 5: Layered Regression And FR Evidence Traceability

### Preconditions

- Scenario 2 dependencies are installed.
- Documentation indexes are present.

### Goal

Verify the repository quality layers and ensure Closed FR status is not
misrepresented as uniformly high file coverage.

### Steps

1. Run:

   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all -- --check
   cd gui && npm run test:all
   cd .. && ./scripts/qa-doc-lint.sh
   ```

2. Inspect the FR-095 through FR-118 evidence index in `docs/qa/README.md`.
3. Confirm live/manual certification is separate from ordinary CI evidence.

### Expected

- All Rust, frontend, browser, formatting, and documentation gates pass.
- Every FR maps DD/QA to unit, integration, shell QA, browser, or live evidence.
- Network-free CI and controlled live certification are visibly distinct.
- No document claims that Closed means every related file has high coverage.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Coverage policy fixtures and cross-platform paths | PASS | 2026-07-25 | Codex | Pass/regression/unsupported and Windows/POSIX paths verified |
| 2 | Unified report, approved baseline, and CI artifact | PASS | 2026-07-25 | Codex | JSON/LCOV summary generated; 32 Playwright scenarios passed |
| 3 | Daemon boundary risk matrix | PASS | 2026-07-25 | Codex | Five production boundary matrices passed |
| 4 | CLI and Tauri real gRPC adapter templates | PASS | 2026-07-25 | Codex | CLI transport plus CLI/Tauri tonic adapters passed |
| 5 | Layered regression and FR evidence traceability | PASS | 2026-07-25 | Codex | Workspace tests, workspace Clippy, 120 Vitest tests, 32 Playwright scenarios, production build, and QA doc lint passed |
