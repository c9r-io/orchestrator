---
self_referential_safe: true
---

# Orchestrator - Process Console v1 Release Acceptance

**Module**: Orchestrator  
**Scope**: Clean-tree release gate, migration identity, populated upgrade, cross-slice vertical behavior, performance, and rollback documentation  
**Scenarios**: 5  
**Priority**: Critical

---

## Background

This document is the executable closure evidence for FR-106 and DD-116. The release coordinator calls the nine owning Console scripts; it does not duplicate their assertions. All daemon/database fixtures are isolated, and the gate requires a clean worktree so current HEAD is the exact release candidate.

```bash
./scripts/qa/test-process-console-release.sh
```

## Scenario 1: Migration Identity Is Forward-compatible

### Preconditions

- `sqlite3` is installed.
- Build current daemon and CLI binaries.

### Goal

Prove action-audit capability is identified by migration/catalog/schema presence instead of latest-version equality.

### Steps

1. Run `./scripts/qa/test-control-plane-action-audit.sh`.
2. Let the script back up the isolated database into schema-31, current schema-32, simulated schema-33, and missing-migration-31 copies.
3. Require migration row 31 to be named `m0031_control_action_audit` and the required audit table columns to exist.

### Expected

- Schema 31, schema 32, and simulated future additive schema pass.
- A catalog missing migration 31 fails even though its maximum version remains 32.
- The script reports `Control-plane action audit QA: 7 passed, 0 failed`.

## Scenario 2: Populated Schema-26 Upgrade Preserves Console State

### Preconditions

- Use the file-backed migration fixture only.

### Goal

Verify the supported historical upgrade retains identities, associations, and rebuildable derived metrics.

### Steps

1. Run:

   ```bash
   cargo test -p agent-orchestrator \
     populated_v26_process_console_upgrade_preserves_entities_and_rebuilds_metrics --lib -- --nocapture
   ```

2. Inspect the test assertions for task, Session, Attention, handoff, source event/binding, audit joins, project backfill, and metric rebuild.

### Expected

- The current successor schema is 34 and exactly three migrations are applied from the populated schema-31 state; the original Console v1 boundary remains migrations 27-32 and FR-113 owns migrations 33-34.
- All seeded entity IDs and the shared request-ID join remain present.
- The exited legacy Session is normalized to `closed` with state version 1.
- Rebuild produces one rollup for each supported bucket.

### Expected Data State

```sql
SELECT version, name FROM schema_migrations WHERE version BETWEEN 27 AND 32;
-- Expected: six applied migrations, including 31=m0031_control_action_audit

SELECT version, name FROM schema_migrations WHERE version IN (33, 34);
-- Expected: successor additive source automation migrations are also applied

SELECT COUNT(*) FROM process_metric_rollups WHERE project_id='console-project';
-- Expected: six supported bucket rollups after rebuild
```

## Scenario 3: Clean Current-HEAD Aggregate Gate Preserves Slice Ownership

### Preconditions

- Required tools are installed.
- `git status --porcelain` is empty.

### Goal

Prove one command builds the candidate and runs repository plus slice acceptance without hiding the failing owner.

### Steps

1. Run `./scripts/qa/test-process-console-release.sh`.
2. Confirm daemon, CLI, Rust GUI, and web GUI builds occur before slice scripts.
3. Confirm workspace tests, strict Clippy, and documentation lint pass.
4. Confirm the coordinator invokes timeline, Attention, handoff/resume, Session, source/Slack, action audit, Console UI, vertical-flow, and process-metrics scripts.

### Expected

- Dirty worktrees and missing tools fail before domain fixtures start.
- Successful output contains 14 named gates with FR/repository ownership and elapsed time.
- Failure is fail-fast; temporary logs are deleted unless `KEEP_RELEASE_QA=1` is explicitly set.

## Scenario 4: Integrated Recovery, UI, Privacy, And Performance

### Preconditions

- Scenario 3 is running from current HEAD.

### Goal

Verify the complete operator loop and release budgets across real client boundaries.

### Steps

1. Require the vertical-flow script to cross production Tauri handlers and an isolated gRPC daemon.
2. Require failure → Attention → evidence → handoff → stale rejection → reviewed resume → Attention resolution.
3. Run the Console UI gate and require Vitest, Playwright, production build, accessibility, role, responsive, motion, and transparency checks.
4. Run release-mode 50,000-event metrics/timeline fixtures.

### Expected

- Request IDs correlate handoff review, rejected stale execution, successful resume, and Attention resolution without exposing payload content.
- 61 frontend unit/component tests and 15 Playwright tests pass.
- Metrics and timeline remain within the DD-114 latency and response-size budgets.

## Scenario 5: Release Notes And Rollback Contract Are Complete

### Preconditions

- Review `CHANGELOG.md` and `docs/guide/agent-process-console-v1-operations.md`.

### Goal

Ensure operators can upgrade, stop one failing domain, or roll back without destructive schema advice.

### Steps

1. Verify the changelog covers FR-095 through FR-105 capabilities, migrations 27-32, compatibility, `_system` Session authority, audit rollout, rollback, and non-goals.
2. Verify the runbook covers tool/disk preflight, SQLite integrity and `.backup`, maintenance/drain/stop, startup migrations, feature rollout, smoke checks, and per-domain stop-loss.
3. Verify normal rollback disables writers before deploying prior binaries and retains migrations 27-32.
4. Verify database restore is labeled a last resort for migration failure or corruption.

### Expected

- No normal rollback step drops Console tables, deletes migration rows, or copies a live SQLite file directly.
- GUI-only, source, Session, resume, Attention, audit, and metrics regressions have independent stop-loss actions.
- Compatibility and unsupported desktop/SaaS/down-migration boundaries are explicit.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Migration identity is forward-compatible | PASS | 2026-07-15 | Codex | Schema 31/32/future accepted; missing migration 31 rejected; action-audit QA 7/7 |
| 2 | Populated schema-26 upgrade preserves Console state | PASS | 2026-07-15 | Codex | Task, Session, Attention, handoff, source, audit joins, backfill, and six rollups preserved |
| 3 | Clean current-HEAD aggregate gate preserves slice ownership | PASS | 2026-07-15 | Codex | 14/14 gates passed in 399 seconds from a clean worktree |
| 4 | Integrated recovery, UI, privacy, and performance | PASS | 2026-07-16 | Codex | Real Tauri/gRPC flow, 21 Vitest, 15 Playwright, builds, accessibility, and both release fixtures passed |
| 5 | Release notes and rollback contract are complete | PASS | 2026-07-15 | Codex | Forward-only runbook separates normal binary rollback from disaster restore |
