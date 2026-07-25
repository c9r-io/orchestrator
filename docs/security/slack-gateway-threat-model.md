---
lifecycle: active
---

# Slack Integration Gateway Threat Model

**Scope**: FR-114 shared OAuth, FR-115 dedicated Slack App provisioning,
FR-123 continuous sandbox certification, and SourceConnection delivery
**Assessment date**: 2026-07-25
**Target**: `crates/slack-gateway`, daemon Gateway client/reconciler,
SourceConnection control plane, Connections UI, and the opt-in live
certification harness

## Executive Summary

The Slack Integration Gateway is an internet-facing, multi-installation and multi-App credential boundary. Its highest risks are theft of official/dedicated App credentials or installation tokens, cross-workspace/App event delivery, OAuth confused-deputy/replay, forged Slack events, broad Configuration Token exposure, partial App creation, and ownership-transfer credential exposure. The implementation reduces these risks with local-only zeroizing Configuration Token use, a fixed manifest, one-time connection-scoped import capabilities, signed durable receipts, per-connection encryption contexts, strict OAuth binding, raw-body Slack verification, App/team cross-checks, installation-scoped pairing, owner/generation/version/cursor fencing, minimal normalized storage, a bounded provider proxy, and two-phase target-side transfer adoption.

The most important deployment assumption is that `SLACK_GATEWAY_ENROLLMENT_KEY` is privileged platform bootstrap material shared only by trusted Orchestrator deployments. It is not tenant authentication. Compromise of that key permits intent creation and pending transfer claim impersonation, although it does not directly authorize normal installation delivery/proxy calls. A future per-daemon enrollment credential would reduce this residual blast radius.

## System And Trust Boundaries

```text
[Untrusted browser] ---- OAuth navigation ----> [Slack]
        |                                         |
        | daemon gRPC/Tauri                       | public callback/events
        v                                         v
[Local daemon boundary] ---- outbound HTTPS ---- [Gateway boundary]
        |                                         |
        | encrypted local pairing                 | encrypted app/token DB
        v                                         v
[Daemon SQLite/config]                       [Gateway SQLite]
```

Trust boundaries:

1. Public internet to Gateway OAuth callback and Events API.
2. Slack provider to Gateway, authenticated by OAuth state or Slack raw-body signature.
3. Daemon to Gateway, authenticated by operator enrollment or installation pairing.
4. Gateway database/master key to the running Gateway process.
5. Local GUI/CLI to daemon gRPC authorization and canonical action audit.
6. Normalized source event to daemon binding/template/task mutation.
7. Operator-owned live environment/private checkpoint state to allowlisted
   certification evidence and CI/release status.

TLS may terminate at a trusted reverse proxy. The proxy must preserve the exact raw body and prevent external header spoofing. Gateway and daemon databases, keys, identities, and backups are independent.

## Assets

- Official Slack app configuration token, client secret, and signing secret.
- Short-lived user/workspace App Configuration Token and each dedicated App's client/signing credentials.
- Per-installation bot tokens and pairing credentials.
- OAuth state, poll secret, authorization code, and owner mapping.
- Verified team/enterprise identity, delivery cursor, and normalized reaction metadata.
- Daemon project ownership, SourceConnection state, Trigger, source provenance, and audit.
- Availability of Slack event acknowledgement, durable backlog, and provider proxy.
- Live certification env values, private cleanup identifiers, checkpoint state,
  and reviewed safe evidence.

## Attacker Capabilities

- Send arbitrary HTTP bodies/headers and replay previously observed requests to public Gateway endpoints.
- Control an untrusted browser and modify GUI/local-storage values.
- Possess a ReadOnly or Operator daemon identity and call RPCs directly.
- Install the official app in a workspace they administer and generate valid events for that workspace.
- Observe non-secret logs/metrics and cause network failures, retries, reordering, or daemon restarts.
- Obtain one installation pairing through compromise of that installation's daemon.

Not assumed: compromise of Slack, the Gateway host/root account, the Gateway master key, or the privileged enrollment credential. These are high-impact platform compromises handled by key rotation, revocation, backup recovery, and incident response rather than tenant isolation alone.

## Threats And Mitigations

