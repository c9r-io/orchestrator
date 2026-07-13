# Orchestrator - Source Events And Slack Process Binding

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-099 provider-neutral ingestion, durable correlation, Slack pilot, audited actions, and Sources UI  
**Related QA**: `docs/qa/orchestrator/146-source-events-and-slack-binding.md`  
**Created**: 2026-07-14  
**Last Updated**: 2026-07-14

## Background

External conversations are not tasks. A Slack thread, webhook delivery, code-analysis result, or document update can start a process, add context to an existing process, answer an agent question, or explicitly branch. Directly firing a new task for every delivery loses provenance and creates duplicate work. Coupling process semantics to Slack would make the same control plane unusable for other sources.

## Goals

- Normalize authenticated external input into a provider-neutral event contract.
- Persist before routing so provider retries and daemon restarts are convergent.
- Bind external conversation coordinates to tasks without making the provider the system of record.
- Pilot Slack signature verification, thread routing, and closed interactive commands.
- Expose source routing, replay, and task provenance through gRPC, CLI, Tauri, and the desktop GUI.

## Non-goals

- Slack OAuth installation, transcript mirroring, or a general Slack client.
- Provider-specific workflow logic in the scheduler.
- Arbitrary commands derived from external text.
- Cross-organization identity federation.

## Scope

- In scope: migration 30, source repository/router, Trigger source-installation fields, Slack adapter, source RPC/CLI/Tauri surfaces, Sources page, TaskDetail bindings, command audit, fixtures, and automated QA.
- Out of scope: outbound provider notifications, attachment download, merge semantics, and a provider marketplace.

## UI Interactions

- The visible "来源" navigation entry and `Cmd+4` open the Sources page.
- The page filters by routing state, opens a resolved process, and exposes "重放" only to admins for `failed` or `needs_attention` events.
- TaskDetail displays a "来源绑定" panel with provider, installation, binding type, conversation, and thread coordinates.
- Source-created Attention items without a resolved task do not render a misleading timeline action.

## API

- `POST /source/slack/{project}/{trigger_name}` authenticates Slack `v0` signatures over the raw body, enforces a 256 KiB limit and timestamp tolerance, normalizes the event, persists it, and returns `accepted` or `deduplicated` before asynchronous routing.
- gRPC reads: `SourceEventList`, `SourceEventGet`, `SourceBindingList` (`read_only+`).
- gRPC mutations: `SourceEventIngest`, `SourceBind` (`operator+`), and `SourceReplay` (`admin`).
- CLI: `orchestrator source list|get|ingest|bindings|bind|replay`.
- The generic `/webhook/...` trigger endpoint remains synchronous and backward compatible; source adapters use the new durable path.

`NormalizedSourceEvent` contains provider/installation identity, a stable external event ID, a closed event kind, actor and conversation references, a bounded text summary, optional artifact references, and an optional `SourceCommand`. The command enum is limited to approve, reject, retry, add-context, cancel, branch, and open-console operations.

## Database Changes

Migration 30 creates:

- `source_events`: normalized payload, authenticated hash, routing state/attempts, stale-claim recovery timestamp, resolved task, stable error code, and unique `(provider, installation_id, external_event_id)`.
- `source_bindings`: task correlation with null-safe `correlation_key`, binding type, creator event, and unique provider/install/key/type.
- `source_routing_attempts`: append-only attempt result and error history.
- `source_command_actions`: actor, resolved role, target, action, request hash, idempotency key, and terminal result.

Foreign keys preserve task/event provenance. Migration is additive; existing trigger and task data require no rewrite.

## Key Design

1. Adapters authenticate, bound, normalize, and persist. The background router performs every process mutation after durable acceptance.
2. Source event identity and deterministic task IDs make replay safe across provider retries and daemon restarts. A routing lease older than five minutes is reclaimable.
3. Exactly one Trigger must match provider and installation. Trigger action, workspace, workflow, concurrency policy, and suspension remain authoritative.
4. A bound ordinary event emits `source_context_added` on the existing task. A top-level unbound event creates one task and primary binding. Multiple or missing reply correlations create an Attention item instead of guessing.
5. External commands resolve actor roles from Trigger configuration and fail closed to `read_only`. Attention approve/reject/retry call the same allowlisted scheduler service used by the normal control plane.
6. Slack action tokens are signed, expiring, and bind `attention_item_id`, expected version, and action. Slack signatures use constant-time HMAC verification over the unmodified body.

