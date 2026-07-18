---
self_referential_safe: true
---

# Orchestrator - Managed Slack Connection And Shared OAuth

**Module**: Orchestrator / Slack Integration Gateway  
**Scope**: Shared official app OAuth, SourceConnection lifecycle, durable delivery, transfer, GUI, security, and release regression  
**Scenarios**: 5  
**Priority**: Critical

## Automated Entry Point

```bash
./scripts/qa/test-slack-managed-shared-oauth.sh
```

The automated gate uses repository tests, deterministic fake provider contracts, and the secret-free manifest fixture:

```bash
orchestrator apply --project default \
  -f fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml
```

It does not connect to Slack or claim live certification. Run with a clean worktree for release evidence. `FR114_ALLOW_DIRTY=1` is for local iteration only and must not appear in final release evidence.

## Scenario 1: Catalog, OAuth Intent, Connection, And Default Trigger

### Preconditions

- Configure daemon `ORCHESTRATOR_SLACK_GATEWAY_URL` and matching deployment-level enrollment key.
- Gateway has the reviewed official app credentials and manifest contract.
- Target project has at least one Workspace and Workflow.

### Steps

1. Verify `source connection catalog` advertises protocol 1, `managed_shared`, reserved unavailable `managed_dedicated`, compatible `manual`, and Gateway permalink capability.
2. Start a managed shared connection as Admin and inspect the safe intent response and OAuth URL.
3. Exercise pending status, page/daemon restart resume, cancel, expiry, denial, replay, redirect mismatch, scope mismatch, callback retry, and duplicate/concurrent callback fixtures.
4. Complete OAuth and verify one active SourceConnection plus one default `connectionRef` Trigger with `reactionRouting: disabled`.
5. Reauthorize the same Slack identity and verify the same connection/Trigger advances generation/version while the old pairing/generation fails.

### Expected

- No Slack app credential, OAuth code/state, poll/pairing secret, bot token, raw body, workspace name, or private URL appears in daemon config, proto/CLI safe output, GUI, log, metric, or task.
- OAuth state is short-lived, single-use, and bound to daemon, project, actor, exact redirect, and exact scopes.
- The Slack-returned team identity, not browser input, determines the installation.
- Repeated OAuth converges to one logical connection and Trigger.
- Users must explicitly create/enable a badge binding after connection; installation alone creates no task.

### Expected Data State

```sql
SELECT provider, provisioning_mode, state, generation, version, trigger_name,
       pairing_secret_ciphertext IS NOT NULL
FROM source_connections
WHERE project_id = '{project_id}' AND id = '{connection_id}';
-- Expected: slack | managed_shared | active | >=1 | >=1 | slack-* | 1

SELECT status, connection_id FROM source_connection_intents WHERE id = '{intent_id}';
-- Expected: completed | {connection_id}
```

## Scenario 2: Multi-workspace Isolation, Signed Delivery, And Recovery

### Preconditions

- Use two fake Slack tenant identities, two projects, and preferably two daemon data roots.
- Configure two badge bindings selecting different templates/Skills/workflows.

### Steps

1. Install the same official app for tenant A and tenant B; verify list/get/watch are project-scoped and each Gateway projection has one owner.
2. Send valid signed `reaction_added` events, invalid signature/timestamp/unknown-installation events, duplicate events, and cross-tenant canaries.
3. Stop one daemon, enqueue events, restart it, and verify claim resumes after its last acknowledged cursor.
4. Exercise lease expiry, duplicate/out-of-order delivery, daemon restart between ingest and ack, and provider timeout/429/invalid-auth responses.
5. Trigger uninstall/revocation and verify delivery is acknowledged before connection becomes revoked and new proxy/task work stops.

### Expected

- Slack success acknowledgement follows durable Gateway enqueue.
- Installation pairing, owner daemon, project, generation, team/enterprise digest, and cursor are checked before ingestion.
- Tenant A cannot list, claim, proxy, acknowledge, or mutate tenant B; no canary appears in the wrong project.
- At-least-once redelivery converges to one source event, route, and task for one automation identity.
- Revocation retains existing source, route, task, Attention, and audit evidence.

### Expected Data State

```sql
SELECT installation_id, external_event_id, COUNT(*)
FROM deliveries GROUP BY installation_id, external_event_id HAVING COUNT(*) != 1;
-- Expected: no rows

SELECT project_id, external_event_id, COUNT(*)
FROM source_events GROUP BY project_id, external_event_id HAVING COUNT(*) != 1;
-- Expected: no rows
```

## Scenario 3: Disconnect, Two-phase Transfer, Migration, And Backup

### Steps

1. Transfer an active connection with a valid expected version from daemon A to daemon B; inject stale version, wrong owner, crash after Gateway switch, and crash after target persistence but before target acknowledgement.
2. Verify Gateway rotates pairing, clears old leases, stores an encrypted target handoff, and never returns the replacement pairing to daemon A.
3. Verify daemon A becomes suspended and clears its local credential; daemon B repeatedly claims, idempotently adopts the connection/default Trigger/cursor, and acknowledges the handoff.
4. Disconnect from the active owner and verify Gateway/local credential destruction while safe history remains.
5. Run populated Gateway v1→v2 and daemon pre-35→35 migration, SQLite integrity/backup/restore, newer-schema rejection, and forward-compatible rollback checks.

