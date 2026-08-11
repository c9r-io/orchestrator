---
lifecycle: active
related_fr: FR-115
---

# Orchestrator - Dedicated Slack App Auto Provisioning

**Module**: Orchestrator / Slack Integration Gateway
**Status**: Released; controlled Slack lifecycle certification complete
**Related Plan**: FR-115
**Related QA**: `docs/qa/orchestrator/163-dedicated-slack-app-auto-provisioning.md`
**Created**: 2026-07-19
**Last Updated**: 2026-07-22

## Background

FR-114 provides the lowest-friction Slack connection by installing one official Orchestrator App into many workspaces. Some organizations instead require a workspace-owned App identity, isolated client and signing credentials, an independent event URL, and revocation that cannot affect another tenant. FR-115 adds that advanced path without forking the SourceConnection, Trigger, badge matcher, template, route, or task runtime.

Slack cannot create an App through installation OAuth. A user must first generate a short-lived App Configuration Token; the daemon uses it with the App Manifest API, transfers only the newly created App credentials to the Gateway, then starts normal OAuth. The Configuration Token has broader user/workspace authority than an App token, so it is never treated as a normal stored secret.

## Goals

- Create one private Slack App per selected workspace from a fixed, versioned manifest.
- Keep the Configuration Token only in zeroizing local daemon memory.
- Encrypt every App credential with a connection-specific Gateway context before OAuth starts.
- Route each public event endpoint to one candidate Signing Secret, then cross-check verified App/team identity.
- Preserve one logical SourceConnection and task history when switching between shared and dedicated modes.
- Expose manifest review, explicit Admin approval, recovery checkpoints, Attention, CLI stdin, and an accessible GUI flow.

## Non-goals

- Bypassing Slack administrator participation, OAuth consent, or enterprise policy.
- Retaining Configuration Token refresh tokens or scanning a user's other Slack Apps.
- Arbitrary user-supplied scopes, events, redirect URLs, or manifest fields.
- Enterprise Grid org-wide deployment, GovSlack, Marketplace, or cross-region key replication.
- Claiming that Slack supports an API for in-place Signing Secret or Client Secret regeneration. It does not; credential replacement is a reviewed new-App migration followed by retirement of the old App.

## Scope

In scope:

- daemon migrations 36-37 and secret-free provisioning/lifecycle checkpoints;
- Gateway schemas 3-4, one-time import capabilities, per-App encryption contexts, signed durable receipts, dedicated OAuth, exact event endpoints, and reviewed mode-migration fences;
- fixed `crates/daemon/assets/dedicated-app-manifest.json` authority;
- gRPC, CLI, Tauri, and `Sources → Connections` provisioning/recovery surfaces;
- shared↔dedicated convergence through the existing unique Slack team installation;
- fake Slack Manifest/OAuth/Event tests and controlled live certification.

Out of scope:

- automatically deleting the workspace-owned App on SourceConnection disconnect;
- an invented provider credential-rotation API. Rotation uses replacement provisioning and the same reviewed migration invariant;
- live provider secrets or workspace identifiers in repository evidence.

## UI Interactions

- Entry: `Sources → Connections`.
- Modes: "Instant — Official Orchestrator App", "Dedicated — Private workspace app", and "Existing app — Manual credentials" remain visible together.
- Dedicated controls: "Dedicated connection label", password-style "One-time Configuration Token", "Validate manifest", "Approve and create app", "Resume secure import", "Open Slack consent", and "Abandon".
- The token field has `autocomplete=off`, is cleared before the preview promise resolves, and is never written to browser storage. Only `{project, provisioning_id}` is retained for safe recovery.
- Permission expansion is shown in a semantic manifest diff before the focus-trapped reviewed-action dialog permits creation.

## Interfaces

### Daemon gRPC

- `SourceConnectionDedicatedPreview`: validates a fixed manifest using a Configuration Token and creates a safe checkpoint.
- `SourceConnectionDedicatedApprove`: performs create → durable Gateway import receipt → dedicated OAuth intent.
- `SourceConnectionDedicatedGet`: returns safe state and turns expired/lost unsafe sessions into Attention.
- `SourceConnectionDedicatedAbandon`: zeroizes the live session and terminally abandons the checkpoint.
- `SourceConnectionMigrateToShared`: creates an official-App OAuth intent bound to the exact dedicated installation, version, and source mode.
- `SourceConnectionDedicatedUpgradePreview/Apply`: exports the exact App, validates and diffs the fixed target manifest, applies only after a second review, and suspends for OAuth when permissions expand.
- `SourceConnectionDedicatedDelete`: deletes only a disconnected exact App after a fresh Configuration Token, typed App ID, Admin reason, and CAS check.
- Existing `SourceConnectionIntentGet`, `SourceConnectionReauthorize`, `SourceConnectionDisconnect`, and `SourceConnectionTransfer` operate on both managed modes. Dedicated reauthorization selects its exact App credentials.

CLI accepts the Configuration Token only through `--config-token-stdin`. It is deliberately unsupported in argv, environment options, files, or safe output.

### Gateway HTTP

