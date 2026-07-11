# FR-096: Attention Inbox

## 优先级: P0

## 状态: Proposed

## 依赖: FR-095

## 计划闭环产物

- `docs/design_doc/orchestrator/106-attention-inbox.md`
- `docs/qa/orchestrator/143-attention-inbox.md`

## Background

Task failures, trace anomalies, sandbox denials, approval needs, stalled work, and agent questions can already be represented as events or structured outputs. A raw task list cannot distinguish autonomous work from work that requires a human decision. Trace escalation labels are useful detection hints, but they do not support ownership, deduplication, snoozing, resolution, or action audit.

The Attention Inbox is a materialized operational queue containing only human-actionable work.

## Goals

- Materialize actionable attention items from runtime and external events.
- Support open, claim, snooze, resolve, auto-resolve, and reopen lifecycles.
- Deduplicate repeated loop failures and escalation events.
- Attach safe, typed actions such as approve, reject, retry, resume, open session, or acknowledge.
- Provide cross-task filtering and a keyboard-first GUI default page.
- Preserve a complete audit trail of human decisions and action results.

## Non-goals

- Replacing task status or anomaly detection.
- Showing every warning or failed command in the Inbox.
- Implementing a general-purpose ticketing system with custom workflows.
- Storing arbitrary executable commands inside attention records.
- Sending Slack notifications in this request; FR-099 integrates notification sources later.

## Scope

### In scope

- Attention schema, repository, service, policy registry, projector, RPCs, CLI, Tauri bridge, and GUI.
- Action descriptors mapped to daemon-owned service methods.
- RBAC, optimistic concurrency, idempotency, deduplication, and audit events.
- Automatic closure when the originating condition clears.

### Out of scope

- Organization-wide on-call scheduling.
- Custom user-defined attention rules in the first release.
- Cross-control-plane federation.

## Interfaces and Data Changes

### Tables

```sql
CREATE TABLE attention_items (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  task_item_id TEXT,
  step_id TEXT,
  session_id TEXT,
  kind TEXT NOT NULL,
  severity TEXT NOT NULL,
  state TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  requested_decision_json TEXT,
  actions_json TEXT NOT NULL,
  dedupe_key TEXT NOT NULL,
  assignee TEXT,
  source_event_id TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  snoozed_until TEXT,
  resolved_at TEXT,
  resolution_json TEXT
);

CREATE UNIQUE INDEX idx_attention_open_dedupe
ON attention_items(project_id, dedupe_key)
WHERE state IN ('open', 'claimed', 'snoozed');
```

An `attention_actions` append-only table records attempts, actor, idempotency key, requested action, target version, result, error code, and timestamps.

### State model

```text
open -> claimed -> resolved
  |        |          |
  +-> snoozed --------+
  +-> resolved
resolved -> open       (condition reappears after resolution)
```

`dismissed` is intentionally not a primary state. Operators resolve with a reason such as `acknowledged`, `false_positive`, `superseded`, or `condition_cleared`, preserving meaning.

### Initial policy kinds

- `approval_required`
- `agent_question`
- `step_failed`
- `retry_exhausted`
- `policy_blocked`
- `sandbox_denied`
- `stalled`
- `budget_threshold`
- `low_confidence`
- `degenerate_loop`

Not every anomaly creates attention. The policy registry maps an event/trace condition to severity, dedupe key, requested decision schema, allowed actions, and auto-resolution condition.

### gRPC

```proto
rpc AttentionList(AttentionListRequest) returns (AttentionListResponse);
rpc AttentionGet(AttentionGetRequest) returns (AttentionGetResponse);
rpc AttentionClaim(AttentionClaimRequest) returns (AttentionMutationResponse);
rpc AttentionSnooze(AttentionSnoozeRequest) returns (AttentionMutationResponse);
rpc AttentionResolve(AttentionResolveRequest) returns (AttentionMutationResponse);
rpc AttentionExecuteAction(AttentionExecuteActionRequest) returns (AttentionMutationResponse);
rpc AttentionFollow(AttentionFollowRequest) returns (stream AttentionDelta);
```

Mutations require `expected_version` and `idempotency_key`. The actor comes from authenticated control-plane identity, never from an untrusted request field.

## Key Design

### Materialized operational state

Unlike timeline entries, attention items are persisted because human workflow changes their state. The event projector processes source events idempotently and upserts by the active dedupe key. Replay must converge on the same open set.

The initial projector runs synchronously after relevant event persistence or through a daemon-owned bounded queue. A periodic reconciliation job repairs missed projections and auto-resolves cleared conditions.

### Typed actions

`actions_json` stores descriptors, not shell commands:

```json
{
  "id": "retry_failed_item",
  "label": "Retry failed step",
  "required_role": "operator",
  "confirmation": "required",
  "input_schema": {"type":"object","properties":{"reason":{"type":"string"}}}
}
```

The daemon maps the action ID to an allowlisted service function. Action execution emits audit and timeline events and may resolve the item only after the target condition changes.

### Ordering

Default ordering is:

1. intervention severity before attention severity;
2. claimed-by-current-actor before unclaimed, then claimed-by-others;
3. SLA deadline;
4. creation time.

The Inbox never sorts ordinary running tasks above actionable items.

## Tradeoffs

- A dedicated table duplicates some event-derived information but is required for mutable human workflow.
- A built-in policy registry is less flexible than CEL rules but safer for a first release. Policy extension can follow once deduplication and action semantics are stable.
- Auto-resolution reduces noise but can surprise operators. The resolution remains visible in timeline/audit history.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| One failure loop floods the Inbox | Active dedupe index and condition-based upsert |
| Two operators execute conflicting actions | Optimistic versioning and action idempotency |
| Projector misses an event | Cursor persistence and periodic reconciliation |
| Untrusted action payload executes code | Action registry and strict input schema |
| Sensitive failure text appears in Inbox | Redacted structured summaries only |
| Resolved items reopen repeatedly | Reopen counters, cooldown policy, and escalation |

## Observability and Operations

- Metrics: open items by kind/severity, time to claim, time to resolution, reopen count, projection lag, dedupe count, and action outcomes.
- Audit events: `attention_opened`, `attention_claimed`, `attention_snoozed`, `attention_resolved`, `attention_reopened`, and `attention_action_executed`.
- Projector reconciliation reports cursor age and drift between expected and materialized open items.
- Feature flag `attention_inbox_enabled` gates new materialization; disabling it does not delete existing rows.

## Testing and Acceptance

Detailed QA will be created at `docs/qa/orchestrator/143-attention-inbox.md` after implementation is approved.

Acceptance criteria:

- [ ] Repeated identical failed-step events result in one open item with an updated occurrence count.
- [ ] Two concurrent claim or action requests cannot both succeed against the same version.
- [ ] Replaying all source events produces the same active Inbox.
- [ ] A successful retry or resumed step auto-resolves the originating item with an auditable reason.
- [ ] Read-only users cannot mutate items; operator/admin actions follow existing role policy.
- [ ] The GUI supports keyboard navigation, filters, claim, snooze, resolve, and timeline deep links.
- [ ] No raw secret, transcript body, or unredacted command output appears in list responses or metrics.
