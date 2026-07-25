---
self_referential_safe: true
---

# Orchestrator - Agent Driver Execution Migration

**Module**: Orchestrator Runner / Scheduler / Workflow Governance
**Scope**: exact production inventory, offline production-contract parity, typed convergence, linked showcase semantics, legacy runner rollback, and release closure
**Scenarios**: 5
**Priority**: High

## Background

FR-126 removes global runner backend selection after every production Agent moves to a typed driver. A strict closure audit reopened the FR because aggregate counts and synthetic shell pilots did not prove parity for the four migrated production contracts. A later audit found that EN/ZH guides, architecture, an authoring skill, and released design status still described retired execution behavior because `docs/guide` was outside the closure lint. A fourth audit followed the corrected guide links and found that the operational mark-done showcase still advertised the removed executor because `docs/showcases` was outside the deterministic target set.

All executable workflows in this document use the deterministic mock bundle below. They do not apply `docs/workflow` or invoke a real provider:

```bash
orchestrator apply \
  --project qa-agent-driver-production-parity \
  -f fixtures/manifests/bundles/agent-driver-production-parity.yaml
```

Primary release entry point:

```bash
./scripts/qa/test-agent-driver-execution-migration.sh
```

`FR126_FAST=1` is for local iteration only and is not release certification.

---

## Scenario 1: Exact Per-Agent Production Inventory

### Preconditions

- Ruby and `jq` are installed.
- The reviewed ledger exists at `config/governance/coordination-collapse-ledger.json`.

### Goal

Prove every production Agent has an individually reviewed typed-driver identity rather than relying on aggregate counts.

### Steps

1. Run:

   ```bash
   ruby scripts/qa/coordination-governance.rb \
     --test-fixtures \
     --require-complete \
     --output /tmp/fr126-execution-inventory.json
   ```

2. Inspect `.executionInventory.agents`.
3. Verify every entry includes `file`, `name`, `workflows`, `classification`, `migrationTarget`, and a 64-character `manifestFingerprint`.
4. Inspect `.executionInventory.legacyCommandOnlyAgents`, `.globalStreamingExecutors`, and `.sourceTouches.legacyRunnerSelection`.
5. Inspect `new-command-only-agent-is-rejected` in `scripts/qa/fixtures/coordination-governance-cases.json`.

### Expected

- Exactly 20 individually fingerprinted Agents are listed.
- Classifications are `shell-script=3` and `ai-provider=17`.
- Driver totals are `shell/cli=3`, `claude/cli=17`, `codex/cli=0`.
- Every Agent is associated with at least one Workflow in the same production file.
- Command-only Agents, global streaming executors, and legacy runner source touches are all zero.
- A production Agent identity, driver, or governed spec change fails against the reviewed ledger.
- The command-only fixture declares `evaluationLayer=production-manifest-governance` and separately records runtime acceptance, `[legacy_agent_command_deprecated]`, and persisted `shell/cli`.

---

## Scenario 2: Three Production Shell Contracts Preserve Observable Behavior

### Preconditions

- Apply only the mock fixture:

  ```bash
  orchestrator apply \
    --project qa-agent-driver-production-parity \
    -f fixtures/manifests/bundles/agent-driver-production-parity.yaml
  ```

- Do not execute `docs/workflow/hello-world.yaml`, `scheduled-scan.yaml`, or `fr-watch.yaml`.

### Goal

Bind the deterministic production shell commands to compatibility and explicit typed-driver tasks, then compare their observable contracts.

### Steps

1. Run:

   ```bash
   FR126_ALLOW_DIRTY=1 \
     ./scripts/qa/test-agent-driver-production-parity.sh
   ```

2. Inspect the evidence entries for `hello-world`, `scheduled-scan`, and `fr-watch`.
3. Confirm the fixture binding check compares both mock commands with the corresponding production Agent command.
4. Confirm each legacy fixture emits `[legacy_agent_command_deprecated]` and persists as `shell/cli`.

### Expected

- All three compatibility/typed task pairs complete with exit code `0`.
- Each pair has exactly matching canonical stdout SHA-256.
- Each SHA-256 matches the recorded legacy baseline.
- Both sides emit normalized `driver_*` evidence.
- Shared sandbox, cancellation, and redaction substrate tests pass.

### Expected Data State

```sql
SELECT task_items.task_id, command_runs.exit_code
FROM command_runs
JOIN task_items ON task_items.id = command_runs.task_item_id
WHERE task_items.task_id IN ({six_shell_parity_task_ids});
-- Expected: six rows with exit_code = 0

SELECT task_id, COUNT(*) AS driver_events
FROM events
WHERE task_id IN ({six_shell_parity_task_ids})
  AND event_type LIKE 'driver_%'
GROUP BY task_id;
-- Expected: six rows, each count > 0
```

---

## Scenario 3: Streaming Mark-Done Typed Claude Matches The Recorded Contract

### Preconditions

- Apply only:

  ```bash
  orchestrator apply \
    --project qa-agent-driver-production-parity \
    -f fixtures/manifests/bundles/agent-driver-production-parity.yaml
  ```

- `scripts/qa/fixtures/fake-claude-agent-driver-migration.sh` is installed as `claude` inside the isolated QA PATH.
- The baseline is `fixtures/driver/legacy-agent-execution-baseline.json`.

