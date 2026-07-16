---
self_referential_safe: true
---

# Orchestrator - Source Events And Slack Process Binding

**Module**: Orchestrator  
**Scope**: Durable provider-neutral ingestion, deterministic process binding, Slack authentication/actions, replay, and GUI provenance  
**Scenarios**: 5  
**Priority**: High

---

## Background

Source adapters persist a bounded `NormalizedSourceEvent` before the asynchronous router resolves a Trigger, task, and binding. Run only the deterministic fixture:

```bash
orchestrator apply -f fixtures/manifests/bundles/source-events-fixture.yaml --project qa-source-events
```

Slack endpoint: `POST /source/slack/{project}/{trigger_name}`  
CLI: `orchestrator source list|get|ingest|bindings|bind|replay`

Compatibility note: FR-107 adds typed `reaction_added` input but deliberately bypasses the fixed Trigger action and thread binding used in this document. Validate that non-mutating branch with `docs/qa/orchestrator/155-slack-reaction-source-event-contract.md`; all message, command, binding, and replay behavior below remains authoritative.

The automated test starts a temporary daemon and database; it does not use live agents or Slack credentials:

```bash
cargo build -p orchestratord -p orchestrator-cli
./scripts/qa/test-source-events-slack.sh
```

## Database Schema Reference

| Table | Purpose |
|---|---|
| `source_events` | Normalized delivery identity and routing state |
| `source_bindings` | Provider conversation/artifact to task correlation |
| `source_routing_attempts` | Durable attempt and dead-letter evidence |
| `source_command_actions` | Actor/role/target/action idempotency linked to FR-101 canonical audit by `request_id` |

---

## Scenario 1: Provider-Neutral Ingestion And Duplicate Convergence

### Preconditions

- Apply `fixtures/manifests/bundles/source-events-fixture.yaml` to project `qa-source-events`.
- Use `fixtures/source-events/generic-message.json`; do not add Slack-only fields.
- `RuntimePolicy.spec.source_ingest_enabled` is true.

### Goal

Verify the core source contract is provider-neutral and an identical delivery creates one row, task, and primary binding through the configured Trigger.

### Steps

1. Run twice:

   ```bash
   orchestrator source ingest --project qa-source-events \
     --file fixtures/source-events/generic-message.json
   ```

2. Poll `orchestrator source list --project qa-source-events -o json` until `fixture-event-001` is `routed`.
3. Run `orchestrator source get {source_event_id} -o json` and `orchestrator source bindings {task_id} -o json`.
4. Run `./scripts/qa/test-source-events-slack.sh` and confirm its non-Slack assertion passes.

### Expected

- First ingest reports `inserted: true`; the duplicate reports `inserted: false` with the same source ID.
- Exactly one deterministic task and one `primary` binding exist.
- The normalized core payload contains `provider: fixture` and no Slack envelope/header fields.
- Task creation uses the Trigger's project, workflow, workspace, and concurrency semantics.

### Expected Data State

```sql
SELECT provider, installation_id, external_event_id, routing_state,
       routing_attempts, routed_task_id
FROM source_events WHERE external_event_id='fixture-event-001';
-- Expected: one row, provider='fixture', routing_state='routed', routed_task_id IS NOT NULL

SELECT COUNT(*) FROM source_bindings
WHERE created_by_event_id='{source_event_id}' AND binding_type='primary';
-- Expected: 1
```

---

## Scenario 2: Slack Authentication, Durable Ack, Replay Protection, And Size Boundary

### Preconditions

- Apply the deterministic fixture and a Slack Trigger whose SecretStore contains `{slack_signing_secret}`.
- The Trigger has `provider: slack`, `installationId: {installation_id}`, and `timestampToleranceSecs: 300`.
- Set `BASE_URL="http://127.0.0.1:{webhook_port}"`, `PROJECT="qa-source-events"`, and `TRIGGER="slack-source"`.

### Goal

Verify valid raw-body Slack signatures are durably acknowledged and invalid, stale, tampered, or oversized requests fail before insertion.

### Steps

