---
self_referential_safe: true
---

# Orchestrator - Slack Reaction Source Event Contract

**Module**: Orchestrator
**Scope**: Typed provider-neutral reactions, signed Slack normalization, non-mutating routing, bounded reads, and Sources UI evidence
**Scenarios**: 5
**Priority**: High

---

## Background

FR-107 records Slack `reaction_added` deliveries without selecting a badge binding, resolving a permalink, rendering a Skill, or creating a task. The deterministic QA script starts its own daemon, ports, HOME, database, and mock agent configuration:

FR-108 adds a standalone SourceTaskTemplate preview. FR-109 adds optional exact binding selection behind Trigger `reactionRouting: bindings`; the default remains `disabled`, preserving this document's `reaction_routing_not_enabled` result. With routing enabled, FR-110 now resolves a permalink and creates a canonical task; that behavior is exclusively verified by `docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md`. This document remains the disabled/non-message reaction contract.

```bash
cargo build -p orchestratord -p orchestrator-cli
./scripts/qa/test-slack-reaction-source.sh
```

It applies only `fixtures/manifests/bundles/source-events-fixture.yaml`; it does not contact Slack or run a live AI agent.

Endpoint: `POST /source/slack/{project}/{trigger_name}`
CLI: `orchestrator source list|get`

## Database Schema Reference

| Table | Purpose |
|---|---|
| `source_events` | Delivery identity, normalized reaction JSON, routing state, attempts, and stable reason |
| `source_bindings` | Must remain empty for reaction-only QA |
| `tasks` | Must remain empty for reaction-only QA |
| `source_routing_attempts` | One terminal ignored attempt per inserted reaction event |

---

## Scenario 1: Provider-Neutral Reaction Contract And Compatibility

### Preconditions

- Use the repository test database only; no daemon or external provider is required.

### Goal

Verify the core model represents reactions without Slack field names and still deserializes normalized events written before the additive field existed.

### Steps

1. Run:

   ```bash
   cargo test -p agent-orchestrator source::tests
   ```

2. Inspect `reaction_ingest_round_trips_provider_neutral_metadata` and confirm the fixture provider is not Slack.
3. Inspect `reaction_contract_rejects_missing_mismatched_or_unsafe_metadata` for name, target, URL, and event-kind boundaries.
4. Inspect `normalized_message_without_reaction_field_remains_compatible` for populated-row compatibility.

### Expected

- `SourceEventKind::ReactionAdded` serializes as `reaction_added`.
- `SourceReactionRef` contains only `name` and a provider-neutral `ExternalArtifactRef`.
- Reaction metadata is required only for reaction events and forbidden on other event kinds.
- Old normalized JSON without `reaction` deserializes with `reaction: None`.
- No database migration is required.

---

## Scenario 2: Signed Message Reaction, Precise Provenance, And Duplicate Convergence

### Preconditions

- Apply the deterministic fixture:

  ```bash
  orchestrator apply --project qa-slack-reaction \
    -f fixtures/manifests/bundles/source-events-fixture.yaml
  ```

- Configure a Slack Trigger with `{slack_signing_secret}`, `{installation_id}`, and `timestampToleranceSecs: 300`.
- Set `BASE_URL="http://127.0.0.1:{webhook_port}"`, `PROJECT="qa-slack-reaction"`, and `TRIGGER="slack-reaction"`.

### Goal

Verify one signed message reaction is durably accepted, precisely normalized, safely ignored, and deduplicated without task mutation.

### Steps

1. Create `{body_file}` containing one `event_callback` with stable `event_id`, `event.type: reaction_added`, `event.user`, `event.reaction`, message `item.channel`/`item.ts`, and fractional `event.event_ts`.
2. Send the same complete signed request twice:

   ```bash
   TS="$(date +%s)"
   BODY="$(cat {body_file})"
   SIG="$(printf 'v0:%s:%s' "$TS" "$BODY" | \
     openssl dgst -sha256 -hmac '{slack_signing_secret}' | awk '{print "v0="$NF}')"
   curl -i -X POST "$BASE_URL/source/slack/$PROJECT/$TRIGGER" \
     -H 'Content-Type: application/json' \
     -H "X-Slack-Request-Timestamp: $TS" \
     -H "X-Slack-Signature: $SIG" \
     --data-binary "$BODY"
   ```

3. Poll `orchestrator source list --project "$PROJECT" -o json` until the event is `ignored`.
4. Run `orchestrator source get {source_event_id} -o json`.
5. Run `./scripts/qa/test-slack-reaction-source.sh`.

### Expected

- Both deliveries return HTTP 200 and reference the same source event.
- The row has `event_type: reaction_added`, the authenticated actor, normalized reaction name, `message` target, `{channel}:{message_ts}`, and RFC 3339 `occurred_at` with the Slack fraction preserved.
- `normalized.text_summary`, `command`, and target `url` are null; no body or transcript is present.
- The event ends `ignored` with `reaction_routing_not_enabled` and exactly one attempt.

### Expected Data State

```sql
SELECT external_event_id, event_type, routing_state, routing_attempts,
       routed_task_id, last_error_code, COUNT(*) AS rows
FROM source_events WHERE external_event_id='{slack_event_id}'
GROUP BY external_event_id;
-- Expected: one row, reaction_added, ignored, attempts=1,
-- routed_task_id IS NULL, last_error_code='reaction_routing_not_enabled'

SELECT COUNT(*) FROM tasks WHERE project_id='qa-slack-reaction';
-- Expected: 0

SELECT COUNT(*) FROM source_bindings WHERE project_id='qa-slack-reaction';
-- Expected: 0
```

---

## Scenario 3: Malformed, Non-Message, Authentication, And Size Boundaries

