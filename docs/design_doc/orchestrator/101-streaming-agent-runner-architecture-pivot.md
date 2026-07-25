---
lifecycle: superseded
superseded_by: docs/design_doc/orchestrator/138-agent-driver-execution-migration.md
---

# Orchestrator - Streaming Agent Runner Architecture Pivot

**Module**: orchestrator
**Status**: Released (Decision Record)
**Related Plan**: Replace the one-shot shell-text agent contract with a long-lived, bidirectional, structured streaming contract (stream-json) where the agent calls orchestrator-owned typed tools (MCP), collapsing coordination logic out of the YAML/CEL declarative layer
**Related QA**: [QA-164](../../qa/orchestrator/164-agent-driver-abstraction.md), [QA-168](../../qa/orchestrator/168-coordination-collapse-mcp-tools.md)
**Created**: 2026-06-27
**Last Updated**: 2026-07-25

> **Implementation status (superseded execution seam, 2026-07-25):** the 2026-06-28 first cut proved structured tool calling behind `RunnerExecutorKind::Streaming`. FR-116 then introduced per-Agent provider drivers and direct normalized event consumption ([DD-127](127-agent-driver-abstraction.md)); FR-126 migrated every production Agent and deleted the global streaming executor, provider-session compatibility bridge, and runner selection seam ([DD-138](138-agent-driver-execution-migration.md)). References below to `StreamingAgentRunner`, `ShellRunnerExecutor`, streaming opt-in, or shell-default rollback describe the historical pivot and are not current configuration guidance.

## Historical Background

At the start of this decision record, the orchestrator treated an agent as a **one-shot shell black box**:

- An agent is a shell command template with a `{prompt}` placeholder — `AgentSpec.command` e.g. `claude -p "{prompt}"` (`core/src/resource/agent.rs:33`).
- Execution is `/bin/sh -c "<rendered command>"` spawned via `tokio::process::Command`, optionally wrapped by a sandbox (`crates/orchestrator-runner/src/runner/spawn.rs`, `sandbox.rs:351`). Spawning happens behind the `RunnerExecutor` trait, today implemented only by `ShellRunnerExecutor` (`spawn.rs:52`).
- The contract is **text**: stdout/stderr + exit code. Structured data is recovered by extracting JSON from stdout via `json_path` into pipeline variables (`item_executor/apply.rs`, `accumulator.rs`).
- The agent is **stateless per step** — it is spawned, runs once, dies. The orchestrator owns the entire outer loop (cycles, segments, guards, finalize).

### The root-cause problem

Because the agent is a black box that can only emit text, the orchestrator **cannot converse with it** or obtain structured intermediate state. As a consequence, all coordination intelligence — *what to run next, whether to loop, how data flows, when work has converged* — is **exiled from the agent into the declarative layer**: CEL prehooks (`core/src/prehook/cel.rs`), `captures` + `json_path`, pipeline variables with 4KB spill-to-disk, finalize rules, `StepScope` segments, and `builtin:` magic strings.

This is the source of the authoring burden. A production workflow such as `docs/workflow/self-bootstrap.yaml` is 443 lines: single-line 5-clause CEL expressions, name-based references that fail silently at runtime, spill-to-disk semantics the author must anticipate, and segment/item-narrowing rules invisible from the YAML itself. **The dumber the agent contract, the smarter the YAML must be.** Roughly 70% of that manifest is coordination "ransom" paid for the black-box contract.

### Product positioning decision

This pivot is predicated on an explicit positioning choice: **the orchestrator is a great agent harness (a developer tool), not a control-plane-as-product ("Kubernetes for agents").** If the manifests were the product, the heavy declarative surface would be acceptable and the fix would be authoring ergonomics. Since the goal is a usable harness, the declarative surface is accidental complexity to be collapsed.

### Enabling capability (verified)

