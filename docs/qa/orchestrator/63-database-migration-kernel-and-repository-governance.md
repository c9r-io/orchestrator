---
self_referential_safe: false
---

# Orchestrator - Database Migration Kernel and Repository Governance

**Module**: orchestrator
**Scope**: FR-009 follow-up governance for migration kernel split, repository expansion boundaries, and DB operations visibility
**Scenarios**: 5
**Priority**: High

---

## Background

This document defines the QA surface for FR-009 closure after persistence bootstrap extraction:

- migration logic moves from the single `core/src/migration.rs` implementation into a dedicated persistence migration kernel
- runtime task/scheduler/config SQL paths are governed behind repository boundaries
- `core/src/db.rs` remains compatibility-only and must not grow new business helpers
- operators gain read-only DB visibility through explicit CLI commands
- historical SQLite upgrade validation is a first-class regression path

Entry points:

- `cargo test -p agent-orchestrator ...`
- `orchestrator db status`
- `orchestrator db migrations list`
- `rg -n ... core/src docs`

---

## Scenario 1: Migration Catalog Has Stable Governance Invariants

### Preconditions
- Repository root is the current working directory.
- Rust toolchain is available.

### Goal
Verify migration registration remains strictly ordered and safe after the kernel split.

### Steps
1. Run focused migration invariant tests:
   ```bash
   cargo test -p agent-orchestrator migration::tests::all_migrations_versions_are_ascending -- --exact
   cargo test -p agent-orchestrator migration::tests::all_migrations_versions_are_contiguous -- --exact
   cargo test -p agent-orchestrator migration::tests::all_migrations_names_are_unique -- --exact
   ```
2. Search for catalog ownership in the new migration kernel:
   ```bash
   rg -n "pub fn registered_migrations|pub struct Migration" core/src/persistence/migration.rs
   ```
3. Search for any migration step implementation still added directly in the old file:
   ```bash
   rg -n "pub\\(crate\\) fn m[0-9]{4}_" core/src/migration.rs
   ```

### Expected
- All invariant tests pass.
- `core/src/persistence/migration.rs` owns the registered migration catalog.
- The legacy file only hosts compatibility forwarding and tests.
- No migration step implementation remains in `core/src/migration.rs`.

### Expected Data State
```sql
-- N/A: source and test governance validation.
```

---

## Scenario 2: Pending Migration Execution Remains Idempotent And Safe

### Preconditions
- Repository root is the current working directory.
- Rust toolchain is available.

### Goal
Verify the migration runner still behaves correctly after responsibility split.

### Steps
1. Run focused migration execution regressions:
   ```bash
   cargo test -p agent-orchestrator migration::tests::run_pending_applies_all_on_fresh_db -- --exact
   cargo test -p agent-orchestrator migration::tests::run_pending_is_idempotent -- --exact
   cargo test -p agent-orchestrator migration::tests::failed_migration_does_not_advance_version -- --exact
   ```

### Expected
- First run applies pending migrations.
- Second run applies zero migrations.
- A failing migration does not advance the recorded schema version.

### Expected Data State
```sql
SELECT version, name FROM schema_migrations ORDER BY version;
-- Rows appear once per applied migration version, with no gaps caused by failed migrations.
```

---

## Scenario 3: Runtime Persistence Continues Moving Behind Repository Boundaries

### Preconditions
- Repository root is the current working directory.

### Goal
Verify FR-009 closure did not allow new business SQL helpers to grow from compatibility modules.

### Steps
1. Search for direct additions to the DB facade:
   ```bash
   rg -n "^pub fn " core/src/db.rs
   ```
2. Search for raw connection access in targeted follow-up areas:
   ```bash
   rg -n "open_conn\\(|Connection::open|rusqlite::params!" core/src/db_write.rs core/src/scheduler* core/src/config_load
   ```
3. Search for repository traits and implementations covering the intended aggregates:
   ```bash
   rg -n "trait (TaskRepository|SchedulerRepository|ConfigRepository)" core/src/persistence core/src
   ```

