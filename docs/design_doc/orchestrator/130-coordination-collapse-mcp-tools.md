---
lifecycle: active
related_fr: FR-118
---

# Orchestrator-Owned Coordination MCP Tools

**Module**: orchestrator scheduler / runner / agent drivers
**Status**: Released
**Related FR**: FR-118
**Related QA**: [QA-168](../../qa/orchestrator/168-coordination-collapse-mcp-tools.md)
**Created**: 2026-07-23
**Last Updated**: 2026-07-23

## Context And Decision

The provider-neutral driver seam from DD-127 made structured tool calls observable, but the original `orch-mcp-tools` binary still returned canned results. Workflow authors therefore still had to coordinate through CEL prehooks, stdout captures, JSONPath, pipeline variables, and post-actions.

FR-118 established the pivot proposed by DD-101. Coordination intent is expressed through typed tools whose results are computed inside the daemon. The existing shell/CEL path remains supported for compatibility; new tool-driven workflows can remove transitional coordination wiring while retaining declarative governance. DD-136 completes the production strangler migration and adds the freeze/retirement gate.

## Architecture

```text
Claude CLI
  │ stdio JSON-RPC
  ▼
orch-mcp-tools (transport-only shim)
  │ HTTP JSON-RPC + Bearer token
  ▼
127.0.0.1:<ephemeral> per-run callback
  │
  ├── allowedTools enforcement
  ├── scheduler/store state
  ├── shared sandboxed spawn path
  └── canonical events table
```

The scheduler starts an ephemeral loopback HTTP host for each eligible driver run. It generates a private token, places the callback URL and token in that run's mode-`0600` MCP config, and adds the token to output redaction. `orch-mcp-tools` contains no coordination business logic: it forwards MCP JSON-RPC between provider stdio and the authenticated callback. The host lives only until the driver run ends.

This split keeps Claude's stdio MCP compatibility while ensuring authoritative tool execution shares daemon state. It introduces no database migration.

## Tool Contracts

| Tool | Contract and boundary |
|---|---|
| `run_tests` | Runs one allowlisted target (`workspace`, `core`, `runner`, or `scheduler`) through the shared runner/profile path and returns exit status, counts, and bounded output tails. |
| `mark_item` | Validates the current item and an allowlisted terminal/status transition, then returns a receipt folded into scheduler state. |
| `create_ticket` | Creates a deduplicated QA ticket only from the latest failing `run_tests` evidence in the same run. |
| `scan_tickets` | Uses the existing ticket scanner and returns the current active-ticket set. |
| `generate_items` | Validates 1–100 non-duplicate workspace-relative paths and creates dynamic items through the existing scheduler service. |
| `record_metric` | Validates an authenticated item, bounded metric name, and finite governed numeric value, then folds it into item state for deterministic selection. |

`mark_done` remains a compatibility alias for existing streaming demonstrations. New workflows should use `mark_item`.

The manifest's `allowedTools` list is normalized to the known `mcp__orch__*` namespace. Unknown, omitted, or unapproved tools are neither advertised nor executable. Permission mode and driver requirement checks remain part of the Agent/Workflow governance boundary.

## State And Event Model

Every accepted call emits `coordination_tool_started` followed by `coordination_tool_completed`. The provider stream independently emits `driver_tool_use` and `driver_tool_result`; together they preserve requested intent, provider-visible result, and daemon execution receipt. Tool effects are folded into the normal item accumulator, so status, ticket, and dynamic-item behavior uses existing persistence paths.

No token, MCP config content, provider session reference, or unbounded stdout/stderr is stored in event payloads. Callback authentication failures return HTTP 401 before dispatch. Disallowed tools fail closed at JSON-RPC dispatch.

## Pilot Migration And Measurement

The paired workflows in `fixtures/manifests/bundles/coordination-collapse-pilot.yaml` prove behavioral parity:

| Measure | Legacy workflow | Tool workflow |
|---|---:|---:|
| Effective YAML lines | 38 | 21 |
| Handwritten coordination lines | 15 | 0 |
| Terminal result | `completed` / `qa_passed` | `completed` / `qa_passed` |

