# Task Trace - Post-Mortem Diagnostics Command

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Implement `task trace` command for execution timeline reconstruction and anomaly detection from the events/command_runs append-only journal.
**Related QA**: `docs/qa/orchestrator/32-task-trace.md`
**Created**: 2026-02-28
**Last Updated**: 2026-02-28

## Background And Goals

### Background

Debugging complex task execution issues — such as the duplicate-execution bug where two `run_task_loop` instances raced on the same task — required extensive manual forensics: querying the events table, cross-referencing command_runs, checking `ps` output, and tracing through segment execution code. The events table already contains a rich append-only journal (`cycle_started`, `step_started`, `step_finished`, `step_skipped`, etc.), and command_runs stores every agent invocation with exit codes, agent IDs, and timestamps. However, there was no tool to reconstruct a coherent timeline or detect anomalies automatically.

`task trace` is a pure, read-only post-mortem command that reconstructs execution history from existing data and surfaces anomalies that previously took hours to find manually.

### Goals

- Reconstruct per-cycle, per-step execution timelines from EventDto and CommandRunDto
- Automatically detect 9 classes of anomalies (duplicate runners, overlapping cycles/steps, unexpanded template variables, nonzero exits, orphan commands, missing step ends, long-running steps, empty cycles)
- Provide both human-readable terminal output (ANSI-colored) and machine-readable JSON output
- Support verbose mode that exposes item-level detail
- Enrich existing event payloads with `max_cycles`, `cycle`, and `pipeline_var_keys` for future diagnostics

### Non-goals

- Modifying task state or triggering any side effects (strictly read-only)
- Real-time tracing or streaming (use `task watch` / `task logs --follow` for that)
- Database schema changes (all enrichment is in existing `payload_json` blobs)
- Automatic remediation or fix suggestions
- Performance profiling or flame-graph generation

## Scope And User Experience

### Scope

- In scope:
  - New `task trace <task-id>` CLI subcommand with `--json` and `--verbose` flags
  - Pure `build_trace()` function in `core/src/scheduler/trace.rs`
  - 9 anomaly detection rules
  - Terminal renderer with ANSI color output
  - Event payload enrichment in `loop_engine.rs` and `item_executor.rs`
  - 17 unit tests for the pure trace logic
- Out of scope:
  - Database schema migrations
  - New event types (uses existing events)
  - Web UI or dashboard visualization
  - Distributed tracing (OpenTelemetry integration)

### CLI Interface

```
orchestrator task trace <task-id>           # Human-readable timeline
orchestrator task trace <task-id> --json    # Machine-readable JSON
orchestrator task trace <task-id> --verbose # Include item IDs per step
```

### Terminal Output Format

```
Task abc12345 — status: completed
Wall time: 4m 32s | 2 cycles | 6 steps | 4 commands (1 failed)

⚠ 2 anomalies detected:
  ERROR  overlapping_cycles — Cycle 2 started at 13:30:35 while Cycle 1 still running
   WARN  unexpanded_template_var — Command contains literal {plan_output_path}

── Cycle 1 ─────────────────────────────────
  13:28:18  ✓ plan            12s  agent=minimax-plan
  13:28:30  ✓ qa_doc_gen       8s  agent=minimax-qa
  13:28:38  ✗ implement       97s  agent=minimax-impl  exit=1
  13:30:15  ⊘ self_test          (prehook: build_failed)

── Cycle 2 ─────────────────────────────────
  13:30:35  ✓ plan            15s  agent=minimax-plan
```

Color coding: green ✓ (success), red ✗ (failure), gray ⊘ (skipped), yellow ⚠ (warnings), red ERROR.

## Interfaces And Data

### Core Data Structures

All structures derive `Serialize` for JSON output:

| Struct | Purpose |
|--------|---------|
| `TaskTrace` | Top-level trace result: task_id, status, cycles, anomalies, summary |
| `CycleTrace` | Per-cycle timeline: cycle number, start/end timestamps, steps |
| `StepTrace` | Per-step detail: step_id, scope, item_id, timestamps, exit_code, agent_id, duration, skip info |
| `Anomaly` | Detected issue: rule ID, severity, human-readable message, timestamp |
| `Severity` | Enum: Error, Warning, Info |
| `TraceSummary` | Aggregated stats: total cycles/steps/commands, failed commands, anomaly counts, wall time |

### Pure Entry Point

```rust
pub fn build_trace(
    task_id: &str,
    status: &str,
    events: &[EventDto],
    command_runs: &[CommandRunDto],
) -> TaskTrace
```

No database access, no side effects. Takes existing DTOs as input, returns a fully serializable trace.

### Event Payload Enrichment

Existing payloads enriched (no schema changes — `payload_json` is already a JSON blob):

| Event | Added Fields | Source |
|-------|-------------|--------|
| `cycle_started` | `max_cycles` | `loop_engine.rs` — from `loop_policy.guard.max_cycles` |
| `step_started` | `cycle`, `pipeline_var_keys` | `item_executor.rs` — from `task_ctx.current_cycle` and accumulated pipeline vars |

## Key Design And Tradeoffs

### Design Decisions

