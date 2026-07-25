---
lifecycle: active
related_fr: FR-125
---

# Legacy Coordination Decommission

**Module**: Orchestrator Scheduler / Workflow Governance  
**Status**: Released  
**Related Plan**: FR-125 deprecate-to-remove execution for legacy coordination channels  
**Related QA**: `docs/qa/orchestrator/175-legacy-coordination-decommission.md`  
**Created**: 2026-07-25  
**Last Updated**: 2026-07-25

## Background

FR-124 moved all seven non-governance production workflows onto daemon-owned
tools and froze new coordination consumers. Its legacy capture, JSONPath, and
generic pipeline-variable machinery remained executable during an independently
green compatibility window.

FR-125 advances that freeze to selective removal. It removes only channels with
zero production consumers, preserves deterministic governance, and records
instead of hiding the remaining compatibility dependencies.

FR-126 subsequently closed the transferred command-only Agent dependency and
removed the legacy runner selection seam; see
`docs/design_doc/orchestrator/138-agent-driver-execution-migration.md`.

## Goals

- Produce a machine-readable inventory for capture/JSONPath,
  `PipelineVariables`, coordination CEL, and command-only Agents.
- Remove the production capture/JSONPath extraction and consumption paths.
- Move `goal` and the three sandbox-denial fields out of the generic string map.
- Preserve governance CEL, builtins, public manifest compatibility, and rollback
  evidence.
- Keep `ShellRunnerExecutor` frozen and transfer its four remaining production
  Agent consumers to FR-126.

## Non-goals

- Removing `cel-interpreter` or deterministic CEL governance gates.
- Removing every `PipelineVariables` compatibility use in this change.
- Introducing a general typed workflow state, reducer, or graph-state layer.
- Removing `ShellRunnerExecutor`.
- Changing the database schema or the public task/session API.

## Scope

- In scope: scheduler-owned execution state, workflow normalization and
  validation, production consumer governance, legacy fixture handling, and
  repository QA.
- Out of scope: command-only Agent driver migration, store binding redesign,
  arbitrary initial/item variable removal, and GUI behavior.

## Interfaces And Data

### Manifest validation

- Non-empty `behavior.captures` fails with
  `[legacy_coordination_removed]`.
- JSONPath-backed `spawn_tasks` and `generate_items` post-actions fail with
  `[legacy_json_path_removed]`.
- The schema types remain deserializable so the CLI can return a stable
  retirement diagnostic instead of an unknown-field error.

### Durable task state

`PipelineVariables` retains its serialized envelope for compatibility, but now
contains two fixed scheduler-owned structures:

- `PreservedExecutionChannels`: `goal`, `last_sandbox_denied`,
  `sandbox_denied_count`, and `last_sandbox_denial_reason`.
- `ExecutionSignals`: fixed self-test, structured-driver tool, and numeric metric
  observations used by deterministic scheduler logic.

On load, the four old keys are removed from `vars` and migrated into
`PreservedExecutionChannels`. New serialization never duplicates them in the
generic map. This is an additive JSON evolution and requires no SQLite
migration.

### Machine-readable inventory

`config/governance/coordination-collapse-ledger.json` records expected consumer
counts. `scripts/qa/coordination-governance.rb` reproduces the inventory:

| Surface | Production consumers | Decision |
|---|---:|---|
| capture / JSONPath | 0 | removed |
| coordination CEL | 0 | removed while governance CEL remains |
| generic `PipelineVariables` | 2 | deprecated, blocked |
| command-only Agent manifests | 4 | frozen, transferred to FR-126, which drove the count to 0; the ledger pins 0 |

The two generic-variable consumers are reviewed `store_inputs` bindings in
`promotion` and `self-evolution`. Public initial/item inputs, command-rule CEL,
and generic output/template compatibility are additional code-level blockers.

## Key Design

1. Production normalization no longer injects capture or JSONPath post-action
   defaults.
2. Apply-time validation rejects author-supplied legacy coordination before a
   task can run.
3. The scheduler directly records known phase flags and typed daemon-tool
   receipts; it does not scrape runner stdout.
