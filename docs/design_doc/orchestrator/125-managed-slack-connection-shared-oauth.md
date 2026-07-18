# Orchestrator - Managed Slack Connection And Shared OAuth

**Module**: Orchestrator / Slack Integration Gateway  
**Status**: Implemented; live Slack certification pending  
**Related Plan**: FR-114  
**Related QA**: `docs/qa/orchestrator/162-managed-slack-connection-shared-oauth.md`  
**Created**: 2026-07-18  
**Last Updated**: 2026-07-18

## Background

FR-107 through FR-113 established the Slack reaction-to-Skill task loop, but an administrator still had to create a Slack app, copy credentials into a SecretStore, and assemble a Trigger. FR-114 adds the fast path: an Orchestrator-operated official Slack app can be installed into many workspaces through standard OAuth, while every installation remains exclusively owned by one daemon and project.

Slack requires public OAuth callback and Events API endpoints, while `orchestratord` is local-first and commonly runs behind NAT. A separate, optional `orchestrator-slack-gateway` therefore owns the internet-facing provider boundary. The daemon only makes outbound requests. Badge matching, templates, task creation, Attention, and audit remain daemon authority.

## Goals

- Offer a visible `Sources → Connections` OAuth flow without credential copy/paste.
- Keep `managed_shared`, reserved `managed_dedicated`, and compatible `manual` provisioning modes in one SourceConnection contract.
- Isolate official app credentials, installation tokens, deliveries, daemon ownership, and projects.
- Persist OAuth intents, connection lifecycle, normalized deliveries, monotonic cursors, and owner-transfer handoffs.
- Create one default managed Trigger with `reactionRouting: disabled`; administrators explicitly configure and enable badge bindings later.
- Preserve the existing manual Slack integration and local-only operation when no Gateway is configured.

## Non-goals

- Automatically creating a private Slack app per workspace; that is FR-115.
- Enterprise Grid org-wide installs, GovSlack, Marketplace distribution, billing, or regional residency.
- Reading message bodies, attachments, threads, or workspace search.
- Sending task progress back to Slack.
- Running binding, rendering, or task mutation logic inside the Gateway.

## Scope

In scope:

- `crates/slack-gateway` service, schema, crypto, OAuth, signed Events API ingress, delivery queue, provider proxy, health, and operator app commands;
- daemon SourceConnection repository, gRPC service, CLI, Tauri commands, outbound reconciliation, and Trigger association;
- Sources/Connections GUI for connect, resumed intent polling, reauthorize, disconnect, role gates, and mode discovery;
- versioned official app manifest and safe provision/validate workflow;
- forward-only Gateway schema versions 1-2 and daemon migration 35.

Out of scope:

- hosted deployment manifests, production Slack credentials, and live workspace consent evidence;
- the `managed_dedicated` runtime; the catalog returns an explicit unavailable reason rather than falling back to shared mode.

## Architecture And Authority

```text
Admin browser ── Slack OAuth consent ───────────────> Slack
      │                                                │
      │ safe intent status                   signed Events API
      v                                                v
local orchestratord ── outbound authenticated HTTPS ─> Slack Gateway
      │                                                ├─ official app secrets
      │                                                ├─ encrypted install tokens
      │                                                ├─ normalized delivery queue
      │                                                └─ transfer handoffs
      v
SourceConnection → Trigger(connectionRef) → source event → binding → task
```

- Slack is authoritative for workspace consent, authorization codes, installation identity, and signed provider events.
- Gateway is authoritative for official app secrets, installation tokens, OAuth state, verified Slack tenant identity, delivery cursors, and the bounded provider proxy.
- Daemon is authoritative for project resources, SourceConnection projection, Trigger, binding, template, route, Attention, audit, and task mutation.
- GUI and CLI can request reviewed operations and display safe projections. They never receive OAuth codes, Slack tokens, signing secrets, raw event bodies, or private provider URLs.

The Gateway and daemon use independent SQLite databases and independent encryption roots. A deployment-level enrollment credential authorizes daemon bootstrap operations; installation-scoped pairing credentials authorize delivery, acknowledgement, proxy, disconnect, and transfer. The enrollment credential is an operator-level secret and must not be distributed to untrusted tenants.

## Interfaces

### Gateway HTTP