The tool workflow removes CEL, captures, JSONPath, post-actions, and pipeline-variable wiring from its authored step, yielding a **100% reduction in measured coordination lines**, exceeding the 80% target.

## Residual Cross-Step Channels

The pilot intentionally records every remaining item-level pipeline variable. These are not reconstructed tool results; they are narrow task context and safety channels that should scope any future typed-channel proposal.

| Key | Producer | Consumer | Spilled | Classification |
|---|---|---|---:|---|
| `goal` | task creation | prompt context | No | User intent carried into execution |
| `last_sandbox_denied` | runner safety fold | subsequent-step safety context | No | Safety signal |
| `sandbox_denied_count` | runner safety fold | subsequent-step safety context | No | Safety counter |
| `last_sandbox_denial_reason` | runner safety fold | subsequent-step safety context | No | Bounded safety diagnosis |

No task-level pipeline variables remain in the pilot. A follow-up typed-channel design, if justified, should cover only these durable cross-step needs rather than recreate a general coordination store.

**Decision: no typed-channel FR is opened at this time.** The measurement shows no central coordination state survives the collapse — state now lives in the agent session and typed tool contracts. The four residual variables are three homogeneous sandbox-safety signals plus one user-intent value, not a general store; a LangGraph-style typed state + reducer layer would be over-engineering. Typing those channels, if ever pursued, belongs to a small safety-scoped change, not a dedicated typed-state proposal. This item is closed, not deferred.

## Security And Failure Handling

- The callback binds only to `127.0.0.1` on an ephemeral port and requires a cryptographically random per-run Bearer token.
- Callback URL validation rejects non-loopback hosts and non-HTTP schemes.
- The private MCP file is run-scoped, mode `0600`, and deleted with run artifacts according to existing lifecycle rules.
- `run_tests` cannot execute arbitrary commands or bypass the selected ExecutionProfile.
- `create_ticket` is evidence-gated; `generate_items` is bounded and path-validated; `mark_item` is item- and status-validated.
- A callback/shim failure fails the tool call and remains visible through driver and coordination events; it does not fabricate success.

## Compatibility, Rollout, And Rollback

This change is additive. Legacy shell commands, CEL prehooks, captures, post-actions, and the provider-owned streaming compatibility runner continue to work. Rollout is per Agent through a typed Claude CLI driver plus explicit `allowedTools`, and per step through `driverRequirements.toolHosting: stdio`.

Rollback removes the tool-capable Agent selection or returns the workflow to its legacy step. No schema downgrade is required. Do not remove CEL compatibility until all production workflows have independent parity evidence.

That evidence and the staged retirement criteria are now maintained by
[DD-136](136-coordination-strangler-completion.md). CEL remains supported for
deterministic governance even after coordination consumers reach zero.

## Verification

`scripts/qa/test-coordination-collapse.sh` provides a network-free gate. It tests real callback authentication and tool execution, the actual stdio shim, legacy/tool pilot parity, event completeness, token privacy, private config mode, authored-line reduction, and residual channel classification. Repository closure also requires formatting, workspace tests, strict Clippy, the FR-116 driver regression, and QA-document lint.

## Code And Artifact Map

- `crates/orchestrator-scheduler/src/scheduler/coordination_tools.rs` — authenticated host and authoritative tools.
- `crates/orchestrator-runner/src/bin/orch_mcp_tools.rs` — transport-only stdio shim.
- `crates/orchestrator-runner/src/driver/{contracts,providers}.rs` — callback contract and private MCP configuration.
- `crates/orchestrator-scheduler/src/scheduler/item_executor/apply.rs` — tool-effect folding.
- `fixtures/manifests/bundles/coordination-collapse-pilot.yaml` — parity fixture.
- `scripts/qa/test-coordination-collapse.sh` — reproducible closure gate.
- `config/governance/coordination-collapse-ledger.json` — exact production inventory and monotonic ratchet.
- `scripts/qa/test-coordination-strangler.sh` — per-production-workflow parity and self-bootstrap gate.
