---
lifecycle: active
related_fr: FR-125
self_referential_safe: true
---

# Orchestrator - Legacy Coordination Decommission

**Module**: Orchestrator Scheduler / Workflow Governance  
**Scope**: consumer inventory, capture/JSONPath removal, narrow residual state,
post-retirement tool execution, and repository closure  
**Scenarios**: 5  
**Priority**: High

## Background

FR-125 removes the zero-consumer capture/JSONPath production runtime after the
FR-124 compatibility window. It does not remove governance CEL or every generic
pipeline-variable compatibility surface.

> Historical baseline: this document records the FR-125 closure state. FR-126
> subsequently migrated the four command-only Agents and removed
> `ShellRunnerExecutor`; use
> `docs/qa/orchestrator/176-agent-driver-execution-migration.md` for the current
> execution inventory and rerunnable assertions.

Primary entry point:

```bash
FR125_ALLOW_DIRTY=1 ./scripts/qa/test-legacy-coordination-decommission.sh
```

Omit `FR125_ALLOW_DIRTY=1` for clean-tree certification. Set `FR125_FULL=1` to
include workspace tests and strict Clippy in the same run.

---

## Scenario 1: Reproducible Consumer Inventory And Ratchet

### Preconditions

- Ruby, `jq`, and the repository toolchain are installed.
- The reviewed ledger exists at
  `config/governance/coordination-collapse-ledger.json`.

### Goal

Prove each retirement decision is backed by an exact production inventory.

### Steps

1. Run:

   ```bash
   ruby scripts/qa/coordination-governance.rb \
     --test-fixtures \
     --require-complete \
     --output /tmp/fr125-consumer-inventory.json
   ```

2. Inspect `.productionConsumers`, `.executionInventory`, and `.sourceTouches`.
3. Confirm negative fixtures for capture, JSONPath post-action, generic safety
   variables, and new `store_inputs` fail the allowance check.

### Expected

- Capture/JSONPath consumers: 0.
- Coordination CEL consumers: 0.
- Generic pipeline-variable manifest consumers: exactly 2 reviewed
  `store_inputs`.
- Command-only Agent manifests: exactly 0. FR-125 froze the count at 4 and
  FR-126 migrated every one of them.
- Production-only capture/JSONPath source touches equal 53, down from 143.
- Any unreviewed consumer fails the command, and any counter that moves in
  either direction fails it: FR-128 made the source ratchets exact, because a
  count below its baseline leaves the ledger asserting debt the repository no
  longer carries while the gate reports green.

---

## Scenario 2: Legacy Manifests Fail Closed While Rollback Evidence Remains

### Preconditions

- Use only the deterministic mock fixture:
  `fixtures/manifests/bundles/coordination-strangler-parity.yaml`.
- Let `scripts/qa/test-coordination-strangler.sh` start its isolated daemon;
  do not validate against a developer's active daemon.

### Goal

Verify removed legacy behavior cannot execute in production while the historical
fixture and test oracle remain usable for rollback.

### Steps

1. Run the isolated strangler suite:

   ```bash
   FR125_ALLOW_DIRTY=1 ./scripts/qa/test-coordination-strangler.sh
   ```

2. Confirm the suite's full-fixture validation fails with the stable retirement
   diagnostic before it derives and applies the tool-only fixture.
3. Run:

   ```bash
   cargo test -p agent-orchestrator \
     validate_workflow_config_rejects_json_path_on_exit_code_capture
   ```

4. Run the scheduler `apply_captures` tests and confirm they are declared only
   in the test module.
5. Search production scheduler sources for `apply_captures`,
   `pending_generate_items`, and `extract_json_array`.

### Expected

- Manifest validation fails with `[legacy_coordination_removed]` or
  `[legacy_json_path_removed]`.
- The historical fixture remains in the repository.
- The compatibility oracle passes but no production scheduler source defines or
  invokes the legacy extraction path.

---

## Scenario 3: Narrow Residual State Preserves Behavior

### Preconditions

- Rust unit tests can run locally.

### Goal

Prove old persisted rows migrate safely and the four residual fields do not
re-enter generic `vars`.

### Steps

