# FR-097: Handoff Briefing and Safe Resume

## 优先级: P1

## 状态: Proposed

## 依赖: FR-095, FR-096

## 计划闭环产物

- `docs/design_doc/orchestrator/107-handoff-and-safe-resume.md`
- `docs/qa/orchestrator/144-handoff-and-safe-resume.md`

## Background

The current task lifecycle supports pause, resume, retry, recover, step filtering, initial variables, workflow checkpoints, and persisted agent session IDs. These mechanisms are operationally distinct but are easy to collapse into an ambiguous “resume” button. Operators also lack a compact, reproducible briefing that explains what has happened and what is safe to do next.

## Goals

- Generate immutable, concise handoff snapshots for humans or replacement agents.
- Model safe logical resume boundaries independently from git rollback checkpoints.
- Keep three actions explicit: resume orchestration, retry from a step boundary, and attach/resume an agent session.
- Provide consequence previews and stale-state protection before mutation.
- Keep provider resume tokens opaque and behind runner-specific adapters.

## Non-goals

- Arbitrary rewind to any raw event.
- Automatic destructive git reset.
- Replaying external side effects without idempotency guarantees.
- Treating a handoff summary as the authoritative audit record.
- Supporting every runner provider in the first implementation.

## Scope

### In scope

- Handoff snapshot persistence and rendering.
- Logical resume-boundary discovery and validation.
- Resume planning and execution APIs.
- One initial provider adapter for existing streaming Claude sessions, with a generic interface.
- CLI, Tauri, and GUI previews and actions.

### Out of scope

- Live terminal attachment, which is FR-098.
- Cross-project handoff migration.
- Automatic context compaction inside provider-owned session storage.

## Interfaces and Data Changes

### Handoff snapshots

```sql
CREATE TABLE handoff_snapshots (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  task_item_id TEXT,
  step_id TEXT,
  session_id TEXT,
  checkpoint_id TEXT,
  source_event_cursor TEXT NOT NULL,
  projection_version INTEGER NOT NULL,
  briefing_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  generated_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);
```

The briefing contains goal, current state, completed work, failure cause, changed files, test evidence, constraints, human decisions, unresolved questions, recommended next actions, and references to sessions/checkpoints/evidence. It stores references and bounded redacted summaries rather than raw logs.

### Logical resume boundary

```rust
pub struct ResumeBoundary {
    pub id: String,
    pub task_id: String,
    pub cycle: u32,
    pub step_id: String,
    pub task_item_id: Option<String>,
    pub command_run_id: Option<String>,
    pub session_id: Option<String>,
    pub checkpoint_ref: Option<String>,
    pub side_effect_class: SideEffectClass,
    pub replay_safe: bool,
    pub reason: String,
    pub state_version: String,
}
```

`SideEffectClass` is `none`, `workspace_only`, `idempotent_external`, or `non_idempotent_external`. Resume is denied by default for unknown or non-idempotent external effects unless an explicit workflow policy and operator confirmation permit it.

### Resume modes

- `continue_task`: current pause/resume semantics.
- `retry_item`: current failed task-item retry semantics.
- `restart_from_boundary`: create a new queued execution using a validated step filter and captured variables/checkpoint.
- `resume_provider_session`: start a new runner process using an opaque provider resume token.
- `attach_live_session`: delegated to FR-098 and never implemented through this API.

### gRPC

```proto
rpc HandoffGenerate(HandoffGenerateRequest) returns (HandoffSnapshot);
rpc HandoffGet(HandoffGetRequest) returns (HandoffSnapshot);
rpc ResumeBoundaryList(ResumeBoundaryListRequest) returns (ResumeBoundaryListResponse);
rpc ResumePlan(ResumePlanRequest) returns (ResumePlanResponse);
rpc ResumeExecute(ResumeExecuteRequest) returns (ResumeExecuteResponse);
```

`ResumeExecute` requires the plan ID, expected task state version, operator reason, idempotency key, and explicit confirmation for elevated risk.

## Key Design

### Deterministic first, generated prose second

The daemon first builds a structured handoff from timeline, evidence, task state, diff metadata, and session/checkpoint references. An optional agent may compress this structure into prose, but the structured payload remains authoritative. If generation fails, the deterministic briefing is still usable.

### Immutable snapshots

A handoff records its source event cursor and content hash. New events do not silently rewrite it. Operators can generate a newer version and compare watermarks.

### Resume plan as a separate read step

Resume is a two-stage operation:

1. `ResumePlan` validates current state, boundary safety, expected re-execution, checkpoint availability, provider capability, and side effects.
2. `ResumeExecute` applies exactly that plan if its state version remains current.

The plan includes steps that will run, workspace/checkpoint effect, whether provider context is reused, and attention items expected to change.

### Provider-neutral session resume

```rust
#[async_trait]
pub trait RunnerSessionAdapter {
    async fn inspect_resume(&self, token: &OpaqueResumeToken) -> Result<ResumeCapability>;
    async fn prepare_command(&self, token: &OpaqueResumeToken, prompt: &str) -> Result<PreparedRun>;
}
```

The public data model never names Claude-specific flags. Provider tokens are encrypted or access-controlled when they can grant access to conversation history.

## Tradeoffs

- Immutable snapshots consume storage but make handoffs reproducible and auditable.
- Two-stage resume adds one interaction but prevents stale or surprising execution.
- Step-filter-based restart reuses current scheduling primitives but is not a true instruction-pointer rewind. The UI must describe it as re-execution from a boundary.
- Optional generated prose improves readability but cannot be the only handoff representation.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Resume repeats external side effects | Side-effect classification and fail-closed plan validation |
| Workspace changed after handoff | State version, git identity, checkpoint verification, and stale-plan rejection |
| Briefing leaks sensitive output | Structured extraction, central redaction, and role checks |
| Provider session no longer exists | Adapter capability probe and new-session fallback |
| “Resume” semantics remain confusing | Distinct API and UI verbs with consequence preview |
| Generated summary is inaccurate | Structured authoritative fields and linked evidence |

## Observability and Operations

- Metrics: handoff generation duration/result, snapshot size, resume plan result, resume execution result by mode, stale-plan rejection, and provider-resume fallback.
- Audit events: `handoff_generated`, `resume_planned`, `resume_rejected`, and `resume_executed`.
- Resume logs include identifiers and safety classification, never tokens or briefing bodies.
- Feature flags separately gate handoff generation and mutating resume modes.

## Testing and Acceptance

Detailed QA will be created at `docs/qa/orchestrator/144-handoff-and-safe-resume.md` after implementation is approved.

Acceptance criteria:

- [ ] A failed task produces a handoff containing the goal, last successful step, failure, test evidence, changed-file summary, and recommended actions.
- [ ] Regenerating against the same event cursor produces the same structured content hash.
- [ ] A stale resume plan is rejected without changing task or workspace state.
- [ ] A boundary with non-idempotent external effects is denied by default.
- [ ] Successful restart-from-boundary uses existing scheduler enqueue semantics and records parent/correlation context.
- [ ] Provider resume failure offers a clearly identified new-session fallback rather than silently dropping context.
- [ ] All actions emit auditable events and update related attention items only after execution state changes.
