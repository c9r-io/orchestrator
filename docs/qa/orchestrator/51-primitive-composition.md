---
lifecycle: active
self_referential_safe: false
---

# Orchestrator - Primitive Composition (WP05)

**Module**: orchestrator
**Scope**: WP01/WP02/WP04 pairwise composition — Store, Spawning, Invariants
**Scenarios**: 2
**Priority**: High

---

## Background

WP01-WP04 each implement a standalone workflow primitive. This document verifies that primitives **compose correctly** when combined in a single workflow execution:

- **WP01 Persistent Store**: `store_put` post-action, `store_inputs` / `store_outputs`
- **WP02 Task Spawning**: `spawn_task` post-action with parent linkage and depth tracking
- **WP04 Invariant Constraints**: `safety.invariants` with `check_at` / `on_violation` at `before_complete`

### What WP03 used to cover, and why it is gone (FR-149)

Scenarios 3 (L1-C), 4 (L1-D) and 5 (L2-A) drove **WP03 Dynamic Items +
Selection** — the `generate_items` post-action and the `item_select` builtin.
DD-137 (`1b0937ca`, 2026-07-25) retired both, and the validator now rejects a
workflow that declares them with `[legacy_json_path_removed]`. Their three
bundles are rejected at `apply`, and `apply` is all-or-nothing over a bundle, so
they could not have run.

**They were not why the gate failed, though, and this document said they were.**
FR-149 ran it instead of reading it: the script dies far earlier, in `ensure_db`
on `orchestrator init` with `daemon socket not found`, before L1-A. `1be4666d`
(2026-03-26) split the CLI from the daemon and this script started none, so it
had not reached its first scenario in four months — four months longer than
FR-148 and DD-158 recorded, and for a different reason. The rotted bundles were
real and would have failed; that is what made the wrong story plausible enough
that nobody ran the gate to check it.

FR-149 rewrote the harness (isolated `ORCHESTRATORD_DATA_DIR`, a daemon the
script starts and reaps, both binaries built by package) and it now passes.

They are removed rather than rewritten: there is no typed replacement to point
them at, because the primitive itself was retired, not re-implemented. The
retirement is recorded in `docs/design_doc/orchestrator/137-legacy-coordination-decommission.md`
and verified by `docs/qa/orchestrator/175-legacy-coordination-decommission.md`.

### Isolation Strategy

Each scenario runs inside its own **project namespace** (`--project wp05-<id>`). This ensures:

- Resources (workspace, agent, workflow) are scoped to the project via `apply --project`
- Tasks are created with `--project`, setting `tasks.project_id`
- Store entries are keyed by `(store_name, project_id, key)` — no cross-scenario leaks
- No database reset required; scenarios are idempotent and repeatable

### Test Fixtures

All scenario manifests live under `fixtures/manifests/bundles/wp05-*.yaml`. Each bundle declares a self-contained set of Workspace + Agent + Workflow resources.

### Automated Test Script

```bash
scripts/qa/test-wp05-integration.sh [--layer 1] [--scenario L1A|L1B] [--verbose]
```

Both bundles drive self-contained `command:` steps (`printf` / `echo`), so the
gate needs no provider and no daemon — only a release build and `sqlite3`.

A `--layer` or `--scenario` value matching no scenario is reported as a
**failure**. Before FR-149 it exited 0 with `PASS: 0 FAIL: 0`, which is
indistinguishable from a clean full run; `--layer 2` and `--scenario L1C` are
exactly the values that became stale, so the check has a live subject.

---

## Scenario 1: Store + Spawning (WP01 x WP02) — L1-A

### Preconditions
- Orchestrator binary built (`cargo build --release`)
- Database initialized (`orchestrator init`)
- Fixture: `fixtures/manifests/bundles/wp05-store-spawn.yaml`

### Goal
Verify that a parent task can write to a persistent store via `store_put` post-action AND spawn a child task via `spawn_task` post-action in the same step. The child must have correct `parent_task_id` and `spawn_depth`.

### Steps
1. Apply the fixture into project scope:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/wp05-store-spawn.yaml --project wp05-L1A
   ```
2. Create and start a task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project wp05-L1A \
     --workspace wp05-ws \
     -W wp05-store-spawn-parent \
     --target-file fixtures/wp05-qa/wp05-check.md \
     --goal "test store+spawn" \
     --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "$TASK_ID" >/dev/null 2>&1
   ```
3. Query task status:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_ID}';"
   ```
4. Verify store entry:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM workflow_store_entries
      WHERE store_name='context' AND project_id='wp05-L1A' AND key='parent_finding';"
   ```