`claude` (verified 2.1.195) supports `--print --input-format stream-json --output-format stream-json`: a long-lived process speaking newline-delimited JSON events bidirectionally. Custom typed tools can be supplied by the orchestrator via MCP and executed by the orchestrator. This makes it possible to keep the process boundary while replacing the text contract. See the Spike Evidence section.

## Goals

- Introduce a `StreamingAgentRunner` alongside `ShellRunnerExecutor` (additive; the shell path is untouched).
- Make the agent a **long-lived, multi-turn, structured** participant: orchestrator observes `tool_use`/`tool_result`/`result` events instead of parsing text.
- Let the agent call **orchestrator-owned typed tools** (via MCP) — `create_ticket`, `run_tests`, `mark_item`, etc. — replacing `post_actions`, `captures`/`json_path`, and much CEL.
- Push per-step coordination into the agent loop and tools; keep only **cross-cutting governance** (capability/cost selection, safety/sandbox, permissions, triggers) in the declarative layer.
- Demonstrate the collapse on one real workflow: target an ~80% reduction in hand-written YAML.

## Non-goals

- Removing `ShellRunnerExecutor` or breaking existing shell-based workflows.
- Rewriting the agent loop in Rust against the raw Messages API (kept as an explicit alternative, not chosen now).
- Deleting the declarative model wholesale — governance stays declarative. This is not a regression into a dumb wrapper.
- Changing the database persistence kernel (event ingestion is additive).

## Scope

- In scope: `StreamingAgentRunner` implementing `RunnerExecutor`; stream-json protocol client (send user messages, read event stream); construction of `RunResult` from the event stream; orchestrator-hosted MCP tool surface; permission/`allowedTools` wiring as governance; one pilot workflow rewritten; event-stream → `events` table ingestion.
- Out of scope: GUI changes; trigger/cron changes; migrating every existing workflow at once; the raw-API runner.

## Key Design

1. **Historical runner seam, revised by FR-116** — `StreamingAgentRunner` originally implemented `RunnerExecutor`, while selection by capability stayed unchanged. That seam remains sufficient for a one-shot compatibility runner, but it is not the durable provider abstraction.

   **Revised seam-fit finding:** once the provider exposes live session input, permission requests, tool I/O, usage, and terminal outcome through one normalized stream, a child-process-only seam is too narrow. Re-reading a bounded stdout file also risks dropping the terminal result. FR-116 therefore refactored explicit-driver phases from `setup → spawn → wait → validate → record` to `setup → start → consume → fold → record`. Legacy commands retain the old path, while all CLI drivers still delegate process creation to the common sandbox spawn function.

   Two consequences for the pipeline:
   - **Validation must become stream-json-aware** (`core/src/output_validation.rs`): instead of expecting one JSON blob on stdout, parse the event stream and take the terminal `result` event (plus `tool_use`/`tool_result` for the events table). This is an additive variant keyed on the runner/agent kind.
   - **Cross-step continuity uses session resume**: `--resume <session_id>` reuses the agent's context across steps. `session_id` is already captured and persisted (`SpawnResult.session_id`, `command_runs.session_id`), so the plumbing exists.

2. **Bidirectional stream-json protocol** — spawn `claude --print --input-format stream-json --output-format stream-json --verbose --model <m>` with `CLAUDECODE` removed from env (mirrors the existing `env_remove` in `spawn.rs`). The orchestrator writes `{"type":"user","message":{...}}` lines to stdin and reads `system`/`assistant`/`user`(tool_result)/`result` events from stdout. The process stays alive across turns; `session_id` is stable.

3. **Orchestrator-owned typed tools via MCP** — coordination actions become MCP tools the orchestrator implements and whose results it computes. Two transport options (see Alternatives): a stdio MCP shim, or an **orchestrator-hosted HTTP MCP endpoint** (preferred — tool logic lives in-process in the Rust daemon, no shim). Tools are pre-approved via `--allowedTools` and `--permission-mode`.

