# Orchestrator - Sandbox Resource And Network Enforcement

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Close FR-001 remaining gaps by adding real runtime resource enforcement, structured sandbox resource/network events, and backend capability gating for unsupported network allowlists
**Related QA**: `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md`
**Created**: 2026-03-10
**Last Updated**: 2026-03-10

## Background

Step-level `ExecutionProfile` routing was already implemented, but three FR-001 gaps remained:

- `max_memory_mb`, `max_cpu_seconds`, `max_processes`, and `max_open_files` existed only as config fields, not stable runtime enforcement
- `sandbox_resource_exceeded` and `sandbox_network_blocked` were not emitted
- `network_mode=allowlist` had no verifiable backend implementation

The current macOS sandbox path already provides file-write isolation through `sandbox-exec`, so the remaining work is to make resource and network outcomes observable without coupling profile policy to agent definitions.

## Goals

- Enforce configured sandbox resource limits at process execution time
- Classify sandbox failures into file, resource, and network violations with structured events
- Reject unsupported `network_mode=allowlist` usage explicitly instead of silently degrading
- Preserve the existing step-level `ExecutionProfile` contract and backward compatibility for host execution

## Non-goals

- Implement a full Linux sandbox backend in this change
- Add a proxy-based or best-effort macOS network allowlist workaround
- Change workflow or agent manifest shapes beyond existing `ExecutionProfile` fields

## Scope

- In scope: runtime `setrlimit` enforcement, sandbox violation classification, `RunResult` diagnostic fields, task event visibility, QA coverage updates
- Out of scope: builtin step isolation, command-step isolation, cluster/remote sandboxing, true allowlist enforcement backend

## Interfaces And Data

## Runtime Interfaces

- `ResolvedExecutionProfile` remains the scheduler-facing execution policy object
- Runner layer now validates backend support before spawn
- Phase runner now carries signal-aware wait results and structured sandbox violation metadata through validation and recording

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
- `reason`
- `stderr_excerpt`

Additional fields when applicable:

- `resource_kind`
- `network_target`

## Key Design

1. Resource limits are enforced in the spawned Unix child via `setrlimit`, so the sandbox wrapper and the eventual agent process inherit the same execution boundary.
2. macOS remains the only active sandbox backend; backend capability validation happens before spawn and turns unsupported allowlist usage into a structured sandbox failure.
3. Phase execution keeps wait-time signal information so `RLIMIT_CPU` style exits can be classified without depending only on stderr heuristics.
4. Sandbox classification is centralized in the phase runner utility layer, so recorders and downstream task logic consume a single normalized result shape.
5. `RunResult` now carries `sandbox_violation_kind`, `sandbox_resource_kind`, and `sandbox_network_target` for downstream diagnostics and future policy hooks.

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
- Events: `sandbox_resource_exceeded` and `sandbox_network_blocked` are persisted alongside existing `sandbox_denied`
- Trace/follow: sandbox-specific events are queryable through the same events table and surfaced by `query_step_events`

Default recommendations:

- Treat `sandbox_network_blocked` with `reason=unsupported_backend_feature` as a configuration/design issue, not as a transient task failure
- Prefer resource limits that can be validated deterministically in QA (`max_open_files`, `max_cpu_seconds`) before expanding profile templates broadly

## Operations / Release

- Config: no new manifest fields
- Compatibility: host mode and old workflows remain unchanged
- Rollback: revert to the previous binary; persisted events and new result fields are additive
- Platform note: true `network_mode=allowlist` remains a future backend feature

## Test Plan

- Unit tests: sandbox violation classification for file-write denial, open-files exhaustion, and network blocking
- Integration-style QA: project-scoped workflows that trigger `sandbox_resource_exceeded` and `sandbox_network_blocked`
- Regression tests: existing execution profile and sandbox write-boundary QA remain valid

## QA Docs

- `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md`
- `docs/qa/orchestrator/54-step-execution-profiles.md`
- `docs/qa/orchestrator/55-sandbox-write-boundaries.md`

## Acceptance Criteria

- Sandbox resource limit fields produce real runtime enforcement for supported Unix paths
- `sandbox_resource_exceeded` and `sandbox_network_blocked` events are emitted with structured payloads
- Unsupported `network_mode=allowlist` does not silently degrade on macOS
- Existing host-mode workflows continue to run without configuration changes
