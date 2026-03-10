# Orchestrator - Exec Interactive Simulation

**Module**: orchestrator  
**Status**: Retired as of 2026-03-10  
**Reason**: This document depended on the removed `orchestrator exec` CLI and the removed helper script `docs/qa/script/test-exec-interactive.sh`.

---

## Retirement Note

The interactive execution scenarios in this file are not valid for the current codebase. The following assumptions are stale:

- `orchestrator exec -it task/<task_id>/step/<step_id>` exists
- `orchestrator exec -it session/<session_id>` exists
- `./docs/qa/script/test-exec-interactive.sh --json` exists and is supported

The script registry already records this removal in `docs/qa/script/README.md`.

## Replacement Guidance

- Do not raise QA failures based on `exec` or `test-exec-interactive.sh`.
- Use maintained regression scripts from `docs/qa/script/README.md`.
- Use supported orchestrator QA coverage for task creation, execution, retry, pause/resume, and trace flows.

## Checklist

| # | Item | Status | Test Date | Tester | Notes |
|---|------|--------|-----------|--------|-------|
| 1 | Interactive `exec` simulation scenarios retired | N/A | 2026-03-10 | Codex | CLI command not implemented |
| 2 | Removed script dependency documented | N/A | 2026-03-10 | Codex | See `docs/qa/script/README.md` |