4. **Coordination collapse** — mapping of what moves out of YAML/CEL:
   - CEL prehook / finalize / convergence → agent self-judgment + a few code-level guards.
   - `captures` + `json_path` → typed tool return values (already structured).
   - pipeline vars + 4KB spill-to-disk → tool inputs/outputs.
   - `post_actions` (create_ticket / generate_items / spawn_task) → MCP tools.
   - `StepScope` segments / item narrowing → a Rust function that maps the agent over items.
   - `builtin:` magic strings → ordinary functions/tools.

5. **What stays declarative (governance)** — Workspace (where/which repo), Agent policy (model/cost/capability/selection), Safety/sandbox profile (rollback/checkpoint/budget caps), Trigger (cron/event). These are precisely the decisions the agent must *not* make for itself; `allowedTools`/permission hooks are the enforcement point.

6. **Event ingestion** — the structured event stream (including tool I/O) feeds the `events` table directly, replacing stdout/stderr file parsing in `command_runs`. `RunResult` (`core/src/dto.rs:278`) is constructed from the terminal `result` event plus accumulated tool calls: map the `result` to `exit_code`/`success`/`validation_status`, build `AgentOutput` (`crates/orchestrator-collab/src/output.rs`) from the assistant text + tool events, and carry `total_cost_usd`. The `sandbox_*` fields are `false`/`None` (sandboxing for the streaming runner, if any, is handled differently). The existing `setup`/`record` stages (initial `NewCommandRun` with `exit_code=-1`, final update with `output_json`) are reused unchanged.

## Spike Evidence

A throwaway Node spike (driver + hand-rolled MCP stdio server) validated the foundation:

- **Phase 1 — long-lived multi-turn, structured.** One `claude` process consumed multiple user turns over stream-json with a stable `session_id` across turns. All output was structured events (`system/init`, `assistant`, `result/success`); no text parsing. Cost is reported per turn (`total_cost_usd`), enabling budget governance.
- **Phase 2 — orchestrator-owned typed tool.** A minimal MCP server exposed `run_tests`. The agent emitted `tool_use mcp__spike__run_tests {"target":"core"}`; the spike's code computed and returned a hard-coded structured result; the result was fed back as `tool_result` and the agent continued using *our* data. The exact injected string (`core::selection::picks_healthy_agent`) appearing in the final answer is conclusive proof the orchestrator owned execution. `sawToolUse=true sawToolResult=true`.

Wrinkles observed (carried into Risks): MCP tools may be **deferred** — the agent ran a `ToolSearch` to load the schema before the first call (one extra round-trip); permissions had to be granted explicitly (`--allowedTools` + `--permission-mode bypassPermissions`); `CLAUDECODE` must be unset when nested.

## Alternatives And Tradeoffs

- **Option A: spawn `claude` in stream-json mode (chosen).** Keeps the process boundary; the CLI continues to provide file editing, context compaction, and permissions for free. The orchestrator only owns the tool-result loop and outer coordination. Lowest cost to hit the root cause.
- **Option B: raw Anthropic Messages API loop in Rust.** Maximum control and a pure-Rust stack, but the orchestrator must re-implement file-edit tools, context management, and permissions. Higher cost; deferred.
- **Option C: Agent SDK (TS/Python) as a sidecar.** Featureful but introduces a second runtime and an extra hop; least clean for a Rust daemon.
- **MCP transport sub-choice:** stdio MCP server (a subprocess `claude` spawns; simplest, mirrors the spike) vs **orchestrator-hosted HTTP MCP** (tool logic in-process in the daemon; preferred for production so tools share daemon state directly).

## Risks And Mitigations

- Risk: stream-json event schema is semi-stable across `claude` versions.
  - Mitigation: pin/record the `claude` version; treat the protocol client as an adapter with a conformance test; version-gate.
- Risk: deferred MCP tools add a `ToolSearch` round-trip / latency.
  - Mitigation: keep the per-agent tool set small so tools are eagerly exposed; accept the one-time handshake otherwise.
