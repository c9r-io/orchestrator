# FR-006 - Sandbox Network Allowlist Backend

**Module**: orchestrator  
**Status**: Proposed  
**Priority**: P1  
**Created**: 2026-03-10  
**Last Updated**: 2026-03-10  
**Source**: step execution isolation follow-up split

## Background

Step-level execution isolation is closed on the current active macOS backend, but that does **not** imply that `network_mode=allowlist` is implemented as a real, verifiable enforcement boundary.

Current behavior remains:

- `network_mode=deny` is enforced and observable
- `network_mode=allowlist` fails fast with `reason_code=unsupported_backend_feature`

This is the correct safe default, but it is not the same thing as true allowlist enforcement.

## Goal

Deliver a backend implementation where `ExecutionProfile.network_mode=allowlist` becomes a real, testable network boundary instead of an explicit structured rejection.

## Non-goals

- Do not change the existing `ExecutionProfile` manifest shape
- Do not silently degrade to best-effort proxy or env-based filtering
- Do not fold this work back into the closed execution-isolation milestone

## Acceptance Criteria

- At least one supported backend can enforce `network_mode=allowlist` deterministically
- QA can verify both allowed and blocked destinations end-to-end
- Documentation clearly distinguishes supported and unsupported backends
