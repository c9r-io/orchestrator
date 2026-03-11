# Orchestrator - gRPC Control Plane Protection

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Unified gRPC control-plane protection with request classification, subject/global budgets, stream occupancy guards, and audit visibility
**Related QA**: `docs/qa/orchestrator/65-grpc-control-plane-protection.md`
**Created**: 2026-03-12
**Last Updated**: 2026-03-12

## Background

The secure TCP control plane already enforced mTLS, subject mapping, and RPC authorization, but it still trusted clients to behave reasonably after authentication. High-frequency unary calls, repeated retries, and unbounded `TaskFollow` / `TaskWatch` subscriptions could still consume daemon CPU, SQLite capacity, and file descriptors.

## Goals

- Add a unified protection layer for all gRPC RPCs.
- Enforce default rate and concurrency budgets for `read`, `write`, `stream`, and `admin` traffic classes.
- Keep TCP identity-aware protection and preserve low-friction UDS behavior.
- Persist structured rejection details to `control_plane_audit`.

## Non-goals

- Distributed or cross-node rate limiting.
- Network-layer DDoS mitigation.
- Proto or CLI surface changes.

## Scope

- In scope: daemon-side request classification, subject/global token buckets, in-flight guards, active stream guards, `tower` transport middleware composition, protection config bootstrap, and audit persistence.
- Out of scope: metrics exporter, distributed rate limiting, and network-layer DDoS mitigation.

## Interfaces And Data

## API

- Existing gRPC RPC names and request/response types remain unchanged.
- Rejected calls now return stable gRPC errors:
  - `RESOURCE_EXHAUSTED` for `rate_limited`, `concurrency_limited`, `stream_limit_exceeded`
  - `UNAVAILABLE` for `load_shed`

## Database Changes

- Existing table: `control_plane_audit`
- New columns: `traffic_class`, `limit_scope`, `decision`, `reason_code`
- Migration strategy: additive only (`m0017_control_plane_protection_fields`)

## Key Design

1. `ControlPlaneProtection` is loaded during daemon startup and bootstraps `data/control-plane/protection.yaml` when absent.
2. A dedicated `tower` layer is attached at `Server::builder()` so all gRPC RPCs pass through one transport-level protection path before typed handlers run.
3. RPCs are classified into `read`, `write`, `stream`, and `admin`; per-RPC overrides can replace the class or budget.
4. Protection resolves caller identity with this priority: `mTLS subject_id` -> `remote_addr` -> `local-process`.
5. Every request hits both a subject-scoped budget and a global budget so one noisy client and many noisy clients are both bounded.
6. `TaskFollow` and `TaskWatch` keep a stream lease on the wrapped HTTP response body for the full connection lifetime so long-running watchers consume active-stream capacity until disconnect.

## Alternatives And Tradeoffs

- A handler-entrypoint implementation was simpler for phase 1, but transport middleware is the cleaner long-term shape because it removes per-RPC protection boilerplate and keeps business handlers focused on authz and service translation.
- A separate protection audit table would isolate semantics, but extending `control_plane_audit` keeps all control-plane decisions queryable in one place.
- Skipping UDS protection would preserve more local headroom, but it would leave the default local mode exposed to accidental DoS from misconfigured clients.

## Risks And Mitigations

- Risk: low default budgets break common local CLI flows.
  - Mitigation: defaults are intentionally moderate and can be overridden in `protection.yaml`.
- Risk: stream permits leak on disconnect.
  - Mitigation: stream responses wrap the receiver in a drop-guard so permit release is tied to stream lifetime.
- Risk: document drift between control-plane security and protection docs.
  - Mitigation: keep transport/auth coverage in doc 22/58 and resource-protection coverage in doc 27/65 with explicit cross-links.

## Observability

- Logs: protection rejections emit structured warnings with `rpc`, `transport`, `subject_id`, `traffic_class`, `limit_scope`, and `reason_code`.
- Metrics: no dedicated exporter yet; `control_plane_audit` remains the authoritative structured store.
- Tracing: protection decisions are emitted through tracing before audit insertion.

## Operations / Release

- Config:
  - Server protection config: `${app_root}/data/control-plane/protection.yaml`
  - Client secure config: `~/.orchestrator/control-plane/config.yaml`
- Rollback: delete or relax `protection.yaml` in the isolated app root, then restart the daemon.
- Compatibility: UDS and secure TCP clients remain protocol-compatible; only rejection behavior changes when budgets are exceeded.

## Test Plan

- Unit tests: RPC classification, route mapping, token bucket exhaustion, override merge behavior.
- Integration tests: secure bootstrap generates `protection.yaml`, read traffic gets rate limited, `TaskWatch` enforces active-stream limits, audit rows contain the new protection fields.
- Pressure QA: `scripts/qa/test-fr013-control-plane-protection.sh` drives repeated `TaskList`, `TaskWatch`, and `Apply` pressure against secure TCP and verifies the daemon remains responsive.

## QA Docs

- `docs/qa/orchestrator/65-grpc-control-plane-protection.md`

## Acceptance Criteria

- All gRPC RPC entrypoints use a unified protection layer.
- Default budgets exist for read/write/stream/admin traffic.
- Protection decisions are queryable via logs and `control_plane_audit`.
- Stream subscriptions cannot exceed configured active-stream limits.
- Protection config is discoverable and overridable without changing proto or CLI contracts.