4. Dynamic item generation writes through the daemon repository immediately.
   Deferred JSONPath post-action buffering is removed.
5. Item selection consumes bounded numeric `ExecutionSignals.metrics`, not
   arbitrary strings.
6. A test-only capture oracle and the historical parity fixture preserve
   rollback evidence without leaving a production execution path.
7. Generic CEL binding remains explicitly labeled as a compatibility surface.
   The production workflow inventory proves it has no coordination consumer.

## Alternatives And Tradeoffs

- Delete every legacy schema type: smaller code, but old manifests would fail
  with unstable parse errors and lose a precise migration message.
- Remove the generic variable map immediately: maximal cleanup, but breaks
  reviewed store bindings and public compatibility contracts.
- Add a general typed state system: makes arbitrary state explicit but reopens
  DD-130's closed decision and expands the coordination model.
- Chosen approach: remove the zero-consumer runtime path, use narrow fixed
  carriers for scheduler-owned state, and expose remaining blockers in the
  ratchet.

## Risks And Mitigations

- Risk: old persisted tasks lose `goal` or sandbox-denial state.  
  Mitigation: normalize the four legacy keys on load and test mixed-version JSON
  precedence and reserialization.
- Risk: governance CEL is mistaken for coordination CEL.  
  Mitigation: scan expressions after removing string literals and allow only the
  reviewed deterministic governance identifiers.
- Risk: historical parity QA silently exercises removed production behavior.  
  Mitigation: production validation must reject the legacy half; the QA script
  derives and executes only the seven tool workflows.
- Risk: generic state is declared removed while hidden consumers remain.  
  Mitigation: the ledger records two exact manifest consumers and named public
  compatibility blockers; count drift fails the governance gate.

## Observability

- Logs: stable manifest rejection codes identify retired authoring surfaces.
- Evidence: the governance JSON report contains exact consumer records, source
  counters, and the command-only Agent inventory.
- Events: existing typed driver and coordination-tool events remain the runtime
  evidence chain; no new telemetry payload is introduced.
- Metrics: the production-only source ratchet decreases capture/JSONPath
  touches from 143 to 53 and requires an exact match with the reviewed
  baseline. FR-125 recorded 55 and rejected only increases; FR-128 corrected the
  `cfg(test)` exclusion the ledger's `scope` already claimed and made the
  comparison exact, so a decrease can no longer leave the ledger overstating
  debt while the gate stays green. See [DD-140](140-governance-ledger-regeneration.md).

## Operations / Release

- Config: no new environment variables.
- Migration: existing `pipeline_vars_json` is read and normalized lazily; the
  next write persists the narrow fields.
- Compatibility: arbitrary variables and governance CEL remain available.
  Capture/JSONPath manifests are rejected at validation.
- Rollback: revert the production removal change, then use
  `fixtures/manifests/bundles/coordination-strangler-parity.yaml` and the
  test-only capture oracle to verify the restored legacy path against FR-124
  parity evidence.
- Follow-up: govern the generic variable blockers separately; FR-126 owns
  command-only Agent driver migration.

## Test Plan

- Unit: legacy-state normalization, narrow carrier rendering and accumulator
  propagation, typed metrics, self-test signals, and stable validation errors.
- Integration: seven deterministic tool workflows on an isolated daemon.
- Governance: exact consumer counts, negative consumer fixtures, exact-equality
  source counts, and boundary coverage fixtures.
- Repository: workspace tests, strict Clippy, formatting, and QA document lint.

## QA Docs

- `docs/qa/orchestrator/175-legacy-coordination-decommission.md`
- Historical compatibility evidence:
  `docs/qa/orchestrator/174-coordination-strangler-completion.md`

## Acceptance Criteria

- Capture/JSONPath and coordination CEL both report zero production consumers.
- The production scheduler has no capture/JSONPath extraction or deferred
  consumption path.
- The four residual intent/safety fields persist and render outside the generic
  string map.
- Remaining generic-variable and command-only Agent dependencies are exact,
  reviewed, and fail closed on drift.
- Post-retirement tool workflows, survival mechanisms, boundary coverage,
  workspace tests, and strict Clippy pass.
