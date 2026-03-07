# WP05: Integration Validation — Primitive Composition Verification

## Goal

Validate that WP01–WP04 **compose correctly** when used together in a single workflow. Individual primitive correctness is already covered by QA docs 46–49; WP05 focuses exclusively on **inter-primitive interactions** and **end-to-end composition**.

## Design Principles

1. **Decoupled layers** — each validation layer is independently runnable; failures isolate to a specific composition boundary
2. **Deterministic** — all agents are mock shell commands with predictable output; no AI variance
3. **Incremental** — layers build from pairwise composition to full integration; skip-to-layer is supported
4. **Self-contained** — each fixture includes its own store seed data, invariant definitions, and expected outputs

## Dependencies

- WP01 (Persistent Store) — implemented (QA: 46)
- WP02 (Task Spawning) — implemented (QA: 47)
- WP03 (Dynamic Items + Selection) — implemented (QA: 48)
- WP04 (Invariant Constraints) — implemented (QA: 49)
- Engine wiring verified (QA: 50)

---

## Validation Architecture

```
Layer 1: Pairwise Composition (4 scenarios)
  Each scenario exercises exactly 2 primitives interacting

Layer 2: Triple Composition (2 scenarios)
  Three primitives cooperating in a realistic sub-workflow

Layer 3: Full Composition (2 scenarios)
  All four primitives in a self-evolving workflow — happy path + violation path
```

Each layer has:
- A fixture YAML (`fixtures/manifests/bundles/wp05-*.yaml`)
- A corresponding section in the E2E script
- Pass/fail assertions checking DB state via `sqlite3`

---

## Layer 1: Pairwise Composition

### L1-A: Store + Spawning (WP01 x WP02)

**What it proves**: A parent task writes context to store; spawned child reads it back.

**Fixture**: `wp05-store-spawn.yaml`

```
Workflow: store-spawn-test
  Step 1 (task-scoped): echo mock plan output
    post_actions:
      - store_put: { namespace: context, key: parent_finding, value_from: plan_output }
      - spawn_task: { goal: "child-task", workflow: store-spawn-child }

Child Workflow: store-spawn-child
  Step 1 (task-scoped): echo child execution
    store_inputs:
      - namespace: context, key: parent_finding, into_var: inherited_context
```

**Assertions**:
1. Child task exists with `parent_task_id` set to parent's ID
2. Store key `context/parent_finding` contains parent's output
3. Child task's pipeline_vars contain `inherited_context` populated from store

**Boundary tested**: store_put in post_action -> spawn -> store_inputs in child

---

### L1-B: Store + Invariants (WP01 x WP04)

**What it proves**: Invariant assertions can reference store values as baselines.

**Fixture**: `wp05-store-invariant.yaml`

```
Pre-seed: store_put(baselines, min_test_count, "10")

Workflow: store-invariant-test
  Step 1 (task-scoped): echo '{"test_count": 8}'
    captures: test_count

  Invariant: test_count_no_regression
    command: "echo 8"
    capture_as: current_count
    assert: "int(current_count) >= int(store('baselines', 'min_test_count'))"
    check_at: [before_complete]
    on_violation: halt
    immutable: true
```

**Assertions**:
1. Task status = `invariant_violated` (8 < 10)
2. Event `invariant_violated` emitted with invariant name
3. Re-run with `echo 12` -> task completes successfully (12 >= 10)

**Boundary tested**: invariant assert expression reads live store values

---

### L1-C: Dynamic Items + Selection (WP03 isolated, baseline)

**What it proves**: generate_items -> parallel execution -> item_select round-trip works end-to-end with mock agents.

**Fixture**: `wp05-items-select.yaml`

```
Workflow: items-select-test
  Step 1 (task-scoped, meta_planner):
    command: echo '{"candidates":[{"id":"fast","name":"fast","description":"opt-speed"},{"id":"safe","name":"safe","description":"opt-safety"},{"id":"balanced","name":"balanced","description":"opt-both"}]}'
    post_actions:
      - generate_items: { from_var: plan_output, json_path: "$.candidates", ... }

  Step 2 (item-scoped, benchmark):
    command per item:
      fast:     echo '{"score": 95}'
      safe:     echo '{"score": 72}'
      balanced: echo '{"score": 88}'
    captures: score

  Step 3 (task-scoped, select_best):
    builtin: item_select
    config: { strategy: max, metric_var: score }
```

**Assertions**:
1. Three items created with source=`dynamic`
2. Winner = `fast` (score 95)
3. Winner status = `qa_passed`, others = `eliminated`
4. Pipeline var `item_select_winner` = `fast`

**Boundary tested**: generate_items -> item fan-out -> item_select convergence

---

### L1-D: Dynamic Items + Invariants (WP03 x WP04)

**What it proves**: Invariant check fires per-item after implement, blocking bad candidates before selection.

**Fixture**: `wp05-items-invariant.yaml`

