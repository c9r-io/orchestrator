---
lifecycle: active
related_fr: FR-111
---

# Orchestrator - Source Automation Reliability And Operations

**Module**: Orchestrator
**Status**: Approved
**Related Plan**: FR-111 source automation reliability, policy, and operations
**Related QA**: `docs/qa/orchestrator/159-source-automation-reliability-operations.md`
**Created**: 2026-07-17
**Last Updated**: 2026-07-17

## Background

FR-110 established one durable Slack badge-to-task route. FR-111 turns that route into an operable process: provider failures are retried without a new Slack delivery, daemon restarts reclaim bounded leases, actionable failures enter Attention once, and operators can inspect or deliberately replay/ignore a route without editing SQLite.

This design deliberately keeps the route worker independent from source delivery acknowledgement. The authenticated source event only matches policy and reserves identity; outbound Slack access and task mutation occur later from the durable automation queue.

## Goals And Non-goals

Goals:

- bounded, restart-safe leases and retries with one canonical task identity;
- stable state/error semantics and immutable policy generations;
- actionable Attention with deduplication and automatic resolution;
- safe list/get/watch/simulate/replay/ignore/status operations;
- immediate binding/installation suspension projection;
- privacy-safe metrics and metadata retention.

GUI work, Slack status replies, reaction-removal cancellation, and distributed multi-region claims remain out of scope.

## Durable State Model

Migration 34 extends `source_automation_routes` and adds three append-oriented tables:

| Storage | Purpose |
|---|---|
| `source_automation_routes` | Current state, optimistic version, retry budget, due time, lease fence, task/result, and active generation |
| `source_automation_route_generations` | Immutable binding/template snapshots, credential reference, request ID, and deterministic task ID per adopted configuration generation |
| `source_automation_route_attempts` | Bounded execution history with stable result/error family and provider retry hint |
| `source_automation_route_changes` | Monotonic reconnect cursor for watch clients |

The route state machine is:

| State | Meaning | Normal next states |
|---|---|---|
| `matched` | Identity and generation reserved | `resolving`, `suspended` |
| `resolving` | A lease owns provider permalink resolution | `rendered`, `retrying`, `needs_attention`, `failed`, `suspended` |
| `rendered` | Permalink is validated and rendering can resume | `creating`, `retrying`, `needs_attention`, `suspended` |
| `creating` | Canonical audit/task mutation boundary reached | `routed`, `retrying`, `needs_attention`, `failed` |
| `retrying` | Lease released until `next_attempt_at` | `resolving`, `rendered`, `suspended` |
| `suspended` | Binding or installation policy paused work | `matched` or `rendered` on matching resume |
| `needs_attention` | Operator-fixable or retry-exhausted | replay to `matched`/`rendered`, or `ignored` |
| `routed` | Canonical task completed successfully | terminal |
| `ignored` | Operator or policy deliberately closed work | terminal |
| `failed` | Stable non-actionable invariant/security failure | optional governed replay, or terminal |

`received` remains a `source_events.routing_state`, not an automation-route state. Historical migration maps `reserved` to `matched`, `rendering` to `rendered`, and `completed` to `routed`.

## Claim, Lease, Retry, And Restart Rules

`claim_due` is one SQLite transaction. It selects due nonterminal routes, closes an expired open attempt with `route_lease_expired`, increments the current retry-budget counter/version, issues a UUID fencing token, appends an attempt/change, and returns only the claimed projection.

Core invariants:

1. One unexpired lease owns a route.
2. At most one route per installation is returned in one worker batch, and an existing unexpired installation lease blocks another claim.
3. Every leased transition compares the opaque lease token.
4. A crash in `creating` reuses the same canonical audit request and deterministic task ID.
5. Replay resets the current attempt budget but preserves historical attempt numbers and rows.

Retry delay is bounded exponential backoff with deterministic per-route jitter. Slack `Retry-After` is a lower bound after the adapter caps it at 300 seconds. Timeout, transport, 429, HTTP 5xx, and provider transient errors retry until the five-attempt budget is exhausted; exhaustion becomes `needs_attention`.

## Failure And Attention Policy

| Family | Examples | Outcome |
|---|---|---|
| Intentional no-match | routing disabled, unbound badge/channel, disallowed actor | source event `ignored`; no Attention |
| Credential | missing/rejected Slack credential | `needs_attention` |
| Visibility | message missing, channel inaccessible, missing scope | `needs_attention` |
| Configuration | ambiguous binding, missing template/credential reference, render failure | Attention; route-specific state when reservation exists |
| Transient exhausted | rate limit, timeout, transport/provider unavailable | `needs_attention` |
| Orphaned reservation | source event or canonical audit result missing | `needs_attention` |
| Security/provider contract | invalid host/path, redirect, malformed response | `failed` |