1. Run:

   ```bash
   cargo test -p orchestrator-config \
     legacy_preserved_keys_migrate_out_of_generic_vars
   cargo test -p orchestrator-config \
     explicit_preserved_goal_wins_and_legacy_duplicate_is_removed
   cargo test -p orchestrator-collab \
     test_preserved_goal_is_rendered_without_generic_variable
   cargo test -p orchestrator-scheduler \
     narrow_preserved_channels_survive_accumulator_merge_without_generic_keys
   cargo test -p orchestrator-scheduler \
     load_task_runtime_context_normalizes_fields
   ```

2. Inspect the serialized test value and runtime context assertions.
3. Run the existing prehook/finalize sandbox-denial CEL tests.

### Expected

- `goal` and all three sandbox-denial fields round-trip in
  `PreservedExecutionChannels`.
- Old generic keys are removed on normalization.
- An explicit narrow `goal` wins over a stale duplicate.
- Template rendering and CEL behavior remain equivalent.
- No general reducer or author-defined typed-state API is introduced.

---

## Scenario 4: Seven Post-retirement Tool Workflows And Survival Boundaries

### Preconditions

- Use the mock fixture from Scenario 2, never production workflows under
  `docs/workflow/`.
- Run:

  ```bash
  FR125_ALLOW_DIRTY=1 ./scripts/qa/test-coordination-strangler.sh
  ```

### Goal

Verify the removed path is unnecessary for every migrated production workflow
model and that self-bootstrap survival behavior remains intact.

### Steps

1. Let the script derive a tool-only manifest from the retained parity fixture.
2. Execute the seven `*-tools` workflows on the isolated daemon.
3. Inspect typed driver/tool events for each task.
4. Inspect the self-bootstrap task's cycle and self-test evidence.

### Expected

- The full legacy fixture is rejected before execution.
- All seven tool workflows reach `completed`.
- Typed coordination event evidence is present.
- Self-bootstrap executes two cycles and retains self-test, snapshot, restart,
  self-reference, and watchdog evidence.

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
-- Expected: matching non-zero typed request/result/receipt evidence
```

---

## Scenario 5: Boundary And Repository Closure

### Preconditions

- Scenarios 1-4 pass.
- Run from a clean checkout for release certification.

### Goal

Close the feature against repository-wide quality gates.

### Steps

1. Run `cargo fmt --all -- --check`.
2. Run `cargo test --workspace`.
3. Run `cargo clippy --workspace --all-targets -- -D warnings`.
4. Run `./scripts/coverage-governance.sh --fixture-test`.
5. Run `./scripts/qa-doc-lint.sh`.
6. Run
   `ruby scripts/qa/coordination-governance.rb --require-complete`.

### Expected

- All commands pass.
- Boundary negative fixtures reject weakened coverage.
- The ledger remains exact: a count that moves in either direction fails.
- `ShellRunnerExecutor` remains frozen with four consumers assigned to FR-126.
- The two generic-variable consumers remain visible as an accepted blocker, not
  an unverified removal claim.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Reproducible consumer inventory and ratchet | PASS | 2026-07-25 | Codex | Governance inventory reproduced 0 capture/JSONPath consumers, 0 coordination CEL consumers, 2 reviewed generic-variable consumers, 4 command-only Agents, and source baseline 55/39/9. Superseded 2026-07-25 by FR-126 (command-only Agents to 0) and FR-128 (baseline 53/30/9, compared exactly); re-running this scenario today reports those figures. |
| 2 | Legacy manifests fail closed with rollback evidence | PASS | 2026-07-25 | Codex | Isolated validation returned the stable retirement diagnostic; the historical fixture and test-only compatibility oracle remain. |
| 3 | Narrow residual state preserves behavior | PASS | 2026-07-25 | Codex | Migration, precedence, rendering, accumulator, runtime normalization, and governance CEL regressions passed. |
| 4 | Seven post-retirement tool workflows and survival boundaries | PASS | 2026-07-25 | Codex | Strangler suite passed 20/20 checks across all seven tool-only workflows, including self-bootstrap survival evidence. |
| 5 | Boundary and repository closure | PASS | 2026-07-25 | Codex | Full workspace tests, strict Clippy, formatting, coverage governance, QA lint, and the FR-125 aggregate suite passed. |