The Gateway exposes versioned JSON endpoints:

- `GET /healthz`, `GET /v1/capabilities`;
- `POST /v1/oauth/intents`, `GET /v1/oauth/intents/{id}`, `POST /v1/oauth/intents/{id}/cancel`;
- `GET /slack/oauth/callback`, `POST /slack/events`;
- `POST /v1/deliveries/claim`, `POST /v1/deliveries/ack`;
- `POST /v1/provider/permalink`;
- `POST /v1/installations/disconnect`, `POST /v1/installations/transfer`;
- `POST /v1/installations/transfers/claim`, `POST /v1/installations/transfers/ack`.

Public production origins must be HTTPS. The provider client follows no redirects, enforces bounded bodies/timeouts, validates Slack hosts and permalink coordinates, and surfaces privacy-safe error codes.

### Daemon, CLI, And GUI

The gRPC service and CLI expose catalog, list, get, watch, connect, intent status, cancel, reauthorize, disconnect, and transfer. Mutations require the canonical action envelope and Admin authorization; ReadOnly can inspect safe state.

Examples:

```bash
orchestrator source connection catalog -o json
orchestrator source connection connect --project default --label "Team Slack" -o json
orchestrator source connection status --project default --intent {intent_id} -o json
orchestrator source connection list --project default -o json
```

The GUI stores only `{project_id, intent_id}` in local storage so an interrupted OAuth tab can resume status polling. It does not persist the authorize URL, state, poll secret, installation pairing, or provider link.

## Data Model And State Machines

Daemon migration 35 adds `source_connections`, `source_connection_intents`, and `source_connection_changes`. A SourceConnection contains safe identity digests, exclusive owner, generation/version fences, mode, capabilities, scopes, Trigger reference, delivery cursor/lag, error code, and timestamps. The encrypted Gateway pairing envelope is internal-only.

Gateway schema version 1 adds official app credentials, OAuth intents, installations, normalized deliveries, and audit. Version 2 adds durable owner-transfer handoffs. All migrations are additive and forward-only.

OAuth intent transitions are `pending → completed | cancelled | failed`. State and poll secrets are random, short-lived, single-use, context-bound digests. Reauthorization rotates the bot token and pairing, increments installation generation/version, and invalidates the old generation.

Connection lifecycle is `connecting → active`, with controlled transitions to `attention`, `suspended`, `revoked`, or `disconnected`. Disconnect destroys local and Gateway access material while retaining safe execution evidence.

### Two-phase Ownership Transfer

1. The old owner sends its installation pairing and expected version to the Gateway.
2. Gateway atomically changes the owner, rotates pairing, clears delivery leases, increments version, and stores the replacement pairing in an encrypted target handoff.
3. The old daemon persists `suspended/owner_transfer_pending_acceptance`, changes owner, and clears its local pairing. The replacement credential is never returned to the old daemon.
4. The target daemon polls with its configured enrollment identity, validates the projection, creates or idempotently adopts the SourceConnection and default Trigger, encrypts the replacement pairing locally, preserves the Gateway cursor, and acknowledges the handoff.
5. Repeated claim/adoption after a crash is version/generation fenced and idempotent. The handoff is deleted only after target acknowledgement.

This creates a deliberate suspended window instead of permitting simultaneous active owners. A target that lacks the project/workflow/workspace prerequisites leaves the handoff durable and emits a stable deferred-adoption diagnostic.

## Official App Contract

`deploy/slack/official-app-manifest.json` is the reviewed, secret-free manifest. The operator binary renders only environment endpoint values and validates installed app configuration against the exact scope/event/redirect contract. Configuration tokens are supplied at execution time and are never printed or stored as a normal daemon SecretStore.

The initial contract requests `reactions:read`, subscribes to `reaction_added`, and uses OAuth V2. Any scope, event, callback host, or Request URL change requires explicit review because it changes the provider security boundary.

## Event Delivery And Provider Proxy

Gateway verifies Slack timestamp and HMAC over raw bytes before parsing. It persists only allowlisted normalized fields: event identity, team/enterprise digest, actor, reaction, channel, message timestamp, and event timestamp. It does not retain raw payloads or message content.

