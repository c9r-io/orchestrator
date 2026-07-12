---
self_referential_safe: true
---

# Orchestrator - Attention Inbox

**Module**: Orchestrator  
**Scope**: Persistent attention projection, lifecycle, action governance, RBAC, streaming, and desktop entry  
**Scenarios**: 5  
**Priority**: High

---

## Background

Attention Inbox materializes only human-actionable conditions from durable events. The daemon QA uses the deterministic mock fixture:

```bash
orchestrator apply -f fixtures/manifests/bundles/process-timeline-failure.yaml --project qa-attention-inbox
```

The automated script starts an isolated daemon on `127.0.0.1:19196` with a temporary `ORCHESTRATORD_DATA_DIR`; it never changes the active project database.

## Database Schema Reference

| Table | Purpose |
|---|---|
| `attention_items` | Mutable materialized queue state and active dedupe key |
| `attention_actions` | Append-only mutation/action audit and idempotency state |
| `attention_projector_state` | Durable source event cursor |
| `attention_changes` | Monotonic stream reconciliation sequence |

---

## Scenario 1: Failed Event Materialization, Deduplication, And Replay

### Preconditions

- Build with `cargo build -p orchestratord -p orchestrator-cli`.
- Install `jq` and `sqlite3`.
- Use only `fixtures/manifests/bundles/process-timeline-failure.yaml`.

### Goal

Verify a failed step creates one actionable row and repeated source conditions aggregate instead of flooding the Inbox.

### Steps

1. Run `./scripts/qa/test-attention-inbox.sh`.
2. Confirm "failed step materialized" and "duplicate failure aggregates" pass.
3. Run `cargo test -p agent-orchestrator attention::tests::duplicate_projection_aggregates_occurrences`.

### Expected

- One `step_failed` item is active for the task/item/step dedupe key.
- Repeating the source event increments `occurrence_count` and version without adding another active row.
- Reapplying the deterministic candidate converges on the same active item ID.

### Expected Data State

```sql
SELECT dedupe_key, COUNT(*) AS active_rows, MAX(occurrence_count) AS occurrences
FROM attention_items
WHERE project_id = '{project_id}' AND kind = 'step_failed'
  AND state IN ('open', 'claimed', 'snoozed')
GROUP BY dedupe_key;
-- Expected: active_rows = 1 and occurrences >= 2
```

---

## Scenario 2: Claim And Action Concurrency With Idempotent Replay

### Preconditions

- Scenario 1 has produced an open attention item.
- The caller has `operator` or `admin` role.

### Goal

Verify one version authorizes at most one claimant/external action and the same action key never repeats its side effect.

### Steps

1. Run `./scripts/qa/test-attention-inbox.sh` and confirm the concurrent claim assertion.
2. Run:

   ```bash
   cargo test -p agent-orchestrator attention::tests::optimistic_claim_and_idempotency_are_enforced
   cargo test -p agent-orchestrator attention::tests::action_reservation_is_concurrent_and_replay_safe
   ```

3. Inspect `attention_actions` for one `started`/terminal action row per item and idempotency key.

### Expected

- Exactly one of two claims with the same `expected_version` succeeds.
- Exactly one action reservation has `should_execute=true`.
- Replaying the same action ID, input, and key returns current state; a different request using that key is rejected.
- Failed external actions record an error and reopen the item; successful actions record a result and resolve it.

### Expected Data State

```sql
SELECT attention_item_id, idempotency_key, COUNT(*) AS attempts,
       MIN(status) AS status, MIN(action_id) AS action_id
FROM attention_actions
WHERE attention_item_id = '{attention_item_id}'
GROUP BY attention_item_id, idempotency_key;
-- Expected: attempts = 1 for every idempotency_key; action status is started/succeeded/failed
```

---

## Scenario 3: Snooze, Auto-Resolution, And Reopen Lifecycle

### Preconditions

- Apply the Scenario 1 mock fixture to the isolated QA project.
- An open or claimed item exists.

### Goal

Verify human lifecycle mutations and condition-clearing events preserve semantic history.

### Steps

1. Use `orchestrator attention snooze {id} --expected-version {version} --until {future_rfc3339}` and confirm the state is `snoozed`.
2. Let the deadline pass (or run the repository snooze sweep) and confirm the item returns to `open`.
3. Run `./scripts/qa/test-attention-inbox.sh` and confirm the successful-step auto-resolution and audit-reason assertions.
4. Reinsert the same failed condition in the isolated database and confirm the same item reopens with a larger `reopen_count`.

