---
self_referential_safe: false
self_referential_safe_scenarios: [S2]
---

# Orchestrator - Database Persistence Bootstrap and Repository Boundaries

**Module**: orchestrator
**Scope**: FR-009 Phase 1 persistence bootstrap, SQLite boundary extraction, and repository-backed session/store wrappers
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates the first delivered phase of FR-009:

- schema bootstrap now routes through `PersistenceBootstrap::ensure_current`
- low-level SQLite access moved under `core/src/persistence/`
- public `crate::db::ensure_column` is removed
- `AsyncSessionStore` delegates to `SessionRepository`
- `LocalStoreBackend` delegates to `WorkflowStoreRepository`

Entry points:

- `cargo test -p agent-orchestrator ...`
- `cargo test -p agent-orchestrator --no-run`
- `rg -n ... core/src`

---

## Scenario 1: Persistence Bootstrap Owns Schema Initialization

### Preconditions
- Repository root is the current working directory.
- Rust toolchain is available.

### Goal
Verify the new persistence bootstrap path owns schema initialization and reports a current schema state.

### Steps
1. Run the focused bootstrap test:
   ```bash
   cargo test -p agent-orchestrator persistence::schema::tests::bootstrap_creates_latest_schema_and_reports_current_status -- --exact
   ```
2. Run the compile-only check for the package:
   ```bash
   cargo test -p agent-orchestrator --no-run
   ```

### Expected
- The focused bootstrap test passes.
- The compile-only test succeeds.
- No schema initialization errors are reported.

### Expected Data State
```sql
-- N/A: validated through isolated temp-db unit test created by the test case itself.
```

---

## Scenario 2: Public DB Facade No Longer Exposes `ensure_column`

### Preconditions
- Repository root is the current working directory.

### Goal
Verify business modules can no longer import or call a public `crate::db::ensure_column`.

### Steps
1. Search for a public `ensure_column` definition:
   ```bash
   rg -n "pub fn ensure_column" core/src/db.rs core/src
   ```
2. Search for call sites still importing the old public helper:
   ```bash
   rg -n "use crate::db::ensure_column|crate::db::ensure_column" core/src
   ```
3. Search for the replacement private helper location:
   ```bash
   rg -n "fn ensure_column_exists" core/src/persistence/migration_steps.rs
   ```

### Expected
- Step 1 returns no matches.
- Step 2 returns no matches.
- Step 3 returns exactly one private helper definition in `core/src/persistence/migration_steps.rs`.

### Expected Data State
```sql
-- N/A: source-level governance check.
```

---

## Scenario 3: Session Async Wrapper Delegates Through Repository Boundary

### Preconditions
- Repository root is the current working directory.
- Rust toolchain is available.

### Goal
Verify the async session wrapper remains behaviorally correct after moving behind `SessionRepository`.

### Steps
1. Run the async session wrapper regression:
   ```bash
   cargo test -p agent-orchestrator session_store::tests::async_session_store_exercises_all_wrapper_methods -- --exact
   ```
2. Run the session lifecycle regression:
   ```bash
   cargo test -p agent-orchestrator session_store::tests::insert_load_and_update_session_lifecycle -- --exact
   ```

### Expected
- Both tests pass.
- Session insert, state update, PID update, reader/writer attachment, and cleanup behavior remain intact.

### Expected Data State
```sql
-- N/A: validated through isolated temp-db unit tests that assert agent_sessions and session_attachments state transitions.
```

---

## Scenario 4: Local Workflow Store Uses Repository-Backed Persistence

### Preconditions
- Repository root is the current working directory.
- Rust toolchain is available.

### Goal
Verify the local workflow store backend still supports CRUD/list/prune semantics after delegating to `WorkflowStoreRepository`.

### Steps
1. Run focused local store regressions:
   ```bash
   cargo test -p agent-orchestrator store::local::tests::put_get_delete_round_trip -- --exact
   cargo test -p agent-orchestrator store::local::tests::put_upserts_existing_key -- --exact
   cargo test -p agent-orchestrator store::local::tests::list_returns_entries -- --exact
   ```
2. Run the service-level retention regression:
   ```bash
   cargo test -p agent-orchestrator service::store::tests::store_prune_uses_workflow_store_retention -- --exact
   ```

### Expected
- All targeted tests pass.
- CRUD, upsert, list ordering, and prune semantics remain unchanged.

### Expected Data State
```sql
-- N/A: validated through isolated temp-db and service tests against workflow_store_entries behavior.
```

---

## Scenario 5: Full Package Regression Remains Green After Persistence Refactor

### Preconditions
- Repository root is the current working directory.
- Rust toolchain is available.

### Goal
Verify the persistence extraction does not regress orchestrator package behavior outside the targeted modules.

### Steps
1. Run the full package test suite:
   ```bash
   cargo test -p agent-orchestrator
   ```

### Expected
- The full `agent-orchestrator` test suite passes.
- No failing regressions appear in scheduler, service, session, store, or migration tests.

### Expected Data State
```sql
-- N/A: package-level regression suite.
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Persistence Bootstrap Owns Schema Initialization | PASS | 2026-03-11 | Codex | Focused bootstrap test and `--no-run` compile check both passed |
| 2 | Public DB Facade No Longer Exposes `ensure_column` | PASS | 2026-03-11 | Codex | `rg` confirmed no public helper and no `crate::db::ensure_column` call sites remain |
| 3 | Session Async Wrapper Delegates Through Repository Boundary | PASS | 2026-03-11 | Codex | Async wrapper regression and lifecycle regression both passed |
| 4 | Local Workflow Store Uses Repository-Backed Persistence | PASS | 2026-03-11 | Codex | Local store CRUD/list and service prune regressions all passed |
| 5 | Full Package Regression Remains Green After Persistence Refactor | PASS | 2026-03-11 | Codex | `cargo test -p agent-orchestrator` passed: 1805 unit + 24 integration |
