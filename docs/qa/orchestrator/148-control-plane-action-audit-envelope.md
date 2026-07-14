---
self_referential_safe: true
---

# Orchestrator - Canonical Control-Plane Action Audit Envelope

**Module**: Orchestrator  
**Scope**: Mutation reservation, request correlation, retry safety, authorization evidence, redaction, and compatibility enforcement  
**Scenarios**: 5  
**Priority**: High

---

## Background

Migration 31 makes `control_action_audit` the durable source of truth for process-console mutations. Domain tables and task/source events retain `request_id` projections. The automated script uses only the deterministic failure fixture:

```bash
cargo build -p orchestratord -p orchestrator-cli
orchestrator apply -f fixtures/manifests/bundles/process-timeline-failure.yaml --project qa-action-audit
./scripts/qa/test-control-plane-action-audit.sh
```

CLI: `orchestrator audit list|get`  
Runtime rollout: `RuntimePolicy.spec.action_audit_mode: compatibility | enforced`

## Database Schema Reference

| Table | Purpose |
|---|---|
| `control_action_audit` | Canonical bounded action envelope and terminal result |
| `control_plane_audit` | Transport authentication/authorization decision linked by `request_id` |
| `attention_actions`, `resume_executions`, `session_control_actions`, `source_command_actions` | Domain projections linked by `request_id` |
| `events`, `source_events`, `source_bindings` | Event/provenance projections linked by `request_id` where applicable |

---

## Scenario 1: Successful Mutation Produces A Complete Join Chain

### Preconditions

- Apply `fixtures/manifests/bundles/process-timeline-failure.yaml` to isolated project `qa-action-audit`.
- Allow the deterministic mock task to create one actionable `step_failed` Attention item.

### Goal

Verify one successful Attention mutation is reserved before the side effect and correlated across transport, canonical, domain, and event evidence.

### Steps

1. Get `{attention_item_id}` and `{version}` from `orchestrator attention list --project qa-action-audit -o json`.
2. Run `orchestrator attention claim {attention_item_id} --expected-version {version} --idempotency-key qa-audit-success`.
3. Find the request with `orchestrator audit list --project qa-action-audit --action attention.claim -o json`.
4. Run `orchestrator audit get {request_id} --project qa-action-audit -o json`.

### Expected

- The canonical status is `succeeded`, actor/role are transport-derived, reason code is present, and result references the Attention action.
- `request_hash` exists while request bodies do not.
- `control_plane_audit` and `attention_actions` contain the same request ID.
- Any emitted mutation event contains and promotes the same request ID.

### Expected Data State

```sql
SELECT a.request_id, a.status, d.request_id, t.request_id
FROM control_action_audit a
JOIN attention_actions d ON d.request_id=a.request_id
JOIN control_plane_audit t ON t.request_id=a.request_id
WHERE a.idempotency_key='qa-audit-success';
-- Expected: one row; all request IDs equal and status='succeeded'
```

---

## Scenario 2: Duplicate And Conflicting Retry Identity Fail Closed

### Preconditions

- Scenario 1 completed or create a fresh open Attention item.

### Goal

Verify matching retries do not repeat a business side effect and changed canonical input with the same key is rejected before mutation.

### Steps

1. Claim a fresh item with key `qa-audit-retry` and its current version.
2. Repeat the identical request and record its non-success response/request ID.
3. Reuse `qa-audit-retry` with a different expected version.
4. Run:

   ```bash
   cargo test -p agent-orchestrator action_audit::tests --lib
   ```

### Expected

- Exactly one canonical retry identity and one succeeded domain row exist.
- The identical retry performs no second side effect.
- The changed request fails with an idempotency/canonical-hash conflict before the Attention version changes.
- The concurrent reservation test reports exactly one execution owner.

### Expected Data State

```sql
SELECT idempotency_key, COUNT(*), MIN(status), MAX(status)
FROM control_action_audit
WHERE project_id='qa-action-audit' AND idempotency_key='qa-audit-retry'
GROUP BY idempotency_key;
-- Expected: one non-denied canonical action row
```

---

## Scenario 3: Stale, Fencing, And Authorization Failures Remain Distinguishable

### Preconditions

- Keep one claimed item and its prior version.
- Run the isolated UDS daemon as the generated admin client; use unit RBAC checks for lower roles.

### Goal

Verify policy denial and stale concurrency failures have different terminal status/error codes, and denial retry identity cannot poison a later authorized attempt.

### Steps

