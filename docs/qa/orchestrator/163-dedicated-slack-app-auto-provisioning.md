---
self_referential_safe: true
---

# Orchestrator - Dedicated Slack App Auto Provisioning

**Module**: Orchestrator / Slack Integration Gateway
**Scope**: Dedicated manifest provisioning, credential handoff, OAuth/delivery isolation, recovery, manifest lifecycle, mode migration, UI, privacy, and compatibility
**Scenarios**: 5
**Priority**: Critical

## Automated Entry Point

```bash
./scripts/qa/test-slack-dedicated-app-provisioning.sh
```

The automated gate uses fake Slack endpoints and the deterministic echo fixture only:

```bash
orchestrator apply --project default \
  -f fixtures/manifests/bundles/slack-managed-dedicated-app-fixture.yaml
```

It does not call Slack or claim live certification. Final evidence requires a clean worktree; `FR115_ALLOW_DIRTY=1` is local iteration only.

---

## Scenario 1: Discover, Validate, Approve, Create, And Install

### Preconditions

- Start a test Gateway and daemon with matching enrollment material and fake Slack API base.
- Apply `fixtures/manifests/bundles/slack-managed-dedicated-app-fixture.yaml`.
- Authenticate as Admin.

### Goal

Prove the visible dedicated path performs one fixed-manifest create, durable credential import, OAuth, and disabled Trigger setup without credential copy/paste.

### Steps

1. Navigate normally to `Sources → Connections`; verify all three provisioning cards are visible and Instant remains labelled recommended.
2. Verify "Dedicated — Private workspace app" explains the Configuration Token and OAuth steps and never advertises a fallback to shared mode.
3. Enter a fixture token in "One-time Configuration Token" and select "Validate manifest".
4. Verify the password field clears before the semantic diff is rendered. Review scope, events, callback origins, App identity, and token rotation.
5. Select "Approve and create app", enter an audit reason, and confirm "Create app".
6. Complete fake OAuth and inspect the resulting connection and default Trigger.

### Expected

- Preview calls validate but not create. Create is impossible until the second Admin confirmation.
- Exactly one App import slot and one App identity exist for the provisioning ID; OAuth starts only after receipt verification.
- Safe output contains App/manifest digests but no Configuration Token, App credential, import/poll/pairing secret, OAuth state/code, or full private endpoint.
- The connection is `managed_dedicated`, `app_ownership=workspace`, `active`, and uses the normal disabled Trigger.

### Expected Data State

```sql
SELECT status, manifest_version, length(manifest_digest),
       app_id_ciphertext IS NOT NULL, length(app_id_digest), oauth_intent_id IS NOT NULL
FROM source_connection_provisioning
WHERE project_id='{project_id}' AND id='{provisioning_id}';
-- Expected: completed | orchestrator-slack-dedicated-v1 | 64 | 1 | 64 | 1

SELECT provisioning_mode, app_ownership, provision_state, state, trigger_name
FROM source_connections
WHERE project_id='{project_id}' AND id='{connection_id}';
-- Expected: managed_dedicated | workspace | completed | active | slack-*
```

---

## Scenario 2: Secret Custody, Receipt Retry, And Cross-App Rejection

### Preconditions

- Create two independent fake provisioning sessions using App/team canaries A and B.

### Goal

Prove bootstrap and App credentials cannot cross connection, project, endpoint, storage, or response boundaries.

### Steps

1. Import App A with slot A, retry the exact import after simulating a lost HTTP response, and compare receipt payload/signature.
2. Attempt slot A against connection B, changed App ID against completed slot A, wrong daemon/project, expired slot, and receipt mutation.
3. Complete OAuth for two different teams and deliver valid events to each dedicated endpoint.
4. Send B-signed bytes to A's endpoint; send A-signed bytes whose `api_app_id` is B; send A/team-B and shared-endpoint canaries.
5. Scan daemon/Gateway SQLite, WAL, logs, audit, CLI output, Tauri payloads, DOM, local/session storage, fixture, and QA artifacts for every secret marker.

### Expected

- Lost-response retry returns the same durable receipt and creates no second App row; every cross-connection/import/receipt variation fails closed.
- Endpoint selection precedes parsing, HMAC uses the selected App's Signing Secret, and verified App/team identity is cross-checked afterward.
- A dedicated App revoke/compromise affects only its connection.
- Plaintext Configuration Token, client secret, Signing Secret, bot token, pairing/import/poll secret, OAuth code/state, raw body, and private URL are absent from retained artifacts.

### Expected Data State