1. Create `{body_file}` with an `event_callback`, stable `event_id`, human actor, channel, timestamp, and top-level message.
2. Send the complete signed request twice:

   ```bash
   TS="$(date +%s)"
   BODY="$(cat {body_file})"
   SIG="$(printf 'v0:%s:%s' "$TS" "$BODY" | \
     openssl dgst -sha256 -hmac '{slack_signing_secret}' | awk '{print "v0="$NF}')"
   curl -i -X POST "$BASE_URL/source/slack/$PROJECT/$TRIGGER" \
     -H 'Content-Type: application/json' \
     -H "X-Slack-Request-Timestamp: $TS" \
     -H "X-Slack-Signature: $SIG" \
     --data-binary "@$PWD/{body_file}"
   ```

3. Repeat with a timestamp older than 300 seconds and a recomputed signature; repeat with `X-Slack-Signature: v0=00`.
4. Send a body larger than 262144 bytes.
5. Run `./scripts/qa/test-source-events-slack.sh`.

### Expected

- Valid requests return HTTP 200 with `accepted`, then `deduplicated`, and the same `source_event_id`.
- The response is produced after durable insert and before asynchronous routing completes.
- Stale and tampered requests return 401; oversized requests return 413; none creates a source row.
- Logs contain provider and hashed installation/external IDs, not body text or secret values.

### Expected Data State

```sql
SELECT external_event_id, COUNT(*) AS rows, MAX(routing_attempts) AS attempts
FROM source_events WHERE external_event_id='{slack_event_id}'
GROUP BY external_event_id;
-- Expected: rows=1

SELECT COUNT(*) FROM source_events
WHERE external_event_id IN ('{stale_event_id}','{tampered_event_id}','{oversized_event_id}');
-- Expected: 0
```

---

## Scenario 3: Thread Correlation, Branching, And Ambiguity Attention

### Preconditions

- Scenario 2 created a routed Slack top-level event and primary binding.
- Use the same channel and root `thread_ts` for replies.

### Goal

Verify normal replies append to the bound process, explicit branch is deterministic, and multiple candidate bindings never mutate an arbitrary task.

### Steps

1. Send a signed ordinary reply with a new Slack `event_id` and the bound `thread_ts`.
2. Confirm `routed_task_id` equals the root task and the task emits `source_context_added` with the source event ID.
3. As a configured operator, send `/orchestrator branch` in the bound thread; confirm one child task and `related` binding.
4. In an isolated unit/integration test, create both `primary` and `related` bindings for the same coordinates, then ingest another reply:

   ```bash
   cargo test -p orchestratord source_router::tests::ambiguous_binding_materializes_attention_without_guessing --bin orchestratord
   ```

### Expected

- An ordinary bound reply creates no task or new binding and routes to the existing task.
- Branch creates one child with `parent_task_id={root_task_id}`; duplicate delivery creates no second child/action.
- Ambiguous routing ends in `needs_attention`, has no routed task, and creates one `source_routing_ambiguous` Attention item.
- An unresolved source Attention item exposes no task timeline button until an operator establishes correlation.

### Expected Data State

```sql
SELECT external_event_id, routing_state, routed_task_id, last_error_code
FROM source_events
WHERE external_event_id IN ('{reply_event_id}','{branch_event_id}','{ambiguous_event_id}');
-- Expected: reply/branch routed; ambiguous needs_attention with routed_task_id NULL

SELECT kind, source_event_id, task_id FROM attention_items
WHERE source_event_id='{ambiguous_source_event_id}';
-- Expected: one source_routing_ambiguous row and task_id=''
```

---

## Scenario 4: Closed Commands, Shared Attention Actions, RBAC, And Audit

### Preconditions

- A bound task and an actionable Attention item exist.
- Trigger `actorRoles` maps `{operator_external_id}` to `operator`; `{unknown_external_id}` is absent.
- Use signed, unexpired action tokens containing action, attention item ID, and expected version.

### Goal

Verify external commands cannot bypass role mapping, optimistic concurrency, action allowlists, or the shared audited service.

### Steps

