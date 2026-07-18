# Orchestrator - Dedicated Slack App Auto Provisioning

**Module**: Orchestrator / Slack Integration Gateway
**Status**: Initial provisioning implemented; App lifecycle and controlled Slack certification pending
**Related Plan**: FR-115
**Related QA**: `docs/qa/orchestrator/163-dedicated-slack-app-auto-provisioning.md`
**Created**: 2026-07-19
**Last Updated**: 2026-07-19

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

- daemon migration 36 and secret-free provisioning checkpoints;
- Gateway schema 3, one-time import capabilities, per-App encryption contexts, signed durable receipts, dedicated OAuth, and exact event endpoints;
- fixed `deploy/slack/dedicated-app-manifest.json` authority;
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
- Existing `SourceConnectionIntentGet`, `SourceConnectionReauthorize`, `SourceConnectionDisconnect`, and `SourceConnectionTransfer` operate on both managed modes. Dedicated reauthorization selects its exact App credentials.

CLI accepts the Configuration Token only through `--config-token-stdin`. It is deliberately unsupported in argv, environment options, files, or safe output.

### Gateway HTTP

- `POST /v1/dedicated/import-slots`: enrollment-authenticated, expiring connection-scoped import capability.
- `POST /v1/dedicated/import`: one-time App credential import with a signed durable receipt and dedicated OAuth intent.
- `POST /v1/dedicated/oauth/intents`: exact-App reauthorization.
- `GET /slack/connections/{connection_id}/oauth/callback`.
- `POST /slack/connections/{connection_id}/events`.

Public payloads never expose client secret, Signing Secret, import secret, poll secret, installation token, OAuth code/state, or full private endpoint.

## Database Changes

Daemon migration 36 adds safe App/provision fields to `source_connections` and `source_connection_provisioning`. The checkpoint stores manifest digest/version, safe state/error, App ID digest, OAuth intent reference, and an encrypted exact App ID used only for governed lifecycle recovery. It never stores the Configuration Token or returned App credentials.

Gateway schema 3 adds mode/App metadata to intents/installations plus `dedicated_import_slots` and `dedicated_apps`. Client ID, Client Secret, and Signing Secret are encrypted under `dedicated-app:{connection_id}:generation:{n}:{field}` contexts. Import secret and App/team identities are stored as purpose-scoped digests where plaintext is unnecessary.

Both migrations are additive and forward-only. Populated daemon v34 and Gateway v2 upgrades preserve existing shared/manual connections; older binaries may continue delivery but cannot create new dedicated Apps.

## Key Design

1. **Local token authority**: only the daemon calls Slack Manifest APIs. The GUI/Tauri layer transports the token once and clears it; Gateway never receives it.
2. **Fixed manifest authority**: repository code renders only two deployment endpoints. The reviewed scope/event profile remains immutable input.
3. **Create-once checkpoint**: after `apps.manifest.create` begins, failures never trigger an automatic second create. A live process can resume credential import; a lost process becomes `attention` with orphan review guidance.
4. **Receipt-before-OAuth**: OAuth begins only after Gateway encryption commits and the daemon verifies a receipt HMAC over connection, App digest, generation, and manifest digest.
5. **Authenticated endpoint selection**: the opaque path selects one candidate secret; Slack HMAC is still mandatory, and verified `api_app_id` plus team identity must match the endpoint's App/installation.
6. **Runtime convergence**: Gateway's unique team digest keeps one logical installation. Reauthorization or a reviewed shared↔dedicated replacement updates that installation, so connection ID, Trigger, route/task dedupe, and evidence remain stable.

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
- Disconnect revokes the installation and retains the workspace-owned App. App replacement/retirement requires a fresh Configuration Token and explicit Slack-side review.
- Live certification is intentionally paired with FR-114 because both modes share the same real callback, OAuth, delivery, Trigger, and badge runtime boundary.

## Test Plan

- Unit: manifest contract, fake Manifest API lifecycle, migration recovery, connection checkpoints, receipt retry/cross-connection rejection, exact App reauthorization, signature/App/team isolation, token-safe debug/output, and UI storage clearing.
- Integration: daemon/Gateway fake-provider create → import → OAuth → disabled Trigger; restart and partial-failure checkpoints; two App/team canaries.
- E2E: navigation, three-mode tradeoffs, password clearing, diff approval, OAuth resume, Attention recovery, RBAC, focus, narrow layout, and accessibility scan.
- Live: one controlled dedicated App plus two badge-to-Skill echo tasks, followed by token revocation and App cleanup under the combined FR-114/FR-115 runbook.

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
- Formal FR closure waits for the controlled Slack sandbox record required by FR-114 and FR-115.