```sql
SELECT connection_id, COUNT(*), COUNT(DISTINCT app_id_digest)
FROM dedicated_apps GROUP BY connection_id HAVING COUNT(*) != 1;
-- Expected: no rows

SELECT installation_id, external_event_id, COUNT(*)
FROM deliveries GROUP BY installation_id, external_event_id HAVING COUNT(*) != 1;
-- Expected: no rows
```

---

## Scenario 3: Crash, Timeout, Attention, Resume, And Abandon

### Goal

Verify every partial boundary is explicit and cannot create a second unmanaged App.

### Steps

1. Inject failure before create, create response timeout/uncertainty, create-before-import receipt, receipt-response loss, OAuth pending expiry, and daemon restart at each boundary.
2. Query `dedicated-status` and reload the Connections page from its safe `{project, provisioning_id}` checkpoint.
3. Where the live daemon still owns credentials, select "Resume secure import"; otherwise verify `attention / provisioning_session_lost` or `provisioning_session_expired`.
4. Repeat status calls and confirm one deduplicated Attention item. Select "Abandon" for a reviewed orphan recovery.
5. Verify no code path calls create again for `creating`, `handoff_pending`, or `attention` without a new explicit provisioning flow.

### Expected

- Before-create failure retains no App credentials. Uncertain create never retries blindly.
- Receipt loss is idempotently recoverable only with the same capability and App identity.
- Attention summary contains only safe IDs/error codes and explains resume-or-abandon; terminal completion/abandon resolves it.
- Configuration Token memory is discarded on success, failure, expiry, abandon, and process exit.

### Expected Data State

```sql
SELECT status, error_code, COUNT(*)
FROM source_connection_provisioning
WHERE id='{provisioning_id}' GROUP BY status,error_code;
-- Expected: exactly one terminal/current checkpoint

SELECT state, occurrence_count, dedupe_key
FROM attention_items
WHERE project_id='{project_id}'
  AND dedupe_key='source-connection-provisioning:{provisioning_id}';
-- Expected: at most one active item; repeated observation increments occurrence_count
```

---

## Scenario 4: App Lifecycle, Mode Migration, Badge Runtime, And RBAC

### Preconditions

- An active shared or dedicated connection exists for the fake Slack team.
- Two fixture badges select different echo Skills/workflows.

### Goal

Prove mode changes retain one logical connection/runtime and all mutation authorization is daemon-enforced.

### Steps

1. Reauthorize a dedicated connection and inspect the OAuth URL client identity and callback path.
2. With a fresh token, export the exact App, preview a semantic manifest diff, apply with the reviewed version, and verify permission expansion produces `suspended / reauthorization_required` plus one Attention item before OAuth.
3. Provision dedicated for a team currently using shared mode; complete OAuth with the exact installation/version/source-mode fence, inject stale and unreviewed callbacks, and verify the original owner remains active until a valid switch commits. Perform the reverse dedicated→shared path and verify the old endpoint/pairing is fenced.
4. Disconnect without deleting the App. Then use a fresh token, typed exact App ID, independent reason/idempotency/version fence, and the dedicated delete action; verify Gateway credentials are retired while local evidence becomes `app_deleted`.
5. Enable the two fixture badge bindings through preview/simulation, deliver both reactions on the same message, verify two distinct deterministic echo tasks, and repeat lifecycle/migration mutations across ReadOnly, Operator, and Admin.

### Expected

- Dedicated reauthorization uses that connection's App client credentials, never the official shared App.
- Upgrade is exact-App/CAS-scoped; secret input is cleared, permission expansion cannot deliver before reauthorization, and lifecycle Attention deduplicates by connection.
- Connection/installation/Trigger and historical source/route/task evidence remain stable; generation/version advance and exactly one owner/pairing is active.
- Disconnect retains the App and all evidence. Delete is unavailable while active and requires the separate typed, audited confirmation.
- `provisioning_mode` does not enter task identity or template variables. Different badge bindings on one message create different automation bindings/tasks, while the same message/reaction/binding identity remains deduplicated.
- ReadOnly can inspect safe state. Dedicated preview/approve/abandon and connection credential mutations require Admin in GUI and direct RPC.

### Expected Data State

```sql
SELECT installation_id, COUNT(*), MAX(generation), MAX(version)
FROM source_connections
WHERE project_id='{project_id}' AND state!='disconnected'
GROUP BY installation_id HAVING COUNT(*) != 1;
-- Expected: no rows

SELECT automation_key, COUNT(DISTINCT task_id)
FROM source_automation_routes
WHERE project_id='{project_id}' GROUP BY automation_key HAVING COUNT(DISTINCT task_id) != 1;
-- Expected: no rows

SELECT conversation_id, thread_id, binding_type,
       COUNT(*) AS bindings, COUNT(DISTINCT task_id) AS tasks
FROM source_bindings
WHERE project_id='{project_id}' AND binding_type='automation'
GROUP BY conversation_id, thread_id, binding_type;
-- Expected for the same-message two-badge fixture: bindings=2 | tasks=2
```

