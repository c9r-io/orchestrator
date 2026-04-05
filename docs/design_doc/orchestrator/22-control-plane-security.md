# Orchestrator - Control Plane Security

**Module**: orchestrator
**Status**: Implemented
**Related Plan**: Secure TCP control plane with default mTLS, local host client bootstrap, role-based RPC authorization, and dedicated audit storage
**Related QA**: `docs/qa/orchestrator/58-control-plane-security.md`
**Created**: 2026-03-10
**Last Updated**: 2026-04-05

## Background

`orchestratord` already exposed a gRPC control plane over UDS or raw TCP, but TCP mode previously had no transport security, no caller authentication, and no RPC-level authorization. That left destructive operations such as `Shutdown`, `Delete`, and write-side task/resource RPCs exposed to any reachable TCP client.

## Goals

- Make `--bind` safe by default with mTLS.
- Preserve low-friction local development through UDS.
- Provide a minimal, explicit RPC role model (`read_only`, `operator`, `admin`).
- Auto-bootstrap local client credentials so the host user can connect without manual certificate setup.
- Record structured control-plane audit events outside task-scoped event storage.

## Non-goals

- OIDC, SSO, or external IAM integration.
- Multi-tenant policy engines or dynamic condition language.
- Replacing filesystem permissions for UDS access control.

## Scope

- In scope: secure TCP startup, CA/server/client certificate bootstrap, local client config generation, policy-file subject mapping, RPC authorization checks, audit table migration, CLI secure-connect auto-discovery.
- Out of scope: bearer-token auth, remote enrollment workflows, policy management through CRDs/resources, UDS token enforcement.

## Interfaces And Data

## API

- Existing gRPC service surface remains unchanged.
- `orchestratord --bind <addr>` now means secure TCP with mTLS.
- `orchestratord --insecure-bind <addr>` is the explicit unsafe development path.
- `orchestratord control-plane issue-client --bind <addr> --subject <id> --role <role>` issues additional client credentials and updates policy.
- `orchestrator --control-plane-config <path>` overrides secure client config discovery.

## Database Changes

- New table: `control_plane_audit`
- Columns: `created_at`, `transport`, `remote_addr`, `rpc`, `subject_id`, `authn_result`, `authz_result`, `role`, `reason`, `tls_fingerprint`
- Added by FR-010: `rejection_stage` (m0015)
- Added by protection follow-up: `traffic_class`, `limit_scope`, `decision`, `reason_code` (m0017)
- Added by UDS hardening: `peer_exe` — resolved executable path of the UDS peer process for forensic audit (m0024)
- Migration strategy: additive migration only; no proto or task table changes required.

## Key Design

1. Secure TCP uses mTLS only in phase 1. The daemon auto-generates a local CA, server certificate, and the first local admin client certificate.
2. User-side client materials are written under `~/.orchestrator/control-plane/`, including a kubeconfig-style YAML file that the CLI discovers automatically.
3. Authorization is derived from client certificate URI SAN identity and a local YAML policy file at `data/control-plane/policy.yaml`.
4. UDS remains unauthenticated at the gRPC layer and continues to rely on socket file permissions.
5. Audit events are persisted in `control_plane_audit` rather than the task `events` table because control-plane requests are not task-bound.

## Alternatives And Tradeoffs

- Raw TCP + token would have been simpler, but it still needs TLS and gives weaker subject identity than client certificates.
- Storing policy inside existing resource/config state would unify configuration, but it creates bootstrap and lockout risks for the control plane itself.
- Enforcing auth on UDS would unify behavior, but it would degrade the current local developer workflow with little extra security value on single-user hosts.

## Risks And Mitigations

- Risk: accidental host home-directory mutation during testing.
  - Mitigation: QA scenarios use an isolated temporary `HOME`.
- Risk: policy lockout or broken generated certs prevent CLI access.
  - Mitigation: keep UDS fallback and provide idempotent bootstrap plus `issue-client`.
- Risk: stale docs imply raw TCP remains acceptable.
  - Mitigation: update the client/server QA doc and add a dedicated control-plane security QA document.

## Observability

- Logs: authentication failures, authorization denials, and insecure TCP startup warnings are emitted through tracing.
- Metrics: the implementation is prepared around audit-event counting; direct metric export is still a follow-up item.
- Tracing: each protected RPC records transport, subject, and decision outcome via the audit table.

