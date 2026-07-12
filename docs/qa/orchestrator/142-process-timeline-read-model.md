---
self_referential_safe: true
---

# Orchestrator - Process Timeline Read Model

**Module**: Orchestrator  
**Scope**: Semantic task timeline projection, pagination, evidence, live bridge, and UI entry  
**Scenarios**: 5  
**Priority**: High

---

## Background

`TaskTimeline` is an on-read semantic projection over existing task, item, command-run, and event records. It is additive to `TaskInfo`, `TaskTrace`, `TaskFollow`, and `TaskWatch`.

The daemon scenario uses only the deterministic mock fixture:

```bash
orchestrator apply -f fixtures/manifests/bundles/process-timeline-failure.yaml --project qa-process-timeline
```

---

## Scenario 1: Failed Workflow Produces Semantic Entries And Evidence

### Preconditions

- Build debug binaries with `cargo build -p orchestratord -p orchestrator-cli`.
- `jq` is installed.

### Goal

Verify a recorded failed workflow explains its goal, execution, state changes, failure reason, and evidence.

### Steps

1. Run `./scripts/qa/test-process-timeline.sh`.
2. Inspect the reported category and evidence assertions.

### Expected

- The isolated fixture produces `goal`, `lifecycle`, `test`, and `failure` entries.
- The failure summary is non-empty and links `command_run` evidence.
- No daemon-owned filesystem path is returned as an evidence URI.

---

## Scenario 2: Determinism, Legacy Compatibility, And Redaction

### Preconditions

- Repository dependencies are available locally.

### Goal

Verify identical source rows yield stable identities and old events remain projectable without leaking configured secrets.

### Steps

1. Run:

   ```bash
   cargo test -p orchestrator-scheduler scheduler::timeline -- --nocapture
   ```

2. Confirm `projection_is_deterministic_and_redacted` and `legacy_missing_optional_fields_remains_projectable` pass.

### Expected

- Reprojection has byte-equivalent IDs and ordering.
- Missing optional correlation fields do not fail projection.
- Redaction patterns replace sensitive text before serialization.

---

## Scenario 3: Snapshot Pagination And Boundary Validation

### Preconditions

- The deterministic mock fixture and QA script from Scenario 1 are available.

### Goal

Verify fixed-watermark cursor pages do not overlap and invalid input fails explicitly.

### Steps

1. Run `./scripts/qa/test-process-timeline.sh` and confirm its two-entry pagination assertion.
2. Run focused projection tests:

   ```bash
   cargo test -p orchestrator-scheduler cursor_pagination_has_no_duplicates_or_omissions
   cargo test -p orchestrator-scheduler category_filter_is_validated
   cargo test -p agent-orchestrator load_task_timeline_source_is_uncapped_and_honors_watermark
   ```

### Expected

- Stable pages contain no duplicate or omitted entry IDs.
- Unknown categories and malformed cursors are rejected.
- Repository loading is not truncated by the legacy `TaskInfo` event cap.

---

## Scenario 4: UI Entry Visibility, Live Reconciliation, And Preserved Diagnostics

### Preconditions

- Run `cd gui && npm ci && npm run build`.
- Start the desktop GUI against a daemon containing at least one task.

### Goal

Verify users discover the timeline through normal navigation and can still reach logs and expert diagnostics.

### Steps

1. From "Attention Inbox", click "查看进程时间线" on an item; if the Inbox is empty, open "进度观察" and select a task.
2. Confirm "进程时间线" is the selected default tab and entries are visible.
3. Select "实时日志", click "追踪", and confirm log following still works.
4. Select "专家" and confirm expert panels remain accessible.
5. Click "跟踪" and confirm structured trace JSON remains accessible.
6. While a task is running, confirm new timeline rows appear without reloading the page.

### Expected

- Attention deep links and the visible task-detail tab are normal entries; no direct route or hidden expert action is required.
- `upsert` deltas merge by stable ID, and `reset_required` reloads the snapshot.
- Logs and structured trace remain available as separate diagnostic views.

---

## Scenario 5: Additive Compatibility And Control-Plane Safety

### Preconditions

- Rust toolchain and frontend dependencies are installed.

### Goal

Verify the new read APIs do not regress existing task surfaces and use bounded stream protection.

### Steps

1. Run:

   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo check -p orchestrator-gui
   cd gui && npm run build
   ```

2. Inspect protection registration:

   ```bash
   rg -n "TaskTimeline|TaskTimelineFollow" crates/daemon/src/control_plane.rs crates/daemon/src/protection.rs
   ```

### Expected

- Existing workspace tests and lint checks pass.
- `TaskTimeline` is read-only and `TaskTimelineFollow` consumes bounded stream capacity.
- The follow stream has a bounded channel, cancellation through client disconnect, reset-on-burst behavior, and terminal-task completion.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Failed workflow produces semantic entries and evidence | PASS | 2026-07-12 | Codex | Isolated daemon QA: 8/8 assertions passed |
| 2 | Determinism, legacy compatibility, and redaction | PASS | 2026-07-12 | Codex | Seven timeline projection tests passed in the full workspace suite |
| 3 | Snapshot pagination and boundary validation | PASS | 2026-07-12 | Codex | Cursor, category, and uncapped repository tests passed |
| 4 | UI entry visibility, live reconciliation, and preserved diagnostics | PASS | 2026-07-12 | Codex | Tauri compiled; React production build passed; entry/tabs/delta handling verified by code inspection |
| 5 | Additive compatibility and control-plane safety | PASS | 2026-07-12 | Codex | Full workspace tests and clippy `-D warnings` passed |