```
Workflow: items-invariant-test
  Step 1 (task-scoped): generate 2 candidates (good, bad)

  Step 2 (item-scoped, implement):
    good: exit 0
    bad:  exit 0

  Invariant: after_implement
    command for good: exit 0
    command for bad: exit 1   (simulated test failure)
    check_at: [after_implement]
    on_violation: halt
```

**Assertions**:
1. `bad` item halted at after_implement checkpoint
2. `good` item proceeds to next step
3. Task does not fully halt (item-level violation, not task-level)

**Boundary tested**: invariant checkpoint interacts correctly with multi-item execution

---

## Layer 2: Triple Composition

### L2-A: Store + Items + Selection (WP01 x WP03)

**What it proves**: Winner's metrics are persisted to store; a subsequent task reads the stored winner data.

**Fixture**: `wp05-store-items-select.yaml`

```
Workflow: store-items-select-test
  Step 1: generate 2 candidates
  Step 2 (item-scoped): benchmark -> captures score
  Step 3: item_select (strategy: max)
    config.store_result: { namespace: evolution, key: winner_latest }
  Step 4: store_put journal entry from task_summary
    post_actions:
      - spawn_task: { goal: "verify-store", workflow: verify-winner }

Child Workflow: verify-winner
  Step 1:
    store_inputs:
      - namespace: evolution, key: winner_latest, into_var: prev_winner
```

**Assertions**:
1. Store key `evolution/winner_latest` contains winner ID and score
2. Child task's `prev_winner` var matches the stored winner data
3. Journal entry exists in `journal` namespace

**Boundary tested**: item_select -> store_result -> spawn -> store_inputs (3-primitive data flow)

---

### L2-B: Items + Invariants + Store (WP03 x WP04 x WP01)

**What it proves**: Invariant violations reference store baselines in a multi-candidate context; only candidates meeting the baseline survive selection.

**Fixture**: `wp05-items-invariant-store.yaml`

```
Pre-seed: store_put(baselines, min_score, "80")

Workflow: items-invariant-store-test
  Step 1: generate 3 candidates
  Step 2 (item-scoped, implement): each produces a score
  Step 3 (item-scoped, benchmark): captures score

  Invariant: score_baseline
    assert: "int(score) >= int(store('baselines', 'min_score'))"
    check_at: [before_complete]
    on_violation: halt

  Step 4: item_select (strategy: max, from surviving items)
```

**Assertions**:
1. Candidates with score < 80 are halted by invariant
2. Remaining candidates proceed to selection
3. If all candidates fail invariant -> task status = `invariant_violated`
4. Store baseline value (80) was read correctly during assertion evaluation

**Boundary tested**: store-backed invariant assertion in multi-candidate pipeline

---

## Layer 3: Full Composition

### L3-A: Self-Evolving Happy Path (WP01 x WP02 x WP03 x WP04)

**What it proves**: All four primitives compose into a complete self-evolving cycle.

**Fixture**: `wp05-self-evolving-mock.yaml`

This is the full `self-evolving` workflow from the overview doc, but with all agents replaced by deterministic mock commands.

```
Pre-seed:
  store_put(journal, recent_improvements, "[]")
  store_put(metrics, latest, '{"test_count": 100}')
  store_put(baselines, min_test_count, "100")
  store_put(baselines, min_qa_docs, "1")

Workflow: self-evolving (mock)
  meta_planner: reads store(journal + metrics + baselines), outputs 3 candidates
  implement (x3 parallel): mock code changes
  self_test (x3 parallel): mock cargo test (all pass)
  benchmark (x3 parallel): mock scores (101, 99, 103)
  select_best: weighted by test_count -> winner = candidate with 103
  self_restart: mock build (exit 0, skip actual restart)
  qa_testing (last cycle): mock QA pass
  record_results:
    store_put journal entry
    store_put metrics
    spawn_tasks: 1 follow-up task

  Invariants (immutable):
    all_tests_pass: mock exit 0
    test_count_no_regression: 103 >= 100 -> pass
    no_deleted_qa_docs: count >= 1 -> pass

  Loop: fixed, max_cycles: 2
```

**Assertions**:
1. **Store round-trip**: meta_planner received seeded journal/metrics/baselines
2. **Dynamic items**: 3 candidates created, all executed in parallel
3. **Selection**: winner = candidate with score 103
4. **Invariants**: all 3 passed at before_complete
5. **Spawn**: 1 follow-up task created with correct parent lineage
6. **Store persistence**: journal and metrics updated post-run
7. **Cycle 2 learning**: second cycle's meta_planner sees cycle 1's journal entry

**This is the capstone test** — it validates the full primitive composition without testing any individual primitive in isolation.

---

### L3-B: Self-Evolving Violation Path (WP01 x WP03 x WP04)

**What it proves**: When all candidates regress below the store baseline, invariants halt the task safely — no broken code proceeds.

