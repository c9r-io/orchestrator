# Orchestrator - Real Session Attach/Re-attach

**Module**: orchestrator
**Status**: Superseded as of 2026-07-12
**Reason**: FR-098 introduced the supported `agent session ...` control plane with different identity, lease, and stream semantics.

---

## Retirement Note

This document described obsolete command names that were never shipped:

- `orchestrator task session list|info|close`
- `orchestrator exec session/<session_id> -- ...`
- `orchestrator exec task/<task_id>/step/<step_id> -- ...`

The supported surface is now `orchestrator agent session ...` plus the top-level Session Inspector and Process Workspace session panel. QA must use QA-149 and must not expect the obsolete `task session` or generic `exec` forms.

## Replacement Guidance

- Do not run this document as a release gate in the current branch.
- Execute `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md` for the current CLI, gRPC, Tauri, Session Inspector, Process Workspace, restart, and fencing behavior. QA-145 remains the original FR-098 specification.
- For supported coverage, use the maintained task lifecycle and script-based QA documents.

## Checklist

| # | Item | Status | Test Date | Tester | Notes |
|---|------|--------|-----------|--------|-------|
| 1 | Session attach/re-attach scenarios retired | N/A | 2026-03-10 | Codex | Commands are not exposed by the current CLI |