## Operations / Release

- Config paths:
  - Server: `${app_root}/data/control-plane/`
  - Client: `~/.orchestrator/control-plane/`
- Rollback: use UDS-only startup or explicit `--insecure-bind` if certificate bootstrapping blocks testing.
- Compatibility: UDS workflows remain backward-compatible; only TCP defaults changed.

## Test Plan

- Unit tests: role mapping, URI SAN extraction, secure bootstrap material generation, CLI config parsing/discovery.
- Integration tests: secure daemon startup, local admin auto-connect, `operator` denied on `Shutdown`, audit row persistence.
- E2E: start secure daemon with isolated `HOME`, run CLI over auto-generated config, issue an extra client, verify policy and audit results.

## QA Docs

- `docs/qa/orchestrator/58-control-plane-security.md`
- `docs/qa/orchestrator/65-grpc-control-plane-protection.md` (follow-up resource protection coverage)

## FR-010 Hardening

FR-010 tightens the security baseline established by FR-002:

1. **Cargo feature gate for `--insecure-bind`**: The `--insecure-bind` CLI argument is gated behind the `dev-insecure` Cargo feature. Default release builds do not expose insecure TCP at all; passing `--insecure-bind` without the feature results in a clap "unexpected argument" error.
2. **Mandatory mTLS**: Secure TCP mode now uses `client_auth_optional(false)`, meaning connections without a valid client certificate fail at the TLS handshake layer before reaching any RPC handler.
3. **Audit rejection classification**: The `control_plane_audit` table gains a `rejection_stage` column that categorizes denials into `cert_validation_failed`, `subject_not_found`, `subject_disabled`, and `role_insufficient`. TLS handshake rejections are captured only in tracing logs (connection never enters application layer).

## UDS Trust Boundary Hardening

Tightens the UDS security baseline without breaking the zero-config local-first experience:

1. **Exhaustive RPC role mapping**: All RPC methods are explicitly classified as `ReadOnly`, `Operator`, or `Admin`. The previous catch-all `_ => Admin` silently promoted 25 RPCs (including read-only operations like `DbStatus`, `AgentList`, `EventStats`) to Admin, making `uds-policy.yaml` caps ineffective for routine use. Unmapped future RPCs still default to Admin but now emit a `tracing::warn!`.
2. **Audit enrichment**: UDS audit records now include `role` (effective role from policy or implicit Admin) and `peer_exe` (resolved executable path of the peer process via `/proc/PID/exe` on Linux, `proc_pidpath` on macOS). `peer_exe` is forensic-only and must not be used for authorization (TOCTOU + trivially spoofable by same-UID processes).
3. **`audit_all_reads` option**: `UdsAuthPolicy` gains `audit_all_reads: bool` (default `false`). When enabled, read-only RPCs are also recorded in `control_plane_audit`, giving full forensic coverage for multi-user deployments.
4. **Startup advisories**: The daemon warns on startup if:
   - `data_dir` has group or world read/write bits set (suggests `0700` for multi-user hosts).
   - No `uds-policy.yaml` exists (advises path to create one for restricting implicit Admin).

### What was not changed (and why)

- **Default `max_role` remains `Admin`**: Changing it to `Operator` would break `daemon stop`, `task delete`, and `delete` for all existing users. Opt-in via `uds-policy.yaml` is the intended path.
- **No PID/exe-based authorization**: Same-UID attacker can trivially spoof binary identity. Record in audit for forensics only.
- **No UDS token/nonce mechanism**: Would require every CLI invocation to present a token, degrading the local-first experience for marginal security gain.

## Acceptance Criteria

- `--bind` starts a TLS-protected control plane and rejects unauthenticated TCP clients.
- Host user gets a usable default client config without manual certificate wiring.
- High-privilege RPCs are independently authorization-gated.
- Authentication and authorization decisions are auditable in SQLite.
- UDS remains available as the low-friction local control-plane path.
- Default builds do not expose `--insecure-bind` (FR-010).
- Secure TCP enforces mTLS at the handshake layer (FR-010).
- Audit records carry `rejection_stage` classification (FR-010).
- All RPC methods have explicit role classifications; `uds-policy.yaml` caps work correctly for operator-restricted deployments (UDS hardening).
- UDS audit records include effective role and peer executable path (UDS hardening).