Major code touchpoints are `core/src/source.rs`, `crates/daemon/src/source_router.rs`, `crates/daemon/src/webhook.rs`, `crates/daemon/src/server/source.rs`, `crates/cli/src/commands/source.rs`, and `gui/src/pages/Sources.tsx`.

## Alternatives And Tradeoffs

- Directly fire a task in the HTTP handler: lower latency, but a crash between task creation and acknowledgement duplicates work and loses replay state.
- Treat Slack threads as tasks: simple for one provider, but prevents multiple bindings, artifact sources, and provider-neutral routing.
- Accept arbitrary textual commands: flexible, but creates a privilege-escalation and prompt-injection boundary. A closed enum is intentionally less expressive.
- Automatically pick one binding when correlation is ambiguous: convenient, but can mutate the wrong process. Attention is slower and safe.

## Risks And Mitigations

- Provider retries create duplicate side effects.
  - Unique event keys, deterministic task IDs, action idempotency, and binding uniqueness converge retries.
- Forged or replayed Slack requests invoke actions.
  - Raw-body signatures, timestamp tolerance, signed short-lived action tokens, role mapping, and existing action allowlists all remain required.
- External text injects instructions or leaks secrets.
  - Only bounded normalized summaries enter process context; no raw body, shell command, manifest, secret, or execution profile is derived from text.
- Routing crashes leave work stranded.
  - The router reclaims stale routing leases and stores every attempt with a stable failure code.
- Source data grows indefinitely.
  - Operators can query terminal state and routing history for lifecycle policy. Binding/audit rows intentionally preserve referenced provenance; future retention must delete only unreferenced terminal events.

## Observability

- Logs record provider, hashed installation/external IDs, source event ID, routing state, task ID, and stable errors; message bodies and signing secrets are excluded.
- Durable operational counters are derivable from `source_events`, `source_bindings`, `source_routing_attempts`, and `source_command_actions`; `source list --state failed` is the dead-letter view.
- Each command audit records authenticated actor, locally resolved role, target, action, status, result, and error code.
- No distributed tracing backend is introduced. Source event ID and task ID are the correlation attributes for existing structured logs and task events.

## Operations / Release

- Set `RuntimePolicy.spec.source_ingest_enabled: true` per project before accepting source events.
- Configure each source installation on one Trigger with `provider` and `installationId`; Slack additionally requires a SecretStore reference, optional `actorRoles`, and `timestampToleranceSecs` in `1..=900`.
- Suspend a Trigger for an installation-level stop; disable `source_ingest_enabled` for a project-wide stop. Existing bindings remain readable.
- Rollback: disable ingestion, suspend source Triggers, stop the router with the daemon, and deploy the previous binary. Migration 30 is additive and may remain; do not drop provenance tables during rollback.
- Compatibility: existing generic webhook behavior and trigger manifests without source fields are unchanged.

## Test Plan

- Unit: repository deduplication, payload mismatch, bindings, routing state, deterministic IDs, migration schema, Slack signatures/timestamps/normalization/action tokens, and project-scoped Trigger apply.
- Integration: router task creation, same-thread routing, ambiguity Attention, unknown-role command audit, and shared Attention action service.
- E2E: `scripts/qa/test-source-events-slack.sh` runs an isolated daemon and verifies a non-Slack fixture plus Slack retry, thread, auth, role, and size boundaries.
- UI: Tauri/Rust checks and React production build; manual navigation and admin replay are specified in QA-146.

## QA Docs

- `docs/qa/orchestrator/146-source-events-and-slack-binding.md`

## Acceptance Criteria

- Identical provider retries create no second source row, task, binding, or action.
- Bound thread messages resolve to the existing process by default.
- Configured top-level messages create exactly one task through canonical Trigger semantics.
- Ambiguous routing creates Attention and mutates no arbitrary process.
- Invalid signature, stale timestamp, oversized body, and unknown privileged actor paths fail closed.
- Slack approve/retry use the same audited allowlisted action service as other clients.
- A non-Slack fixture uses the same repository, router, binding, and public source interfaces.
