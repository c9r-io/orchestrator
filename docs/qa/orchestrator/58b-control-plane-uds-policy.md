---
lifecycle: active
self_referential_safe: false
---

# Orchestrator - Control Plane UDS Policy

**Module**: Orchestrator
**Scope**: UDS trust boundary, role defaults, overrides, audit enrichment, and FR-101 request-ID correlation
**Scenarios**: 4
**Priority**: Critical

Detailed shell commands and SQL are retained in Appendices A-D of `docs/qa/orchestrator/58-control-plane-security.md`.

## Scenario 1: UDS Trust Boundary And Audit Enrichment

### Steps
1. Execute Appendix A in `docs/qa/orchestrator/58-control-plane-security.md`.

### Expected
- Permission and absent-policy advisories are logged.
- ReadOnly and Operator RPCs succeed under the operator cap; Admin RPCs fail.
- Audit rows include effective role and `peer_exe`; mutation rows also carry the request ID shared with `control_action_audit`.

## Scenario 2: Default UDS Role Is Operator

### Steps
1. Execute Appendix B in the parent security document.

### Expected
- Without a policy file or flag, ReadOnly and Operator RPCs succeed and Admin RPCs are denied.

## Scenario 3: Explicit Admin Flag Restores Admin Access

### Steps
1. Execute Appendix C in the parent security document.

### Expected
- `--uds-max-role admin` permits Admin RPCs when no policy file exists.

## Scenario 4: Policy File Takes Precedence

### Steps
1. Execute Appendix D in the parent security document.

### Expected
- A `read_only` policy denies Operator RPCs even when the daemon receives `--uds-max-role admin`.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | UDS trust boundary and audit enrichment | PASS | 2026-04-05 | Claude | Audit includes role and peer executable |
| 2 | Default UDS role is operator | ☐ | | | |
| 3 | Explicit admin flag restores admin access | ☐ | | | |
| 4 | Policy file takes precedence | ☐ | | | |
