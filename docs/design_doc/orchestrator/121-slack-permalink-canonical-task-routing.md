# Orchestrator - Slack Permalink And Canonical Task Routing

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-110 Slack permalink resolution, durable automation identity, canonical task mutation, and role-aware source deep links  
**Related QA**: `docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md`  
**Created**: 2026-07-17  
**Last Updated**: 2026-07-17

## Background

FR-107 through FR-109 established authenticated reaction evidence, trusted SourceTaskTemplate rendering, and exact SourceTaskBinding selection. The remaining MVP gap was a real vertical effect: resolve the reacted Slack message to a stable permalink and create exactly one governed task, even when Slack retries delivery or the daemon restarts.

This slice crosses three trust boundaries: SecretStore-backed outbound credentials, provider-controlled HTTP responses, and paid task mutation. The daemon therefore freezes the selected policy before the provider call, reserves durable identity before mutation, validates the returned URL, and uses the existing task and action-audit services.

## Goals

- Resolve Slack message coordinates with `chat.getPermalink` through a bounded provider adapter.
- Create one canonical workflow/workspace task from the exact selected binding and template revision.
- Converge duplicate deliveries, worker retries, and restart recovery on one automation route and task ID.
- Preserve a queryable event → attempt → route → audit → task → binding provenance chain.
- Expose safe route summaries to readers and the permalink only through an Operator-authorized API.
- Keep the webhook acknowledgement path free of outbound Slack calls and task mutation.

## Non-goals

- Fetch or retain Slack message text, attachments, files, or thread replies.
- Implement general Slack OAuth installation management or a general Web API SDK.
- Add manual retry, backoff policy, route administration, or Attention policy; FR-111 owns those operations.
- Add template/binding management UI; FR-112 owns it.
- Start a task when its SourceTaskTemplate explicitly configures `action.start: false`.

## Scope

- In scope: outbound credential references, cross-resource validation, Slack permalink client, migration 33, durable route repository, canonical task/audit integration, automation SourceBinding, source projections, protected route RPC/CLI, and role-aware Sources/timeline links.
- Out of scope: message content ingestion, reaction-removal cancellation, provider replies, operator retries, route retention controls, and management forms.

## Trigger Credential Interface

Signing and outbound credentials are distinct references:

```yaml
spec:
  event:
    source: webhook
    webhook:
      provider: slack
      installationId: T012345
      reactionRouting: bindings
      secret:
        fromRef: slack-signing
      outboundCredential:
        fromRef: slack-api
        key: BOT_TOKEN
```

`reactionRouting: bindings` requires `outboundCredential`. The referenced SecretStore and exact key must exist in the same project. The active config resolves the token only inside the daemon immediately before the provider request, so a SecretStore rotation affects a pending/reclaimed route without changing its frozen routing policy. The route stores only the SecretStore name and key.

## Slack Provider Boundary

The adapter performs `GET chat.getPermalink` with `channel` and `message_ts` query parameters and a bearer token. It applies:

- 3-second connect and 8-second total timeouts;
- TLS verification and HTTPS-only production transport;
- redirects disabled;
- a 32 KiB streamed response bound;
- stable error classes for timeout, transport, HTTP rejection, rate limit, provider errors, invalid JSON, missing message, and rejected credentials;
- `Retry-After` parsing capped at 300 seconds;
- returned URL length, scheme, credential, Slack-host, `/archives/{channel}` validation.

Production uses `https://slack.com/api/`. `ORCHESTRATOR_SLACK_API_BASE_URL` accepts only loopback HTTP in debug or explicit `dev-insecure` builds, making deterministic provider fixtures possible without creating a production SSRF input.

## Durable Automation Identity

Migration 33 adds `source_automation_routes` and route links on `source_events` and `source_routing_attempts`. The unique automation key hashes:

```text
project + installation + message identity + reaction + binding name
```

Before any provider call, the router freezes:

- project/provider/installation/message/reaction coordinates;
- trusted resolved role;
- binding name, normalized revision, and serialized snapshot;
- template name, content hash, and serialized snapshot;
- credential store/key reference;
- deterministic request and task IDs.

The unique reservation is the mutation fence. A routed reservation is attached to later delivery rows. FR-111 replaces the original coarse reclaim behavior with bounded leases and retries while preserving the same frozen snapshots and deterministic IDs. Task insertion itself is idempotent by the requested task ID, closing the ambiguous crash window between insertion and route completion.

## Canonical Task And Audit Flow