### Expected

- At every point no more than one pairing can claim/proxy/ack an installation.
- Transfer failure creates a visible suspended/deferred state, not simultaneous active owners.
- The target resumes at `MAX(local_cursor, gateway_cursor)` and does not replay acknowledged history.
- Disconnect is terminal for the credential and does not delete created task/source/audit evidence.
- Additive forward migrations preserve populated installations and create the handoff tables/indexes.

### Expected Data State

```sql
SELECT owner_daemon_id, state, pairing_secret_ciphertext IS NULL, last_error_code
FROM source_connections WHERE id = '{connection_id}';
-- Old daemon expected during handoff: {daemon_b} | suspended | 1 | owner_transfer_pending_acceptance

SELECT COUNT(*) FROM ownership_transfers WHERE installation_id = '{installation_id}';
-- Expected: 1 before target ack, 0 after target ack
```

## Scenario 4: CLI, Tauri, GUI, RBAC, Accessibility, And Privacy

### Entry Visibility

Open the normal primary navigation path `Sources → Connections`. Connections is the default Sources subsection and does not require a hidden hash route.

### Steps

1. Validate catalog/list/get/watch/connect/status/cancel/reauthorize/disconnect/transfer through CLI, direct gRPC, and real Tauri command serialization.
2. Exercise the three provisioning cards, Gateway unavailable state, OAuth popup blocked/reload resume, pending/cancel/error/completed states, connection detail, destructive confirmation, and next-step binding guidance.
3. Repeat as ReadOnly, Operator, and Admin, including direct-RPC bypass attempts and canonical action request/version/idempotency audit assertions.
4. Run keyboard-only dialogs, focus return, axe, reduced motion/transparency, 640 px layout, and connection deep links.
5. Scan safe response payloads, stdout/stderr, retained logs, DOM, and local/session storage for all fixture secrets, state/code/token markers, raw Slack bytes, provider URLs, and workspace canaries.

### Expected

- ReadOnly can inspect safe status; only Admin can mutate connection credential ownership. GUI visibility and daemon authorization agree.
- Dedicated mode is clearly unavailable and never silently uses the shared app.
- Local storage contains only the bounded project/intent resume key and clears it at terminal state.
- No serious/critical accessibility violations or unreachable controls occur at desktop/narrow widths.
- Privacy scan finds no credential or private provider material.

## Scenario 5: Repository Aggregate And Controlled Slack Sandbox Certification

### Automated Steps

1. Run workspace tests and strict Clippy with all targets/features.
2. Run Gateway, daemon/core migration/security/provider tests, frontend Vitest coverage/build, Playwright, and documentation lint.
3. Run the FR-113 aggregate to prove manual Slack reaction automation remains compatible.
4. Verify the operator guide, threat model, architecture, changelog, manifest, and fixture references are present and internally consistent.

### Live Sandbox Steps (Non-CI)

1. In a controlled non-production Slack workspace, provision/validate the reviewed official app without retaining the configuration token.
2. Complete OAuth in the GUI, reload during pending state, and verify one active connection/default disabled Trigger.
3. Enable two reviewed badge bindings, add the reactions, and verify two distinct deterministic echo tasks.
4. Reauthorize, revoke/uninstall, reconnect, transfer between two controlled daemons, and disconnect.
5. Record only date, app manifest digest, build commit, anonymous installation/daemon digests, state transitions, request IDs, and pass/fail. Do not record workspace name, channel/message URL, user identity, token, OAuth state/code, or raw payload.

### Expected

- Automated gates are reproducible from a clean commit and attribute failures to the owning slice.
- The manual Slack integration and FR-107 through FR-113 release aggregate remain green.
- Live provider behavior matches the fake-provider contract and safe evidence policy.
- FR-114 is not marked complete until live certification is attached.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Catalog, OAuth intent, connection, and default Trigger | PARTIAL | 2026-07-18 | Codex | Gateway/SourceConnection focused tests pass; live consent remains |
| 2 | Multi-workspace isolation, signed delivery, and recovery | PARTIAL | 2026-07-18 | Codex | Gateway owner/delivery/dedupe tests pass; controlled vertical remains |
| 3 | Disconnect, transfer, migration, and backup | PARTIAL | 2026-07-18 | Codex | Pairing rotation, target claim/ack, Gateway v1→v2, daemon v34→v35 pass; restore drill remains |
| 4 | CLI/Tauri/GUI/RBAC/accessibility/privacy | PARTIAL | 2026-07-18 | Codex | 4 component and 2 Playwright Connections tests pass; live Tauri vertical remains |
| 5 | Repository aggregate and live Slack sandbox | PENDING | — | — | Live portion is intentionally non-CI |
