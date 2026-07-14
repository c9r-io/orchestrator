---
self_referential_safe: true
---

# Orchestrator - Process Console Operational Metrics And Local Dashboard

**Module**: Orchestrator  
**Scope**: FR-104 formulas, persistence, bounded control-plane queries, privacy, Operations UI, maintenance, compatibility, and performance  
**Scenarios**: 5  
**Priority**: High

---

## Background

This document verifies the local Process Console metrics read model. Tests use temporary SQLite databases, the isolated integration harness, and the in-page deterministic Tauri mock. They do not invoke a live AI agent or a workflow under `docs/workflow/`.

The reproducible aggregate gate is:

```bash
./scripts/qa/test-process-console-metrics.sh
```

---

## Database Schema Reference

| Table | Purpose | Retention |
|---|---|---|
| `process_metric_observations` | Idempotent, allowlisted optional samples | Configured, default 90 days |
| `process_metric_rollups` | Fixed bucket aggregates rebuilt from observations | Configured, default 90 days |
| `process_metric_projector_state` | Cursor, lag, failure category, and freshness | Retained operational state |
| `attention_changes` | Authoritative Attention episode transitions with project/resulting state | Existing authoritative lifecycle |

---

## Scenario 1: Exact Formula Golden, Privacy, And Project Isolation

### Preconditions

- Rust dependencies are available.
- No daemon is running or required.

### Goal

Verify exact Attention, autonomous completion, handoff, resume, session, source, and loop values from deterministic durable records while rejecting high-cardinality dimensions.

### Steps

1. Run `cargo test -p agent-orchestrator deterministic_fixture_produces_exact_process_metrics -- --nocapture`.
2. Run `cargo test -p agent-orchestrator high_cardinality_dimensions_are_rejected -- --nocapture`.
3. Inspect `allowed_dimensions` and confirm only closed low-cardinality label keys are accepted.
4. Inspect the serialized response and confirm `source_key`, task IDs, actor IDs, session IDs, and request IDs are absent from labels.

### Expected

- The golden reports Attention opens `2`, claim sum `15s`, resolution sum `60s`, and actionable human attention `60s`.
- Autonomous completion is `3/4`; handoff generation has two samples; first productive action is `20s`.
- Resume, session attachment, and source duplicate counts are each `1`.
- Repeated failures are `3/4`; degenerate item/phase groups are `1/1`.
- A `task_id` dimension is rejected and no content-bearing value is returned.

### Expected Data State

```sql
SELECT COUNT(*) FROM process_metric_observations
WHERE project_id = '{project_id}' AND metric_name = 'source_event_deduplicated_total';
-- Expected: 1 for the deterministic project; duplicate source keys do not add rows
```

---

## Scenario 2: Bounded gRPC/CLI Contract And Rebuild

### Preconditions

- The integration harness can bind an ephemeral local endpoint.
- No external daemon, credentials, or provider API is required.

### Goal

Verify project-scoped record/get/rebuild, schema versioning, and rejection of invalid windows.

### Steps

1. Run `cargo test -p orchestrator-integration-tests --test process_metrics -- --nocapture`.
2. Inspect `crates/proto/orchestrator.proto` for the four additive Process Metrics RPCs.
3. Inspect `orchestrator metrics process --help` wiring or `crates/cli/src/commands/metrics.rs` for JSON/YAML/table output.
4. Confirm `window=31d` is rejected with `INVALID_ARGUMENT` under the default 30-day maximum.
5. Confirm rebuild returns one affected retained observation in the integration fixture.

### Expected

- Version 1 JSON is scoped to the requested project and includes the recorded reconnect metric.
- Supported bucket and 744-bucket limits apply before database aggregation.
- Invalid windows do not return partial data.
- Rebuild is admin-only and reproducibly regenerates rollups.

### Expected Data State

```sql
SELECT COUNT(*) FROM process_metric_rollups
WHERE project_id = '{project_id}' AND metric_name = 'stream_reconnect_total';
-- Expected: one row per supported bucket width after one unique accepted observation
```

---

## Scenario 3: Discoverable Read-only Operations UI And Accessibility

### Preconditions

- Install frontend dependencies with `cd gui && npm ci` when absent.
- Install Chromium with `cd gui && npx playwright install chromium` when absent.
- Use the deterministic in-page Tauri mock only.

### Goal

Verify the dashboard is reachable through normal navigation and presents bounded data and state safely to a read-only user.

### Entry Visibility

The dashboard must be reachable from the visible "System" destination and its "Operations" section. A direct hash route or Expert-only entry is not sufficient.

