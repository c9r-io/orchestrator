# FR-006 - Sandbox Network Allowlist Backend

**Module**: orchestrator  
**Status**: Implemented  
**Priority**: P1  
**Created**: 2026-03-10  
**Last Updated**: 2026-03-11  
**Source**: step execution isolation follow-up split

## Background

Step-level execution isolation is closed on the current active macOS backend, but that does **not** imply that `network_mode=allowlist` is implemented as a real, verifiable enforcement boundary.

Current behavior is now backend-specific:

- macOS `macos_seatbelt`: `network_mode=deny` is enforced and observable; `network_mode=allowlist` still fails fast with `reason_code=unsupported_backend_feature`
- Linux `linux_native`: `network_mode=allowlist` is implemented as a real sandbox boundary when the daemon is running as `root`, `ip` and `nft` are available, and the profile uses `fs_mode=inherit`

This preserves the safe default on unsupported backends while adding one real, testable allowlist backend.

## Goal

Deliver at least one backend implementation where `ExecutionProfile.network_mode=allowlist` becomes a real, testable network boundary instead of an explicit structured rejection.

## Non-goals

- Do not change the existing `ExecutionProfile` manifest shape
- Do not silently degrade to best-effort proxy or env-based filtering
- Do not fold this work back into the closed execution-isolation milestone

## Acceptance Criteria

- At least one supported backend can enforce `network_mode=allowlist` deterministically
- QA can verify both allowed and blocked destinations end-to-end
- Documentation clearly distinguishes supported and unsupported backends

## Delivered

- Added allowlist entry parsing and validation for exact hostname/IP plus optional port forms
- Added `linux_native` sandbox backend wiring with network namespace + nftables enforcement for `network_mode=deny` and `network_mode=allowlist`
- Added `network_allowlist_blocked` reason code so policy rejection is distinguishable from backend unavailability
- Added preflight surfacing for unsupported or unavailable sandbox backends
- Added TCP sandbox probes to support deterministic allowlist QA