| ID | Threat / abuse path | Impact | Implemented controls | Residual risk / follow-up |
|---|---|---|---|---|
| T1 | Forge or replay Slack events | Unauthorized task creation | HMAC over exact raw body, timestamp tolerance, bounded body, allowlisted event parser, durable external-event dedupe | Trusted proxy must not transform body; rotate signing secret after exposure |
| T2 | OAuth state theft, replay, redirect or scope confusion | Installation attached to wrong project/daemon | 256-bit random state/poll secrets, stored digests, TTL, one-time consumption, exact redirect/scopes, Slack-returned tenant identity, owner conflict rejection | Browser can abandon an intent; expiry/cancel cleanup remains operational |
| T3 | Cross-workspace delivery/proxy using another tenant ID | Data disclosure or wrong task | Installation-scoped pairing, daemon owner and project mapping, verified team/enterprise digests, generation/version/cursor fences, no caller-supplied project lookup in Slack ingress | Gateway compromise bypasses application controls |
| T4 | Official app or bot token disclosure at rest/in logs | Multi-workspace or installation takeover | Context-bound authenticated encryption, separate master key, file mode restriction, redacted Debug, no secret projection, privacy scans, bounded audit fields | SQLite and master key on same compromised host can be combined; use external secret/HSM backend for high assurance |
| T5 | SSRF/open proxy through provider endpoints | Internal network access | Fixed Slack API base in production, strict HTTPS/host validation, no redirects, only reviewed permalink method, response coordinate validation, timeouts/body limits | Operator-controlled test base supports loopback intentionally |
| T6 | Delivery ack before persistence or cursor manipulation | Lost or skipped reactions | Persist-before-Slack-ack, durable queue, leases, monotonic cursor, owner pairing, daemon ingest before Gateway ack, cursor preserved during transfer | Retention expiry/gap Attention needs production policy validation |
| T7 | Old owner retains access after transfer | Simultaneous owners or secret theft | Gateway CAS changes owner and rotates pairing, clears leases, replacement stored in target handoff, old daemon clears local credential, target claim/ack, idempotent adoption | Privileged enrollment key can impersonate target; per-daemon enrollment recommended |
| T8 | Concurrent connect/reauthorize/disconnect/transfer | Duplicate connection, stale token, state corruption | Unique team digest, single-use intent, generation/version CAS, idempotency/action request IDs, forward-only changes, fail-closed conflicts | Operational reconciliation is needed if Gateway succeeds and daemon persistence permanently fails |
| T9 | ReadOnly/Operator bypasses GUI controls | Credential ownership mutation | Daemon RPC role map requires Admin for all connection mutations; canonical audit and CAS are enforced server-side | UDS deployments must configure an Admin-capable policy for intended administrators |
| T10 | Private Slack data leaks through normalized events/UI/logs | Workspace confidentiality loss | No message body/raw payload persistence, digested tenant identity, safe proto/UI projections, no URL in safe connection state, stable error codes, DOM/storage/log scans | Channel/message coordinates remain sensitive inside protected source routing evidence |
| T11 | Gateway request flood or large response | Availability/resource exhaustion | Request body cap, fixed-window intent limiter, provider/client timeouts, bounded response bytes, bounded delivery batch and claim lease | Distributed rate limiting and upstream WAF are deployment responsibilities |
| T12 | Migration/backup/key failure destroys evidence or credentials | Prolonged outage/data loss | Independent forward-only migrations, populated upgrade tests, SQLite integrity/backup procedures, separate key recovery, no cross-database deletion | Live restore drill and external key backend certification remain pending |
| T13 | Configuration Token is persisted, logged, forwarded, or reused to manage unrelated Apps | Broad workspace App takeover | GUI password/stdin-only entry, immediate field clearing, local daemon `Zeroizing`, no SecretStore/SQLite/Gateway/audit field, fixed allowlisted Manifest calls, no refresh-token custody, artifact scans | A compromised daemon process can read live memory; use a dedicated admin workstation and revoke promptly |
| T14 | Create succeeds but response/import fails, causing blind duplicate Apps | Orphan Apps, credential confusion | Durable safe checkpoint, receipt-before-OAuth, exact same-App receipt retry, in-memory import resume, Attention on uncertain/lost sessions, no automatic create retry or user-App scan | A daemon crash after Slack create but before receipt requires human orphan review |
| T15 | Dedicated event endpoint selects another App's Signing Secret or trusts unverified `api_app_id` | Cross-tenant task creation | Unique connection endpoint selects one candidate secret, HMAC over raw bytes, then verified App/team/mode cross-check; shared and dedicated selectors are disjoint | Gateway host compromise can bypass lookup isolation |
| T16 | Import capability/receipt is replayed across connection, daemon, project, or manifest | Credential overwrite or OAuth confusion | Random expiring import secret digest, connection/owner/project/manifest binding, one-time transaction, App digest/generation receipt HMAC, changed/cross-boundary retries fail closed | Deployment-wide enrollment key remains a privileged bootstrap root |
| T17 | Shared↔dedicated replacement produces two active consumers | Duplicate paid work | Unique verified-team installation, one active App connection reference and pairing generation, old endpoint/mode lookup fails after switch, source/route/task idempotency remains mode-neutral | Live failure-injection certification is required before production migration |
| T18 | A stale or cross-App lifecycle request updates the wrong manifest, or expanded scopes continue on old consent | Cross-tenant App mutation or undeclared provider access | Fresh local-only token, exact encrypted App ID, export-before-review, semantic diff, ten-minute in-memory lifecycle session, project/connection/version CAS, exact App digest at Gateway, suspension plus deduplicated Attention before reauthorization | A provider success followed by a permanent local/Gateway failure still requires operator reconciliation from safe audit IDs |
| T19 | Disconnect is confused with App deletion, or a destructive request deletes a still-active/wrong App | Irrecoverable App loss or retained credential exposure | Delete is a separate Admin RPC/UI/CLI action; requires disconnected state, fresh token, typed exact App ID, reason/idempotency/version fence, provider verification, then encrypted Gateway credential retirement while local evidence remains | Slack-side manual deletion can bypass Orchestrator audit; restrict App collaborators and reconcile provider state |
| T20 | The live certifier evaluates a malicious env file, exports all secrets to a child, leaks provider material into artifacts, or silently abandons external objects | Host command execution, credential theft, or orphaned App/workspace/domain resources | Mode-0600/0400 inert allowlisted parser, no shell sourcing, no Configuration Token env, per-stage `env -i`, private XDG state, allowlisted safe projection, known-value plus generic token scan, deterministic cleanup inventory, and run-ID confirmation for destructive cleanup | A privileged local operator can still mis-attest a manual checkpoint; require independent evidence review and sandbox-only resources |