### Expected
- `core/src/db.rs` does not gain new schema helpers.
- `SchedulerRepository` exists and owns scheduler queue-selection SQL.
- `ConfigRepository` exists and owns config snapshot/heal-log/resource persistence database access.
- `DbWriteCoordinator` no longer owns runtime write-path SQL; task write SQL lives in `TaskRepository`.

### Expected Data State
```sql
-- N/A: source-level governance check.
```

---

## Scenario 4: CLI Exposes Read-Only Schema And Migration Status

### Preconditions
- A runnable `orchestrator` binary is available.
- Test database path is available or the default local DB exists.

### Goal
Verify operators can inspect schema state without mutating the database.

### Steps
1. Run:
   ```bash
   orchestrator db status
   ```
2. Run:
   ```bash
   orchestrator db migrations list
   ```
3. Run focused regression coverage:
   ```bash
   cargo test -p agent-orchestrator service::system::tests::db_status_reports_current_schema -- --exact
   cargo test -p agent-orchestrator service::system::tests::db_migrations_list_marks_all_migrations_applied_on_seeded_state -- --exact
   cargo test -p orchestrator-cli cli::tests::db_status_subcommand_accepts_json_flag -- --exact
   cargo test -p orchestrator-cli commands::db::tests::print_migrations_accepts_table_output -- --exact
   ```

### Expected
- `db status` prints current version, target version, and pending state clearly.
- `db migrations list` shows applied and/or pending migration descriptors in a readable form.
- Neither command mutates application state.
- Focused core and CLI regressions for the DB commands pass.

### Expected Data State
```sql
SELECT COALESCE(MAX(version), 0) AS current_version FROM schema_migrations;
-- Read-only inspection should match the CLI-reported current version.
```

---

## Scenario 5: Historical SQLite Upgrade And Full Package Regression

### Preconditions
- File-backed historical SQLite sample tests exist for empty, old-version, partial-upgrade, and current states.
- Repository root is the current working directory.
- Rust toolchain is available.

### Goal
Verify the migration kernel can safely upgrade representative historical databases and that migration-kernel and repository-boundary work does not regress orchestrator behavior.

### Steps
1. Run the focused historical upgrade regressions:
   ```bash
   cargo test -p agent-orchestrator migration::tests::file_backed_blank_database_upgrades_to_latest -- --exact
   cargo test -p agent-orchestrator migration::tests::file_backed_mid_schema_database_upgrades_to_latest -- --exact
   cargo test -p agent-orchestrator migration::tests::file_backed_partial_upgrade_database_recovers_to_latest -- --exact
   cargo test -p agent-orchestrator migration::tests::file_backed_current_database_is_noop -- --exact
   ```
2. Run the full package regression suite:
   ```bash
   cargo test -p agent-orchestrator
   ```

### Expected
- Empty databases upgrade to the latest schema.
- Older databases upgrade in place without losing version tracking.
- Partially upgraded databases recover and converge to latest.
- Current databases report zero pending migrations.
- The full package test suite passes with no regressions in bootstrap, scheduler, repository, session, store, or migration paths.

### Expected Data State
```sql
SELECT version, name FROM schema_migrations ORDER BY version;
-- Every upgraded sample ends at the latest registered version.
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Migration Catalog Has Stable Governance Invariants | PASS | 2026-03-11 | Codex | Invariant tests passed; catalog ownership moved to `core/src/persistence/migration.rs` |
| 2 | Pending Migration Execution Remains Idempotent And Safe | PASS | 2026-03-11 | Codex | Fresh-db, idempotency, and failed-migration regressions all passed |
| 3 | Runtime Persistence Continues Moving Behind Repository Boundaries | PASS | 2026-03-11 | Codex | `SchedulerRepository`、`ConfigRepository` 和 task write repository seam 已落地；`DbWriteCoordinator` 不再持有 SQL |
| 4 | CLI Exposes Read-Only Schema And Migration Status | PASS | 2026-03-11 | Codex | Core service + CLI command regressions passed after `db` command rollout |
| 5 | Historical SQLite Upgrade And Full Package Regression | PASS | 2026-03-11 | Codex | File-backed upgrade tests + `cargo test -p agent-orchestrator` all passed |
