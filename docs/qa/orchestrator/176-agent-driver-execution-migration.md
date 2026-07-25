---
self_referential_safe: true
---

# Orchestrator - Agent Driver Execution Migration

**Module**: Orchestrator Runner / Scheduler / Workflow Governance  
**Scope**: production driver inventory, command-only promotion, shell behavior,
legacy runner removal, and repository closure  
**Scenarios**: 5  
**Priority**: High

## Background

FR-126 removes global runner backend selection after every production Agent
moves to a typed driver. The retained compatibility fixture is deterministic
and consumes no provider API credits.

Primary entry point:

```bash
FR126_ALLOW_DIRTY=1 ./scripts/qa/test-agent-driver-execution-migration.sh
```

Omit `FR126_ALLOW_DIRTY=1` for clean-tree certification. Set `FR126_FULL=1` to
include workspace tests and strict Clippy.

---

## Scenario 1: Exact Production Inventory And Removal Ratchet

### Preconditions

- Ruby, `jq`, and the repository toolchain are installed.
- The reviewed ledger exists at
  `config/governance/coordination-collapse-ledger.json`.

### Goal

Prove the production tree and Rust execution source contain no legacy consumer.

### Steps

1. Run:

   ```bash
   ruby scripts/qa/coordination-governance.rb \
     --test-fixtures \
     --require-complete \
     --output /tmp/fr126-execution-inventory.json
   ```

2. Inspect `.executionInventory` and
   `.sourceTouches.legacyRunnerSelection`.
3. Search production Rust source for `RunnerExecutorKind`,
   `ShellRunnerExecutor`, `StreamingAgentRunner`, session spawn variants, and
   the legacy streaming command bridge.

### Expected

- Production Agents: 20.
- Driver counts: `shell/cli=3`, `claude/cli=17`, `codex/cli=0`.
- Command-only Agents and global streaming executors: 0.
- Legacy runner selection source touches: 0.
- A negative command-only production fixture or streaming RuntimePolicy fails
  the governance command.

---

## Scenario 2: Compatibility Promotion Is Explicit And Executable

### Preconditions

- Use only
  `fixtures/manifests/bundles/agent-driver-fixture.yaml`.
- Let `scripts/qa/test-agent-driver-abstraction.sh` start its isolated daemon.

### Goal

Verify historical command-only Agents remain migratable without leaving a
legacy runtime branch.

### Steps

1. Run:

   ```bash
   FR116_ALLOW_DIRTY=1 ./scripts/qa/test-agent-driver-abstraction.sh
   ```

2. Inspect apply output for `[legacy_agent_command_deprecated]`.
3. Describe `agent/legacy-shell-pilot` in the isolated project.
4. Compare its task status, command-run exit code, and `driver_*` events with
   `explicit-shell-pilot`.

### Expected

- Apply succeeds with the stable promotion warning.
- Describe returns `provider: shell` and `transport: cli`.
- Both tasks complete with exit code `0`.
- Both paths emit normalized driver evidence and no provider session secret.

### Expected Data State

```sql
SELECT task_id, COUNT(*) AS driver_events
FROM events
WHERE task_id IN ('{promoted_task_id}', '{explicit_task_id}')
  AND event_type LIKE 'driver_%'
GROUP BY task_id;
-- Expected: one non-zero row for each task
```

---

## Scenario 3: Shell Semantics And Provider Boundaries

### Preconditions

- Rust unit tests can run locally.

### Goal

Prove migration preserves conditional commands, prompt delivery, and
interactive boundaries.

### Steps

1. Run:

   ```bash
   cargo test -p orchestrator-runner \
     shell_driver_delivers_stdin_payload_and_closes_stdin
   cargo test -p orchestrator-runner \
     command_rules_are_only_supported_by_shell_driver
   cargo test -p orchestrator-scheduler \
     tty_is_only_supported_by_typed_shell_cli_driver
   ```

2. Run existing prompt-delivery tests for arg, env, and file rendering.
3. Run runner policy, sandbox, process-group cancellation, and redaction tests.
4. Run `workflow_failing_step` and the DAG replay fallback regressions.

### Expected

- Shell stdin receives the exact prompt and EOF, so the process terminates.
- `command_rules` work with shell and fail closed for Claude/Codex.
- TTY is accepted only for `shell/cli`.
- Arg/env/file behavior and the shared security substrate remain green.
- A failed typed shell terminal fails the task, while engine-owned direct Step
  commands remain on the shared safe spawn substrate.

---

## Scenario 4: Removed Paths Fail Closed

### Preconditions

- Use the unit fixtures; do not modify a developer daemon's RuntimePolicy.

### Goal

Verify neither invalid persisted state nor an old global executor can silently
re-enter the retired path.

### Steps

1. Run:

   ```bash
   cargo test -p agent-orchestrator \
     validate_rejects_removed_streaming_executor
   cargo test -p agent-orchestrator \
     apply_legacy_command_agent_warns_and_persists_shell_driver
   ```

2. Confirm scheduler source contains
   `[legacy_agent_execution_removed]` for a missing normalized driver.
3. Confirm the typed `SessionRef` tests still pass and no database-to-global
   streaming session resolver exists.

### Expected

- `runner.executor=streaming` fails with
  `[legacy_runner_executor_removed]`.
- Compatibility ingress persists a driver before execution.
- An impossible non-driver runtime state fails with
  `[legacy_agent_execution_removed]`.
- Provider session material remains inside the typed driver boundary.

---

## Scenario 5: Aggregate Repository Closure

### Preconditions

- Scenarios 1-4 pass.
- Run from a clean checkout for release certification.

### Goal

Close FR-126 against all repository quality gates.

### Steps

1. Run `cargo fmt --all -- --check`.
2. Run `cargo test --workspace`.
3. Run `cargo clippy --workspace --all-targets -- -D warnings`.
4. Run `./scripts/coverage-governance.sh --fixture-test`.
5. Run `./scripts/qa-doc-lint.sh`.
6. Run
   `FR126_ALLOW_DIRTY=1 ./scripts/qa/test-agent-driver-execution-migration.sh`.

### Expected

- All commands pass.
- The ledger remains exact and monotonic.
- Shared runner security behavior is covered after executor deletion.
- FR-126 has design, QA, script, inventory, and rollback evidence.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Exact production inventory and removal ratchet | PASS | 2026-07-25 | Codex | 20 typed Agents, exact 3/17/0 driver split, and zero command-only/global-streaming/source-selection consumers. |
| 2 | Compatibility promotion is explicit and executable | PASS | 2026-07-25 | Codex | Isolated daemon showed warning, persisted shell driver, completed/0 parity, and normalized events for both tasks. |
| 3 | Shell semantics and provider boundaries | PASS | 2026-07-25 | Codex | Stdin EOF, command rules, TTY gate, failed-terminal propagation, engine direct-command DAG fallback, policy, and driver regressions pass. |
| 4 | Removed paths fail closed | PASS | 2026-07-25 | Codex | Stable RuntimePolicy and missing-driver retirement diagnostics are present; global session bridge is absent. |
| 5 | Aggregate repository closure | PASS | 2026-07-25 | Codex | Full workspace tests, strict Clippy, coverage governance, QA lint, governance inventory, and isolated FR-126 suite pass. |