## Security Invariants

- A Slack tenant identity is accepted only after Slack authentication; browser or daemon project fields never determine Slack ingress tenancy.
- One active installation has exactly one owner daemon/project and one valid pairing generation.
- Official app secrets and installation tokens never enter daemon config, proto responses, task data, browser storage, or routine logs.
- Configuration Tokens exist only in the local daemon's bounded live provisioning session; dedicated App credentials cross once into connection-context encrypted Gateway storage and never enter safe projections.
- Manifest upgrade/delete Configuration Tokens and exact App IDs are consumed into zeroizing memory before validation; lifecycle session state is memory-only and bounded, while audit stores only stable IDs, digests, versions, outcomes, and reasons.
- An opaque dedicated endpoint is routing context, not authentication; Slack HMAC and verified App/team identity remain mandatory.
- Live certification credentials and raw external identifiers remain in
  operator-owned private state. CI consumes only sanitized recorded contracts
  and reviewed safe evidence; stale evidence requests recertification without
  asserting a runtime regression.
- The old owner never receives the replacement pairing during transfer.
- Slack success acknowledgement occurs only after durable normalized enqueue.
- Source task mutation occurs only in the daemon after existing binding/template policy and dedupe checks.
- Gateway provider calls are allowlisted operations, not arbitrary URLs or Slack methods.

## Required Verification

- Run QA-162 and `docs/qa/orchestrator/163-dedicated-slack-app-auto-provisioning.md` together with authentication, authorization, SSRF, sensitive-data, logging, workflow-abuse, and race-condition security suites.
- Scan binaries' test logs, daemon/Gateway logs, Tauri payloads, DOM, and browser storage for fixture credentials, OAuth state/code, token prefixes, raw bodies, and private Slack URLs.
- In a controlled Slack sandbox, certify consent, callback, signed event, reinstall, revoke, manifest upgrade/reauthorization, reviewed mode migration, disconnect, and separate App deletion without retaining private workspace data.
- Use `docs/guide/slack-managed-sandbox-certification-runbook.md` for the required two-workspace topology, stop-loss rules, offline recovery, ownership transfer, backup/restore, privacy scan, and evidence allowlist.
- Before production, validate TLS/proxy raw-body behavior, upstream rate limiting, backup restore, enrollment-key rotation, Gateway master-key recovery, and alert delivery.

## Residual Risks And Recommendations

1. Replace the deployment-wide enrollment secret with per-daemon issued credentials or mTLS identities before accepting mutually untrusted tenant-operated daemons.
2. Add an external KMS/secret-backend adapter for official app and installation tokens in hosted production.
3. Add explicit queue retention and gap-Attention enforcement with a tested provider outage budget.
4. Add distributed rate limiting/WAF controls for horizontally scaled Gateway deployments.
5. Before enabling dedicated provisioning, require the QA-163 secret scan and controlled Slack addendum; treat any orphan App or retained Configuration Token as a security incident until reviewed.
