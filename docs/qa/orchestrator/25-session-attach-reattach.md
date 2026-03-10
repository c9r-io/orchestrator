# Orchestrator - Real Session Attach/Re-attach

**Module**: orchestrator  
**Status**: Retired as of 2026-03-10  
**Reason**: The current CLI does not expose `task session ...` or `exec` commands, so this session-attach workflow is not testable as written.

---

## Retirement Note

This document described a session control surface that is not present in the shipped CLI:

- `orchestrator task session list|info|close`
- `orchestrator exec session/<session_id> -- ...`
- `orchestrator exec task/<task_id>/step/<step_id> -- ...`

The underlying database tables for sessions still exist, but they are not currently exposed as a supported user-facing QA flow. QA must not treat the absence of these commands as a runtime regression.

## Replacement Guidance

- Do not run this document as a release gate in the current branch.
- If session-management commands are reintroduced, author a new QA spec from the implemented CLI help output and end-to-end behavior.
- For supported coverage, use the maintained task lifecycle and script-based QA documents.

## Checklist

| # | Item | Status | Test Date | Tester | Notes |
|---|------|--------|-----------|--------|-------|
| 1 | Session attach/re-attach scenarios retired | N/A | 2026-03-10 | Codex | Commands are not exposed by the current CLI |
