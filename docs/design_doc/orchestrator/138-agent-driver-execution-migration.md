# Orchestrator Runner - Agent Driver Execution Migration

**Module**: Orchestrator Runner / Scheduler / Workflow Governance
**Status**: Released
**Related Plan**: FR-126 command-only Agent migration, legacy runner retirement, and strict evidence remediation
**Related QA**: `docs/qa/orchestrator/176-agent-driver-execution-migration.md`
**Created**: 2026-07-25
**Last Updated**: 2026-07-25

## Background

FR-116 introduced provider-neutral `shell/cli`, `claude/cli`, and `codex/cli` drivers, but a global `RunnerExecutorKind` still selected a legacy shell or Claude streaming backend. Four production Agent manifests also omitted `spec.driver`.

The initial FR-126 implementation migrated every production Agent, normalized historical command-only manifests, and removed global executor selection. A strict closure audit later found that its aggregate inventory and synthetic shell pilots did not prove the original per-production-object acceptance criteria. The FR was reopened to add exact inventory, offline parity, explicit compatibility/rollback boundaries, and a mandatory release gate.

The evidence remediation also exposed a runtime defect: typed `driver_tool_use` events were recorded, but item-level typed signals were not promoted to the task convergence context. A `mark_done` workflow therefore completed only after reaching its cycle ceiling. The scheduler now promotes typed signals before loop continuation evaluation.

## Goals

- Keep zero command-only Agent and global streaming consumers in production.
- Make all 20 production Agent identities and driver targets machine-reviewable.
- Prove the four migrated production contracts offline without provider credentials.
- Preserve shell command, output, sandbox, cancellation, redaction, and TTY behavior.
- Prove typed Claude `mark_done` events reach task-level convergence in one cycle.
- Separate production admission rejection from historical runtime compatibility.
- Retain executable compatibility-window and rollback evidence.
- Make every original repository gate mandatory for release certification.

## Non-goals

- Restore a legacy runner or global streaming executor.
- Execute production AI workflows during QA or consume provider API credits.
- Infer Claude or Codex from legacy command text.
- Remove runner policy, sandbox profiles, resource limits, process groups, environment filtering, output capture, or redaction.
- Remove engine-owned direct Step commands.
- Add a provider or enable SDK transport.

## Scope

- In scope: production Agent manifests, governance inventory, compatibility normalization, typed signal projection, loop convergence, offline parity fixtures, runner removal rollback, and aggregate QA.
- Out of scope: GUI behavior, database schema changes, live provider certification, and generic coordination-state redesign.

## Interfaces And Data

### Production execution inventory

`coordination-governance.rb` emits `.executionInventory.agents`. Every entry contains:

- `file`
- `name`
- `workflows`
- `driverId`
- `classification`: `shell-script` or `ai-provider`
- `migrationTarget`
- `manifestFingerprint`

The ledger stores the reviewed identity subset and exact fingerprints. Aggregate counts alone cannot hide an Agent swap or provider change.

### Agent compatibility

A manifest with non-empty `spec.command` and no `spec.driver` remains accepted at the historical runtime compatibility ingress. Apply returns:

```text
[legacy_agent_command_deprecated] ... applying it promotes the Agent to driver shell/cli
```

Normalization persists `provider: shell` and `transport: cli`; execution never sees a command-only Agent.

This differs from production admission governance: raw command-only Agents under reviewed production roots are rejected by the inventory ratchet. The compatibility path protects stored and third-party manifests; it is not permission to add new production legacy consumers.

`command_rules` remain supported only by `shell/cli`. Claude and Codex reject them because vendor drivers construct commands from typed options.

### RuntimePolicy compatibility

`runner.executor` remains a parse-only public schema field. `shell` is accepted for round-trip compatibility. `streaming` fails with `[legacy_runner_executor_removed]`; provider execution belongs to the Agent.

### Typed convergence state

Driver validation creates typed `ToolCall`, `driver_tool_result`, and `driver_terminal` artifacts. `stream_signal_vars` now accepts either a legacy `stream_run_summary` or typed `driver_terminal` as a structured terminal marker. The scheduler promotes item-level:

- `tools_called`
- `tool_error_count`
- self-test state
- governed metrics

into task-level `PipelineVariables.signals` before guard and continuation evaluation. This keeps typed signals outside the generic author-defined state map while making CEL convergence deterministic.

No SQLite schema change is required. Provider `SessionRef` material remains private to the driver boundary.

## Key Design

1. Config normalization deterministically promotes command-only Agents to `AgentDriverConfig::shell_cli()`.
2. Production governance rejects raw command-only manifests and fingerprints every reviewed Agent.
3. Typed drivers own Agent process start, event consumption, terminal validation, and session privacy.
4. A missing driver at scheduler execution fails with `[legacy_agent_execution_removed]`.
5. Engine-owned direct Step commands continue to use the shared safe spawn substrate.
6. `shell/cli` retains arg, env, file, stdin, EOF, and TTY behavior.
7. All CLI drivers share policy, sandbox, rlimit, process-group, environment, output-capture, and redaction infrastructure.
8. Typed tool signals are promoted from item accumulators into task convergence state.
9. `RunnerExecutorKind`, `ShellRunnerExecutor`, `StreamingAgentRunner`, and the global provider-session bridge remain deleted.
10. The default FR-126 aggregate is the release gate; `FR126_FAST=1` is explicitly non-certifying.

## Offline Production Parity

