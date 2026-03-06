# WP05: Integration Validation — Self-Evolving Workflow End-to-End

## Goal

Validate that all four workflow primitives (WP01–WP04) compose into a single `self-evolving.yaml` workflow that demonstrates:
1. **Learning** — each run uses knowledge from previous runs
2. **Goal discovery** — the workflow autonomously identifies what to improve next
3. **Exploration** — multiple candidate approaches are tried in parallel
4. **Safety** — immutable invariants prevent quality degradation

All of this expressed in **workflow YAML** — no new built-in Rust behavior beyond the four primitives.

## Dependencies

- WP01 (Persistent Store) — implemented and tested
- WP02 (Task Spawning) — implemented and tested
- WP03 (Dynamic Items) — implemented and tested
- WP04 (Invariant Constraints) — implemented and tested

## The Self-Evolving Workflow

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  meta_planner (task-scoped)                                      │
│  Reads: store(journal), store(metrics), store(baselines)         │
│  Outputs: improvement goals, candidate strategies                │
│  Post-action: generate_items from candidates                     │
├─────────────────────────────────────────────────────────────────┤
│  implement (item-scoped, parallel)                               │
│  Each candidate implements its strategy independently            │
│  [after_implement: invariant check]                              │
├─────────────────────────────────────────────────────────────────┤
│  self_test (item-scoped, parallel)                               │
│  Each candidate must pass cargo check + cargo test               │
├─────────────────────────────────────────────────────────────────┤
│  benchmark (item-scoped, parallel)                               │
│  Measure: test count, compile time, test time                    │
├─────────────────────────────────────────────────────────────────┤
│  select_best (task-scoped)                                       │
│  Pick winner by weighted metrics                                 │
│  Store winner's metrics + approach to journal                    │
├─────────────────────────────────────────────────────────────────┤
│  self_restart (task-scoped)                                      │
│  Build + verify + exit(75) for winner's code                     │
│  [before_restart: invariant check]                               │
├─────────────────────────────────────────────────────────────────┤
│  qa_testing (item-scoped, last cycle)                            │
│  Full QA on the evolved codebase                                 │
├─────────────────────────────────────────────────────────────────┤
│  record_results (task-scoped)                                    │
│  Write metrics + journal entries to store                        │
│  Optionally spawn follow-up tasks for discovered sub-goals       │
│  [before_complete: invariant check]                              │
└─────────────────────────────────────────────────────────────────┘
```

### Workflow YAML (Target State)

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: self-evolving
spec:
  steps:
    # ── Phase 1: Learn + Plan ──────────────────────────────
    - id: meta_planner
      type: plan
      scope: task
      enabled: true
      repeatable: false
      store_inputs:
        - namespace: journal
          key: recent_improvements
          into_var: history
          default: "[]"
        - namespace: metrics
          query: "ORDER BY updated_at DESC LIMIT 10"
          into_var: recent_metrics
        - namespace: baselines
          key: quality_baseline
          into_var: baseline
          default: "{}"
      post_actions:
        - generate_items:
            from_var: plan_output
            json_path: "$.candidates"
            mapping:
              item_id: "$.id"
              label: "$.name"
              vars:
                approach: "$.description"
                target_files: "$.files"
            replace: true

    # ── Phase 2: Implement Candidates ──────────────────────
    - id: implement
      type: implement
      scope: item
      enabled: true
      repeatable: true
      max_parallel: 3

    # ── Phase 3: Test + Benchmark ──────────────────────────
    - id: self_test
      type: self_test
      scope: item
      builtin: self_test

    - id: benchmark
      type: test
      scope: item
      max_parallel: 3
      command: |
        cd core
        TEST_OUTPUT=$(cargo test --lib 2>&1)
        TEST_COUNT=$(echo "$TEST_OUTPUT" | grep -oP '\d+ passed' | grep -oP '\d+')
        echo "{\"test_count\": $TEST_COUNT}"
      captures:
        - regex: '"test_count":\s*(\d+)'
          var: test_count

    # ── Phase 4: Select Winner ─────────────────────────────
    - id: select_best
      type: evaluate
      scope: task
      builtin: item_select
      config:
        strategy: weighted
        weights:
          test_count: 1.0
        store_result:
          namespace: evolution
          key: "winner_{{task_id}}"

    # ── Phase 5: Apply + Restart ───────────────────────────
    - id: self_restart
      type: self_restart
      scope: task
      builtin: self_restart
      repeatable: false

    # ── Phase 6: QA (final cycle only) ─────────────────────
    - id: qa_testing
      type: qa_testing
      scope: item
      enabled: true
      repeatable: true
      prehook:
        engine: cel
        when: "is_last_cycle"
        reason: "QA deferred to final cycle"

    - id: align_tests
      type: align_tests
      scope: task
      enabled: true
      repeatable: true
      prehook:
        engine: cel
        when: "is_last_cycle"

    # ── Phase 7: Record + Spawn Follow-ups ─────────────────
    - id: record_results
      type: doc_governance
      scope: task
      enabled: true
      repeatable: false
      post_actions:
        - store_put:
            namespace: journal
            key: "run_{{task_id}}"
            value_from: task_summary
            append_to: recent_improvements
            max_entries: 50
        - store_put:
            namespace: metrics
            key: "metrics_{{task_id}}"
            value_from: benchmark_output
        - spawn_tasks:
            from_var: discovered_subgoals
            json_path: "$.goals"
            mapping:
              goal: "$.goal"
              workflow: "$.workflow"
            max_tasks: 3
            queue: true

    - id: loop_guard
      type: loop_guard
      enabled: true
      repeatable: true
      is_guard: true
      builtin: loop_guard

  loop:
    mode: fixed
    max_cycles: 2
    enabled: true

  safety:
    max_consecutive_failures: 3
    auto_rollback: false
    checkpoint_strategy: git_tag
    max_spawned_tasks: 10
    max_spawn_depth: 3

    invariants:
      - name: all_tests_pass
        description: "All library unit tests must pass"
        command: "cd core && cargo test --lib 2>&1"
        expect_exit: 0
        check_at: [after_implement, before_restart, before_complete]
        on_violation: halt
        immutable: true

      - name: test_count_no_regression
        description: "Test count must not decrease from baseline"
        command: "cd core && cargo test --lib 2>&1 | grep -oP '\\d+ passed' | grep -oP '\\d+'"
        capture_as: current_test_count
        assert: "int(current_test_count) >= int(store('baselines', 'min_test_count'))"
        check_at: [before_complete]
        on_violation: halt
        immutable: true

      - name: no_deleted_qa_docs
        description: "QA documents must not be deleted"
        command: "find docs/qa -name '*.md' | wc -l"
        capture_as: qa_doc_count
        assert: "int(qa_doc_count) >= int(store('baselines', 'min_qa_docs'))"
        check_at: [before_complete]
        on_violation: halt
        immutable: true
```