---

## Scenario 5: Migration, UI Accessibility, Aggregate, And Live Certification

### Steps

1. Upgrade populated daemon v34→37 and Gateway v2→4; verify existing shared/manual entities, deliveries, and ownership handoffs remain intact. Exercise transaction recovery and newer-schema rejection.
2. Run Connections Vitest, production build, and Playwright for visible entry, token clearing, diff approval, recovery, popup/reload, keyboard focus/return, axe, reduced effects, and 640 px layout.
3. Run strict workspace Clippy/tests, FR-114 aggregate, FR-113 badge release aggregate, documentation lint, and diagnostic privacy scan.
4. Execute `docs/guide/slack-managed-sandbox-certification-runbook.md` followed by the dedicated addendum in `docs/guide/slack-dedicated-app-provisioning.md` using a non-production workspace and echo-only content.
5. Revoke/discard the Configuration Token, disconnect the installation, delete the sandbox App through reviewed Slack controls, and retain only allowlisted evidence.

### Expected

- Additive migrations preserve populated state and are idempotently recoverable from a partial last migration.
- No serious/critical accessibility violations, hidden Admin mutation, unreachable control, or narrow-layout overflow occurs.
- Automated gates are reproducible from a clean commit; live provider work is clearly separate and never faked by CI.
- Evidence contains only date, commit, manifest digest/version, anonymous App/installation/daemon digests, safe state transitions/request IDs, and pass/fail.

### Expected Data State

```sql
SELECT MAX(version) FROM schema_migrations;
-- Expected: 37

SELECT MAX(version) FROM gateway_schema_migrations;
-- Expected: 4
```

### Controlled Live Certification Record

Only allowlisted, non-provider-identifying evidence is retained:

| Evidence | Result |
|---|---|
| Date / candidate | PASS — 2026-07-22 / `83c063129f2a63e095d8543bd5ac7f9dfe7345f7` |
| Manifest | PASS — `orchestrator-slack-dedicated-v1`; SHA-256 `088a2ab58d6160f64630d5c5f1f927f02a65f44d6075d7a337ac1969a1936c1f` |
| Final aggregate | PASS — all 12 FR-115 gates, including workspace/all-features, strict Clippy, GUI, FR-114 shared OAuth, FR-113 same-message vertical, documentation, and diagnostic privacy |
| Provision / OAuth | PASS — real App create, receipt-gated import, exact-App OAuth, `active / managed_dedicated / workspace`, default disabled Trigger |
| Same-message badges | PASS — two reactions, two routed automation bindings, two distinct completed echo tasks; duplicate-delivery regression remains one route/task |
| Reauthorization / recovery | PASS — exact-App reauthorization advanced the installation generation; one Gateway delivery stayed pending while daemon was offline, then acked after restart and created one completed task |
| Negative canaries | PASS — wrong endpoint plus invalid-signature App/team payload produced zero new deliveries; signed cross-App/App-team cases pass in the automated two-App suite |
| Disconnect / delete | PASS — disconnect preserved App/evidence; separate fresh-token, exact-App typed delete produced `disconnected / app_deleted`; Configuration Token was revoked |
| Privacy / cleanup | PASS — browser, OAuth, tunnel, database, log, and token artifacts were destroyed; retained evidence contains no workspace/channel/user/message/App ID, secret, raw payload, OAuth material, or private URL |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Discover, validate, approve, create, and install | PASS | 2026-07-22 | Codex | Automated GUI/Gateway contract and controlled real Manifest/OAuth provisioning both pass |
| 2 | Secret custody, receipt retry, and cross-App rejection | PASS (automated) | 2026-07-22 | Codex | Two App/team canaries, encrypted storage, exact endpoint lookup, cross-App rejection, and isolated revocation pass |
| 3 | Crash, timeout, Attention, resume, and abandon | PASS (automated) | 2026-07-22 | Codex | Create-once state machine, lost receipt retry, session loss/expiry Attention, resume/abandon, and no-blind-retry checks pass |
| 4 | App lifecycle, mode migration, badge runtime, and RBAC | PASS | 2026-07-22 | Codex | Automated lifecycle/migration/RBAC plus live reauthorization, same-message two-badge routing, offline cursor recovery, disconnect, and exact-App delete pass |
| 5 | Migration, UI accessibility, aggregate, and live certification | PASS | 2026-07-22 | Codex | Daemon 37/Gateway 4 upgrades, strict tests/Clippy, GUI unit/build/Playwright, FR-114/FR-113 regression, doc/privacy gates, and the controlled dedicated Slack cleanup record pass |