### Preconditions

- Use the isolated daemon and mock fixture from Scenario 2.

### Goal

Verify untrusted or unsupported Slack inputs fail closed and never become routable work.

### Steps

1. Send individually signed reaction payloads missing actor, name, item, message channel, message timestamp, or occurrence timestamp; also send invalid name and timestamp forms.
2. Send signed `file` and `file_comment` reactions with valid target IDs.
3. Repeat a valid request with an older-than-tolerance timestamp and recomputed signature; repeat with `X-Slack-Signature: v0=00`.
4. Send a request body larger than 262144 bytes.
5. Run:

   ```bash
   ./scripts/qa/test-slack-reaction-source.sh
   ./scripts/qa/test-source-events-slack.sh
   cargo test -p orchestratord webhook::tests --bin orchestratord
   ```

### Expected

- Missing/invalid reaction fields return HTTP 400 and the documented stable `slack_reaction_*` reason; no source row is inserted.
- Valid file/file-comment reactions are durable and terminal `ignored` with `unsupported_reaction_target`; no task is created.
- Stale and invalid signatures return 401; oversized input returns 413; none reaches normalization or storage.
- Existing Slack message, thread, command, action-token, and URL-verification behavior remains unchanged.

### Expected Data State

```sql
SELECT external_event_id, routing_state, last_error_code, routed_task_id
FROM source_events WHERE external_event_id IN ('{file_event_id}','{file_comment_event_id}');
-- Expected: ignored, unsupported_reaction_target, routed_task_id NULL

SELECT COUNT(*) FROM source_events
WHERE external_event_id IN ('{missing_event_id}','{stale_event_id}',
                            '{invalid_signature_event_id}','{oversized_event_id}');
-- Expected: 0

SELECT COUNT(*) FROM tasks WHERE project_id='qa-slack-reaction';
-- Expected: 0
```

---

## Scenario 4: Fixed-Trigger And Bound-Thread Non-Mutation

### Preconditions

- Apply `fixtures/manifests/bundles/source-events-fixture.yaml` and a Slack Trigger with a valid fixed action.
- For the bound-thread branch, create an existing task and Slack `primary` binding whose channel/thread matches the reaction target.

### Goal

Verify the reaction guard runs before Trigger fixed-action routing and before thread correlation.

### Steps

1. Run:

   ```bash
   cargo test -p orchestratord source_router::tests --bin orchestratord
   ```

2. Confirm `reaction_is_ignored_without_task_binding_or_duplicate_attempt` creates neither task nor binding.
3. Confirm `reaction_does_not_append_to_matching_bound_thread` emits no task context event for an existing binding.
4. Confirm `non_message_reaction_is_ignored_with_target_reason` uses the target-specific stable reason.
5. Run the isolated reaction script and confirm task/binding counts remain zero.

### Expected

- A fixed Slack Trigger action is never executed for `reaction_added`.
- A matching thread binding is not looked up or mutated; no `source_context_added` event is emitted.
- Replaying the same delivery does not increment routing attempts.
- The guard remains observable through terminal state and reason without creating Attention or task work.

### Expected Data State

```sql
SELECT routing_state, routing_attempts, routed_task_id, last_error_code
FROM source_events WHERE id='{reaction_source_event_id}';
-- Expected: ignored, 1, NULL, reaction_routing_not_enabled

SELECT COUNT(*) FROM events
WHERE task_id='{bound_task_id}' AND event_type='source_context_added'
  AND payload_json LIKE '%{reaction_source_event_id}%';
-- Expected: 0
```

---

## Scenario 5: Sources Entry Visibility, Bounded Reaction Card, And Action Absence

### Preconditions

- Run `cd gui && npm ci && npm run build`.
- Use the GUI fixture containing an ignored `reaction_added` card.
- Test once as `read_only` and once as `admin`.

### Goal

Verify reaction provenance is discoverable from the normal Sources entry without exposing content or presenting a misleading process/replay action.

### Steps

1. Launch the GUI and activate the visible “来源” / Sources navigation item; repeat with `Cmd/Ctrl+4`.
2. Find the `reaction_added` card and inspect its accessible text.
3. Confirm the card shows `:{reaction_name}:`, `{target_kind} / {target_external_id}`, routing state, and stable reason.
4. Confirm no body, target URL, “打开进程”, or “重放” is present on that ignored card for either role.
5. Run:

   ```bash
   cd gui
   npm test -- --run src/pages/Sources.test.tsx
   npx playwright test -g "Sources supports|read-only Sources"
   npm run build
   ```

### Expected

- Sources remains reachable through visible navigation and the keyboard shortcut.
- The card uses the existing list/listitem semantics and bounded fields remain readable without color dependence.
- The reaction card contains no message body, transcript, raw payload, secret, or target URL.
- Since no task exists and ignored reactions are not replayable failures, the card has no action buttons.
- Existing filters, routed-process correlation, read-only visibility, and admin-only failure replay remain intact.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Provider-neutral reaction contract and compatibility | PASS | 2026-07-17 | Codex | 86 source/core tests passed |
| 2 | Signed message reaction, precise provenance, and duplicate convergence | PASS | 2026-07-17 | Codex | Isolated reaction QA passed 5/5 |
| 3 | Malformed, non-message, authentication, and size boundaries | PASS | 2026-07-17 | Codex | Reaction QA 5/5 and FR-099 Slack regression 8/8 passed |
| 4 | Fixed-Trigger and bound-thread non-mutation | PASS | 2026-07-17 | Codex | Router tests passed 7/7; full workspace tests passed |
| 5 | Sources entry, bounded reaction card, and action absence | PASS | 2026-07-17 | Codex | Sources Vitest 3/3, Chromium 2/2, and production build passed |
