---
lifecycle: superseded
superseded_by: docs/design_doc/orchestrator/138-agent-driver-execution-migration.md
---

# Orchestrator - Stream-JSON Event Ingestion

**Module**: orchestrator
**Status**: Proposed (Decision Record)
**Related Plan**: Parse the streaming agent runner's `stream-json` output into structured records — projecting `tool_use`/`tool_result`/`result` into the `events` table and onto the run's `AgentOutput` — so tool I/O and run economics become first-class data instead of opaque stdout text
**Related QA**: TBD (to be generated when implementation begins)
**Created**: 2026-06-28
**Last Updated**: 2026-06-28

> **Post-release status (superseded execution seam, 2026-07-25):** this record
> describes the first stream-json ingestion increment behind the historical
> streaming runner. Current Agent execution uses per-Agent typed drivers, which
> normalize provider output into `driver_*` events and typed artifacts including
> `driver_terminal`; the global streaming runner and `RunnerExecutorKind` have
> been deleted by FR-126. References below to a streaming runner are historical,
> not current configuration guidance. See [DD-127](127-agent-driver-abstraction.md)
> and [DD-138](138-agent-driver-execution-migration.md).

> **Implementation status (2026-06-28):** landed. Added a tolerant stream-json parser (`core/src/stream_json.rs` → `StreamRun`/`StreamToolCall`), an additive `ArtifactKind::ToolCall` variant, a detection-gated branch in `validate_phase_output` that projects tool calls onto `AgentOutput.artifacts` plus a `stream_run_summary` artifact and `metrics.api_calls`, and event projection in `record_phase_results` emitting `agent_tool_call` / `agent_run_summary` / `stream_truncated` (with promoted `step`/`step_scope`). No schema or pipeline-control-flow change; non-streaming runs are unaffected (detection-gated). The projection is factored into a testable `project_stream_events`. Verified: workspace compiles, 437 scheduler tests pass (including a DB-backed integration test that runs `validate_phase_output` → `project_stream_events` → `insert_event` and reads back `agent_tool_call`/`agent_run_summary` rows, asserting the promoted `step` column), parser + validation unit tests green.

## Background

The first cut of the streaming agent runner (see
`docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`)
spawns `claude` in `stream-json` mode and captures the full event stream to the
run's stdout file. It deliberately did **not** parse that stream: validation
falls through the non-strict path (`core/src/output_validation.rs`), the run is
marked `passed`, and the structured events — `tool_use`, `tool_result`, the
terminal `result` — sit unparsed in the stdout file.

That is the opposite of the pivot's intent. The whole point of the structured
contract is that coordination and observability stop being text to grep. Today,
a streaming step's tool calls and its cost/turn economics are invisible to the
`events` table, to `task trace`, and to any downstream finalize/guard logic.

The plumbing to fix this already exists:

- Events are written as `DbEventRecord { task_id, task_item_id, event_type, payload_json }` (`core/src/task_repository/types.rs:78`). The `events` table promotes `step`, `step_scope`, and `cycle` out of the payload into columns for fast queries.
- `record_phase_results` (`crates/orchestrator-scheduler/src/scheduler/phase_runner/record.rs`) already assembles an `events` vec and persists it atomically with the command-run via `update_command_run_with_owned_events(run, events)`. It currently pushes only `output_validation_failed` and `sandbox_*` events.
- The run's structured payload is `AgentOutput` (`crates/orchestrator-collab/src/output.rs`) carrying `artifacts: Vec<Artifact>` and `metrics: ExecutionMetrics`. `Artifact` (`crates/orchestrator-collab/src/artifact.rs`) has a typed `kind: ArtifactKind` and an optional structured `content: serde_json::Value`.

So the increment is: parse the stream, carry the structured results on `AgentOutput`, and project events in `record_phase_results`. No new pipeline stage, no DB schema change.

## Goals

- Parse the `stream-json` event stream into a typed `StreamRun` summary: paired tool calls, assistant final text, and run economics (cost, turns, session id, error flag).
- Project per-tool-call events (`agent_tool_call`) and one run summary event (`agent_run_summary`) into the `events` table so they appear in `task trace`/`watch`.
- Attach tool calls to the run's `AgentOutput.artifacts` and populate `AgentOutput.metrics` from the `result` event.
- Keep everything additive: the shell path, existing event types, and DB schema are untouched.

## Non-goals

- Changing the streaming runner or per-agent routing (separate follow-ups).
- Moving coordination logic (CEL prehooks/finalize) onto tool calls — this increment only makes the data available; consuming it in control flow comes later.
- Strict-phase semantics or the single-JSON-blob validation path.
- Real tool implementations (the `run_tests` tool stays canned for now).

## Scope

- In scope: a `stream-json` parser module; detection of stream-json stdout; enrichment of `AgentOutput` (artifacts + metrics); event projection in `record_phase_results`; new `event_type` values; new `ArtifactKind` variant if required.
- Out of scope: changes to `events`/`command_runs` schema; the runner; finalize/guard consumption; incremental (streaming) parsing of unbounded output (addressed only as a truncation guard, see Risks).

## Interfaces / Data Changes