### Steps

1. Run `cd gui && npm test -- --run && npm run build && npm run test:e2e`.
2. From the visible navigation select "System", then locate the "Operations" section.
3. Verify a read-only user can inspect metrics but sees no mutation control.
4. Select "7d" and confirm the request changes to a seven-day bounded window.
5. Verify data, loading/error, freshness/partial/disabled presentation and the axe assertion; retain the existing reduced-motion/transparency console regression.

### Expected

- Attention load/latency, autonomous ratio, handoff, resume, session, source, repeated failure, loop health, timeline, reconnect, and projector panels render.
- Empty/error/fresh/stale/partial/disabled states are distinguishable without motion or color alone.
- Read-only users retain access because the dashboard is a control-plane read surface.
- Axe reports no serious or critical violations.

---

## Scenario 4: Migration, Retention, Disable, Cursor Recovery, And Rollback

### Preconditions

- Rust dependencies and a writable temporary directory are available.

### Goal

Verify optional metric lifecycle operations cannot damage authoritative process state and a failed projector retains its replay boundary.

### Steps

1. Run `cargo test -p agent-orchestrator observations_are_allowlisted_idempotent_and_rebuildable -- --nocapture`.
2. Run `cargo test -p agent-orchestrator retention_prunes_only_expired_optional_metric_state -- --nocapture`.
3. Run `cargo test -p agent-orchestrator projector_failure_retains_the_last_successful_cursor -- --nocapture`.
4. Run `cargo test -p orchestrator-config process_metrics_can_be_disabled_independently -- --nocapture`.
5. Run the migration registry/populated-upgrade tests through `cargo test -p agent-orchestrator persistence::migration -- --nocapture`, then inspect migration 32 as additive-only.

### Expected

- Duplicate observations are idempotent and rebuild produces the same rollup.
- Prune deletes expired optional observations/rollups only.
- Failure increments health and lag while preserving the last successful cursor; only a stable error token is retained.
- Collection and UI telemetry can be disabled without deleting or hiding authoritative query-derived state.
- Rollback needs no down migration: disable writers/projectors or run the older binary with additive tables retained.

### Expected Data State

```sql
SELECT cursor, lag_count, failure_count, last_error_code
FROM process_metric_projector_state
WHERE projector = 'attention' AND project_id = '{project_id}';
-- Expected after the fixture failure: cursor='42', lag_count=7, failure_count=1, last_error_code='sqlite_busy'
```

---

## Scenario 5: Release Performance And Backward-compatibility Gate

### Preconditions

- Run in release mode on a normally loaded development machine.
- Rust and Node.js toolchains are available.

### Goal

Verify large histories remain bounded and the new product metrics do not change existing QA doctor, agent selection, TaskInfo/log/watch, or repository gates.

### Steps

1. Run `cargo test --release -p agent-orchestrator large_fixture_query_meets_process_metrics_budget --lib -- --ignored --nocapture`.
2. Run `cargo test --release -p orchestrator-scheduler large_timeline_meets_projection_budget --lib -- --ignored --nocapture`.
3. Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.
4. Confirm `core/src/qa_doctor.rs` still reads `task_execution_metrics` and `core/src/metrics.rs` still owns agent-selection metrics.
5. Confirm the complete integration and GUI regressions in Scenarios 2 and 3 pass.

### Expected

- The 50,000-event/5,000-Attention metrics query remains under 300 ms and 256 KiB.
- The 50,000-event timeline remains under 750 ms and 512 KiB.
- Existing workspace tests, strict Clippy, task reads/log/watch paths, QA doctor, and selection metrics pass unchanged.
- `timeline_projection_seconds`, `timeline_response_bytes`, and projector health are additive and semantically distinct.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Exact formula golden, privacy, and project isolation | PASS | 2026-07-14 | Codex | Exact-value, allowlist, idempotency, and privacy tests passed |
| 2 | Bounded gRPC/CLI contract and rebuild | PASS | 2026-07-14 | Codex | Isolated integration test and invalid 31d rejection passed |
| 3 | Discoverable read-only Operations UI and accessibility | PASS | 2026-07-14 | Codex | React/build and 10 Playwright scenarios passed, including Operations axe/window checks |
| 4 | Migration, retention, disable, cursor recovery, and rollback | PASS | 2026-07-14 | Codex | Additive migration, rebuild, prune, disable, and cursor tests passed |
| 5 | Release performance and backward-compatibility gate | PASS | 2026-07-14 | Codex | Both release budgets, workspace tests, and strict Clippy passed |
