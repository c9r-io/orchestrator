# Orchestrator - Structured Stream-Run Signals in CEL

**Module**: orchestrator
**Status**: Proposed (Decision Record)
**Related Plan**: Let prehook / convergence-guard / finalize CEL consume the structured outcome of a streaming agent run — which tools the agent called, whether any failed, and run economics — instead of only regex-captured stdout, so coordination decisions can be driven by what the agent *did*
**Related QA**: TBD (to be generated when implementation begins)
**Created**: 2026-06-28
**Last Updated**: 2026-06-28

> **Implementation status (2026-06-28):** landed (unified approach). Extracted a shared `bind_pipeline_vars` helper (`core/src/prehook/context.rs`) now used by all three CEL builders — upgrading convergence (gains JSON-array→`list`) and finalize (gains a new `ItemFinalizeContext.vars` bag, populated in `to_finalize_context`) to prehook parity. `agent_orchestrator::stream_json::stream_signal_vars` derives the six signals from a run's artifacts; `item_executor/apply.rs` injects them into the accumulator's pipeline vars (empty for non-streaming runs). Verified: whole workspace + tests compile, core lib 1578 tests pass (incl. new prehook/convergence/finalize signal-consumption tests + `stream_signal_vars` derivation tests), scheduler 437, fmt clean.

## Background

Doc 102 made a streaming run's structure first-class *data*: tool calls and a run summary land on `AgentOutput.artifacts` and project into the `events` table. But the orchestrator's *control flow* still cannot see any of it. Step prehooks, the loop convergence guard, and finalize rules are CEL expressions evaluated against fixed contexts that expose scalar fields (`qa_exit_code`, `active_ticket_count`, `qa_failed`, …) plus — for prehook/convergence — captured pipeline `vars`. None of these expose "did the agent call `mark_done`?" or "did any tool error?" or "how much did this run cost?".

This is the next step of the pivot: coordination that today is encoded as CEL over regex-scraped stdout (`qa_exit_code == 0 && active_ticket_count == 0`) should instead be expressible over what the agent actually did (`'mark_done' in tools_called && tool_error_count == 0`). The streaming runner already produces that signal; this increment routes it into the three CEL contexts.

### What the three contexts already do (grounding)

- **Prehook** (`build_step_prehook_cel_context`, `core/src/prehook/context.rs:5`): binds each captured `vars` entry as a typed CEL variable — JSON arrays → `list<string>`, then `i64 → f64 → bool → string` inference — *then* built-in scalar fields.
- **Convergence** (`build_convergence_cel_context`, `context.rs:452`): also binds `vars`, but with **scalar inference only** (no JSON-array → `list` branch).
- **Finalize** (`build_finalize_cel_context`, `context.rs:324`; `ItemFinalizeContext`): has **no `vars` field at all**. It exposes scalar qa/fix fields and artifact-derived booleans (`total_artifacts`, `has_ticket_artifacts`, `has_code_change_artifacts`).

So the three contexts need different treatment — hence this doc.

## Goals

- Derive a small, typed set of signals from a streaming run's `ToolCall` / `stream_run_summary` artifacts (already produced in doc 102): `tools_called`, `tool_error_count`, `num_tool_calls`, `agent_reported_error`, `run_cost_usd`, `run_turns`.
- Make those signals referenceable in prehook, convergence-guard, and finalize CEL.
- Keep it additive and detection-gated: non-streaming runs inject nothing and behave exactly as today.

## Non-goals

- Exposing arbitrary nested tool *results* in CEL (e.g. `tool_result('run_tests').failed`). Only call names, an error count, and economics in this slice.
- Replacing existing CEL fields or rewriting any shipped workflow. This adds capability; migrating workflows is separate.
- Changing the streaming runner, the events schema, or doc 102's projection.

## Scope

- In scope: a shared `bind_pipeline_vars` helper used by all three CEL builders; an `ItemFinalizeContext.vars` bag + population from the accumulator; a helper deriving the six stream signals from a run's artifacts and injecting them as typed pipeline vars; reserved-name documentation; unit tests for each context.
- Out of scope: nested tool-result access; per-tool result schemas; threading `vars` into any context beyond the three CEL coordination contexts.

## Interfaces / Data Changes

1. **Signal derivation** — a helper (in `core`, near `stream_json`) producing:
   ```rust
   pub struct StreamSignals {
       pub tools_called: Vec<String>,   // distinct tool names, in first-seen order
       pub tool_error_count: i64,
       pub num_tool_calls: i64,
       pub agent_reported_error: bool,
       pub run_cost_usd: Option<f64>,
       pub run_turns: Option<i64>,
   }
   ```
   Derived from `AgentOutput.artifacts` (`ToolCall` + `stream_run_summary`) — no re-parsing of stdout.

2. **Unified pipeline-var binding across all three contexts.** Extract the prehook builder's var-binding loop (skip-truncated → JSON-array→`list<string>` → `i64 → f64 → bool → string`) into one shared helper `bind_pipeline_vars(&mut CelContext, &HashMap<String,String>)` in `core/src/prehook/context.rs`, and call it from **all three** builders before their built-in fields (so built-ins still take precedence on name collisions). This upgrades convergence (gains `list` support) and finalize (gains var support) to parity with prehook — the unifying groundwork.

