# Orchestrator - Sandbox Resource And Network Enforcement

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Close step-level execution isolation on the active backend with deterministic probe-based QA, structured sandbox events, and explicit allowlist backend gating
**Related QA**: `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md`
**Created**: 2026-03-10
**Last Updated**: 2026-03-11

## Background

Step-level `ExecutionProfile` routing was already implemented. The remaining work was to make resource-limit and network outcomes deterministic enough to close execution-isolation work while adding one backend that can enforce a real `network_mode=allowlist` boundary.

The macOS sandbox path still provides file-write isolation through `sandbox-exec` and keeps an explicit unsupported-backend contract for `network_mode=allowlist`. Linux now adds a `linux_native` backend for real allowlist enforcement under explicit host prerequisites.

## Goals

- Enforce configured sandbox resource limits at process execution time
- Classify sandbox failures into file, resource, and network violations with structured events
- Enforce `network_mode=allowlist` on at least one backend with deterministic, testable behavior
- Reject unsupported `network_mode=allowlist` usage explicitly instead of silently degrading
- Preserve the existing step-level `ExecutionProfile` contract and backward compatibility for host execution

## Non-goals

- Add a proxy-based or best-effort macOS network allowlist workaround
- Change workflow or agent manifest shapes beyond existing `ExecutionProfile` fields

## Scope

- In scope: runtime `setrlimit` enforcement, sandbox violation classification, Linux native allowlist enforcement, `RunResult` diagnostic fields, task event visibility, QA coverage updates
- Out of scope: builtin step isolation, command-step isolation, cluster/remote sandboxing

## Interfaces And Data

## Runtime Interfaces

- `ResolvedExecutionProfile` remains the scheduler-facing execution policy object
- Runner layer now validates backend support before spawn
- Phase runner now carries signal-aware wait results and structured sandbox violation metadata through validation and recording
- `orchestrator debug sandbox-probe ...` provides internal deterministic probes for QA fixtures and scripts

## Event Payload Changes

The following event types are part of the supported runtime surface:

- `execution_profile_applied`
- `sandbox_denied`
- `sandbox_resource_exceeded`
- `sandbox_network_blocked`

Shared payload fields:

- `step`, `step_id`, `step_scope`
- `agent_id`, `run_id`
- `execution_profile`, `execution_mode`
- `backend`
- `reason_code`
- `reason`
- `stderr_excerpt`

Additional fields when applicable:

- `resource_kind`
- `network_target` (best-effort)

## Key Design

1. Resource limits are enforced in the spawned Unix child via `setrlimit`, so the sandbox wrapper and the eventual agent process inherit the same execution boundary.
2. macOS remains the only active sandbox backend; backend capability validation happens before spawn and turns unsupported allowlist usage into a structured sandbox failure.
3. Phase execution keeps wait-time signal information so `RLIMIT_CPU` style exits can be classified without depending only on stderr heuristics.
4. QA fixtures call internal sandbox probes that emit canonical `SANDBOX_PROBE ...` stderr markers, allowing deterministic classification for memory, process, open-file, and DNS-block scenarios.
5. `network_mode=deny` classification still falls back to outbound-network failure signatures for non-probe commands; DNS-resolution failures remain valid network-block outcomes on macOS.
6. Sandbox classification is centralized in the phase runner utility layer, so recorders and downstream task logic consume a single normalized result shape.
7. `RunResult` now carries `sandbox_violation_kind`, `sandbox_resource_kind`, and `sandbox_network_target` for downstream diagnostics and future policy hooks.
8. Linux `linux_native` builds a per-run network namespace and nftables ruleset. `allowlist` entries are resolved up front to exact IPs, with optional TCP port restriction. DNS egress is allowed only to the host resolver set when `network_mode=allowlist`.
9. Linux `linux_native` is intentionally explicit about prerequisites: `root`, `ip`, and `nft` are required; `fs_mode=inherit` is required until a Linux filesystem boundary is implemented.

## Alternatives And Tradeoffs

- Best-effort allowlist on macOS via env/proxy tricks: rejected because it is not a real execution boundary and would be difficult to verify in QA.
- Event detection only from stderr parsing: rejected because resource limits need signal-aware classification to be stable.
- Put backend-specific classification in the scheduler: rejected because it would leak sandbox implementation details into orchestration logic.

## Risks And Mitigations

- Risk: some resource classes are still partially heuristic on stderr text
  - Mitigation: CPU uses wait-signal data, and the event model is now explicit enough to refine backend-specific detection later without API churn
- Risk: existing consumers only know `sandbox_denied`
  - Mitigation: old field remains, while new event types and result fields extend behavior without removing legacy signals
- Risk: unsupported allowlist profiles now fail earlier
  - Mitigation: this is explicit by design and prevents silent false confidence about outbound network restrictions

## Observability

- Logs: `execution_profile_applied` now includes `backend`
- Events: `sandbox_resource_exceeded` and `sandbox_network_blocked` are persisted alongside existing `sandbox_denied`, with stable `reason_code`
- Trace/follow: sandbox-specific events are queryable through the same events table and surfaced by `query_step_events`

Default recommendations:

- Treat `sandbox_network_blocked` with `reason_code=unsupported_backend_feature` as a configuration/design issue, not as a transient task failure
- Prefer probe-backed resource validation for backend acceptance instead of raw stderr matching

## Operations / Release

- Config: no new manifest fields
- Compatibility: host mode and old workflows remain unchanged
- Rollback: revert to the previous binary; persisted events and new result fields are additive
- Platform note: `network_mode=allowlist` is now implemented on Linux `linux_native` and remains explicitly unsupported on macOS `macos_seatbelt`

## Test Plan

- Unit tests: sandbox violation classification for file-write denial, CPU signal handling, probe-backed memory/process/open-files markers, and network blocking
- Integration-style QA: project-scoped workflows that trigger `sandbox_resource_exceeded` and `sandbox_network_blocked` across the full resource matrix
- Regression tests: existing execution profile and sandbox write-boundary QA remain valid

## QA Docs

- `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md`
- `docs/qa/orchestrator/54-step-execution-profiles.md`
- `docs/qa/orchestrator/55-sandbox-write-boundaries.md`

## Acceptance Criteria

- Sandbox resource limit fields produce real runtime enforcement for supported Unix paths
- `sandbox_resource_exceeded` and `sandbox_network_blocked` events are emitted with structured payloads
- Linux `network_mode=allowlist` can allow one destination and block another deterministically
- Unsupported `network_mode=allowlist` does not silently degrade on macOS
- Existing host-mode workflows continue to run without configuration changes