Slack receives success only after durable enqueue. The daemon claims bounded batches using the installation pairing, validates owner/generation/tenant/cursor, ingests with the existing external-event dedupe, and acknowledges monotonic cursors. The permalink proxy exposes only `chat.getPermalink`; response URLs must be HTTPS Slack hosts and contain the requested channel coordinate.

## UI Design And Accessibility

Connections is the default Sources subsection. Three cards remain visible:

- **Instant — Official Orchestrator App**: available only when Gateway capability negotiation succeeds;
- **Dedicated — Private app for this workspace**: explicit FR-115 unavailable state;
- **Existing app — Manual credentials**: points to the compatible manual setup.

Connection rows expose safe state, generation, scopes, Trigger, lag/error, and role-appropriate actions. Dialogs use labelled controls, keyboard focus management, explicit destructive confirmation, narrow-layout support, and reduced-motion behavior from the project design system. Provider secrets and private workspace names are never rendered.

## Alternatives And Tradeoffs

- Direct Slack callbacks to each daemon avoid a shared service but break the one-click NAT-safe experience and multiply public secret-bearing endpoints.
- One private Slack app per workspace improves branding and blast-radius isolation but requires Slack configuration-token lifecycle and asynchronous app provisioning. The stable `managed_dedicated` mode reserves this future without weakening the shared fast path.
- Storing OAuth tokens in daemon SecretStore would simplify proxy calls but would spread provider credentials across local installations. Gateway custody keeps the official app boundary centralized and lets the daemon retain only an installation pairing.
- Returning a replacement pairing to the old owner makes transfer simpler but violates target credential custody. Durable target claim/ack adds a suspended interval and migration, but fail-closes crashes and prevents credential forwarding through the old daemon.

## Risks And Mitigations

- **Gateway compromise affects many installations**: separate deployment, encryption root, least-scope manifest, installation pairing, bounded proxy, audit, rate/size limits, and an explicit threat model.
- **Cross-tenant delivery**: verified Slack identity maps to one installation; every claim validates daemon owner, installation, generation, tenant digest, and cursor.
- **OAuth replay or confused deputy**: random single-use state binds daemon/project/actor/scopes/redirect; callback identity comes from Slack, not browser input.
- **Offline daemon loses reactions**: Gateway persists before acknowledging Slack and resumes from the last acknowledged cursor.
- **Duplicate tasks**: at-least-once delivery converges through source event ID and the existing automation identity.
- **Transfer crash creates two owners**: Gateway changes ownership once, old pairing is revoked, old daemon clears its credential, and target activation requires the replacement pairing.
- **Secret leakage**: encrypted-at-rest credentials, redacted debug output, safe projections, stable error codes, log scans, and no credential fields in proto/UI types.
- **Shared enrollment secret misuse**: treat it as privileged operator bootstrap material, rotate it independently, restrict network access, and do not share it with tenant users. Per-daemon enrollment credentials are a future hardening option.

## Observability And Operations

Recommended default alerts:

- OAuth intents by terminal error code and age;
- active/attention/suspended/revoked connection count;
- delivery lag and oldest pending delivery age;
- provider proxy error/rate-limit latency;
- transfer handoff age and deferred adoption count;
- signature, owner, generation, and cursor rejections as low-cardinality counters.

Logs and audit include request/intent/installation IDs or stable digests, generation/version, state, and safe error codes. They exclude credentials, OAuth code/state, raw Slack bytes, workspace names, message URLs, and rendered task goals.

Deploy Gateway with its own database backup, master-key recovery, TLS termination, endpoint allowlist, and retention policy. Upgrade Gateway schema before enabling a daemon capability that depends on it. Stop-loss disables managed connection creation/delivery while preserving Gateway queue and daemon source/task evidence. Normal rollback is forward-only; restore backups only for migration failure or corruption.

## Testing And Acceptance

Automated acceptance is defined in `docs/qa/orchestrator/162-managed-slack-connection-shared-oauth.md` and `scripts/qa/test-slack-managed-shared-oauth.sh`. It covers schema/state fencing, OAuth failure contracts, provider verification, durable delivery, transfer claim/ack, CLI/Tauri/UI/RBAC/privacy, migration compatibility, FR-113 regression, and repository quality gates.

A controlled Slack sandbox certification remains a separate, non-CI gate because it requires real workspace consent and external credentials. FR-114 must remain In Progress until that evidence is recorded without private workspace data.