Route Attention uses `source-automation:{automation_key}` as its active dedupe key and stores only route, binding, task, and source-event references. Repeated failures increment occurrence/version instead of creating inbox noise. Successful routing and deliberate ignore resolve the item; a later failure reopens the same logical condition.

## Pinned Generations And Governed Replay

Normal retries use the generation captured at match time. Credential values are always resolved fresh from the pinned SecretStore/key reference, so rotation does not silently change binding/template meaning.

`source automation replay` requires Operator authority, a non-empty reason, positive expected route version, and idempotency key. By default it replays the pinned generation. `--adopt-current-config` first runs the current matcher with the authenticated source evidence, requires that policy still selects the same stable binding, then appends a new immutable generation. Cross-binding reroute is denied. The automation key and deterministic task ID never change.

Generic `source replay` rejects events linked to an automation route so operators cannot bypass generation/version fences.

## Operations API And CLI

| CLI | RPC | Authority | Result |
|---|---|---|---|
| `source automation list` | `SourceAutomationList` | ReadOnly | Filtered keyset page, no permalink/message coordinates |
| `source automation get` | `SourceAutomationGet` | ReadOnly | Safe route plus bounded attempt history |
| `source automation watch` | `SourceAutomationWatch` | ReadOnly | Monotonic deltas after a reconnect cursor |
| `source automation simulate` | `SourceAutomationSimulate` | ReadOnly | Exact matcher/renderer result; `mutation_performed=false`, `network_performed=false` |
| `source automation status` | `SourceAutomationStatusGet` | ReadOnly | Backlog, oldest age, leases, retries, Attention, failure families |
| `source automation replay` | `SourceAutomationReplay` | Operator | Audited optimistic replay, optional generation adoption |
| `source automation ignore` | `SourceAutomationIgnore` | Operator | Audited terminal ignore and Attention resolution |

The pre-existing Operator-only `source route` remains the explicit protected permalink view. Operational APIs never return permalink, message body, rendered goal, installation/message identity, lease token, or credential value. Watch polling is bounded to 200 changes, uses a 64-item channel, stops on disconnect, and accepts a reconnect cursor.

## Suspension Semantics

Binding suspend/resume and Trigger suspend/resume remain canonical audited configuration mutations. They also project to matching unleased routes:

- suspend moves pending states to `suspended` with `binding:{name}` or `installation:{id}` scope;
- an active lease finishes one bounded transition, then the worker observes active suspension before provider/task work;
- resume requeues only routes suspended by the exact scope, preserving route/generation/attempt history;
- terminal routes and existing tasks are unchanged.

## Metrics, Health, And Privacy

Process metrics derive from authoritative source/route/attempt/change tables:

- `source_reaction_received_total`;
- `source_binding_match_total`;
- `source_permalink_resolution_total`;
- `source_task_render_total`;
- `source_task_creation_total`;
- `source_route_retry_total`;
- `source_route_latency_seconds`.

Labels are limited to `slack`, `fixture`, or `other`, closed result values, and normalized low-cardinality error families. Installation, channel, message, binding, template, task, request, URL, goal, and credential identifiers are omitted. The daemon records `source_automation` projector health best-effort; metric failure never gates routing.

## Retention And Rollback

The daemon reuses the configured event-retention window. For old terminal routes it deletes bounded attempt/change detail and expires the protected permalink while preserving route/task/audit/generation provenance. Permalink expiry appends a route change. RFC3339 timestamps are normalized through SQLite `datetime()` for retention comparisons.

Migration 34 is additive and forward-only. Operational rollback is to suspend the binding/Trigger or set `reactionRouting: disabled`; existing routes, tasks, audits, and Attention remain inspectable. A binary rollback must retain a schema-34-capable database because old route-state spelling is not restored.

## Verification

Automated coverage includes migration upgrades, retry/Retry-After, lease expiry, installation occupancy, replay/ignore optimistic versions, suspension, simulation equivalence, Attention dedupe/resolve/reopen, Slack error classification, duplicate-task convergence, privacy-safe metrics, CLI parsing, and the FR-110 signed Slack vertical flow.

Run the consolidated gate with:

```bash
./scripts/qa/test-source-automation-operations.sh
```