- `POST /v1/dedicated/import-slots`: enrollment-authenticated, expiring connection-scoped import capability.
- `POST /v1/dedicated/import`: one-time App credential import with a signed durable receipt and dedicated OAuth intent.
- `POST /v1/dedicated/oauth/intents`: exact-App reauthorization.
- `POST /v1/dedicated/apps/manifest`: exact-owner/App manifest metadata update with installation version advancement.
- `POST /v1/installations/suspend`: pairing-authenticated suspension before permission-expanded OAuth.
- `POST /v1/dedicated/apps/delete`: retires encrypted App credentials only after the installation is disconnected.
- `GET /slack/connections/{connection_id}/oauth/callback`.
- `POST /slack/connections/{connection_id}/events`.

Public payloads never expose client secret, Signing Secret, import secret, poll secret, installation token, OAuth code/state, or full private endpoint.

## Database Changes

Daemon migration 36 adds safe App/provision fields to `source_connections` and `source_connection_provisioning`. Migration 37 adds the optional reviewed migration target to the provisioning checkpoint. The checkpoint stores manifest digest/version, safe state/error, App ID digest, OAuth intent reference, and an encrypted exact App ID used only for governed lifecycle recovery. It never stores the Configuration Token or returned App credentials.

Gateway schema 3 adds mode/App metadata to intents/installations plus `dedicated_import_slots` and `dedicated_apps`. Schema 4 adds `migration_installation_id`, `migration_expected_version`, and `migration_source_mode` to OAuth intents. Client ID, Client Secret, and Signing Secret are encrypted under `dedicated-app:{connection_id}:generation:{n}:{field}` contexts. Import secret and App/team identities are stored as purpose-scoped digests where plaintext is unnecessary.

Both migrations are additive and forward-only. Populated daemon v34 and Gateway v2 upgrades preserve existing shared/manual connections; older binaries may continue delivery but cannot create new dedicated Apps. For the daemon side that is the contract in `crates/orchestrator-persistence/src/migration.rs`; the Gateway keeps its own schema in its own database and states the equivalent rule separately.

## Key Design

1. **Local token authority**: only the daemon calls Slack Manifest APIs. The GUI/Tauri layer transports the token once and clears it; Gateway never receives it.
2. **Fixed manifest authority**: repository code renders only two deployment endpoints. The reviewed scope/event profile remains immutable input.
3. **Create-once checkpoint**: after `apps.manifest.create` begins, failures never trigger an automatic second create. A live process can resume credential import; a lost process becomes `attention` with orphan review guidance.
4. **Receipt-before-OAuth**: OAuth begins only after Gateway encryption commits and the daemon verifies a receipt HMAC over connection, App digest, generation, and manifest digest.
5. **Authenticated endpoint selection**: the opaque path selects one candidate secret; Slack HMAC is still mandatory, and verified `api_app_id` plus team identity must match the endpoint's App/installation.
6. **Runtime convergence**: Gateway's unique team digest keeps one logical installation. Reauthorization or a reviewed shared↔dedicated replacement updates that installation, so connection ID, Trigger, route/task dedupe, and evidence remain stable.
7. **Lifecycle review sessions**: upgrade secrets and exact App identity live only in a ten-minute, in-memory daemon session. Apply is CAS-fenced to the reviewed connection version; permission expansion atomically advances Gateway/local versions, suspends delivery, emits one Attention item, and starts exact-App OAuth.
8. **Delete is not disconnect**: disconnect destroys installation access but retains App/evidence. Delete requires a fresh token and typed exact App ID, verifies the App through Slack, then retires the Gateway credential envelope and records `app_deleted` without erasing history.
9. **Same-message badge fan-out**: primary/related source correlations remain exclusive, but each reserved automation route owns an idempotent automation binding identity. Different reviewed badges on one Slack message can therefore create different tasks, while retries for the same message/reaction/binding still converge on one route and task.

## Alternatives And Tradeoffs

- One official App is simpler and remains the recommended instant path, but has a wider App-identity blast radius.
- A private App per workspace improves isolation and ownership at the cost of a Configuration Token step, more credentials, and lifecycle operations.
- Persisting Configuration Tokens would simplify crash recovery but creates an unacceptable broad workspace-management secret. The design accepts a bounded orphan-Attention case instead.
- Letting Gateway generate manifests would centralize provisioning but would make an internet-facing credential service authoritative for permission expansion. Manifest authority remains versioned in the daemon/repository.

## Risks And Mitigations

- Configuration Token leakage: password/stdin-only entry, immediate UI clearing, `Zeroizing<String>`, no persistence/log/audit fields, and retained-artifact scans.
- Cross-App signature confusion: unique endpoint lookup before parsing, raw-body signature verification, then App/team cross-check.
- Lost create response or daemon crash: safe checkpoint and Attention; no blind retry or scan of other user Apps.
- Lost import response: the same connection capability and App identity return the same durable receipt; cross-connection or changed-App retries fail closed.
- Duplicate paid work during migration: one team installation, one active pairing generation, existing source-event and automation idempotency.
- Gateway compromise: per-connection encryption context limits accidental/cross-row disclosure, but Gateway host compromise remains a centralized risk requiring isolation, key management, and audit.

