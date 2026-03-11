# Sandbox Network Enforcement Governance

**Module**: orchestrator  
**Date**: 2026-03-11  
**Related Design Doc**: `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md`  
**Related QA**: `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md`

## Purpose

This governance note records how sandbox network-enforcement behavior should be documented and verified after the `network_mode=deny` ticket fix, so implementation, QA expectations, and user-facing guides stay aligned.

## Current Source Of Truth

- `network_mode=deny` on the macOS sandbox backend emits `sandbox_network_blocked` for outbound-network failures recognized as DNS or connection denial.
- `sandbox_network_blocked` is an observability event, not a guarantee that the backend can always identify the exact destination.
- `network_mode=allowlist` is implemented on Linux `linux_native` and still rejected on macOS with `reason_code=unsupported_backend_feature`.
- QA fixtures should prefer the internal `orchestrator debug sandbox-probe dns-resolve ...` and `tcp-connect ...` probes over ad-hoc shell or Python snippets when validating network blocking.

## Governance Decisions

1. Design intent lives in the design doc.
2. QA validates behavior, not one exact stderr phrase.
3. Guides must describe backend semantics explicitly.
4. `network_target` is best-effort metadata; `stderr_excerpt` is the mandatory fallback.
5. `reason_code=network_allowlist_blocked` means the backend supported allowlist and the destination was denied by policy.

## Required Alignment Checklist

When sandbox-network behavior changes, update these artifacts together:

1. `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md`
2. `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md`
3. `docs/guide/03-workflow-configuration.md`
4. `docs/guide/zh/03-workflow-configuration.md`

## Drift Prevention Rules

- Do not describe `network_mode=deny` as “connection denied only”; document it as “outbound network blocked, possibly surfacing during DNS or connect”.
- Do not describe `network_mode=allowlist` as universally supported; document the backend matrix and prerequisites.
- If a QA ticket is caused by a platform-specific error shape, prefer broadening the classifier and clarifying the QA doc rather than hardcoding one command’s stderr.

## Verification Snapshot

Current verification snapshot:

- unit coverage includes DNS-style network denial classification
- unit coverage includes `network_allowlist_blocked` classification
- QA Scenario 2 uses a deterministic DNS probe
- Linux QA now has a deterministic TCP probe path for allowlist validation

## Follow-up

- If another backend adds real allowlist enforcement, extend this note with a backend-by-backend support matrix rather than replacing the existing Linux/macOS split.