1. **New parser** `core/src/stream_json.rs`:
   ```rust
   pub struct StreamRun {
       pub detected: bool,            // stdout was a stream-json event stream
       pub result_text: Option<String>,
       pub is_error: bool,
       pub cost_usd: Option<f64>,
       pub num_turns: Option<u32>,
       pub session_id: Option<String>,
       pub tool_calls: Vec<StreamToolCall>,
       pub assistant_texts: Vec<String>,
   }
   pub struct StreamToolCall {
       pub name: String,                       // e.g. "mcp__orch__run_tests"
       pub input: serde_json::Value,
       pub result: Option<serde_json::Value>,  // paired by tool_use_id
       pub is_error: bool,
   }
   pub fn parse_stream_run(stdout: &str) -> StreamRun;
   ```
   Line-by-line: parse each line as JSON, match on `type` — `assistant`→collect `tool_use` (id→{name,input}) and `text`; `user`→`tool_result` (paired by `tool_use_id`); `result`→economics + final text. Unknown lines/types are ignored (tolerant).

2. **Detection**: `parse_stream_run` sets `detected = true` when the first non-empty line is a JSON object whose `type` is a known stream event (`system`/`assistant`/`result`). Validation branches on this rather than on phase name.

3. **`AgentOutput` enrichment** (in validation): when detected, map each `StreamToolCall` to an `Artifact { kind: ArtifactKind::ToolCall, content: { name, input, result, is_error } }`, and populate `metrics`/derived fields from the `result` event. `status = failed` iff `is_error`. New enum variant `ArtifactKind::ToolCall` (additive).

4. **Event projection** (in `record_phase_results`): from `validated.redacted_output`, push to the existing `events` vec:
   - `agent_tool_call` per tool call — payload `{ step, cycle, tool, input, result_summary, is_error }`.
   - `agent_run_summary` — payload `{ step, cycle, cost_usd, num_turns, session_id, num_tool_calls }`.
   `step` and `cycle` are included for column promotion (`cycle` threaded into `record_phase_results` if not already available).

No `events`/`command_runs`/migration changes: `payload_json` is free-form and only new `event_type` string values are introduced.

## Key Design And Tradeoffs

- **Parse in validation, project in record.** Validation is a pure function with no DB handle, so it parses and enriches `AgentOutput`; `record_phase_results` (which owns the DB writer and already batches events) projects events from that `AgentOutput`. Data flows entirely through `AgentOutput` — no new parameters threaded through the pipeline (except possibly `cycle` for promotion).
- **Artifacts AND events, not either/or.** Tool calls live on the run record as artifacts (durable, queryable per run) and are projected as events (timeline/observability). The two serve different consumers.
- **Auto-detect vs thread runner kind.** Auto-detection keeps the change local and also benefits today's SDLC stream-json agents. The alternative — threading `RunnerExecutorKind` into validation — is more explicit but wider; deferred. Detection false-positives are bounded (a non-streaming agent would have to emit a leading `{"type":"system",...}` line).

## Risks And Mitigations

- Risk: the validate stage reads stdout capped (~256KB); a long streaming step can truncate the terminal `result` line.
  - Mitigation: if `detected` but no `result` event is found, fall back to `exit_code`-based success and emit a `stream_truncated` event. Track full-stream sidecar capture as a follow-up (cross-ref doc 101 risk).
- Risk: `stream-json` schema drift across `claude` versions.
  - Mitigation: tolerant parsing (ignore unknown `type`s, best-effort field reads); a parser fixture test pinned to the validated CLI version.
- Risk: output redaction rewriting bytes inside a JSON line.
  - Mitigation: redaction replaces secret substrings with `[REDACTED]`, preserving JSON validity; parser tolerates a line that fails to parse by skipping it.
- Risk: very chatty streams inflate the `events` table.
  - Mitigation: project only paired tool calls + one summary (not raw `thinking_tokens`/`assistant` deltas); rely on existing `events` TTL/archival (doc 38).

## Observability

- New events: `agent_tool_call`, `agent_run_summary` (and `stream_truncated` on truncation) surface in `task trace`, `task watch`, and event stats.
- Run economics (`cost_usd`, `num_turns`) become queryable per step via the summary event payload and `AgentOutput.metrics`, enabling budget governance.
- Logs: `tracing::debug` for parse outcome (detected, #tool_calls, truncation).

## Operations / Release

- Fully additive; no migration. Existing non-streaming runs are unaffected (detection is false for single-JSON / plain-text stdout).
- Rollback: the projection and enrichment are guarded by `detected`; reverting the parser leaves the runner and pipeline working as in the first cut.

## Test Plan

- Unit: `parse_stream_run` against a recorded fixture — the captured e2e stream from doc 101's live run (asserts paired `mcp__orch__run_tests` call, `result_text`, `cost_usd`, `is_error=false`); truncated-stream and plain-text/non-detected cases.
- Unit: artifact mapping (`StreamToolCall` → `Artifact`) and event projection (`AgentOutput` → `DbEventRecord`s) including `step`/`cycle` promotion fields.
- Integration (follow-up): a scheduler-level test asserting the `events` table contains an `agent_tool_call` row with `tool = mcp__orch__run_tests` after a streaming step.

## QA Docs

- TBD — `docs/qa/orchestrator/<n>-stream-json-event-ingestion.md`.

## Acceptance Criteria

- `cargo build` / `cargo test` pass; existing tests and event types unchanged.
- After a streaming step, the run's `AgentOutput.artifacts` contains the tool call(s) and `events` contains `agent_tool_call` + `agent_run_summary` rows with promoted `step`/`cycle`.
- Run economics (`cost_usd`, `num_turns`, `session_id`) are recorded and queryable.
- Non-streaming runs produce identical output and events to before (detection-gated).
