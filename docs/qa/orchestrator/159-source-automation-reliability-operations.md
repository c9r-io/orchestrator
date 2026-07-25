---
lifecycle: active
related_fr: FR-111
self_referential_safe: true
---

# Orchestrator - Source Automation Reliability And Operations

**Module**: Orchestrator
**Scope**: Route leases/retries, Attention, operations API/CLI, simulation, suspension, metrics, retention, and regression
**Scenarios**: 5
**Priority**: Critical

## Automated Entry Point

```bash
cargo build -p orchestratord -p orchestrator-cli
./scripts/qa/test-source-automation-operations.sh
```

The script uses unit/provider fixtures and the isolated signed-Slack vertical flow. It must not contact a real Slack workspace or start a paid coding agent.

## Safe State Reference

`matched → resolving → rendered → creating → routed` is the successful path. Transient failures release the lease to `retrying`; operator-fixable or exhausted failures become `needs_attention`; policy pause becomes `suspended`; deliberate closure is `ignored`; stable non-actionable security/contract failure is `failed`.

Operational list/get/watch output may contain route/source/task IDs, provider, reaction, binding/template name and revision/hash, safe state/error family, generation/version, attempt budget, due/lease-expiry time, and audit request ID. It must not contain message coordinates/body, permalink, rendered goal, installation identity, lease token, or credential value.

## Scenario 1: Retry, Attempt Exhaustion, And Restart Recovery

### Steps

1. Run the deterministic retry repository and Slack adapter tests.
2. Exercise timeout, HTTP 5xx, provider transient error, and 429 with `Retry-After` above and below the local backoff.
3. Advance the fake clock before and after `next_attempt_at`.
4. Continue until the route reaches its attempt budget.

### Expected

- Early claims return no work; due claims use a new lease token.
- Delay is deterministic, exponentially bounded, jittered, and never below capped `Retry-After`.
- Each released attempt stores only stable code/family/hint.
- Recovery converges to one task; exhaustion creates one `needs_attention` route and one logical Attention item.

### Restart And Stale Lease Recovery

### Steps

1. Claim a route and simulate daemon loss before completing the leased transition.
2. Claim before lease expiry, then after expiry with a different worker owner.
3. Repeat around the `creating` boundary and run the signed Slack duplicate-delivery/restart QA.

### Expected

- The pre-expiry claim is rejected; the post-expiry claim closes the prior attempt as `route_lease_expired` and issues a different fence.
- The pinned generation, automation key, request ID, and deterministic task ID survive restart.
- Canonical audit/task idempotency yields one route, one task, one automation binding, and one successful create audit.

## Scenario 2: Attention And Governed Recovery

### Steps

1. Produce missing/rejected credential, forbidden/missing Slack message, missing template, binding ambiguity, and exhausted transient retry.
2. Repeat the same route failure.
3. Replay successfully, then fail the same route again.
4. Exercise an intentionally unbound badge/channel.

### Expected

- Actionable failures create/reopen one item keyed by the stable automation identity; occurrence/version increments without duplicate inbox rows.
- Attention contains safe route/binding/source references and replay/ignore action descriptors, not Slack content or URL.
- Successful replay and deliberate ignore resolve it; a later failure reopens it.
- Intentional no-match is `ignored` and silent.

## Scenario 3: Query, Watch, Simulation, And Generation Adoption

### Steps

1. Create routes in multiple projects/states/providers/bindings.
2. Query `source automation list` with each filter and a page size smaller than the result set; reuse `next_page_token`.
3. Query `source automation get <route-id> --attempt-limit 2`.
4. Start `source automation watch --after <cursor>`, create transitions, disconnect, and resume from the last cursor.

### Expected

- Keyset pages contain no duplicate/omitted route at the page boundary; malformed/oversized token is rejected.
- Get bounds the newest attempt rows.
- Watch cursors increase strictly, replay from a cursor returns only later changes, and client disconnect ends server work.
- JSON/YAML/table projections satisfy the safe-field reference above.

### Simulation And Generation Adoption

### Steps

1. Run `source automation simulate` with the same safe provider/installation/reaction/channel/actor/URL evidence used by a live fixture.
2. Compare selected binding revision and rendered task plan.
3. Change the template while a route is blocked; replay once without and once with `--adopt-current-config`.
4. Change policy so a different binding or unauthorized actor would win.

### Expected

- Simulation and live routing call the same matcher/renderer and agree for identical input.
- Simulation reports no mutation/network work and creates no route, Attention, audit, or task.
- Default replay keeps the pinned generation; explicit adoption appends a generation only after the same binding remains authorized.
- Cross-binding or unauthorized adoption fails closed.

## Scenario 4: Governed Controls, Metrics, And Retention

### Steps

1. Call replay/ignore without reason, expected version, idempotency, or Operator authority; then call with valid evidence.
2. Repeat a valid mutation with the same idempotency key and with a stale expected version.
3. Suspend/resume a SourceTaskBinding and its Slack Trigger while routes are pending and while one lease is active.

### Expected

- Missing authority/context is denied; duplicate idempotency is safe; stale version aborts.
- Generic `source replay` rejects an automation-linked event.
- Suspend immediately projects matching unleased work to the exact scope; active work observes suspension before later provider/task work.
- Resume requeues only that scope and preserves attempt/generation history.

### Metrics, Status, Retention, And Redaction

### Steps

1. Create accepted, matched, resolved, retried, routed, Attention, and failed route evidence.
2. Query `source automation status` and Process metrics.
3. Search serialized metrics, CLI output, audits, changes, attempts, and logs for fixture installation/channel/binding/template/token/URL/body values.
4. Age terminal attempt/change/permalink data beyond retention and run cleanup.

### Expected

- Counts, oldest age, active lease, retry and failure-family status match authoritative tables.
- All seven source automation metrics exist with allowlisted low-cardinality labels only.
- Metric/projector failure does not block routing.
- Cleanup removes bounded detail and expires permalink while retaining route/task/audit/generation provenance and a monotonic change.

## Scenario 5: Compatibility And Security Regression

### Steps

Run migration, source router, Attention, action audit, process metrics, Slack adapter, CLI, signed Slack routing, frontend tests/build, Clippy, and documentation lint.

### Expected

- Migration 34 upgrades populated v26/v30/v33 databases without entity loss and is idempotent.
- FR-110 state assertions use `routed`; source/template/binding and Process Console behavior remain green.
- No GUI feature is added by FR-111; existing Sources rendering accepts the normalized state.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Retry, exhaustion, restart | PASS | 2026-07-17 | Codex | Backoff, provider hints, lease recovery, canonical convergence |
| 2 | Attention and governed recovery | PASS | 2026-07-17 | Codex | Dedupe, resolve/reopen, replay/ignore/version/idempotency |
| 3 | Query, watch, simulation, generation | PASS | 2026-07-17 | Codex | Cursor/filter contracts and shared matcher/renderer |
| 4 | Suspension, metrics, retention | PASS | 2026-07-17 | Codex | Scope projection and authoritative low-cardinality evidence |
| 5 | Compatibility and security | PASS | 2026-07-17 | Codex | Migration/router/Slack/CLI/FR-110 and redaction gates |
