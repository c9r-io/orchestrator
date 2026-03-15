# Orchestrator - follow_task_logs Callback

**Module**: orchestrator
**Scope**: Verify follow_task_logs callback refactor — gRPC TaskFollow delivers real log lines
**Scenarios**: 3
**Priority**: High

---

## Background

FR-042 refactored `follow_task_logs` and `follow_one_stream` to accept a synchronous callback instead of writing directly to stdout/stderr. This enables the gRPC `TaskFollow` endpoint to deliver real-time log lines through its streaming channel.

Related design doc: `docs/design_doc/orchestrator/54-follow-task-logs-callback.md`

Entry point: `orchestrator task logs --follow <task-id>` (CLI) or gRPC `TaskFollow` RPC

---

## Scenario 1: follow_one_stream callback receives stdout content

**Goal**: Verify `follow_one_stream` routes stdout content through callback with `is_stderr=false`

**Covered by unit test**: `follow_one_stream_uses_callback_for_stdout`

```bash
cargo test -p agent-orchestrator --lib follow_one_stream_uses_callback_for_stdout
```

**Expected**: Test passes; callback receives file content with `is_stderr=false`

---

## Scenario 2: follow_one_stream callback receives stderr content

**Goal**: Verify `follow_one_stream` routes stderr content through callback with `is_stderr=true`

**Covered by unit test**: `follow_one_stream_uses_callback_for_stderr`

```bash
cargo test -p agent-orchestrator --lib follow_one_stream_uses_callback_for_stderr
```

**Expected**: Test passes; callback receives file content with `is_stderr=true`

---

## Scenario 3: follow_one_stream incremental read via callback

**Goal**: Verify that successive calls to `follow_one_stream` with the same `pos` tracker only deliver new data through the callback

**Covered by unit test**: `follow_one_stream_callback_incremental_read`

```bash
cargo test -p agent-orchestrator --lib follow_one_stream_callback_incremental_read
```

**Expected**: Test passes; first call delivers initial content, second call delivers only appended content

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
