# Implementation Plan: Refactor scheduler/query.rs into Module Directory

## Overview

Refactor `core/src/scheduler/query.rs` (1787 lines) into `core/src/scheduler/query/` directory structure, following the pattern established by `core/src/task_repository/`. The goal is to improve code organization while maintaining 100% backward compatibility for external callers.

## Files to Change

### 1. Create `core/src/scheduler/query/mod.rs`
**Purpose:** Module root with re-exports and shared retry utilities

**Contents:**
- Module declarations (`mod task_queries; mod log_stream; mod watch; mod format;`)
- Test module declaration (`#[cfg(test)] mod tests;`)
- Constants: `QUERY_RETRY_ATTEMPTS`, `QUERY_RETRY_DELAY_MS`, `FOLLOW_POLL_MS`, `FOLLOW_WARNING_THROTTLE_SECS`, `LOG_UNAVAILABLE_MARKER`
- Functions: `is_transient_query_error()`, `retry_query()` (shared utilities)
- Re-exports of all public functions from submodules:
  ```rust
  pub use task_queries::{resolve_task_id, load_task_summary, list_tasks_impl, get_task_details_impl, delete_task_impl};
  pub use log_stream::{stream_task_logs_impl, follow_task_logs};
  pub use watch::watch_task;
  pub(crate) use log_stream::tail_lines;  // Internal use only
  ```

### 2. Create `core/src/scheduler/query/task_queries.rs`
**Purpose:** Task CRUD query operations

**Contents (~150 lines):**
- `pub fn resolve_task_id(state: &InnerState, task_id: &str) -> Result<String>`
- `pub fn load_task_summary(state: &InnerState, task_id: &str) -> Result<TaskSummary>`
- `pub fn list_tasks_impl(state: &InnerState) -> Result<Vec<TaskSummary>>`
- `pub fn get_task_details_impl(state: &InnerState, task_id: &str) -> Result<TaskDetail>`
- `pub fn delete_task_impl(state: &InnerState, task_id: &str) -> Result<()>`

**Dependencies:**
- `crate::task_repository::{SqliteTaskRepository, TaskRepository}`
- `crate::state::InnerState`
- `crate::dto::{TaskDetail, TaskSummary}`
- Uses `retry_query()` and `is_transient_query_error()` from mod.rs via `super::`

**Tests to include:**
- `resolve_task_id_exact_match`
- `resolve_task_id_prefix_match`
- `resolve_task_id_not_found`
- `load_task_summary_returns_counts`
- `load_task_summary_with_prefix`
- `list_tasks_impl_returns_seeded_task`
- `list_tasks_impl_empty_when_no_tasks`
- `list_tasks_impl_multiple_tasks_ordered_desc`
- `get_task_details_impl_returns_items_and_empty_runs`
- `get_task_details_impl_with_command_run`
- `delete_task_impl_removes_task_and_log_files`
- `delete_task_impl_nonexistent_returns_error`

### 3. Create `core/src/scheduler/query/log_stream.rs`
**Purpose:** Log streaming and file tailing utilities

**Contents (~250 lines):**
- `pub fn stream_task_logs_impl(state: &InnerState, task_id: &str, tail_count: usize, show_timestamps: bool) -> Result<Vec<LogChunk>>`
- `pub async fn follow_task_logs(state: &InnerState, task_id: &str) -> Result<()>`
- `pub(crate) fn tail_lines(path: &Path, limit: usize) -> Result<String>`
- `async fn follow_one_stream(path: &str, pos: &mut u64, stderr: bool) -> Result<()>`

**Dependencies:**
- `crate::task_repository::{SqliteTaskRepository, TaskRepository}`
- `crate::state::InnerState`
- `crate::dto::LogChunk`
- `crate::runner::redact_text`
- `crate::config_load::read_loaded_config`
- `crate::anomaly::AnomalyRule`
- Uses `super::{retry_query, is_transient_query_error, LOG_UNAVAILABLE_MARKER, FOLLOW_POLL_MS}`
- Uses `super::watch::emit_anomaly_warning` (cross-module call)

**Tests to include:**
- `tail_lines_zero_limit_returns_empty`
- `tail_lines_empty_file_returns_empty`
- `tail_lines_returns_last_n_lines`
- `tail_lines_returns_all_when_limit_exceeds_file`
- `tail_lines_missing_file_returns_error`
- `tail_lines_large_file`
- `stream_task_logs_impl_returns_log_chunks`
- `stream_task_logs_impl_works_when_active_config_is_not_runnable`
- `stream_task_logs_impl_with_stderr`
- `stream_task_logs_impl_with_timestamps`
- `stream_task_logs_impl_tail_count_limits_output`
- `stream_task_logs_impl_no_runs_returns_empty`
- `stream_task_logs_impl_returns_placeholder_when_logs_missing`
- `stream_task_logs_impl_returns_partial_results_when_one_run_is_unavailable`

### 4. Create `core/src/scheduler/query/watch.rs`
**Purpose:** Real-time task monitoring (watch command)