1. Send signed Slack approve and retry `block_actions` using the configured operator and advertised action IDs.
2. Repeat the same interaction payload and action token.
3. Send `/orchestrator cancel` from the unknown actor in a bound thread.
4. Send an expired token, a token whose action differs from `action_id`, and an unsupported action.
5. Run:

   ```bash
   cargo test -p orchestratord webhook::tests --bin orchestratord
   cargo test -p orchestratord source_router::tests::unknown_actor_privileged_command_fails_closed_and_is_audited --bin orchestratord
   ```

### Expected

- Approve/retry call `execute_allowlisted_action`, the same service used by gRPC/GUI/CLI Attention actions.
- Expected version, advertised action, role, and idempotency key remain enforced; a duplicate executes no second side effect.
- Unknown actors resolve to `read_only`; privileged commands fail with `actor_not_authorized`.
- Expired, mismatched, or unsupported actions are rejected before command execution.
- Audit records include actor, resolved role, target, action, request hash, status/result, stable error code, and the request ID shared with `control_action_audit`.

### Expected Data State

```sql
SELECT source_event_id, actor, resolved_role, target_type, target_id,
       action, idempotency_key, status, error_code, COUNT(*) AS rows
FROM source_command_actions
WHERE source_event_id='{command_source_event_id}'
GROUP BY source_event_id, idempotency_key;
-- Expected: rows=1; authorized action succeeded or unknown actor failed/read_only

SELECT attention_item_id, idempotency_key, COUNT(*)
FROM attention_actions WHERE idempotency_key='{source_attention_action_key}'
GROUP BY attention_item_id, idempotency_key;
-- Expected: 1
```

---

## Scenario 5: UI Entry Visibility, Filtering, Task Provenance, And Admin Replay

### Preconditions

- Run `cd gui && npm ci && npm run build`.
- Start the desktop GUI with routed, failed, and `needs_attention` source fixtures.
- Test once as `read_only` and once as `admin`.

### Goal

Verify users discover source operations through visible navigation and privilege-sensitive actions remain accessible and safe.

### Steps

1. Launch the GUI, activate the visible "来源" navigation item, and repeat with `Cmd+4`.
2. Select each routing-state filter and confirm the list and empty state update.
3. On a routed source card, choose the open-process action; confirm Process Workspace opens and its source-binding panel matches the card coordinates.
4. As `read_only`, inspect failed/attention cards and confirm "重放" is absent.
5. As `admin`, choose "重放" for a failed event, confirm the list refreshes, and verify deterministic routing creates no duplicate process.

### Expected

- Sources is the fourth navigation entry with a unique active state; `Cmd/Ctrl+1..5` map to Attention, Processes, Sessions, Sources, and System.
- Provider, installation, routing state, timestamp, conversation/thread, and stable error code are readable without raw message text.
- Opening a process selects the integrated Process Workspace and SourcePanel preserves provenance.
- Only admins can see/use replay; errors use `role="alert"`, list updates use `aria-live`, and controls are keyboard reachable with visible focus.

### Expected Data State

```sql
SELECT id, routing_state, routing_attempts, routed_task_id, last_error_code
FROM source_events WHERE id='{replayed_source_event_id}';
-- Expected after replay/reroute: routed or needs_attention with attempts reflecting the new route

SELECT COUNT(DISTINCT task_id) FROM source_bindings
WHERE created_by_event_id='{replayed_source_event_id}';
-- Expected: <= 1 for a deterministic top-level replay
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Provider-neutral ingestion and duplicate convergence | PASS | 2026-07-14 | Codex | Isolated E2E fixture passed |
| 2 | Slack authentication, durable ack, replay protection, and size boundary | PASS | 2026-07-14 | Codex | Automated Slack E2E assertions passed |
| 3 | Thread correlation, branching, and ambiguity Attention | PASS | 2026-07-14 | Codex | Thread E2E and ambiguity/router tests passed |
| 4 | Closed commands, shared Attention actions, RBAC, and audit | PASS | 2026-07-14 | Codex | Signature/token/role/audit tests passed |
| 5 | UI entry visibility, filtering, task provenance, and admin replay | PASS | 2026-07-14 | Codex | Tauri check and React production build passed; manual semantics reviewed |
