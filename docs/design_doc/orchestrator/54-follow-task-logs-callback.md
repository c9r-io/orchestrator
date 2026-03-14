# follow_task_logs Callback Refactor

**Module**: orchestrator
**Status**: Approved
**Related FR**: FR-042
**Related QA**: `docs/qa/orchestrator/97-follow-task-logs-callback.md`
**Created**: 2026-03-14

## Background And Goals

### Background

`follow_task_logs` (in `core/src/scheduler/query/log_stream.rs`) and its helper `follow_one_stream` originally wrote log output directly to stdout/stderr via `print!/eprint!`. The service-layer wrapper `follow_task_logs_stream` accepted a `send_fn` callback but ignored it (parameter named `_send_fn`), delegating to the stdout-based implementation. This caused the gRPC `TaskFollow` endpoint to return an empty stream to remote clients, even though the daemon correctly constructed an mpsc channel and passed a `send_fn`.

### Goals

- Make `follow_task_logs` and `follow_one_stream` output-agnostic by accepting a synchronous `FnMut(String, bool) -> Result<()>` callback
- Wire `follow_task_logs_stream` to correctly forward logs via the callback
- Enable gRPC `TaskFollow` to deliver real-time log lines to remote clients
- Preserve CLI `task logs --follow` behavior (terminal output via callback)

### Non-goals

- Changing proto definitions (`TaskFollowRequest`/`TaskLogLine`)
- Modifying the historical log query path (`stream_task_logs_impl`)
- Adding new dependencies

## Design

### Callback Signature

```rust
FnMut(String, bool) -> anyhow::Result<()>
// (text_chunk, is_stderr)
```

Synchronous `FnMut` was chosen over async `Fn` to avoid `Arc<F>` + `Send` trait complexity discovered during self-bootstrap validation. The caller handles async concerns (e.g., channel send) at the boundary.

### Modified Functions

1. **`follow_one_stream`** — gains `output_fn: &mut F` parameter; replaces `print!/eprint!` with `output_fn(text, stderr)?`

2. **`follow_task_logs`** — gains `output_fn: &mut F` parameter; all `eprintln!` status messages (waiting notice, phase change, task completion) routed through callback with `is_stderr=true`; forwards `output_fn` to `follow_one_stream`

3. **`follow_task_logs_stream`** — signature simplified from `Fn(String) -> Future` to `FnMut(String, bool) -> Result<()>`; directly forwards `send_fn` to `follow_task_logs`

### Callers Updated

- **Daemon** (`crates/daemon/src/server/task.rs`): closure uses `tx.try_send()` (sync, non-blocking) instead of async `tx.send().await`
- **Integration tests** (`crates/integration-tests/src/lib.rs`): same pattern

### Key Decision: `try_send` vs `blocking_send`

`try_send` was chosen because the callback executes within an async task (`tokio::spawn`). Using `blocking_send` would block the tokio runtime thread. `try_send` may drop messages if the channel is full (capacity 64), but this is acceptable for log streaming where backpressure-based dropping is preferable to runtime blocking.

## Files Changed

| File | Change |
|------|--------|
| `core/src/scheduler/query/log_stream.rs` | `follow_task_logs` and `follow_one_stream` accept callback; no more direct `print!/eprint!` |
| `core/src/service/task.rs` | `follow_task_logs_stream` simplified to sync callback; `TODO: Phase 3` removed |
| `crates/daemon/src/server/task.rs` | Closure updated to `(String, bool)` with `try_send` |
| `crates/integration-tests/src/lib.rs` | Same as daemon |
