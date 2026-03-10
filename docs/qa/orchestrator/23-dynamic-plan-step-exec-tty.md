# Orchestrator - Dynamic Plan Step Injection and Exec TTY

**Module**: orchestrator  
**Status**: Retired as of 2026-03-10  
**Reason**: The `orchestrator exec` root command and `orchestrator task edit` subcommand are not part of the current CLI surface. The reusable script `docs/qa/script/test-exec-interactive.sh` was removed for the same reason.

---

## Retirement Note

This QA document previously described an interactive planning flow built around:

- `orchestrator task edit ... --insert-before ...`
- `orchestrator exec [-it] task/<task_id>/step/<step_id>`
- `orchestrator exec [-it] session/<session_id>`

Those commands are not implemented in the current product and must not be used as release-gating QA expectations.

## Verified Current Behavior

1. `orchestrator --help` does not list `exec`.
2. `orchestrator task --help` does not list `edit`.
3. `docs/qa/script/README.md` marks `test-exec-interactive.sh` as removed because it depended on the removed CLI surface.

## Replacement Guidance

- Use the supported regression scripts documented in `docs/qa/script/README.md`.
- Use task lifecycle coverage under the active orchestrator QA docs, such as pause/resume, retry, workflow execution, and trace scenarios.
- If interactive session tooling is reintroduced later, create a new QA document from the shipped CLI and backend behavior instead of re-enabling this file as-is.

## Checklist

| # | Item | Status | Test Date | Tester | Notes |
|---|------|--------|-----------|--------|-------|
| 1 | `task edit` / `exec` scenarios retired | N/A | 2026-03-10 | Codex | Commands are not implemented in the current CLI |
| 2 | `docs/qa/script/test-exec-interactive.sh` dependency removed | N/A | 2026-03-10 | Codex | See `docs/qa/script/README.md` |