3. **`ItemFinalizeContext` gains a `vars` bag.** Add `pub vars: std::collections::HashMap<String, String>` to `ItemFinalizeContext`, populated where the context is constructed (`StepExecutionAccumulator::to_finalize_context`) from the accumulator's pipeline vars — the same vars prehook/convergence already see. `build_finalize_cel_context` binds them via the shared helper. Finalize thus consumes any captured var, not only stream signals.

4. **Signal injection (the only stream-specific code).** At step-result application (`item_executor/apply.rs`), derive the signals from the run's `ToolCall` / `stream_run_summary` artifacts and write them into the accumulator's pipeline `vars` using the typed-var convention:
   - `tools_called` = JSON array string → `list<string>`
   - `tool_error_count`, `num_tool_calls`, `run_turns` = integer strings → `int`
   - `agent_reported_error` = `"true"`/`"false"` → `bool`
   - `run_cost_usd` = decimal string → `double`

   Because all three contexts now bind pipeline vars uniformly, these signals are visible in prehook, convergence, and finalize with no per-context stream-specific code.

Enables, for example:
- convergence: `'mark_done' in tools_called && tool_error_count == 0`
- finalize: `tool_error_count == 0 ? 'verified' : 'failed'`
- prehook budget gate: `run_cost_usd > 5.0` to skip further work

## Key Design And Tradeoffs

- **One unified var-binding mechanism for all three contexts (chosen).** Rather than special-casing each context, a single shared `bind_pipeline_vars` helper gives prehook, convergence, and finalize identical typed access to pipeline vars. This is slightly more than the minimal change (finalize gains a `vars` bag; convergence gains list support), but it removes the existing prehook/convergence asymmetry and means *any* future structured signal — not just these six — is a pipeline var available everywhere. The stream signals then need no per-context code at all. Chosen for the better long-term foundation.
- **Rejected: per-context typed fields.** Adding specific `tools_called`/`tool_error_count` fields only to `ItemFinalizeContext` would be a smaller diff but cements three divergent context shapes and forces a new field (and three edits) for every future signal.
- **Reserved variable names.** `tools_called`, `tool_error_count`, `num_tool_calls`, `agent_reported_error`, `run_cost_usd`, `run_turns` become reserved pipeline-var names. A user var of the same name would be overwritten by the injected signal. Documented; low collision risk given the prefixed/specific names.
- **Single source via artifacts.** Signals derive from the doc-102 artifacts already on the run, not a second stdout parse — one parse, one projection, consistent data in events and CEL.

## Risks And Mitigations

- Risk: injected reserved vars shadow a user's pipeline var of the same name.
  - Mitigation: document reserved names; choose specific names; (optional) log when an injection overwrites an existing var.
- Risk: convergence list support diverges from prehook (only prehook parses arrays today).
  - Mitigation: this doc explicitly aligns the convergence builder with the prehook branch and adds a test asserting `tools_called` is a `list` in both.
- Risk: finalize gains access to *all* pipeline vars (not just stream signals), a capability expansion.
  - Mitigation: built-in finalize fields are bound after vars so they still take precedence; existing finalize rules are unaffected (they only reference names that already resolved). `to_finalize_context` populates `vars` from the same accumulator pipeline vars prehook/convergence already expose.
- Risk: scope creep toward full nested tool-result access.
  - Mitigation: explicitly out of scope; revisit once a real consumer needs it.

## Observability

- No new events (doc 102 already emits `agent_tool_call` / `agent_run_summary`). When a prehook/finalize/convergence rule references a stream signal, the existing prehook/finalize decision logging captures the outcome and reason.
- Optional `tracing::debug` when signals are injected (names + counts) to aid authoring.

## Operations / Release

- Additive and detection-gated: only streaming runs produce artifacts, so only they inject signals / populate finalize fields. Existing workflows that never reference the new names are unaffected.
- Rollback: the signals are inert unless a CEL expression references them; reverting the injection + field additions restores prior behavior.

## Test Plan

- Unit: `stream_signal_vars` derivation from a representative artifact set (tool calls incl. one error, summary economics; MCP prefixes stripped to bare names).
- Unit: prehook CEL over injected vars — `'mark_done' in tools_called`, `tool_error_count == 0`, `run_cost_usd > 5.0`.
- Unit: convergence CEL `'mark_done' in tools_called` (verifies the new list branch) and a scalar guard.
- Unit: finalize CEL over injected vars (`tool_error_count == 0`).
- Regression: existing prehook/convergence/finalize tests unchanged; non-streaming runs inject nothing.

## QA Docs

- TBD — `docs/qa/orchestrator/<n>-cel-stream-run-signals.md`.

## Acceptance Criteria

- `cargo build` / `cargo test` pass; existing CEL tests and shipped workflows unaffected.
- A convergence guard and a finalize rule can each make a decision from stream signals (`tools_called` / `tool_error_count`), proven by tests.
- `tools_called` evaluates as a CEL `list<string>` in both prehook and convergence.
- Non-streaming runs expose none of the new signals (no behavior change).
