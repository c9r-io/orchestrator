---
self_referential_safe: true
---

# Orchestrator - Coordination Strangler Completion

**Module**: Orchestrator Scheduler / Workflow Governance  
**Scope**: production inventory, legacy freeze, per-workflow parity, explicit tool/session boundaries, and self-bootstrap survival  
**Scenarios**: 5  
**Priority**: High

> **FR-125 update**: this document remains the authority for the completed
> compatibility window and historical seven-pair parity evidence. Production
> validation now rejects the legacy halves. The script derives and executes only
> the seven tool workflows; current removal acceptance is QA-175.

## Background

This document closes the gap between the single FR-118 coordination pilot and
the complete production workflow migration. All workflow executions use
deterministic mock agents and an isolated local daemon; no provider credentials
or network access are required.

Primary entry point:

```bash
FR125_ALLOW_DIRTY=1 ./scripts/qa/test-coordination-strangler.sh
```

Omit `FR125_ALLOW_DIRTY=1` for clean-tree certification.

---

## Scenario 1: Exact Inventory And Freeze Ratchet

### Preconditions

- Ruby and the repository toolchain are installed.
- The reviewed ledger exists at
  `config/governance/coordination-collapse-ledger.json`.

### Goal

Prove the production workflow inventory is complete and new legacy
coordination cannot enter silently.

### Steps

1. Run:

   ```bash
   ruby scripts/qa/coordination-governance.rb --test-fixtures
   ```

2. Inspect the reported classification and migration counts.
3. Confirm capture, JSONPath post-action, generic safety-variable, and new
   `store_inputs` fixtures are rejected.
4. Confirm reviewed CEL governance is accepted.
5. Compare source counters with the ledger baseline.

### Expected

- Exactly 11 production workflows are found: 3 tool-migratable, 4 hybrid, and
  4 governance-only.
- The completed ledger has 7 migrated and 4 classified workflows.
- Unreviewed capture/JSONPath coordination is rejected.
- Reviewed deterministic CEL is not misclassified.
- Source counters do not exceed the production-only post-retirement
  `55 / 39 / 9` baseline.

---

## Scenario 2: Historical Parity Evidence And Seven Tool Regressions

### Preconditions

- Build `orchestratord`, `orchestrator`, and `orch-mcp-tools`.
- Retain the deterministic mock fixture:

  ```bash
  target/debug/orchestrator manifest validate \
    -f fixtures/manifests/bundles/coordination-strangler-parity.yaml
  ```

### Goal

Preserve every migrated workflow's historical independent parity evidence while
proving its tool path remains sufficient after legacy removal.

### Steps

1. Run `./scripts/qa/test-coordination-strangler.sh`.
2. Confirm the complete fixture is rejected with a stable retirement code.
3. For each of `command_rules`, `qa_loop`, `plan_execute`, `full-qa`,
   `self-bootstrap`, `promotion`, and `self-evolution`, inspect its tool task ID.
4. Query the typed task's driver and authoritative coordination events.
5. Confirm dynamic-item and metric-selection scenarios reached their
   deterministic downstream steps.

### Expected

- The legacy fixture is retained but cannot enter production execution.
- Every tool task reaches `completed`.
- Tool-driven QA items converge to their governed terminal status.
- Every tool-driven coordination scenario records both provider events and
  daemon receipts.
- `generate_items` creates the expected bounded items and `record_metric`
  supplies `score=91` to `item_select`.

### Expected Data State

```sql
SELECT event_type, COUNT(*)
FROM events
WHERE task_id = '{tool_task_id}'
  AND event_type IN (
    'driver_tool_use',
    'driver_tool_result',
    'coordination_tool_started',
    'coordination_tool_completed'
  )
GROUP BY event_type;
-- Expected: matching non-zero use/result and started/completed evidence
```

---

## Scenario 3: Explicit Tool Host And Session Continuation

### Preconditions

- The parity fixture from Scenario 2 is applied.
- The fake-driver trace is retained with `KEEP_FR124_QA=1`.

### Goal

Verify the daemon attaches private provider context and starts coordination
tools only when the step explicitly requests them.

### Steps

1. Execute `parity-command-tools`.
2. Inspect `.fr124-fake-trace` in the isolated workspace.
3. Confirm `SESSION_INIT` is marked fresh.
4. Confirm `SESSION_RESUME` receives `--resume`.
5. Run the scheduler unit tests for `phase_runner::spawn::tests`.

### Expected

- The initial step does not receive a provider reference.
- Only the step with `sessionResume: true` receives `--resume`.
- Only a step with `toolHosting: stdio` starts the callback host.
- Provider session material is absent from task pipeline variables and events.

---

## Scenario 4: Self-Bootstrap Two-Cycle Survival

### Preconditions

- Use the mock fixture from Scenario 2, never
  `docs/workflow/self-bootstrap.yaml`.
- The temporary workspace is a committed minimal Cargo repository.

### Goal

Prove coordination migration preserves the two-cycle execution contract and
does not weaken the production survival envelope.

### Steps

1. Execute `parity-bootstrap-tools`.
2. Query `cycle_started` and self-test-related events for the tool task.
3. Confirm the tool path creates `docs/qa/pilot.md` through `generate_items`.
4. Inspect the production manifest for binary snapshot, `self_test`,
   `self_restart`, self-reference, rollback, and watchdog references.
5. Confirm the historical legacy terminal result remains recorded in this
   document and FR-124 evidence.

### Expected

- The tool variant reaches `completed`; the legacy manifest is rejected before
  execution.
- The tool variant emits exactly two `cycle_started` events.
- The real builtin `self_test` executes successfully.
- All four survival layers remain represented by production code, manifest,
  and watchdog evidence.

### Expected Data State

```sql
SELECT COUNT(*)
FROM events
WHERE task_id = '{bootstrap_tool_task_id}'
  AND event_type = 'cycle_started';
-- Expected: 2
```

---

## Scenario 5: Repository And CI Closure

### Preconditions

- Scenarios 1-4 pass.
- Run from a clean repository checkout for the final certification.

### Goal

Confirm the strangler gate participates in normal repository quality controls.

### Steps

1. Run `cargo fmt --all -- --check`.
2. Run `cargo test --workspace --exclude orchestrator-gui`.
3. Run
   `cargo clippy --workspace --exclude orchestrator-gui --all-targets -- -D warnings`.
4. Run `./scripts/qa-doc-lint.sh`.
5. Run `ruby scripts/qa/coordination-governance.rb --require-complete`.

### Expected

- All commands pass.
- CI contains the `coordination-strangler` job.
- The ledger has no pending workflow and every migrated workflow links this QA
  evidence.
- Legacy execution is removed; only the fixture and test-only rollback oracle
  remain.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Exact inventory and freeze ratchet | PASS | 2026-07-25 | Codex | Negative/allowed fixtures and non-increasing counters |
| 2 | Seven independent parity pairs | PASS | 2026-07-25 | Codex | All pairs completed with typed event evidence |
| 3 | Explicit tool/session boundaries | PASS | 2026-07-25 | Codex | Fresh initialization and opt-in resume traced |
| 4 | Self-bootstrap survival | PASS | 2026-07-25 | Codex | Two cycles, real self-test, four-layer static evidence |
| 5 | Repository and CI closure | PASS | 2026-07-25 | Codex | Formatting, tests, Clippy, lint, and completion gate |