1. The webhook adapter authenticates, normalizes, persists, and acknowledges the Slack event.
2. The asynchronous router claims the source row and reads one immutable config generation.
3. The exact matcher selects one binding; the router freezes binding/template snapshots and reserves automation identity.
4. The provider adapter resolves and validates the permalink with a freshly resolved token.
5. The daemon renders the frozen template with trusted variables only.
6. `source.automation.create_task` reserves a canonical action audit using IDs and hashes, never the token, permalink, or rendered goal.
7. The canonical task service creates the deterministic task and enqueues it only when the template requests `start: true`.
8. An `automation` SourceBinding, safe `source_automation_routed` event, audit result, route, and source attempt are completed with the same task/request identity.

Initial variables are bounded to source event ID, provider, reaction, permalink, template, and binding identity. Raw webhook payload, message content, and token are never task inputs.

## Public APIs And UI

`SourceEventList` and `SourceEventGet` remain read-only and add only safe fields: route ID/status, binding name, template name, and template hash. They never return the permalink.

`SourceAutomationRouteGet` requires Operator authority and returns the protected permalink plus route/audit/task provenance. The CLI exposes it explicitly:

```bash
orchestrator source route <source-event-id> -o json
```

The Sources page and selected timeline evidence fetch this protected API only for Operator/Admin roles. The resulting Slack link is a native keyboard-focusable anchor with `target="_blank"` and `rel="noreferrer"`. Read-only users see safe automation status and policy identity, but no hidden or focusable permalink action. Daemon authorization remains authoritative; UI role checks reduce unnecessary denied calls.

## Key Decisions And Tradeoffs

1. **Freeze policy, resolve credentials late**: retries preserve the reviewed task meaning while still using rotated credentials.
2. **One route per message/badge/binding, not delivery ID**: provider retries and distinct Slack delivery IDs converge without preventing a deliberately different binding from creating a different task.
3. **Deterministic canonical task ID plus route reservation**: two idempotency layers cover both concurrent reservation and post-insert crashes.
4. **Protected permalink value, safe read summary**: everyday source triage remains read-only while private workspace navigation requires Operator authority.
5. **No provider content fetch**: the goal contains only a validated Slack link and trusted configured Skill invocation.

## Risks And Mitigations

- Duplicate paid work: unique automation key, deterministic task ID, canonical audit idempotency, and restart reclaim.
- Token leakage: SecretStore-only resolution, no secret value in route/audit/event/proto, and bounded stable provider errors.
- Provider SSRF/redirect abuse: fixed production endpoint, no redirects, strict Slack permalink host/path validation, and test override limited to loopback development builds.
- Config drift during retry: binding/template snapshots and content hashes are captured before the outbound call.
- Private Slack URL exposure: read-only summaries omit it; explicit route retrieval is Operator-authorized.
- Webhook latency: acknowledgement follows durable ingest and never waits on Slack or task creation.

## Observability

- `source_events` and `source_routing_attempts` link to `automation_route_id`.
- `source_automation_routes` records safe state, stable error code, retry hint, frozen revisions, request ID, and task ID.
- `control_action_audit.action = source.automation.create_task` joins the route request ID to the canonical task result.
- `source_bindings.binding_type = automation` joins the task to Slack message coordinates.
- `source_automation_routed` provides a semantic timeline event containing reaction, binding/template IDs and hashes, route/request IDs, and status—never permalink or token.

FR-111 added bounded operational retry, lease recovery, safe replay, and Attention semantics on this durable state; see `122-source-automation-reliability-operations.md`.

## Operations And Rollback

Recommended rollout:

1. Apply or rotate the signing and outbound token SecretStore keys.
2. Apply the Trigger, template, and binding with `reactionRouting: disabled`.
3. Validate template preview and binding simulation.
4. Set `reactionRouting: bindings` and observe one test badge through Sources and `source route`.

Rollback sets `reactionRouting: disabled`. Existing tasks, route evidence, and protected links remain inspectable; new reactions stop before reservation/provider/task work. Migration 33 is additive and forward-only, so binary rollback must retain the upgraded database.

## Test Plan And Acceptance

- Unit: credential/reference validation, migration compatibility, durable uniqueness, provider success/error/timeout/redirect/rate-limit/URL policy, router task convergence, and role mapping.
- UI unit: read-only absence, Operator/Admin protected fetch, accessible link semantics in Sources and timeline evidence.
- Isolated E2E: real signed webhook bytes, loopback fake Slack API, canonical task/provenance queries, duplicate delivery across restart, and read-only denial.
- Regression: QA 146, 155, 156, and 157 plus full Rust workspace, clippy, frontend tests/build, and docs lint.

Executable acceptance is defined in `docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md` and `scripts/qa/test-slack-reaction-task-routing.sh`.