## Validation Scenarios

### Scenario 1: Store Round-Trip Across Tasks

**Goal**: Verify task A writes to store, task B reads it.

**Steps**:
1. Run task A with workflow that writes `{plan_output}` to `store://journal/run_A`
2. Run task B with workflow that reads `store://journal/run_A` into `{history}` var
3. Verify task B's plan step received the value from task A

**Expected**: Task B's agent command includes task A's plan output in `{history}`.

### Scenario 2: Task Spawning Chain

**Goal**: Verify parent task spawns children, lineage is tracked, safety caps work.

**Steps**:
1. Run parent task whose plan outputs 3 goals
2. Verify 3 child tasks created with status `pending`
3. Verify `parent_task_id` set correctly
4. Start child tasks, verify they complete
5. Run a task that tries to spawn 10 children with `max_tasks: 3` — verify only 3 created

**Expected**: Correct lineage, safety cap enforced.

### Scenario 3: Dynamic Items with Selection

**Goal**: Verify 3 candidates are created, executed in parallel, and the best one selected.

**Steps**:
1. Run workflow with `generate_items` producing 3 candidates
2. Each candidate runs a benchmark step capturing `bench_time_ms`
3. `item_select` with `strategy: min` picks the fastest
4. Verify winner status = `qa_passed`, others = `eliminated`
5. Verify subsequent task-scoped steps use winner's pipeline vars

**Expected**: Winner selection correct, losers eliminated, winner context propagated.

### Scenario 4: Invariant Violation Halts Task

**Goal**: Verify an immutable invariant stops a task that introduces regression.

**Steps**:
1. Set baseline `min_test_count = 1334` in store
2. Run self-evolving workflow where implement step "deletes" a test file (mock)
3. Invariant `test_count_no_regression` fires at `before_complete`
4. Verify task status = `invariant_violated`
5. Verify event `invariant_violated` with details

**Expected**: Task halted, no broken code shipped.

### Scenario 5: Invariant Immutability

**Goal**: Verify that a workflow cannot modify its own immutable invariant.

**Steps**:
1. Start workflow with `immutable: true` invariant checking test count
2. Implement step modifies the workflow config (tries to set invariant command to `echo 99999`)
3. Invariant check runs at `before_complete`
4. Verify the **original** invariant command was used, not the tampered one

**Expected**: Engine uses pinned invariant, ignores modification.

### Scenario 6: Full Self-Evolving Cycle

**Goal**: End-to-end run of the self-evolving workflow.

**Steps**:
1. Initialize baselines in store (`min_test_count`, `min_qa_docs`)
2. Run self-evolving workflow with goal "improve test coverage"
3. Verify:
   - meta_planner read history from store
   - 3 candidates generated and implemented in parallel
   - Winner selected by test_count metric
   - self_restart built new binary
   - QA ran on final cycle
   - Results recorded to store
   - Follow-up tasks spawned (if agent produced sub-goals)
   - All invariants passed
4. Run a **second** self-evolving task
5. Verify the second run's meta_planner received the first run's journal entry

**Expected**: Two consecutive self-evolving runs demonstrate learning (second run sees first run's history).

## Deliverables

1. `fixtures/manifests/bundles/self-evolving.yaml` — the full workflow
2. `fixtures/manifests/bundles/self-evolving-mock.yaml` — mock agents for deterministic testing
3. `scripts/qa/test-self-evolving-e2e.sh` — automated E2E validation script
4. `docs/qa/self-bootstrap/50-self-evolving-workflow.md` — QA document with all 6 scenarios

## Success Criteria

The self-evolving workflow:
- Runs entirely from YAML configuration — no hardcoded self-improvement logic in Rust
- Demonstrates measurable improvement across consecutive runs (metrics in store trend upward)
- Halts safely when invariants are violated
- Spawns follow-up tasks without human intervention
- Composes all four primitives naturally without special-case interactions