The mock-only bundle `fixtures/manifests/bundles/agent-driver-production-parity.yaml` binds to four production objects:

| Production contract | Agent | Target | Offline proof |
|---|---|---|---|
| hello-world | `echo-agent` | `shell/cli` | compatibility/typed terminal, exit, canonical stdout hash, driver events |
| scheduled-scan | `scan-agent` | `shell/cli` | compatibility/typed terminal, exit, canonical stdout hash, driver events |
| fr-watch | `fr-governance-agent` | `shell/cli` | compatibility/typed terminal, exit, canonical stdout hash, driver events |
| streaming-mark-done | `streamer` | `claude/cli` | fake Claude terminal, typed tool events, one-cycle convergence, session privacy |

The harness compares fixture commands and driver options with production manifests before execution. `fixtures/driver/legacy-agent-execution-baseline.json` anchors observable legacy contracts to commit `4bac6915`.

## Compatibility And Rollback

The legacy runtime selection window is bounded by:

- opening: `e06a404b` — typed Agent driver implementation
- closing: `c0d58e6e` — legacy runner removal

The ledger records full commit identities, scope, fixture, and rollback evidence. QA verifies commit ancestry.

Rollback evidence is executable:

```bash
git diff c0d58e6e^ c0d58e6e -- \
  core/src/resource/runtime_policy.rs \
  crates/orchestrator-config/src/config/runner.rs \
  crates/orchestrator-runner/src \
  crates/orchestrator-scheduler/src/scheduler/phase_runner |
  git apply -R --check
```

This proves the source removal patch remains mechanically reversible without applying it to the active worktree. The retained command-only fixture proves forward compatibility after removal.

## Alternatives And Tradeoffs

- Reject every historical command-only manifest immediately: simpler, but breaks stored and third-party configuration.
- Treat runtime promotion as the production ratchet: incorrect because it conflates compatibility with reviewed admission.
- Execute real production Claude workflows: strongest live-provider signal, but costly, credential-dependent, and flaky.
- Chosen approach: strict production fingerprints plus a fake-provider, mock-only parity bundle and recorded legacy contracts.

## Risks And Mitigations

- Risk: parity fixture drifts away from production.
  - Mitigation: compare shell commands and Claude driver options before any task starts; ledger fingerprints also fail closed.
- Risk: typed events persist but do not affect loop convergence.
  - Mitigation: promote typed item signals into task state and assert one-cycle `mark_done` convergence.
- Risk: shell stdin commands hang after receiving a prompt.
  - Mitigation: close stdin after the initial payload and retain the EOF test.
- Risk: removing the executor weakens sandbox or cancellation.
  - Mitigation: retain a shared spawn function and mandatory substrate tests.
- Risk: rollback documentation becomes stale.
  - Mitigation: run the reverse-patch applicability check during every aggregate certification.
- Risk: a fast local run is mistaken for release certification.
  - Mitigation: full gates are default; only explicit `FR126_FAST=1` skips them and the script labels that mode non-certifying.

## Observability

- Logs: stable compatibility and retirement reason codes identify operator action.
- Events: `driver_started`, `driver_tool_use`, `driver_tool_result`, `driver_usage`, and `driver_finished` are normalized and session-safe.
- Inventory: exact per-Agent identity, driver, Workflow association, and fingerprint are emitted as JSON.
- QA evidence: terminal states, output hashes, typed event counts, cycle count, and session-persistence result are written by the isolated harness.

## Operations / Release

- Config: no new environment variables.
- Compatibility: command-only ingress warns and promotes; global streaming selection is retired.
- Migration: re-apply historical Agent manifests to persist the explicit shell driver.
- Release certification:

  ```bash
  ./scripts/qa/test-agent-driver-execution-migration.sh
  ```

- Local iteration only:

  ```bash
  FR126_FAST=1 FR126_ALLOW_DIRTY=1 \
    ./scripts/qa/test-agent-driver-execution-migration.sh
  ```

- Rollback: reverse the removal commit after verifying the reverse patch; use the retained baseline and compatibility bundle to confirm observable contracts.

## Test Plan

- Unit: typed artifact signals, item-to-task promotion, shell factory, command rules, stdin EOF, RuntimePolicy rejection, TTY, failed terminal, sandbox, cancellation, and redaction.
- Integration: three production shell compatibility/typed pairs plus typed fake-Claude mark-done convergence in an isolated daemon.
- Governance: exact production inventory, negative admission fixtures, manifest fingerprints, compatibility ancestry, and zero legacy source symbols.
- Repository: coordination strangler, format, workspace tests, strict Clippy, coverage governance, and QA lint.

## QA Docs

- `docs/qa/orchestrator/176-agent-driver-execution-migration.md`
- Compatibility predecessor: `docs/qa/orchestrator/164-agent-driver-abstraction.md`

## Acceptance Criteria

- All 20 production Agents are individually fingerprinted typed drivers.
- Three production shell contracts retain terminal, exit, output, and normalized event behavior.
- Streaming mark-done retains terminal, tool, convergence, and session-privacy behavior through `claude/cli`.
- Production admission rejects command-only Agents while historical Apply warns and promotes.
- Legacy runner selection remains absent with an ordered compatibility interval and executable rollback check.
- `command_rules`, prompt delivery, TTY, sandbox, cancellation, events, and session privacy remain governed.
- The execution ledger is `removed` and source baselines remain monotonic.
- The default aggregate and every repository gate pass.
