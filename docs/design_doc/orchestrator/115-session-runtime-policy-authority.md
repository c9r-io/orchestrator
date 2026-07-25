---
lifecycle: active
related_fr: FR-105
---

# Orchestrator - Session RuntimePolicy Authority

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-105 deterministic global session rollout and rollback gates  
**Related QA**: `docs/qa/orchestrator/152-session-runtime-policy-authority.md`  
**Created**: 2026-07-15  
**Last Updated**: 2026-07-15

## Background

The Session control plane defines `RuntimePolicy.spec.session_read_enabled` and `session_control_enabled` as global rollout and emergency rollback switches owned by the `_system` project. The previous parameterless `OrchestratorConfigExt::runtime_policy()` selected the first RuntimePolicy found across all projects. Because the underlying resource store is unordered, an ordinary project's policy could silently become authoritative and allow session mutation while `_system.session_control_enabled` was false.

FR-105 removes that ambiguity. Global consumers now name the global policy operation explicitly, while project-scoped consumers continue to resolve the target project and then fall back to `_system` and safe defaults.

## Goals

- Make `_system` the only authority for global Session read and mutation gates.
- Keep project policy lookup deterministic and preserve its existing project-to-system fallback.
- Make a successful policy apply visible to the next Session request without daemon restart.
- Classify every parameterless RuntimePolicy consumer as global or project-scoped.
- Preserve Session RPC, CLI, persistence, RBAC, fencing, idempotency, audit, and redaction contracts.

## Non-goals

- Per-project Session feature flags.
- New protobuf, CLI, Tauri, manifest, or database interfaces.
- A general redesign of RuntimePolicy fields.
- Session Inspector UI changes.
- Changes to project-scoped runner, workspace, source, Attention, or process-metric policy semantics.

## Scope

- `core/src/config_ext.rs` owns explicit global and project RuntimePolicy accessors.
- Session read and mutation gates consume the global accessor.
- Global logging and process-metric retention consume the global accessor.
- Session transcript redaction and scheduler task/timeline redaction remain project-scoped, with `_system` fallback only through the project accessor.
- The isolated Session QA covers conflicting policies, hot apply, restart, existing safety invariants, and current-HEAD binaries.

## Interfaces And Data Changes

No public interface or persisted schema changes.

The internal configuration contract is:

- `global_runtime_policy()` reads only `_system`. If `_system` has no RuntimePolicy, it returns `RuntimePolicySpec::default()`.
- `runtime_policy_for_project(project)` reads that project, then `_system`, then the default.
- `runtime_policy()` remains a compatibility alias for `global_runtime_policy()` so an unqualified lookup can no longer mean an unordered cross-project scan.

The established safe defaults remain `session_read_enabled=true` and `session_control_enabled=false`.

## Key Design

1. Global policy resolution performs a keyed `_system` lookup; it never iterates across project policies.
2. Session handlers read the current atomic configuration snapshot for every request. A completed apply swaps the snapshot before returning, so the next request observes the new gate.
3. `session_control_enabled=false` is checked before writer lease reservation, heartbeat, input I/O, detach state changes, close state changes, or process signaling.
4. `session_read_enabled=false` rejects List, Get, Read, Resolve, and reader attachment without terminating an existing agent process.
5. Content redaction remains tied to the Session task's project. When a task cannot be resolved, the scheduler uses the explicit global fallback rather than an arbitrary project.
6. Missing global policy uses safe defaults. Invalid policy input remains rejected by the existing apply/load validation path and cannot replace the last valid atomic snapshot.

## Alternatives And Tradeoffs

- Making Session gates project-scoped would offer more rollout granularity, but the current Session RPCs do not carry an authoritative project argument and the approved rollback contract is global.
- Fixing only the Session handler would leave the parameterless accessor unsafe for future consumers. Retaining it as a deterministic compatibility alias reduces migration risk while making its meaning safe.
- Caching the two flags in the Session service would reduce trivial lookup work but could drift after hot apply. Snapshot reads preserve immediate rollback behavior.

## Risks And Mitigations

- **Consumer semantic drift**: all workspace call sites were classified; task-owned redaction uses `runtime_policy_for_project`, while process-wide operations use `global_runtime_policy`.
- **Accidental fail-open**: the missing `_system` default disables mutation, and rejected/invalid apply never replaces the active snapshot.
- **QA false positives from control-plane throttling**: the isolated script retries only explicit transient `rate_limited` responses and requires the final denial text to match the RuntimePolicy gate.
- **Stale local binaries**: the QA script builds daemon and CLI binaries from the current checkout unless the caller explicitly opts into `SKIP_BUILD=1` for local iteration.

## Observability

- Policy denials retain the existing `permission_denied` status and request correlation behavior.
- No terminal input, transcript content, internal path, or process fingerprint is added to logs or audit records.
- No new metrics or high-cardinality labels are introduced.

## Operations / Release

- Roll out with `_system.session_read_enabled=true` and `_system.session_control_enabled=false`.
- Verify read-only inspection, then explicitly enable mutation in `_system`.
- Emergency rollback is a successful apply of `_system.session_control_enabled=false`; no restart is required.
- Ordinary project RuntimePolicy resources cannot weaken or strengthen the global Session gate.
- Restart preserves the persisted `_system` decision and re-evaluates it through the same explicit accessor.

## Test Plan

- Unit: conflicting `_system` and project policies in both insertion orders; project override/fallback; missing `_system` safe defaults.
- Integration: every Session mutation denied by global false without state change; read APIs denied/restored; reverse conflict follows `_system`; hot apply and restart retain the same decision.
- Regression: existing writer race, fencing, exactly-once input, PID identity, restart reconciliation, RBAC, audit, redaction, workspace tests, and strict Clippy.
- UI: existing Session Inspector unit and Playwright suites remain unchanged and passing because the public contract is unchanged.

## QA Docs

- `docs/qa/orchestrator/152-session-runtime-policy-authority.md`
- `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md` remains the FR-102 lifecycle baseline and delegates deterministic policy authority to QA-152.

## Acceptance Criteria

- `_system=false` overrides an ordinary project policy set to true for every Session read or mutation gate.
- `_system=true` remains authoritative when the ordinary project policy is false.
- Resource insertion order does not affect the result.
- Successful apply affects the next request and the same policy survives daemon restart.
- Missing or invalid global policy never enables mutation.
- The isolated Session QA builds current binaries and reports five passes and zero failures.
- Workspace tests, strict Clippy, and unchanged GUI regressions pass.