- Risk: losing CEL's deterministic, auditable control flow by delegating decisions to the agent.
  - Mitigation: keep hard guardrails as code/policy (safety, budget caps, `allowedTools`); the agent decides *within* the fence, not the fence itself.
- Risk: headless auth and nested-session refusal.
  - Mitigation: unset `CLAUDECODE`; document the auth path (OAuth/keychain or `ANTHROPIC_API_KEY`) for daemon contexts.
- Risk: per-turn token cost and runaway loops.
  - Mitigation: consume `total_cost_usd` from `result` events; enforce max-turns/budget at the runner.
- Risk: the validate stage reads stdout capped (~256KB); a long agentic step's event stream can exceed this, truncating the terminal `result`.
  - Mitigation: parse the stream incrementally as it arrives (don't rely on re-reading a capped file), or persist the full event stream to a sidecar artifact and validate from it.
- Risk: out-of-band MCP hosting adds a network/IPC surface and a tool-auth concern (any local process could hit the orchestrator's HTTP MCP endpoint).
  - Mitigation: bind locally, require a per-run token passed to `claude` via `--mcp-config`; or use the stdio MCP shim (as in the spike) where `claude` owns the server lifecycle.
- Risk: migration churn across many existing workflows.
  - Mitigation: additive runner; migrate one pilot workflow first; shell path stays default until parity is proven.

## Observability

- Logs: runner-level `tracing` for spawn, each turn, tool dispatch, and protocol decode errors.
- Metrics: reuse existing step/run metrics; add per-turn token/cost capture from `result` events.
- Tracing: ingest `tool_use`/`tool_result`/`result` events into the `events` table; this is strictly richer than the current stdout/stderr file capture (tool I/O becomes first-class structured events rather than parsed text).

## Historical Operations / Release

- The original rollout was additive and opt-in: agents selected the streaming runner via spec while shell remained the default.
- Pilot first: rewrite one workflow (candidate: the QA fix-loop or `self-bootstrap`) and compare YAML size and behavior against the shell version side by side.
- The original rollback removed streaming opt-in to restore `ShellRunnerExecutor`; current rollback uses a reviewed explicit `shell/cli` Agent because both legacy executors are removed.

## Migration Plan (completed)

1. **Complete** — implement the stream-json compatibility runner, followed by the durable per-Agent driver seam.
2. **Complete** — host `run_tests`, `mark_item`, `create_ticket`, `scan_tickets`, and `generate_items` in the daemon behind a per-run authenticated callback.
3. **Complete** — enforce `allowedTools`, permission mode, private MCP configuration, and driver requirements.
4. **Complete** — migrate a paired pilot off CEL/captures/JSONPath/post-actions and prove terminal parity.
5. **Complete** — ingest provider tool I/O and daemon execution receipts into `events`.
6. **Complete** — record 38→21 effective YAML lines, 15→0 coordination lines, equal `completed/qa_passed` behavior, and four residual non-spilled cross-step fields in DD-130.

## Test Plan

- Unit: stream-json event decoding (each event type), `RunResult` construction from an event stream, MCP tool request/response handling.
- Integration: a pilot workflow runs end-to-end on the streaming runner; tool calls execute orchestrator code; final status matches the shell-based equivalent.
- Conformance: a recorded-protocol fixture test pinned to the validated `claude` version.
- Regression: all existing shell-runner tests pass unchanged (additive change).

## QA Docs

- [QA-164 — Agent Driver Abstraction](../../qa/orchestrator/164-agent-driver-abstraction.md)
- [QA-168 — Coordination MCP Tools](../../qa/orchestrator/168-coordination-collapse-mcp-tools.md)

## Acceptance Criteria

- `cargo build` / `cargo test` pass; existing shell-based workflows unchanged.
- An agent flagged for the streaming runner executes a multi-turn session in one process and calls at least one orchestrator-owned MCP tool whose result it consumes.
- The pilot workflow reproduces shell-version behavior with a substantial (~80% target) reduction in hand-written YAML.
- Tool I/O appears as structured events in the `events` table.