## Observability

- Logs/audit: request ID, provisioning/connection ID, safe state, manifest version/digest, App ID digest, generation/version, and stable error code only.
- Attention dedupe key: `source-connection-provisioning:{provisioning_id}`.
- Recommended metrics: provisioning state/age, manifest API latency/rate-limit errors, import receipt retries, OAuth completion latency, dedicated delivery lag, and cross-App rejection count.
- Never retain token prefixes, credentials, raw Slack payload, OAuth material, workspace/user identity, or full dedicated callback/event URL.

## Operations / Release

- Configure the existing Gateway origin/enrollment secret and daemon Slack API base. Production provider/Gateway origins require HTTPS; loopback HTTP is test-only.
- Upgrade Gateway schema before enabling daemon dedicated capability. Back up Gateway and daemon databases with their independent keys.
- Stop-loss: disable new provisioning while preserving existing delivery. A provisioning Attention item must be reviewed; do not rerun create blindly.
- Disconnect revokes the installation and retains the workspace-owned App. App replacement, manifest upgrade, and retirement require a fresh Configuration Token and explicit reviewed operations.
- Schema rollback is fail-closed: an older Gateway refuses schema 4 and an older daemon refuses migration 37. During a binary rollback, keep the upgraded stores intact, disable new provisioning/lifecycle mutations, and restore the compatible binary; existing delivery continues only on a binary that understands the current schema.
- Live certification is intentionally paired with FR-114 because both modes share the same real callback, OAuth, delivery, Trigger, and badge runtime boundary.
- FR-123 supplies the shared/dedicated/combined checkpoint controller, safe
  evidence TTL, and cleanup inventory without changing this product boundary.
  Use `scripts/qa/certify-slack-managed-live.sh run --mode dedicated|both`;
  QA-173 governs continued certification.

## Test Plan

- Unit: manifest contract, fake Manifest API lifecycle, migration recovery, connection checkpoints, receipt retry/cross-connection rejection, exact App reauthorization, signature/App/team isolation, token-safe debug/output, and UI storage clearing.
- Integration: daemon/Gateway fake-provider create → import → OAuth → disabled Trigger; restart and partial-failure checkpoints; two App/team canaries.
- E2E: navigation, three-mode tradeoffs, password clearing, diff approval, OAuth resume, Attention recovery, RBAC, focus, narrow layout, and accessibility scan.
- Live: one controlled dedicated App plus two badge-to-Skill echo tasks, followed by token revocation and App cleanup under the combined FR-114/FR-115 runbook.

## Controlled Live Certification

The 2026-07-22 controlled sandbox run used candidate `83c063129f2a63e095d8543bd5ac7f9dfe7345f7` and manifest `orchestrator-slack-dedicated-v1` with SHA-256 `088a2ab58d6160f64630d5c5f1f927f02a65f44d6075d7a337ac1969a1936c1f`.

- Real Manifest API provisioning, receipt-gated import, dedicated OAuth, exact-App reauthorization, and disabled-Trigger creation passed.
- Two different badges on one synthetic message converged to two routed automation bindings and two distinct completed echo tasks. The live run exposed and repaired the prior exclusive automation-correlation key; the deterministic vertical suite now fixes both badges to the same message and still proves duplicate delivery convergence.
- A real Reaction delivered while the daemon was stopped remained pending in the Gateway, was acknowledged after daemon restart, and produced one additional completed task from the restored cursor.
- Wrong endpoint and invalid-signature App/team canaries produced no delivery. Strong signed cross-App/App-team isolation remains covered by the two-App automated contract suite.
- Disconnect retained the workspace-owned App and evidence. A separate fresh-token, exact-App, typed-confirmation delete moved the retained state to `app_deleted`; the Configuration Token was then revoked.
- All browser, OAuth, database, tunnel, and log artifacts were destroyed after the allowlisted state/count evidence was recorded. No workspace, channel, user, message, App ID, OAuth URL, token, credential, raw body, or private endpoint is retained.

## QA Docs

- `docs/qa/orchestrator/163-dedicated-slack-app-auto-provisioning.md`
- `docs/guide/slack-dedicated-app-provisioning.md`

## Acceptance Criteria

- Dedicated provisioning validates and creates one fixed-profile App, durably imports its credentials, and starts exact-App OAuth without manual credential copy.
- Configuration Tokens and App credentials are absent from persistent stores, projections, logs, audit, DOM/storage, and artifacts.
- Two Apps/workspaces use different identities, signing secrets, event endpoints, and encryption contexts; every cross-App attempt fails.
- Partial create/import states are retry-safe where provable and otherwise produce deduplicated human Attention without blind App recreation.
- Dedicated OAuth creates the normal disabled Trigger and the existing two-badge runtime has no mode-specific task semantics.
- Shared/manual compatibility, strict Clippy, workspace tests, frontend unit/build/E2E, security/doc lint, and populated upgrades pass.
- Controlled Slack sandbox provisioning, badge routing, reauthorization, offline cursor recovery, disconnect, reviewed App deletion, token revocation, and privacy cleanup are certified.
