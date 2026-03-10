# Orchestrator - Control Plane Security

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Secure TCP control plane with default mTLS, local host client bootstrap, role-based RPC authorization, and dedicated audit storage
**Related QA**: `docs/qa/orchestrator/58-control-plane-security.md`
**Created**: 2026-03-10
**Last Updated**: 2026-03-10

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

## Acceptance Criteria

- `--bind` starts a TLS-protected control plane and rejects unauthenticated TCP clients.
- Host user gets a usable default client config without manual certificate wiring.
- High-privilege RPCs are independently authorization-gated.
- Authentication and authorization decisions are auditable in SQLite.
- UDS remains available as the low-friction local control-plane path.