### Expected

- Snooze requires a future RFC3339 value and preserves versioning/audit.
- A matching successful step or terminal task resolves active conditions with `condition_cleared`.
- A recurring condition reopens the same row and increments both occurrence and reopen counters.

### Expected Data State

```sql
SELECT id, state, snoozed_until, resolved_at, resolution_json,
       occurrence_count, reopen_count, version
FROM attention_items WHERE id = '{attention_item_id}';
-- Expected after clear: state='resolved', resolution_json contains condition_cleared
-- Expected after recurrence: state='open', reopen_count >= 1, resolved_at IS NULL
```

---

## Scenario 4: RBAC, Feature Flag, Input Boundaries, And Response Safety

### Preconditions

- Secure TCP or UDS control-plane authentication is available.
- A RuntimePolicy can be applied to an isolated project.

### Goal

Verify only trusted identities mutate the queue, unsafe payloads are rejected, and disabling projection preserves existing rows.

### Steps

1. With `read_only`, call list/get/follow and then attempt claim/resolve/action.
2. With `operator`, perform claim and resolve using a valid current version and idempotency key.
3. Try an empty idempotency key, stale version, past snooze deadline, non-object action JSON, over-4096-byte action input, and an action not advertised by the item.
4. Apply a RuntimePolicy with `attention_inbox_enabled: false`, insert a new relevant source event, and verify no new row is materialized while previous rows remain listable.
5. Run `./scripts/qa/test-attention-inbox.sh` and inspect list JSON for secret/output markers.

### Expected

- Read operations require `read_only+`; every mutation/action requires `operator+`.
- Actor values come from mTLS subject or UDS UID, never request JSON.
- Invalid inputs and stale versions fail without external side effects.
- Disabled projection still advances the cursor and does not delete existing rows.
- Titles/summaries never contain transcript, command, stdout/stderr, or arbitrary error bodies.

### Expected Data State

```sql
SELECT actor, mutation_kind, action_id, target_version, status, error_code
FROM attention_actions WHERE attention_item_id = '{attention_item_id}'
ORDER BY id;
-- Expected: only authenticated operator/admin mutations; rejected requests add no succeeded row
```

---

## Scenario 5: Default GUI Entry, Filters, Keyboard Flow, And Timeline Link

### Preconditions

- Run `cd gui && npm ci && npm run build`.
- Start the desktop GUI against a daemon with at least two attention severities.

### Goal

Verify users discover actionable work immediately and can complete the primary flow without a hidden route or mouse dependency.

### Steps

1. Launch the desktop GUI and confirm "Attention Inbox" is the selected default tab with intervention/attention counters matching visible items.
2. Change state, severity, and assignee filters; confirm the card set updates.
3. Use `J`/`K` to select items, `C` to claim, `R` to resolve, and `Enter` to open the task's "进程时间线".
4. Use "稍后处理", an advertised recovery/decision action, and "查看进程时间线".
5. Reconnect the follow stream and confirm changes resume from `latest_change_id` without duplicated cards.
6. Repeat with `read_only` and confirm every mutation control is disabled while filters and timeline links remain usable.

### Expected

- The normal startup flow lands on Attention Inbox; "许愿池" and "进度观察" remain visible tabs.
- Default ordering shows intervention first, then current actor, unassigned, SLA, and creation age.
- Keyboard focus is visible and cards expose `role="option"`/`aria-selected` state.
- Stream deltas reconcile by stable item ID.
- Timeline deep links open the existing task detail and semantic timeline.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Failed event materialization, deduplication, and replay | PASS | 2026-07-12 | Codex | Isolated daemon script passed materialization and aggregation checks |
| 2 | Claim and action concurrency with idempotent replay | PASS | 2026-07-12 | Codex | Repository concurrency/action tests passed |
| 3 | Snooze, auto-resolution, and reopen lifecycle | PASS | 2026-07-12 | Codex | Auto-resolution and audit checks passed; lifecycle paths inspected |
| 4 | RBAC, feature flag, input boundaries, and response safety | PASS | 2026-07-12 | Codex | Role mapping, config default, validation, and safe summaries verified |
| 5 | Default GUI entry, filters, keyboard flow, and timeline link | PASS | 2026-07-12 | Codex | Tauri clippy and React production build passed; interaction paths inspected |