5. Verify child task:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT id, parent_task_id, spawn_depth FROM tasks
      WHERE parent_task_id='${TASK_ID}';"
   ```

### Expected
- Step 3: Task status = `completed`
- Step 4: Count >= 1 (store entry exists with correct project_id)
- Step 5: Exactly 1 child task with `parent_task_id = $TASK_ID` and `spawn_depth >= 1`

### Expected Data State
```sql
-- Store is project-scoped
SELECT store_name, project_id, key FROM workflow_store_entries
WHERE project_id = 'wp05-L1A';
-- Result: context | wp05-L1A | parent_finding

-- Child task has lineage
SELECT parent_task_id, spawn_depth FROM tasks
WHERE parent_task_id = '<TASK_ID>';
-- Result: <TASK_ID> | 1
```

---

## Scenario 2: Store + Invariants — Halt & Pass (WP01 x WP04) — L1-B

### Preconditions
- Fixture: `fixtures/manifests/bundles/wp05-store-invariant.yaml`
- Contains TWO workflows:
  - `wp05-store-invariant-fail`: invariant command `exit 1`, expect_exit 0 → violation
  - `wp05-store-invariant-pass`: invariant command `exit 0`, expect_exit 0 → pass

### Goal
Verify that `before_complete` invariant violations halt the task (status = failed) while passing invariants allow normal completion. Tests the guard-step invariant integration fixed in `loop_engine.rs`.

### Steps — Violation Path
1. Apply fixture:
   ```bash
   orchestrator apply -f fixtures/manifests/bundles/wp05-store-invariant.yaml --project wp05-L1B
   ```
2. Create and start task with the failing workflow:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project wp05-L1B \
     --workspace wp05-ws \
     -W wp05-store-invariant-fail \
     --target-file fixtures/wp05-qa/wp05-check.md \
     --goal "test invariant fail" \
     --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "$TASK_ID" >/dev/null 2>&1 || true
   ```
3. Verify task failed:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_ID}';"
   ```
4. Verify invariant halt event:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}'
      AND event_type='task_failed'
      AND json_extract(payload_json,'\$.reason')='invariant_halt_before_complete';"
   ```

### Steps — Pass Path
5. Create and start task with the passing workflow:
   ```bash
   TASK_ID2=$(orchestrator task create \
     --project wp05-L1B \
     --workspace wp05-ws \
     -W wp05-store-invariant-pass \
     --target-file fixtures/wp05-qa/wp05-check.md \
     --goal "test invariant pass" \
     --no-start 2>&1 | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "$TASK_ID2" >/dev/null 2>&1
   ```
6. Verify task completed:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT status FROM tasks WHERE id='${TASK_ID2}';"
   ```

### Expected
- Step 3: Task status = `failed`
- Step 4: Count >= 1 (invariant halt event with reason `invariant_halt_before_complete`)
- Step 6: Task status = `completed`

---

## Unit Test Coverage

Backing the two surviving scenarios. Paths re-derived at FR-149 closure with
`git grep -l "fn <name>" -- '*.rs'`; the previous table pointed at
`config/…` and `scheduler/…` paths that the workspace split moved into
`crates/`, and named two tests that no longer exist
(`test_post_action_spawn_task_serde_round_trip`,
`workflow_spec_to_config_converts_steps`). Rows for the WP03 tests are dropped
along with the scenarios they backed — the tests themselves still exist, because
DD-137 retired the *manifest constructs*, not the config types.

| Test | File | Verified |
|------|------|----------|
| `test_post_action_store_put_serde_round_trip` | `crates/orchestrator-config/src/config/step.rs` | PostAction::StorePut serialization |
| `test_invariant_config_defaults` | `crates/orchestrator-config/src/config/invariant.rs` | Default check_at includes before_complete |
| `check_invariants_returns_none_for_empty_invariants` | `crates/orchestrator-scheduler/src/scheduler/loop_engine/tests.rs` | Empty invariants no-op |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Store + Spawning (L1-A) | ✅ | 2026-03-07 | claude | 5/5 assertions. store_put + spawn_task in same step, child lineage verified |
| 2 | Store + Invariants halt/pass (L1-B) | ✅ | 2026-03-07 | claude | 3/3 assertions. Guard-step invariant bypass fixed in loop_engine.rs:850-860 |
| — | A selection matching no scenario fails | ☐ | | | `--layer 2` and `--scenario L1C` must report FAIL, not `PASS: 0 FAIL: 0` |

> The 2026-03-07 results above predate DD-137 and describe a script that could
> still reach the end. FR-149 re-ran L1-A and L1-B on the excised script; that
> run is recorded in `docs/qa/orchestrator/197-dd137-fixture-residue-retirement.md`.