**Contents (~320 lines):**
- `pub async fn watch_task(state: &InnerState, task_id: &str, interval_secs: u64) -> Result<()>`
- `pub fn load_task_detail_snapshot(state: &InnerState, task_id: &str) -> Result<TaskDetail>`
- `pub fn emit_anomaly_warning(rule: &AnomalyRule, message: &str, last_warning_at: &mut Option<Instant>)`
- `fn render_watch_frame(task: &TaskSummary, events: &[StepEvent], task_id: &str) -> String`
- `struct StepWatchInfo { step, scope, binding_item_id, agent_id, status, duration_ms, details, started_at }`
- `struct WatchAnomalyCounts { intervene, attention, notice }`

**Dependencies:**
- `crate::task_repository::{SqliteTaskRepository, TaskRepository}`
- `crate::state::InnerState`
- `crate::dto::TaskSummary`
- `crate::anomaly::AnomalyRule`
- `crate::events::{StepEvent, ObservedStepScope, observed_step_scope_label, query_step_events_db}`
- Uses `super::{retry_query, is_transient_query_error, FOLLOW_POLL_MS}`
- Uses `super::format::{colorize_status, format_duration, format_bytes}`
- Uses `super::task_queries::load_task_summary`

**Tests to include:**
- `render_watch_frame_includes_running_step_and_cycle`
- `render_watch_frame_shows_low_output_details_for_heartbeat`
- `render_watch_frame_keeps_active_state_for_active_heartbeat`
- `render_watch_frame_shows_legacy_scope_for_legacy_event`

### 5. Create `core/src/scheduler/query/format.rs`
**Purpose:** Display formatting utilities

**Contents (~35 lines):**
- `pub fn colorize_status(status: &str) -> String`
- `pub fn format_duration(ms: u64) -> String`
- `pub fn format_bytes(bytes: u64) -> String`

**Dependencies:** None (pure functions)

**Tests to include:**
- `format_duration_milliseconds`
- `format_duration_seconds`
- `format_duration_minutes`
- `format_bytes_bytes`
- `format_bytes_kilobytes`
- `format_bytes_megabytes`
- `colorize_status_completed`
- `colorize_status_failed`
- `colorize_status_running`
- `colorize_status_paused`
- `colorize_status_unknown_passes_through`

### 6. Create `core/src/scheduler/query/tests/mod.rs`
**Purpose:** Test module root with shared fixtures

**Contents:**
- `fn test_dir(name: &str) -> PathBuf` helper
- `fn seed_task(fixture: &mut TestState) -> (Arc<InnerState>, String)` helper
- `fn first_item_id(state: &InnerState, task_id: &str) -> String` helper
- Re-export test utilities for use by submodule tests

**Note:** Tests can remain in their respective modules using `#[cfg(test)] mod tests { ... }` pattern, or be consolidated here. The task_repository pattern keeps tests in submodules.

### 7. Update `core/src/scheduler.rs`
**Change:** Replace `mod query;` with `mod query;` (directory module)

**No changes needed to pub use statements** - they remain identical:
```rust
pub use query::{
    delete_task_impl, follow_task_logs, get_task_details_impl, list_tasks_impl, load_task_summary,
    resolve_task_id, stream_task_logs_impl, watch_task,
};
```

### 8. Delete `core/src/scheduler/query.rs`
**Action:** Remove the original monolithic file after successful migration

## Approach

### Phase 1: Create Directory Structure (Minimal Blast Radius)

1. Create `core/src/scheduler/query/` directory
2. Create `mod.rs` with all constants, shared utilities, and re-exports
3. Change `core/src/scheduler.rs` from `mod query;` (file) to `mod query;` (directory) - Rust handles this transparently

### Phase 2: Extract Pure Utilities (No Dependencies)

1. Create `format.rs` with `colorize_status`, `format_duration`, `format_bytes`
2. Move corresponding tests
3. Run `cargo test` to verify

### Phase 3: Extract Task Queries

1. Create `task_queries.rs` with 5 public functions
2. Update imports to use `super::retry_query` etc.
3. Move task query tests
4. Run `cargo test` to verify

### Phase 4: Extract Log Stream

1. Create `log_stream.rs` with streaming functions and `tail_lines`
2. Handle cross-module dependency on `emit_anomaly_warning` (import from `super::watch`)
3. Move log stream tests
4. Run `cargo test` to verify

### Phase 5: Extract Watch Module

1. Create `watch.rs` with watch functions and structs
2. Import format functions from `super::format`
3. Import `load_task_summary` from `super::task_queries`
4. Move watch tests
5. Run `cargo test` to verify

### Phase 6: Finalize mod.rs and Cleanup

1. Ensure all re-exports are correct in `mod.rs`
2. Run full test suite: `cargo test`
3. Run clippy: `cargo clippy --all-targets`
4. Delete original `query.rs`

### Cross-Module Dependencies