1. **Pure function architecture**: `build_trace()` takes `&[EventDto]` and `&[CommandRunDto]` as input with no database access. This makes the function fully testable with synthetic data and eliminates side-effect concerns.

2. **Reuse existing data loading**: The handler calls `get_task_details_impl()` which already loads events and command_runs in a single query batch. No new database queries needed.

3. **Decoupled layers**: Three distinct concerns are separated:
   - Trace logic (`build_trace`) — pure data transformation
   - CLI integration (handler in `task.rs`) — data loading + dispatch
   - Terminal rendering (`render_trace_terminal`) — presentation only

4. **Anomaly detection as rules**: Each anomaly detector is a standalone function with a clear rule ID, making it easy to add new rules or disable specific ones in the future.

5. **No regex dependency**: Template variable detection (`{var_name}` pattern) uses a hand-written scanner instead of adding a `regex` crate dependency, keeping the dependency tree lean.

### Alternatives And Tradeoffs

- **Option A**: SQL-based trace reconstruction (query events with window functions)
  - Pros: Could handle very large event sets efficiently
  - Cons: Complex SQL, harder to test, couples logic to database engine
- **Option B**: Pure function over loaded DTOs (chosen)
  - Pros: Fully testable, no DB coupling, portable
  - Cons: Loads all events into memory (acceptable for task-level granularity)
- **Option C**: Streaming trace with real-time anomaly detection
  - Pros: Could integrate with `task watch`
  - Cons: Over-engineering for a post-mortem tool; `task watch` already serves the live case

## Risks And Mitigations

- **Risk**: Very large tasks with thousands of events could be slow to load
  - Mitigation: `get_task_details_impl()` already loads all events efficiently; if needed, add pagination later
- **Risk**: False positive anomalies on edge-case event sequences
  - Mitigation: Each rule has a dedicated unit test; `clean_sequence_no_anomalies` test verifies zero false positives on normal runs
- **Risk**: Timestamp parsing failures for non-standard formats
  - Mitigation: `parse_timestamp()` tries 4 common formats; returns `None` gracefully on failure (wall_time shows "?" instead of crashing)

## Observability And Operations

### Logs

- No new log lines emitted by `task trace` (pure read-only command)
- Enriched event payloads (`max_cycles`, `cycle`, `pipeline_var_keys`) are visible in existing event queries and `task info` JSON output

### Metrics

- No new metrics required
- Anomaly counts in trace output serve as ad-hoc diagnostic metrics

### Operations / Release

- No database migrations required (payload enrichment uses existing JSON blob column)
- No configuration changes required
- Backward-compatible: old events without enriched fields are handled gracefully (fields default to `None`)

## Testing And Acceptance

### Test Plan

All tests in `core/src/scheduler/trace.rs` via `#[cfg(test)]`:

| Category | Tests | Description |
|----------|-------|-------------|
| Timeline reconstruction | 4 | Single cycle, multi-cycle, skipped steps, command enrichment |
| Anomaly detection | 9 | One test per rule: duplicate_runner, overlapping_cycles, overlapping_steps, unexpanded_template_var, nonzero_exit, orphan_command, missing_step_end, empty_cycle, long_running_step |
| No false positives | 1 | Clean event sequence produces zero anomalies |
| Edge cases | 1 | Empty events produces empty trace |
| JSON output | 1 | `serde_json::to_string` round-trips with expected structure |
| Wall time | 1 | Timestamp parsing and duration calculation |
| **Total** | **17** | All testing the pure `build_trace()` function with no DB |

### QA Docs

- `docs/qa/orchestrator/32-task-trace.md`

### Acceptance Criteria

1. `task trace <task-id>` renders a human-readable timeline with cycle/step structure
2. `task trace <task-id> --json` outputs valid, parseable JSON with all trace fields
3. `task trace <task-id> --verbose` shows item IDs for item-scoped steps
4. Anomaly detection correctly identifies: duplicate runners, overlapping cycles, overlapping steps, unexpanded template variables, nonzero exits, orphan commands, missing step ends, empty cycles, long-running steps
5. Clean task executions produce zero anomalies (no false positives)
6. Event payloads enriched: `cycle_started` includes `max_cycles`, `step_started` includes `cycle` and `pipeline_var_keys`
7. `cargo test --lib trace` passes all 17 unit tests
8. `cargo clippy` reports no new warnings
9. Full test suite passes with no regressions

## Files Changed

| File | Action | Description |
|------|--------|-------------|
| `core/src/scheduler/trace.rs` | Created | Core trace logic, anomaly detection, terminal rendering, 17 unit tests |
| `core/src/scheduler.rs` | Modified | Added `pub mod trace;` |
| `crates/cli/src/cli.rs` | Modified | Added `Trace { task_id, json, verbose }` to `TaskCommands` |
| `crates/cli/src/commands/task.rs` | Modified | Added trace handler: RPC load → render/serialize |
| `core/src/scheduler/loop_engine.rs` | Modified | Enriched `cycle_started` payload with `max_cycles` |
| `core/src/scheduler/item_executor.rs` | Modified | Enriched `step_started` payload with `cycle`, `pipeline_var_keys` |
