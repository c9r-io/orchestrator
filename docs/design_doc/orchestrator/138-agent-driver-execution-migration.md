# Orchestrator Runner - Agent Driver Execution Migration

**Module**: Orchestrator Runner / Scheduler / Workflow Governance
**Status**: Released
**Related Plan**: FR-126 command-only Agent migration, legacy runner retirement, strict evidence remediation, and documentation alignment ratchet
**Related QA**: `docs/qa/orchestrator/176-agent-driver-execution-migration.md`
**Created**: 2026-07-25
**Last Updated**: 2026-07-25

## Background

FR-116 introduced provider-neutral `shell/cli`, `claude/cli`, and `codex/cli` drivers, but a global `RunnerExecutorKind` still selected a legacy shell or Claude streaming backend. Four production Agent manifests also omitted `spec.driver`.

The initial FR-126 implementation migrated every production Agent, normalized historical command-only manifests, and removed global executor selection. A strict closure audit later found that its aggregate inventory and synthetic shell pilots did not prove the original per-production-object acceptance criteria. The FR was reopened to add exact inventory, offline parity, explicit compatibility/rollback boundaries, and a mandatory release gate.

The evidence remediation also exposed a runtime defect: typed `driver_tool_use` events were recorded, but item-level typed signals were not promoted to the task convergence context. A `mark_done` workflow therefore completed only after reaching its cycle ceiling. The scheduler now promotes typed signals before loop continuation evaluation.

A third closure audit found a different class of gap: EN/ZH user guides still advertised the removed global streaming executor, CEL documentation attributed typed signals to that executor, released design records described a deleted compatibility bridge as current, and the production governance fixture did not name its stricter decision layer. The default closure chain had no deterministic check over `docs/guide`, architecture, or the repository authoring skill. FR-126 was reopened again to align those surfaces and add a fail-closed documentation semantics gate.

A fourth audit followed the links from the corrected CEL guides and found that
their operational mark-done showcase still described the removed `streaming`
executor as current. The first documentation gate listed ten known files and
never scanned `docs/showcases`, so it could certify the guides while their
recommended next page remained stale. FR-126 was reopened once more to align the
showcase, cover the entire showcase directory, and verify the linked page's
positive typed-driver semantics.

## Goals

- Keep zero command-only Agent and global streaming consumers in production.
- Make all 20 production Agent identities and driver targets machine-reviewable.
- Prove the four migrated production contracts offline without provider credentials.
- Preserve shell command, output, sandbox, cancellation, redaction, and TTY behavior.
- Prove typed Claude `mark_done` events reach task-level convergence in one cycle.
- Separate production admission rejection from historical runtime compatibility.
- Retain executable compatibility-window and rollback evidence.
- Make every original repository gate mandatory for release certification.
- Keep EN/ZH guides, architecture, authoring skills, and released design status aligned with the typed-driver runtime.
- Keep operational showcases reached from those guides aligned with the same runtime.
- Make production-admission rejection and runtime compatibility promotion machine-readable as separate decisions.

## Non-goals

- Restore a legacy runner or global streaming executor.
- Execute production AI workflows during QA or consume provider API credits.
- Infer Claude or Codex from legacy command text.
- Remove runner policy, sandbox profiles, resource limits, process groups, environment filtering, output capture, or redaction.
- Remove engine-owned direct Step commands.
- Add a provider or enable SDK transport.

## Scope

- In scope: production Agent manifests, governance inventory, compatibility normalization, typed signal projection, loop convergence, offline parity fixtures, runner removal rollback, normative guide/architecture/skill/showcase semantics, and aggregate QA.
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
11. Governance fixtures label production admission separately from runtime Apply compatibility and assert the warning/promotion contract.
12. A deterministic documentation alignment script checks EN/ZH guides, architecture, authoring skill, operational showcases, released design status, fixture layering, and stable diagnostics; `qa-doc-lint` and the FR-126 aggregate both execute it.
13. DD-102/DD-103 retain their historical first-cut detail behind explicit
    superseded-execution-seam banners. DD-58 remains a dated incident analysis,
    while DD-137 already records that runner removal transferred to and completed
    in FR-126; neither is current authoring guidance.

## Documentation Alignment Boundary

The LLM-driven guide-alignment workflow remains useful for broad CLI prose review, but release certification needs deterministic invariants for retired execution behavior. `scripts/qa/test-agent-driver-documentation-alignment.sh` therefore fails when:

- `runner.executor: streaming` is presented as a usable option;
- CEL signals are attributed to the removed global executor rather than typed `driver_terminal` and normalized tool artifacts;
- architecture or authoring guidance recommends new command-only Agents;
- DD-101/DD-127 describe the deleted compatibility bridge as current;
- any Markdown file under `docs/showcases/` presents the removed executor as a
  current runnable path;
- the EN/ZH CEL guides stop referencing the governed mark-done showcase, or that
  page stops naming `claude/cli`, normalized tool events, and `driver_terminal`;
- the governance fixture omits its `production-manifest-governance` layer or runtime compatibility outcome;
- documented stable diagnostics disappear from production source.

`--fixture-test` injects representative stale phrases and proves the detector fails closed. This targeted ratchet complements rather than replaces full guide-alignment review.

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
- Risk: runtime migration completes while user-facing guides keep advertising retired configuration.
  - Mitigation: execute the driver documentation alignment ratchet from both `qa-doc-lint` and the FR-126 aggregate, scan the entire showcase directory, verify the EN/ZH downstream link, and include representative guide and showcase phrases in the negative fixture.

## Observability

- Logs: stable compatibility and retirement reason codes identify operator action.
- Events: `driver_started`, `driver_tool_use`, `driver_tool_result`, `driver_usage`, and `driver_finished` are normalized and session-safe.
- Inventory: exact per-Agent identity, driver, Workflow association, and fingerprint are emitted as JSON.
- QA evidence: terminal states, output hashes, typed event counts, cycle count, and session-persistence result are written by the isolated harness.
- Documentation evidence: EN/ZH semantics, linked showcase semantics, design-record status, authoring examples, fixture decision layers, and source diagnostic presence are reported as a bounded alignment result.

## Operations / Release

- Config: no new environment variables.
- Compatibility: command-only ingress warns and promotes; global streaming selection is retired.
- Migration: re-apply historical Agent manifests to persist the explicit shell driver.
- Release certification:

  ```bash
  ./scripts/qa/test-agent-driver-execution-migration.sh
  ```

- Documentation-only diagnostic:

  ```bash
  ./scripts/qa/test-agent-driver-documentation-alignment.sh --fixture-test
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
- Governance: exact production inventory, layered negative admission fixtures, manifest fingerprints, compatibility ancestry, and zero legacy source symbols.
- Documentation: EN/ZH, linked showcase, architecture/skill/design alignment plus a stale-semantics negative fixture.
- Repository: coordination strangler, format, workspace tests, strict Clippy, coverage governance, and QA lint (which also executes documentation alignment).

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
- EN/ZH guides and authoring surfaces describe only typed-driver execution, with parse-only/compatibility boundaries explicit.
- Every operational showcase is scanned for retired execution semantics, and
  the mark-done page linked by both CEL guides documents current typed-driver
  events and artifacts.
- Governance fixtures machine-readably distinguish production admission from runtime compatibility.
- The stale-documentation negative fixture and `qa-doc-lint` integration pass.
- The default aggregate and every repository gate pass.
