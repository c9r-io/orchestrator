# Sandbox Network Enforcement Governance

**Module**: orchestrator  
**Date**: 2026-03-10  
**Related FR**: `docs/feature_request/FR-001-agent-execution-isolation.md`  
**Related Design Doc**: `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md`  
**Related QA**: `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md`

## Purpose

This governance note records how sandbox network-enforcement behavior should be documented and verified after the `network_mode=deny` ticket fix, so implementation, QA expectations, and user-facing guides stay aligned.

## Current Source Of Truth

- `network_mode=deny` on the active macOS sandbox backend emits `sandbox_network_blocked` for outbound-network failures recognized as DNS or connection denial.
- `sandbox_network_blocked` is an observability event, not a guarantee that the backend can always identify the exact destination.
- `network_mode=allowlist` is still not implemented as a verifiable backend capability on macOS; current behavior is a structured rejection with `reason_code=unsupported_backend_feature`.

## Governance Decisions

1. Design intent lives in FR + design doc.
2. QA validates behavior, not one exact stderr phrase.
3. Guides must describe backend semantics explicitly.
4. `network_target` is best-effort metadata; `stderr_excerpt` is the mandatory fallback.

## Required Alignment Checklist

When sandbox-network behavior changes, update these artifacts together:

1. `docs/feature_request/FR-001-agent-execution-isolation.md`
2. `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md`
3. `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md`
4. `docs/guide/03-workflow-configuration.md`
5. `docs/guide/zh/03-workflow-configuration.md`

## Drift Prevention Rules

- Do not describe `network_mode=deny` as “connection denied only”; document it as “outbound network blocked, possibly surfacing during DNS or connect”.
- Do not describe `network_mode=allowlist` as supported unless a verifiable backend implementation and QA scenario exist.
- If a QA ticket is caused by a platform-specific error shape, prefer broadening the classifier and clarifying the QA doc rather than hardcoding one command’s stderr.

## Verification Snapshot

After the ticket fix:

- unit coverage includes DNS-style network denial classification
- QA Scenario 2 uses a deterministic DNS probe
- the prior ticket was closed after re-test succeeded

## Follow-up

- If a future backend adds real allowlist enforcement, revise this note to separate “backend unsupported” from “backend supported but blocked by policy”.