```
mod.rs
  └── retry_query, is_transient_query_error (shared utilities)

format.rs
  └── (no internal deps)

task_queries.rs
  └── uses: super::retry_query, super::is_transient_query_error

log_stream.rs
  └── uses: super::retry_query, super::LOG_UNAVAILABLE_MARKER
  └── uses: super::watch::emit_anomaly_warning

watch.rs
  └── uses: super::retry_query, super::is_transient_query_error
  └── uses: super::format::{colorize_status, format_duration, format_bytes}
  └── uses: super::task_queries::load_task_summary
```

## Scope Boundary

### IN Scope

1. **File structure refactoring only** - Split query.rs into query/ directory with submodules
2. **Re-export public API** - All 8 public functions remain accessible via `scheduler::query::*`
3. **Move existing tests** - Relocate tests to corresponding submodules
4. **Update imports within submodules** - Use `super::` for cross-module references
5. **Verify compilation** - `cargo check` passes
6. **Verify tests** - All 49 existing tests pass
7. **Verify clippy** - No new warnings

### OUT of Scope

1. **No API changes** - Function signatures remain identical
2. **No behavior changes** - Logic is moved, not modified
3. **No new abstractions** - No new traits, generics, or wrapper types
4. **No new tests** - Only relocate existing tests
5. **No performance optimizations** - Pure refactoring
6. **No documentation changes** - Except module-level docs if needed
7. **No changes to external callers** - `scheduler.rs` pub use statements unchanged
8. **No renaming** - All function/struct/constant names preserved
9. **No test consolidation** - Keep tests in their respective modules

## Test Strategy

### Unit Tests

All 49 existing tests will be relocated to their respective submodules:

| Module | Tests Count | Test Names |
|--------|-------------|------------|
| `format.rs` | 11 | format_duration_*, format_bytes_*, colorize_status_* |
| `task_queries.rs` | 12 | resolve_task_id_*, load_task_summary_*, list_tasks_impl_*, get_task_details_impl_*, delete_task_impl_* |
| `log_stream.rs` | 14 | tail_lines_*, stream_task_logs_impl_* |
| `watch.rs` | 4 | render_watch_frame_* |
| `mod.rs` | 2 | retry_query_* |
| `tests/mod.rs` | 6 | Helper functions + integration-style tests |

### Test Execution

```bash
# Run all tests
cargo test

# Run tests for specific module
cargo test scheduler::query::task_queries
cargo test scheduler::query::log_stream
cargo test scheduler::query::watch
cargo test scheduler::query::format

# Run with coverage
cargo llvm-cov --html
```

### Verification Commands

```bash
# Compilation check
cargo check

# Full test suite
cargo test --all

# Clippy lint
cargo clippy --all-targets -- -D warnings

# Format check
cargo fmt --check
```

## QA Strategy

### Classification: REFACTORING

This task is a **pure refactoring** with no behavioral changes. The code is reorganized but the logic remains identical.

### QA Approach: Behavioral Equivalence via Unit Tests

Since this is a refactoring task:

1. **No new QA documents needed** - The existing unit tests are sufficient to verify behavioral equivalence
2. **Test coverage must not decrease** - All 49 existing tests must pass after refactoring
3. **External API verification** - The 8 public functions remain accessible at the same paths

### Verification Checklist

- [ ] `cargo test` passes with all 49 tests
- [ ] `cargo clippy` reports no new warnings
- [ ] `cargo fmt --check` passes
- [ ] Manual verification: `scheduler::query::*` exports work correctly
- [ ] No changes to downstream callers required

### Regression Prevention

The existing test suite covers:
- Task ID resolution (exact, prefix, not found)
- Task summary loading (with counts, by prefix)
- Task listing (empty, single, multiple, ordering)
- Task details (with/without command runs)
- Task deletion (with log file cleanup)
- Log streaming (various tail counts, timestamps, missing logs, partial results)
- Watch frame rendering (running steps, heartbeats, anomaly display)
- Formatting utilities (duration, bytes, status colors)
- Retry logic (transient vs permanent errors)

These tests ensure the refactored code behaves identically to the original.

## File Size Estimates

| File | Estimated Lines |
|------|-----------------|
| `mod.rs` | ~80 |
| `task_queries.rs` | ~150 |
| `log_stream.rs` | ~250 |
| `watch.rs` | ~320 |
| `format.rs` | ~35 |
| `tests/mod.rs` | ~150 (helpers + integration tests) |
| **Total** | ~985 (code) + ~800 (tests) = ~1785 |

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Circular dependencies between modules | Careful ordering: format.rs → task_queries.rs → watch.rs → log_stream.rs |
| Missing re-exports | Verify each public function is re-exported in mod.rs |
| Test import paths | Update `use super::*` in test modules |
| Clippy warnings on internal calls | Use `pub(crate)` for internal-only functions |

## Success Criteria

1. All 49 tests pass
2. No new clippy warnings
3. External callers require no changes
4. `scheduler::query::*` public API unchanged
5. Code compiles with `cargo check`