**Fixture**: Reuses `wp05-self-evolving-mock.yaml` with override seed.

```
Pre-seed:
  store_put(baselines, min_test_count, "200")  # impossibly high baseline

Same workflow, but all candidates score < 200
```

**Assertions**:
1. All candidates eliminated by invariant at before_complete
2. Task status = `invariant_violated`
3. No spawn_tasks executed (post-action gated on success)
4. Store journal NOT updated (task did not succeed)
5. Event stream contains `invariant_violated` with correct invariant name

---

## Fixture Design

### Mock Agent Convention

All mock agents use shell `echo` commands that output deterministic JSON:

```yaml
command: |
  echo '{"confidence":0.95,"quality_score":0.9,"artifacts":[]}'
```

For item-scoped steps that need per-item variation, use the `{item_id}` template variable:

```yaml
command: |
  case "{item_id}" in
    fast)     echo '{"score": 95}' ;;
    safe)     echo '{"score": 72}' ;;
    balanced) echo '{"score": 88}' ;;
  esac
```

### Store Seeding

Each fixture includes `WorkflowStore` CRDs and a seed script section:

```yaml
apiVersion: orchestrator.dev/v2
kind: WorkflowStore
metadata:
  name: baselines
spec:
  provider: local
  schema:
    fields:
      - name: min_test_count
        type: integer
  retention:
    max_entries: 100
```

Seed data is inserted by the test script before workflow execution:

```bash
$ORCH store put baselines min_test_count 100
```

---

## Test Script Architecture

**File**: `scripts/qa/test-wp05-integration.sh`

```
Usage: test-wp05-integration.sh [--layer N] [--scenario ID] [--verbose]

  --layer 1|2|3     Run only scenarios in the specified layer
  --scenario L1-A   Run a single scenario by ID
  --verbose         Show full orchestrator output
  (no args)         Run all scenarios sequentially
```

### Script Structure

```bash
#!/usr/bin/env bash
set -euo pipefail

# ── Shared utilities ──
setup_db()          # init fresh DB + seed store
teardown_db()       # cleanup
assert_task_status()    # query tasks table
assert_store_value()    # query store via CLI
assert_event_exists()   # query events table
assert_item_count()     # query task_items table
assert_pipeline_var()   # query pipeline_vars

# ── Layer 1 ──
test_L1A_store_spawn()
test_L1B_store_invariant()
test_L1C_items_select()
test_L1D_items_invariant()

# ── Layer 2 ──
test_L2A_store_items_select()
test_L2B_items_invariant_store()

# ── Layer 3 ──
test_L3A_self_evolving_happy()
test_L3B_self_evolving_violation()

# ── Runner ──
run_layer() { ... }
run_scenario() { ... }
main() { parse_args; run_selected; report_summary; }
```

Each `test_*` function follows the pattern:

```bash
test_L1A_store_spawn() {
  info "L1-A: Store + Spawning"
  setup_db
  $ORCH apply -f fixtures/manifests/bundles/wp05-store-spawn.yaml
  $ORCH store put context parent_finding '{"finding":"optimize-parser"}'
  $ORCH task create --goal "test-store-spawn" --workflow store-spawn-test

  $ORCH run --task "$TASK_ID" --max-steps 10

  assert_store_value context parent_finding '{"finding":"optimize-parser"}'
  assert_task_status "$CHILD_ID" pending
  assert_pipeline_var "$CHILD_ID" inherited_context '{"finding":"optimize-parser"}'
  teardown_db
}
```

---

## Deliverables

| # | File | Purpose |
|---|------|---------|
| 1 | `fixtures/manifests/bundles/wp05-store-spawn.yaml` | L1-A fixture |
| 2 | `fixtures/manifests/bundles/wp05-store-invariant.yaml` | L1-B fixture |
| 3 | `fixtures/manifests/bundles/wp05-items-select.yaml` | L1-C fixture |
| 4 | `fixtures/manifests/bundles/wp05-items-invariant.yaml` | L1-D fixture |
| 5 | `fixtures/manifests/bundles/wp05-store-items-select.yaml` | L2-A fixture |
| 6 | `fixtures/manifests/bundles/wp05-items-invariant-store.yaml` | L2-B fixture |
| 7 | `fixtures/manifests/bundles/wp05-self-evolving-mock.yaml` | L3-A/B fixture |
| 8 | `scripts/qa/test-wp05-integration.sh` | Modular E2E test script |
| 9 | `docs/qa/orchestrator/51-primitive-composition.md` | QA doc with all 8 scenarios |

## Success Criteria

1. All 8 scenarios pass with deterministic mock agents
2. Each layer can run independently (`--layer N`)
3. No scenario depends on another scenario's side effects (fresh DB per test)
4. Layer 3 proves that the four primitives compose without special-case engine code
5. Violation path (L3-B) confirms safety: invariant halt prevents broken state propagation
