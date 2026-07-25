# Agent Driver Execution Migration

**Module**: Orchestrator Runner / Scheduler / Workflow Governance  
**Status**: Released  
**Related Plan**: FR-126 command-only Agent migration and legacy runner retirement  
**Related QA**: `docs/qa/orchestrator/176-agent-driver-execution-migration.md`  
**Created**: 2026-07-25  
**Last Updated**: 2026-07-25

## Background

FR-116 introduced provider-neutral `shell/cli`, `claude/cli`, and `codex/cli`
drivers, but a global `RunnerExecutorKind` still selected a legacy shell or
Claude streaming backend. Four production Agent manifests also omitted
`spec.driver`. That left two execution models and made session, prompt, and
policy behavior harder to govern.

FR-126 completes the migration. Every production Agent now declares a typed
driver, command-only compatibility is normalized at the resource boundary, and
the scheduler has one Agent execution model.

## Goals

- Reach zero command-only Agent and global streaming consumers in production.
- Preserve shell command, `command_rules`, prompt-delivery, TTY, sandbox, and
  cancellation semantics through `shell/cli`.
- Remove legacy executor selection without removing the shared safe spawn
  substrate.
- Provide stable compatibility warnings and fail-closed retirement errors.
- Make execution inventory and source removal machine-verifiable.

## Non-goals

- Removing engine-owned direct Step commands.
- Removing runner policy, sandbox profiles, resource limits, process groups,
  environment filtering, output capture, or redaction.
- Adding a provider or enabling SDK transport.
- Redesigning coordination state or provider event contracts.

## Scope

- In scope: production Agent manifests, config normalization and validation,
  runner/scheduler execution selection, shell prompt delivery, TTY compatibility,
  governance inventory, and QA evidence.
- Out of scope: GUI behavior, database schema changes, live provider
  certification, and generic pipeline-variable retirement.

## Interfaces And Data

### Agent compatibility

A manifest with non-empty `spec.command` and no `spec.driver` remains accepted
during the compatibility window. Apply returns:

```text
[legacy_agent_command_deprecated] ... applying it promotes the Agent to driver shell/cli
```

Normalization persists `provider: shell` and `transport: cli`; execution never
re-enters a non-driver Agent branch.

`command_rules` remain supported only by `shell/cli`. Claude and Codex reject
them because those providers construct commands from typed options.

### RuntimePolicy compatibility

`runner.executor` remains in the public manifest schema as a parse-only field.
`shell` is accepted for round-trip compatibility. `streaming` fails with
`[legacy_runner_executor_removed]`; provider execution must be configured on
the Agent.

### Data model

No SQLite schema changes are required. Normalized driver events and command-run
records keep their existing shape. The old database lookup that translated a
task resume token into a global streaming-runner session was removed; typed
drivers retain their private `SessionRef` path.

## Key Design

1. Config normalization deterministically promotes command-only Agents to
   `AgentDriverConfig::shell_cli()`.
2. Production governance still rejects raw command-only manifests under
   `docs/workflow` and `config`, preventing new compatibility consumers.
3. Typed drivers own Agent process start and event consumption. A missing
   driver at scheduler execution fails with
   `[legacy_agent_execution_removed]`.
   Engine-owned direct Step commands (`agent_id=builtin`) are not Agents and
   continue to use the shared safe spawn substrate.
4. `shell/cli` supports arg, env, file, and stdin prompt delivery. Initial stdin
   is written and closed so EOF-dependent commands terminate.
5. `shell/cli` alone may use the existing TTY session adapter; vendor drivers
   fail closed because their structured sessions do not implement that
   interactive contract.
6. Engine-owned command spawning and all CLI drivers retain the same shared
   policy/sandbox/rlimit/process-group/env/redaction function.
7. `RunnerExecutorKind`, `ShellRunnerExecutor`, `StreamingAgentRunner`, and the
   global provider-session bridge are deleted.

## Alternatives And Tradeoffs

- Reject every command-only manifest immediately: simpler, but breaks stored
  and third-party configuration without a migration path.
- Guess Claude/Codex from command text: convenient but ambiguous, unsafe, and
  provider-version dependent.
- Keep the global executor indefinitely: minimizes code churn but preserves two
  competing session and execution models.
- Chosen approach: deterministic shell promotion at ingress, strict production
  inventory, and immediate internal convergence on typed drivers.

## Risks And Mitigations

- Risk: shell stdin commands hang after receiving a prompt.  
  Mitigation: close the driver stdin handle after the initial shell payload and
  test an EOF-dependent `cat` command.
- Risk: TTY workflows regress after every Agent gains a driver.  
  Mitigation: retain the session adapter only for typed `shell/cli` and test the
  provider/transport gate.
- Risk: deleting the executor also deletes sandbox enforcement.  
  Mitigation: retain one shared spawn function and assert legacy selection
  symbols are zero while runner policy tests remain green.
- Risk: historical manifests silently change provider.  
  Mitigation: always promote to shell, emit a stable warning, and persist the
  explicit result.
- Risk: a failed driver terminal is recorded but the item converges.  
  Mitigation: map every non-zero/failed terminal to hard validation failure and
  keep the failing-workflow integration fixture deterministic.

## Observability

- Logs/CLI: stable compatibility and retirement reason codes identify the
  required operator action.
- Events: promoted and explicit shell runs both emit normalized `driver_*`
  events and a `step_spawned.driver` identifier.
- Governance: the JSON report records 20 production Agents, exact driver
  counts, zero command-only Agents, zero global streaming executors, and zero
  legacy runner selection symbols.
- Default recommendation: alert on compatibility warning frequency before a
  future schema version removes command-only ingress.

## Operations / Release

- Config: no new environment variables.
- Migration: re-apply old Agent manifests to persist the explicit shell driver.
- Compatibility: `runner.executor: shell` remains a no-op schema field;
  `streaming` is retired.
- Rollback: revert the runner-removal commit and use
  `fixtures/manifests/bundles/agent-driver-fixture.yaml` plus the retained
  compatibility test to compare terminal state and exit codes.
- Release gate:
  `FR126_ALLOW_DIRTY=1 ./scripts/qa/test-agent-driver-execution-migration.sh`.

## Test Plan

- Unit: shell factory, normalization idempotency, warning persistence,
  provider/command-rule validation, stdin EOF, RuntimePolicy rejection, and TTY
  capability.
- Integration: isolated daemon runs command-only and explicit shell fixtures
  through normalized driver events.
- Governance: exact production inventory, negative fixtures, and zero legacy
  source-symbol baseline.
- Repository: full workspace tests, strict Clippy, formatting, coverage
  governance, and QA documentation lint.

## QA Docs

- `docs/qa/orchestrator/176-agent-driver-execution-migration.md`
- Compatibility predecessor:
  `docs/qa/orchestrator/164-agent-driver-abstraction.md`

## Acceptance Criteria

- All 20 production Agents have explicit typed drivers.
- No global streaming RuntimePolicy or legacy runner selection remains.
- Command-only apply warns and persists `shell/cli`.
- `command_rules`, prompt delivery, TTY, sandbox, cancellation, events, and
  session privacy retain their governed behavior.
- The execution ledger is `removed` with compatibility and rollback evidence.
- Aggregate QA and repository gates pass.
