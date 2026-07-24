# Coordination Strangler Completion

**Module**: Orchestrator Scheduler / Workflow Governance  
**Status**: Released  
**Related Plan**: FR-124 production migration, parity evidence, freeze ratchet, and retirement criteria  
**Related QA**: `docs/qa/orchestrator/174-coordination-strangler-completion.md`  
**Created**: 2026-07-25  
**Last Updated**: 2026-07-25

## Background

DD-130 proved that daemon-owned MCP tools can replace declarative coordination
in one pilot, but the production workflow library still used captured stdout,
JSONPath, post-actions, pipeline variables, and coordination CEL. Maintaining
both models indefinitely would preserve two authoring languages and two
regression surfaces.

FR-124 completes the strangler migration without reopening DD-130's
`closed-not-deferred` decision against a general typed-state/reducer layer.

## Goals

- Classify every production `Workflow` and migrate every non-governance-only
  workflow to agent-local coordination plus typed daemon tools.
- Make tool hosting and provider-session continuation explicit per-step
  capabilities instead of implicit behavior for every Claude driver run.
- Reject new legacy coordination while retaining reviewed deterministic
  governance gates and four residual intent/safety channels.
- Retain a runnable legacy compatibility fixture until removal criteria are met.

## Non-goals

- Removing CEL, captures, `PipelineVariables`, builtins, or the shell executor
  in this change.
- Introducing a general typed workflow-state or reducer abstraction.
- Moving capability selection, budgets, sandbox rules, publication policy, or
  self-referential safety into an agent prompt.

## Scope

- In scope: the 11 production workflows under `docs/workflow/` and `config/`,
  the coordination tool host, session attachment, workflow manifests,
  governance ledger, CI ratchet, and offline parity evidence.
- Out of scope: historical fixtures except the dedicated parity matrix, provider
  credentials, GUI behavior, and external network testing.

## Key Design

### Production inventory and classification

`config/governance/coordination-collapse-ledger.json` is the reviewed source of
truth. The discovered inventory is exact: three `tool-migratable`, four
`hybrid`, and four `governance-only` workflows. Inventory drift, unreviewed
touches, stale allowances, incomplete migrations, or source-count growth fail
the ratchet.

The seven migrated workflows have independent legacy/tool scenarios:

| Production workflow | Coordination moved into the run | Governance retained |
|---|---|---|
| `command_rules` | daemon-private provider continuation | capability and workspace boundary |
| `qa_loop` | tests, tickets, terminal status | driver/tool allowlist |
| `plan_execute` | private plan context, test receipt, status | ordered steps and workspace policy |
| `full-qa` | tests and ticket scan | safe QA-path CEL, self-test, loop guard |
| `self-bootstrap` | dynamic QA selection, tests, tickets | two cycles, self-test/restart, safe-path CEL, rollback |
| `promotion` | dynamic platform items and draft status | `api_publishable` CEL and command publisher |
| `self-evolution` | candidate items and bounded score | deterministic `item_select`, self-test, loop guard |

### Explicit run capabilities

`driverRequirements.toolHosting: stdio` now decides whether the scheduler starts
the authenticated coordination callback. Merely selecting a Claude driver no
longer starts a tool host.

`driverRequirements.sessionResume: true` decides whether a step receives the
task's private provider reference. Fresh independent review steps omit it.
Provider references never enter manifests, captures, pipeline variables, DTOs,
or events. Because the current reference is task-scoped, parallel item steps
must not opt into session continuation.

### Tool contract extension

`record_metric(name, value)` adds the one missing primitive needed by
`self-evolution`. Names match `[a-z][a-z0-9_]{0,63}` and values must be finite
within the governed range. Authenticated receipts fold into the existing item
accumulator so deterministic `item_select` can read the score without stdout
capture or JSONPath.

There are six primary tools (`run_tests`, `mark_item`, `create_ticket`,
`scan_tickets`, `generate_items`, and `record_metric`) plus the compatibility
alias `mark_done`.

