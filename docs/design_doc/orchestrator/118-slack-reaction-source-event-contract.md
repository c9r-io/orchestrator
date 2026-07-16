# Orchestrator - Slack Reaction Source Event Contract

**Module**: Orchestrator
**Status**: Approved
**Related Plan**: FR-107 provider-neutral reaction contract, Slack normalization, fail-safe routing gate, and bounded operator projection
**Related QA**: `docs/qa/orchestrator/155-slack-reaction-source-event-contract.md`
**Created**: 2026-07-17
**Last Updated**: 2026-07-17

## Background

Slack `reaction_added` deliveries use a different shape from message events: the actor is `event.user`, the reaction name is `event.reaction`, and the target coordinates are under `event.item`. Treating that payload as a generic system event loses the badge and target identity required by later source-automation work. Routing it through the existing fixed Trigger action would be worse because a badge could immediately create or mutate a task before bindings, templates, and permalink resolution exist.

FR-107 establishes the durable input contract only. It makes reaction events authenticated, typed, queryable, and visible while deliberately keeping them non-mutating until later roadmap slices install an explicit reaction router.

## Goals

- Define a provider-neutral “actor added a reaction to an external artifact” value object.
- Normalize signed Slack `reaction_added` message, file, and file-comment targets without a Slack API call.
- Preserve delivery deduplication, persist-before-ack behavior, timestamp precision, and old normalized JSON compatibility.
- Prevent reaction events from invoking a fixed Trigger action or appending to an existing thread-bound task.
- Expose only bounded reaction provenance through CLI, Tauri, and the Sources UI.

## Non-goals

- Resolve Slack permalinks or fetch message bodies, attachments, files, or transcripts.
- Match badges to templates, render Skill invocations, or create tasks.
- Implement `reaction_removed` cancellation behavior.
- Introduce outbound Slack credentials or network calls.
- Implement another provider adapter; the core contract remains reusable by one.

## Scope

- In scope: `SourceEventKind::ReactionAdded`, `SourceReactionRef`, Slack event normalization and stable validation errors, a non-mutating router gate, public projections, UI presentation, unit tests, and isolated webhook QA.
- Out of scope: new CRDs, database tables, task identity rules, permalink caching, retry policies for Slack Web API, and automation management screens.

## UI Interactions

- The existing visible “来源” / Sources entry and `Cmd/Ctrl+4` remain the entry point.
- Each card now shows `event_type`. A reaction card additionally shows `:{reaction_name}:` and `{target_kind} / {target_external_id}`.
- An ignored reaction has neither “打开进程” nor “重放” because it has no task and is not an actionable routing failure.

## API

- `POST /source/slack/{project}/{trigger_name}` accepts signed Slack `event_callback` payloads and normalizes `reaction_added` before durable acknowledgement.
- gRPC `SourceEventList` and `SourceEventGet` continue returning `event_type` and normalized JSON; no protobuf schema change is required.
- CLI table output adds `TYPE`; JSON/YAML source reads expose the additive `normalized.reaction` object.
- Tauri `source_event_list` projects `reaction_name`, `reaction_target_kind`, and `reaction_target_id` for the GUI. It does not project a target URL or message body.

Stable normalization errors include:

- `slack_reaction_missing_actor`
- `slack_reaction_missing_name`
- `slack_reaction_invalid_name`
- `slack_reaction_missing_item`
- `slack_reaction_missing_target_type`
- `slack_reaction_missing_message_channel`
- `slack_reaction_missing_message_ts`
- `slack_reaction_invalid_message_ts`
- `slack_reaction_missing_file_id`
- `slack_reaction_missing_file_comment_id`
- `slack_reaction_unsupported_target`
- `slack_reaction_missing_event_ts`
- `slack_reaction_invalid_event_ts`

## Database Changes

No migration is required. Existing `source_events.event_type` stores `reaction_added`, and `normalized_payload_json` stores the additive reaction descriptor:

```json
{
  "kind": "reaction_added",
  "reaction": {
    "name": "agent_fix",
    "target": {
      "kind": "message",
      "external_id": "C123:1712345678.000100",
      "url": null
    }
  }
}
```

`reaction` is optional with a serde default, so populated rows written before FR-107 continue to deserialize. The existing unique delivery identity `(provider, installation_id, external_event_id)` remains authoritative.

## Key Design