### Goal

Prove the production `streamer` driver retains terminal, tool, convergence, and session-privacy behavior without provider credentials or API cost.

### Steps

1. Run `./scripts/qa/test-agent-driver-production-parity.sh`.
2. Inspect the `streaming-mark-done` evidence object.
3. Query `driver_tool_use`, `driver_tool_result`, `cycle_started`, and the final command-run exit code.
4. Search the isolated database for the fake provider session identifier.

### Expected

- The fixture driver options exactly match the production `streamer` Agent.
- The task completes with exit code `0`.
- `mcp__orch__mark_done` has one typed use and one successful typed result.
- Item-level typed signals are promoted into the task convergence context.
- The workflow terminates after one cycle, matching the recorded legacy contract.
- The provider session identifier is absent from persisted database evidence.

### Expected Data State

```sql
SELECT event_type, COUNT(*)
FROM events
WHERE task_id = '{streaming_task_id}'
  AND event_type IN ('cycle_started', 'driver_tool_use', 'driver_tool_result')
GROUP BY event_type;
-- Expected: one row for each event type, count = 1
```

---

## Scenario 4: Admission, Compatibility, Removal, And Rollback Boundaries

### Preconditions

- Use the unit fixtures and repository history; do not mutate a developer daemon.

### Goal

Verify production admission and historical compatibility are distinct, while removed execution paths remain fail-closed and mechanically reversible.

### Steps

1. Run the governance negative fixtures.
2. Run the documentation alignment negative fixture:

   ```bash
   ./scripts/qa/test-agent-driver-documentation-alignment.sh --fixture-test
   ```
3. Run:

   ```bash
   cargo test -p agent-orchestrator \
     apply_legacy_command_agent_warns_and_persists_shell_driver
   cargo test -p agent-orchestrator \
     validate_rejects_removed_streaming_executor
   ```

4. Verify `openedByCommit` is an ancestor of `closedByCommit` in the ledger compatibility window.
5. Reverse-check the runner-removal source patch:

   ```bash
   git diff c0d58e6e^ c0d58e6e -- \
     core/src/resource/runtime_policy.rs \
     crates/orchestrator-config/src/config/runner.rs \
     crates/orchestrator-runner/src \
     crates/orchestrator-scheduler/src/scheduler/phase_runner |
     git apply -R --check
   ```

### Expected

- Production governance rejects raw command-only Agent manifests.
- Historical runtime Apply warns and persists `shell/cli`; it never leaves a command-only runtime consumer.
- The fixture and Ruby helper name the production-admission layer rather than presenting it as daemon Apply behavior.
- `runner.executor=streaming` and scheduler missing-driver state fail with stable retirement diagnostics.
- EN/ZH guides bind structured signals to typed driver artifacts and do not advertise a streaming executor; architecture, authoring skill, DD-101, DD-102, DD-103, and DD-127 agree.
- Both CEL guides reference the existing mark-done showcase; every showcase is
  scanned for retired semantics, and the linked page names `claude/cli`,
  `driver_tool_use`, `driver_tool_result`, and `driver_terminal`.
- The stale-documentation negative fixture is detected.
- The compatibility commit interval is ordered and reachable.
- The runner-removal source patch remains reverse-applicable.
- `command_rules` remain supported only by `shell/cli`.

---

## Scenario 5: Mandatory Aggregate Release And Guide Alignment

### Preconditions

- Scenarios 1-4 pass.
- Run from a clean checkout.
- Do not set `FR126_FAST=1`.

### Goal

Close FR-126 only when every original repository and coordination gate is part of one default command.

### Steps

1. Run:

   ```bash
   ./scripts/qa/test-agent-driver-execution-migration.sh
   ```

2. Confirm the command runs the production parity harness, documentation alignment harness, and coordination strangler.
3. Confirm it runs format, workspace tests, strict Clippy, coverage governance, and QA documentation lint.
4. Run `./scripts/qa-doc-lint.sh` directly and confirm it invokes the Agent driver guide alignment check.

### Expected

- Inventory and source-retirement ratchets pass.
- Agent driver documentation alignment reports 10 passes and zero failures.
- Production parity reports 11 passes and zero failures.
- Coordination strangler reports 20 passes and zero failures.
- `cargo fmt --all -- --check`, `cargo test --workspace`, strict Clippy, coverage governance, and QA lint—including `docs/guide` and `docs/showcases` driver semantics—all pass.
- Fast-mode output is never accepted as release certification.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Exact per-Agent production inventory | ☐ PENDING | — | — | Reopened audit requires a fresh clean-tree aggregate. |
| 2 | Three production shell contracts preserve observable behavior | ☐ PENDING | — | — | Reopened audit requires a fresh clean-tree aggregate. |
| 3 | Streaming mark-done typed Claude matches the recorded contract | ☐ PENDING | — | — | Reopened audit requires a fresh clean-tree aggregate. |
| 4 | Admission, compatibility, removal, rollback, and documentation boundaries | ☐ PENDING | — | — | Expanded to linked showcase semantics and directory-wide scanning. |
| 5 | Mandatory aggregate release and guide alignment | ☐ PENDING | — | — | Awaiting clean-tree release certification. |