### Freeze and retirement

The ledger defines three stages:

1. **Freeze**: the production inventory is exact and new coordination touches
   fail CI.
2. **Deprecate**: a legacy channel has zero production consumers and every
   migrated workflow has independent parity evidence.
3. **Remove**: deprecation remains true for the compatibility window and the
   legacy fixture, rollback proof, workspace tests, strict Clippy, and boundary
   coverage remain green.

The shell executor remains frozen, not removed, until all non-governance
production workflows meet those conditions.

## Alternatives And Tradeoffs

- Keep both authoring models indefinitely: lowest short-term churn, but retains
  duplicate mental models and lets coordination debt grow.
- Remove legacy code immediately: smaller codebase, but no compatibility window
  or rollback path.
- Add a general typed state graph: centralizes state mechanically, but models
  coordination that no longer survives outside the agent run.
- Chosen approach: migrate consumers, freeze growth, retain deterministic
  governance and compatibility, then remove only after measured zero use.

## Risks And Mitigations

- Risk: a tool-capable provider receives tools on a step that did not request
  them.  
  Mitigation: tool host startup is step opt-in and capability-gated.
- Risk: an independent QA step inherits implementation context.  
  Mitigation: provider continuation is step opt-in; a unit and offline trace
  verify fresh versus resumed steps.
- Risk: self-bootstrap loses its recovery envelope.  
  Mitigation: production manifest checks and isolated two-cycle execution retain
  binary snapshot, `self_test`, self-reference policy, `self_restart`, and
  watchdog evidence.
- Risk: a new capture is disguised as harmless compatibility.  
  Mitigation: exact per-workflow touch allowances and a monotonic source baseline
  reject additions until the ledger is deliberately reviewed.

## Observability

- Logs: daemon startup and normal step diagnostics remain unchanged.
- Events: `driver_tool_use`, `driver_tool_result`,
  `coordination_tool_started`, and `coordination_tool_completed` form the
  requested/result/authoritative-receipt chain.
- Metrics: the governance report exposes workflow counts, classifications,
  migration statuses, and source touch counts. It is a CI artifact in command
  output rather than a runtime telemetry series.
- Tracing: provider session material remains private; the offline fake-driver
  trace records only fresh/resume behavior for the test workspace.

## Operations / Release

- Config: no new environment variables or database migration.
- CI: `.github/workflows/ci.yml` runs
  `scripts/qa/test-coordination-strangler.sh` on Ubuntu.
- Rollback: revert an individual production manifest to its reviewed legacy
  form and update the ledger in the same change. The compatibility executor
  remains available.
- Compatibility: legacy manifests continue to parse and execute; they cannot be
  added to production roots without an explicit ledger review.

## Test Plan

- Unit: tool name/value bounds, authenticated real tool execution, accumulator
  folding, session opt-in, and tool-host opt-in.
- Integration: seven network-free legacy/tool pairs on an isolated daemon,
  including dynamic items, metric selection, typed events, and two cycles.
- Governance: exact inventory, negative capture fixture, allowed CEL fixture,
  allowed safety fixture, monotonic source counters, and completion evidence.
- Repository: formatting, workspace tests, strict Clippy, QA document lint, and
  boundary coverage governance.

## QA Docs

- `docs/qa/orchestrator/174-coordination-strangler-completion.md`
- Historical pilot: `docs/qa/orchestrator/168-coordination-collapse-mcp-tools.md`

## Acceptance Criteria

- All 11 production workflows are classified and all seven non-governance-only
  workflows are marked migrated with independent parity evidence.
- Production coordination consumers of captures, JSONPath, post-actions, and
  step-variable handoffs are zero; reviewed CEL and builtins remain.
- The source ratchet is non-increasing from `143 / 47 / 9`; the completed
  implementation reports `143 / 46 / 9`.
- Self-bootstrap completes two isolated cycles with its survival mechanisms
  present.
- DD-130's no-general-typed-state decision remains closed.