1. Attempt an Attention mutation with a stale expected version and a new key.
2. Run daemon action-audit and RBAC tests:

   ```bash
   cargo test -p orchestratord server::action_audit::tests --bin orchestratord
   cargo test -p orchestratord control_plane::tests::required_role_mapping_is_stable --bin orchestratord
   ```

3. Run the core denial isolation test and session fencing tests:

   ```bash
   cargo test -p agent-orchestrator denied_retry_identity_does_not_block_later_authorized_attempt --lib
   cargo test -p agent-orchestrator writer_lease_fencing_rejects_stale_tokens --lib
   ```

### Expected

- Stale mutation is `failed` with a stale/version error and no domain state change.
- Authorization rejection is terminal `denied` with `authorization_denied` and retains trusted transport context.
- A denied retry key does not block a later authorized reservation.
- Heartbeat may omit a retry key only with request ID, expected version, fencing token, and `lease_heartbeat` reason.

### Expected Data State

```sql
SELECT status, error_code, COUNT(*)
FROM control_action_audit
WHERE project_id='qa-action-audit'
GROUP BY status, error_code;
-- Expected: succeeded and failed/stale rows; denial is covered by isolated RBAC/unit evidence
```

---

## Scenario 4: Project-Scoped Query And Redaction Boundaries

### Preconditions

- Scenarios 1-3 generated canonical rows.

### Goal

Verify list/get filters cannot cross projects and expose only the bounded envelope.

### Steps

1. Query by project, actor, target type, action, status, and time range using `orchestrator audit list`.
2. Get one result by request ID in its project, then repeat with another project ID.
3. Search JSON/YAML output and the database for terminal input, prompt text, provider body, source body, handoff briefing content, and known fixture secret markers.
4. Run `cargo test -p agent-orchestrator stored_envelope_contains_hash_not_request_body --lib`.

### Expected

- Filters return only matching rows and list clamps to 500.
- Cross-project get returns not found.
- Output contains hashes and result references, not `canonical_request`, raw input, prompt, transcript, or source body.
- CLI and GUI error helpers retain `x-request-id` without response content telemetry.

### Expected Data State

```sql
SELECT request_id, length(request_hash), operator_reason
FROM control_action_audit WHERE project_id='qa-action-audit';
-- Expected: request_hash length=64; operator_reason is NULL or <=500 bytes
```

---

## Scenario 5: Compatibility Rollout And Populated Migration

### Preconditions

- Use the isolated database created by the QA script.

### Goal

Verify old optional clients remain usable in compatibility mode, enforcement rejects missing audit context, and version-30 data survives migration 31.

### Steps

1. Confirm the default RuntimePolicy reports `action_audit_mode: compatibility`.
2. Run the resolver tests for compatibility/enforced behavior.
3. Apply a RuntimePolicy with `action_audit_mode: enforced`; use a current CLI mutation and confirm success.
4. Send a legacy mutation without `ActionAuditContext` through an integration fixture and confirm `INVALID_ARGUMENT` before domain mutation.
5. Run:

   ```bash
   cargo test -p agent-orchestrator populated_v30_database_upgrades_with_action_audit_links --lib
   ```

### Expected

- Compatibility creates `legacy_client` context for old mutations; current clients supply explicit reasons and retry keys.
- Enforced mode rejects missing reason/retry context but keeps heartbeat's documented exemption.
- Switching back to compatibility disables enforcement without dropping migration 31.
- Populated rows survive and every projection table has a `request_id` column.

### Expected Data State

```sql
SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1;
-- Expected: 31

SELECT COUNT(*) FROM pragma_table_info('control_action_audit');
-- Expected: greater than 0
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Successful mutation produces a complete join chain | PASS | 2026-07-14 | Codex | Isolated enforced-mode run joined transport, canonical, domain, and event rows |
| 2 | Duplicate and conflicting retry identity fail closed | PASS | 2026-07-14 | Codex | Script plus concurrent/hash unit coverage passed |
| 3 | Stale, fencing, and authorization failures remain distinguishable | PASS | 2026-07-14 | Codex | Stale failure and read-only UDS denial produced distinct durable evidence |
| 4 | Project-scoped query and redaction boundaries | PASS | 2026-07-14 | Codex | gRPC integration, CLI filters, cross-project not-found, and redaction checks passed |
| 5 | Compatibility rollout and populated migration | PASS | 2026-07-14 | Codex | Enforced current client, missing-context resolver, compatibility model, and populated v30 migration passed |
