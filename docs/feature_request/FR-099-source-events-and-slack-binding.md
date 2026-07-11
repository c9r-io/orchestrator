# FR-099: Source Events and Slack Process Binding

## 优先级: P1

## 状态: Proposed

## 依赖: FR-095, FR-096, FR-097

## 计划闭环产物

- `docs/design_doc/orchestrator/109-source-events-and-slack-binding.md`
- `docs/qa/orchestrator/146-source-events-and-slack-binding.md`

## Background

Triggers can create tasks from cron or events, but a product-facing integration needs durable provenance and correlation. Multiple Slack messages may refer to the same process, modify its goal, answer an agent question, approve an action, or intentionally branch into new work. Treating every message as a new task creates noise; treating Slack threads as the process model prevents reuse for GitHub, code analysis, documents, email, and generic webhooks.

## Goals

- Normalize external input into provider-neutral source events.
- Bind one or more external conversations or artifacts to a task/process.
- Make event ingestion idempotent and correlation deterministic.
- Pilot Slack with verified webhooks, thread-aware routing, and audited interactive actions.
- Surface source provenance in timelines, attention items, and the Sources UI.

## Non-goals

- Implementing a full Slack client or message archive.
- Using Slack as the system of record for process state.
- Automatically executing untrusted message instructions without workflow policy.
- Supporting every external provider in the first release.
- Building cross-organization identity federation.

## Scope

### In scope

- Source event/binding schema and service.
- Provider adapter interface, normalization, routing policies, and idempotency.
- Slack signature verification, replay protection, event normalization, thread binding, and interactive approve/retry/open-console actions.
- Timeline/attention provenance and Sources GUI.

### Out of scope

- Slack OAuth installation marketplace flow in the first pilot.
- Bidirectional transcript mirroring.
- Provider-specific workflow logic inside scheduler code.

## Interfaces and Data Changes

### Source events

```sql
CREATE TABLE source_events (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  installation_id TEXT NOT NULL,
  external_event_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  external_actor_id TEXT,
  conversation_id TEXT,
  thread_id TEXT,
  occurred_at TEXT NOT NULL,
  received_at TEXT NOT NULL,
  normalized_payload_json TEXT NOT NULL,
  raw_payload_ref TEXT,
  payload_hash TEXT NOT NULL,
  routing_state TEXT NOT NULL,
  UNIQUE(provider, installation_id, external_event_id)
);
```

### Bindings

```sql
CREATE TABLE source_bindings (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  installation_id TEXT NOT NULL,
  conversation_id TEXT,
  thread_id TEXT,
  binding_type TEXT NOT NULL,
  created_by_event_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(provider, installation_id, conversation_id, thread_id, binding_type)
);
```

`binding_type` initially supports `primary`, `related`, and `notification_target`.

### Normalized source event

```rust
pub struct NormalizedSourceEvent {
    pub provider: String,
    pub external_event_id: String,
    pub kind: SourceEventKind,
    pub actor: ExternalActorRef,
    pub conversation: Option<ConversationRef>,
    pub text_summary: Option<String>,
    pub command: Option<SourceCommand>,
    pub attachments: Vec<ExternalArtifactRef>,
    pub occurred_at: String,
}
```

`SourceCommand` is a closed provider-neutral enum such as `approve`, `reject`, `retry`, `add_context`, `cancel`, or `branch`. Adapters cannot submit arbitrary daemon commands.

### Routing defaults

- Existing bound thread + ordinary message -> append context/source timeline entry to the bound process.
- Existing bound thread + explicit command -> execute the corresponding audited attention/process action.
- New top-level message matching a configured trigger -> create a new task and primary binding.
- Explicit branch command -> create a child task with `parent_task_id` and source correlation.
- Ambiguous correlation -> create an attention item rather than guessing.

## Key Design

### Ingest, persist, route

Inbound adapters perform only authentication, normalization, size limits, and durable insertion. Routing runs after persistence so provider retries are safe and failed routing can be replayed. A routing cursor and state record `received`, `routed`, `ignored`, `needs_attention`, or `failed`.

### Slack pilot

- Verify Slack signing secret, timestamp tolerance, content type, and body hash before parsing.
- Acknowledge within provider deadlines after durable acceptance; route asynchronously.
- Use Slack event IDs for idempotency and interaction payload IDs plus action timestamps for commands.
- Resolve user identity through configured installation mapping; unknown actors receive no privileged default role.
- Responses contain concise status and a deep link; they do not mirror private logs or transcripts.

### Trigger integration

The adapter resolves an existing `Trigger` action or a provider-neutral routing policy. Workflow, workspace, concurrency policy, and project selection remain governed by orchestrator resources. Source bindings supplement, rather than bypass, the trigger engine.

### Security boundary

External text is untrusted input. It may update bounded context but cannot modify manifests, execution profiles, secrets, or action allowlists. Approval buttons use short-lived signed correlation tokens and still pass control-plane RBAC/action policy.

## Tradeoffs

- Persisting normalized source events adds storage but provides replay, provenance, and provider retry safety.
- Default same-thread correlation is understandable but not universally correct; explicit branch/merge and ambiguous-routing attention provide escape hatches.
- A provider-neutral command enum limits flexibility but prevents adapter-specific privilege escalation.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Provider retries create duplicate processes | Unique external event key and idempotent routing |
| Wrong thread correlation changes unrelated work | Explicit bindings, deterministic rules, and ambiguity attention |
| Forged Slack requests invoke actions | Signature verification, timestamp tolerance, signed action context, and RBAC |
| Prompt injection through source text | Treat as untrusted context; workflow/tool permission boundaries remain authoritative |
| Sensitive Slack content leaks | Redaction, bounded summaries, retention policy, and role-aware reads |
| Provider outage blocks workflows | Durable asynchronous routing and retry backoff |

## Observability and Operations

- Metrics: source events accepted, deduplicated, rejected, routing latency/result, binding count, ambiguous routing, and provider response errors.
- Logs contain provider, installation ID hash, external event ID hash, routing state, and task ID, but not message bodies.
- Audit events record source command actor, resolved role, target, action, and result.
- Dead-letter routing state is queryable and replayable by admin action.
- Feature flag and per-installation suspend allow immediate ingestion shutdown without deleting bindings.

## Testing and Acceptance

Detailed QA will be created at `docs/qa/orchestrator/146-source-events-and-slack-binding.md` after implementation is approved.

Acceptance criteria:

- [ ] Replaying an identical Slack event never creates a second source row, task, binding, or action.
- [ ] Messages in a bound thread append to the same process timeline by default.
- [ ] A new configured top-level message creates one task through the existing trigger/service path.
- [ ] Ambiguous routing creates an attention item and does not mutate an arbitrary process.
- [ ] Invalid signatures, stale timestamps, oversized bodies, and unknown privileged actors fail closed.
- [ ] Slack approve/retry actions invoke the same audited service functions as GUI/CLI actions.
- [ ] A non-Slack fixture can use the same source event and binding interfaces without Slack fields in core services.