1. `SourceReactionRef` contains only a normalized name and `ExternalArtifactRef`; Slack envelope field names do not enter the core model.
2. Reaction names are 1–128 ASCII alphanumeric, `_`, `+`, or `-` characters and never include surrounding colons.
3. Slack message identity is `{channel}:{message_ts}`. The conversation projection retains the channel and message timestamp, but no body or permalink.
4. Slack event timestamps preserve up to nanosecond fractional precision when converted to RFC 3339.
5. The source router claims the event once and immediately ends it as `ignored`: message targets use `reaction_routing_not_enabled`; other targets use `unsupported_reaction_target`.
6. The guard runs before Trigger lookup and thread-binding lookup. Therefore it cannot create a task, create a binding, execute a command, or emit `source_context_added`.
7. Public UI fields are an allowlist projection. Malformed historical JSON produces empty reaction fields rather than a Tauri failure.

Major code touchpoints are `core/src/source.rs`, `crates/daemon/src/webhook.rs`, `crates/daemon/src/source_router.rs`, `crates/cli/src/commands/source.rs`, `crates/gui/src/commands/source.rs`, and `gui/src/pages/Sources.tsx`.

## Alternatives And Tradeoffs

- Reuse `ArtifactUpdated`: fewer enum variants, but loses the distinct reaction name and makes future exact badge matching ambiguous.
- Put Slack channel and timestamp directly on the core event: simple for Slack, but couples every provider to Slack coordinates.
- Leave reactions in `received` until FR-109: avoids an explicit ignored state, but creates an ever-growing queue and unclear operational ownership.
- Route through the existing fixed Trigger action: reuses current code, but violates the FR boundary and can start paid agent work from an unconfigured badge.
- Add searchable reaction columns now: faster SQL filtering, but requires a premature migration before matching/query requirements are stable.

The selected additive JSON contract plus an explicit ignored gate minimizes migration risk and provides an observable, non-mutating handoff to FR-109.

## Risks And Mitigations

- Risk: duplicate Slack deliveries create multiple route attempts.
  - Mitigation: existing source delivery uniqueness returns the original row; only the inserted event is claimed once.
- Risk: a reaction matches an existing Slack thread and mutates its task.
  - Mitigation: the reaction guard executes before binding correlation.
- Risk: Slack-controlled values leak bodies or create executable input.
  - Mitigation: no message fetch, body, URL, command, workflow, or Skill value exists in this slice; name and target fields are bounded.
- Risk: a new enum variant breaks populated databases.
  - Mitigation: reaction metadata is optional, old JSON fixtures are round-tripped, and no schema migration occurs.

## Observability

- `source_events.routing_state`, `routing_attempts`, and `last_error_code` provide durable route evidence.
- `orchestrator source list` includes the event type; `source get` shows the bounded normalized descriptor.
- Existing structured source identifiers remain the correlation mechanism. Message bodies, Slack secrets, and target URLs are not added to logs or metric labels.
- No distributed tracing span or new metric series is introduced. Later automation FRs can correlate from the durable source event ID.

## Operations / Release

- Config: use the existing Slack source Trigger, signing SecretStore, timestamp tolerance, and project `source_ingest_enabled` gate.
- Migration: none. Deploying the new binary is sufficient.
- Rollback: suspend the Slack Trigger or disable source ingestion, then deploy the prior binary. Existing reaction rows remain inert JSON records; the prior router cannot create tasks from them.
- Compatibility: message, command, interaction, URL verification, generic webhook, and non-Slack fixture paths remain unchanged.

## Test Plan

- Unit: provider-neutral serde/validation, old JSON compatibility, Slack payload normalization/error codes, timestamp precision, non-message targets, router non-mutation, duplicate attempts, and Tauri projection.
- Integration: `scripts/qa/test-slack-reaction-source.sh` runs an isolated daemon and verifies signed delivery, deduplication, public reads, stable rejection, and zero tasks/bindings.
- Regression: `scripts/qa/test-source-events-slack.sh`, full workspace tests, and clippy preserve FR-099 behavior.
- UI: Sources Vitest coverage, production build, and the existing Chromium Sources navigation/role scenarios.

## QA Docs

- `docs/qa/orchestrator/155-slack-reaction-source-event-contract.md`

## Acceptance Criteria

- A valid signed message reaction is stored as `reaction_added` with actor, normalized name, stable message target, and precise occurrence time.
- A duplicate Slack `event_id` creates neither another row nor another routing attempt.
- Missing/invalid fields, stale timestamps, invalid signatures, and oversized bodies fail closed with stable behavior.
- File and file-comment reactions remain queryable but cannot route.
- Reactions cannot invoke a fixed Trigger action or append to a bound task.
- CLI, Tauri, and Sources show bounded reaction provenance without message content, raw payload, secret, or target URL.
- Existing Slack message/command and non-Slack provider-neutral regressions pass.
