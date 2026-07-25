---
lifecycle: active
self_referential_safe: true
---

# Orchestrator - Step Scope Directory Scan Diagnostics

**Module**: Orchestrator
**Scope**: QaDirectoryScan diagnostic events from FR-094
**Scenarios**: 1
**Priority**: High

## Scenario 1: QaDirectoryScan Emits A Diagnostic Event

### Preconditions
- A `TestState` workflow with one conventional item-scoped `qa` step.
- One QA file at `docs/qa/scenario.md` and no explicit target files.

### Steps
1. Run:
   ```bash
   cargo test -p agent-orchestrator --lib -- \
     task_ops::tests::create_task_emits_qa_directory_scan_event_when_triggered
   ```

### Expected
- One `qa_directory_scan_triggered` event contains `trigger_step_id="qa"`, `materialized_count=1`, and `level="info"`.
- No `qa_directory_scan_oversize` event is emitted below the threshold of 50.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | QaDirectoryScan emits a diagnostic event | ☐ | | | |
