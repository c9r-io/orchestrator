---
lifecycle: active
related_fr: FR-110
self_referential_safe: true
---

# Orchestrator - Slack Permalink And Canonical Task Routing

**Module**: Orchestrator  
**Scope**: Outbound credential boundary, permalink resolution, canonical task convergence, provenance, RBAC deep links, and reaction compatibility  
**Scenarios**: 5  
**Priority**: Critical

---

## Background

FR-110 is the first complete Slack badge-to-task slice. Its executable proof starts a separate daemon, HOME, database, ports, loopback fake Slack API, and an echo-only fixture whose SourceTaskTemplate uses `action.start: false`; no paid agent or real Slack workspace is contacted.

```bash
cargo build -p orchestratord -p orchestrator-cli
./scripts/qa/test-slack-reaction-task-routing.sh
```

Required fixture: `fixtures/manifests/bundles/source-task-routing-fixture.yaml`.

The debug-only `ORCHESTRATOR_SLACK_API_BASE_URL` points to the loopback fixture. Production builds reject that override unless explicitly compiled with `dev-insecure`.

## Database Schema Reference

| Table | Evidence |
|---|---|
| `source_events` | Authenticated delivery, safe route state, route/task links |
| `source_routing_attempts` | Claim attempt and automation route/task result |
| `source_automation_routes` | Unique automation identity, frozen revisions, protected permalink, request/task IDs |
| `control_action_audit` | Canonical `source.automation.create_task` mutation envelope |
| `tasks` | Deterministic canonical task and rendered goal |
| `source_bindings` | `automation` relationship from task to source coordinates |

---

## Scenario 1: Signed Reaction Resolves A Permalink And Creates One Canonical Task

### Preconditions

- Build current debug binaries.
- Apply the isolated mock fixture:

  ```bash
  orchestrator apply --project qa-source-routing \
    -f fixtures/manifests/bundles/source-task-routing-fixture.yaml
  ```

### Steps

1. Send a correctly signed Slack `reaction_added` for actor `U_OPERATOR`, reaction `agent-implement`, channel `C_QA_ROUTING`, and a stable message timestamp.
2. Confirm the webhook returns HTTP 200 before the asynchronous route reaches `routed`.
3. Inspect the fake Slack request for `GET /api/chat.getPermalink`, `channel`, `message_ts`, and the expected bearer credential.
4. Read the source event, `orchestrator source route <event-id> -o json`, and the created task row.

### Expected

- The fake API is contacted only after durable acknowledgement.
- The source safe summary names `slack-implement`, `implement-from-slack`, its hash, and `routed` state without exposing a URL.
- The protected route returns a validated `https://*.slack.com/archives/C_QA_ROUTING/...` permalink.
- Exactly one task is `created`; its goal is `$docs: inspect <permalink>` and contains no message body or token.

---

## Scenario 2: Durable Provenance And Secret Redaction

### Steps

1. Join the routed source event to its attempt and automation route.
2. Join the route request ID to `control_action_audit` and its task ID to `tasks` and `source_bindings`.
3. Inspect frozen binding/template snapshots, audit request/result, source/timeline projections, CLI output, and daemon logs for the fake token.

### Expected

- One chain contains the source event, attempt, binding revision, template hash, request ID, task ID, and `automation` SourceBinding.
- Audit action is `source.automation.create_task`, status `succeeded`, and result type `task`.
- Route records only credential store/key names; token, raw webhook body, and Slack message body are absent.
- Semantic timeline evidence contains safe reaction/template/binding provenance but not the permalink.

### Expected Data State

```sql
SELECT e.id, a.automation_route_id, r.binding_revision, r.template_hash,
       r.request_id, r.task_id, b.binding_type, ca.status
FROM source_events e
JOIN source_routing_attempts a ON a.source_event_id=e.id
JOIN source_automation_routes r ON r.id=a.automation_route_id
JOIN source_bindings b ON b.task_id=r.task_id
JOIN control_action_audit ca ON ca.request_id=r.request_id
WHERE e.external_event_id='Ev-route-first';
-- Expected: one row; binding_type=automation; status=succeeded
```

---

## Scenario 3: Duplicate Delivery And Restart Converge

### Steps

1. Complete Scenario 1 and record route/task IDs.
2. Restart the isolated daemon without deleting its data directory.
3. Deliver a different Slack delivery event ID for the same installation, message, reaction, and binding.
4. Inspect both source rows, provider request count, route count, task count, and audit count.
5. Run router/repository concurrency and crash-window regressions:

   ```bash
   cargo test -p orchestratord source_router::tests --bin orchestratord
   cargo test -p agent-orchestrator source_automation
   ```

### Expected

- The second event attaches to the original completed route and task.
- The provider permalink lookup, route, task, and create-task audit each occur once.
- Deterministic repository/router tests prove concurrent reservation and post-insert recovery cannot create a second task.
- `reaction_removed` does not release the automation identity.

---

## Scenario 4: Provider Failures, Credential Policy, And Role-Aware UI

### Steps

1. Run Slack client unit fixtures for invalid JSON, redirect, timeout, 429, provider rejection, and invalid/mismatched permalink hosts.
2. Validate that `reactionRouting: bindings` without `outboundCredential`, a missing SecretStore/key, or an empty token fails closed and creates no task.
3. Start the isolated daemon with read-only UDS authority; call source get and protected source route get.
4. Run UI tests for Sources and timeline evidence:

   ```bash
   cd gui
   npm test -- src/pages/Sources.test.tsx src/components/EvidencePanel.test.tsx
   ```

### Expected

- Provider failures persist stable bounded codes and no token/body/provider response text.
- Redirects and non-Slack/mismatched-channel permalinks are rejected.
- Read-only source get returns safe route status/template/binding fields but no permalink; route get is denied.
- Operator/Admin UI explicitly fetches the route and renders one keyboard-focusable external link with `rel="noreferrer"`; read-only UI performs no protected fetch and has no hidden focusable link.

---

## Scenario 5: Feature Gate And Compatibility Regression

### Steps

1. Set the Slack Trigger to `reactionRouting: disabled` and send a new reaction.
2. Verify it terminates as `ignored/reaction_routing_not_enabled` with no new provider call, route, or task.
3. Run the preceding source/template/binding suites:

   ```bash
   ./scripts/qa/test-source-events-slack.sh
   ./scripts/qa/test-slack-reaction-source.sh
   ./scripts/qa/test-source-task-template.sh
   ./scripts/qa/test-source-task-binding.sh
   ```

4. Run the full workspace and frontend gates.

### Expected

- Feature disable blocks only new reaction automation and preserves existing tasks/evidence.
- Fixed Slack message/thread routing, reaction normalization, template preview, binding lifecycle, and canonical task lifecycle remain green.
- Full Rust tests, clippy, frontend tests/build, and documentation lint pass.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Signed reaction to permalink and canonical task | PASS | 2026-07-17 | Codex | Isolated signed webhook + strict fake Slack query |
| 2 | Provenance and secret redaction | PASS | 2026-07-17 | Codex | SQL/public projection token-free joins |
| 3 | Duplicate delivery and restart convergence | PASS | 2026-07-17 | Codex | Same route/task/provider lookup after restart |
| 4 | Failure policy, RBAC, and accessible UI link | PASS | 2026-07-17 | Codex | Provider fixtures, read-only UDS denial, 5 focused UI tests |
| 5 | Feature gate and compatibility regression | PASS | 2026-07-17 | Codex | Disabled-after-route E2E, QA 146/155/156/157, workspace/clippy, 63 UI tests, build, docs lint |
