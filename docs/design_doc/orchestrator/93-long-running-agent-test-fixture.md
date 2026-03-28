# Design Doc 93: Long-Running Agent Test Fixture for Inflight Wait Scenarios

**Related**: FR-052 (Heartbeat-Aware Inflight Wait Timeout), QA Doc 106, Design Doc 64

## Problem

QA-106 scenarios S1 (heartbeat resets timeout), S2 (timeout without heartbeat), and S4 (diagnostic event fields) require agent processes that remain in-flight (`exit_code = -1`, PID alive) during the post-loop `wait_for_inflight_runs()` phase. The existing `mock_echo` agent exits immediately, so no runs remain in-flight by the time post-loop cleanup runs.

## Design Decision

**Approach**: Direct integration tests in `loop_engine/tests.rs` that call `wait_for_inflight_runs()` with crafted database state and real child processes.

**Why not full daemon E2E tests**: Step execution waits for child processes to complete, so runs don't naturally remain in-flight at post-loop time when orchestrated through the full scheduler. Daemon-level tests would require complex timing orchestration and would be fragile.

**Why direct function tests work**: `wait_for_inflight_runs()` only depends on:
- `InnerState` (database access)
- `task_id` (string)
- `SafetyConfig` (timeout/grace values)

All inputs can be crafted by inserting rows into SQLite (`command_runs` with `exit_code = -1` and a real PID) and spawning actual `sleep` processes.

## Implementation

### Test Infrastructure

- **`seed_inflight_command_run()`**: Inserts a `command_runs` row with `exit_code = -1` and a real process PID
- **`query_event_payloads()`**: Queries the `events` table for assertions on emitted events
- **`seed_inflight_test()`**: Creates a full `TestState` with task and item, ready for inflight testing

### Test Coverage

| Test | QA-106 Scenario | What It Verifies |
|------|----------------|-----------------|
| `inflight_wait_heartbeat_resets_timeout` | S1 | Heartbeats via async `insert_event` reset timeout timer; no timeout event emitted |
| `inflight_wait_timeout_without_heartbeat` | S2 | No heartbeats → timeout at ~4s → `reap_inflight_runs` kills process → `exit_code = -9` |
| `inflight_wait_timeout_diagnostic_fields` | S4 | Timeout event payload contains all 7 required diagnostic fields with correct values |

### Key Design Choices

1. **Real child processes**: Tests spawn `sleep 120` to get valid PIDs for `libc::kill(pid, 0)` liveness checks
2. **Zombie reaping**: After killing processes, tests call `child.wait()` to reap zombies before asserting PID death (zombies report as alive to `kill(pid, 0)`)
3. **Async heartbeats**: S1 uses `agent_orchestrator::events::insert_event` (async) rather than direct SQL, ensuring writes are visible through the same async database reader that `count_recent_heartbeats_for_items` uses
4. **Short timeouts**: Tests use 4-5s timeouts with 2-4s grace periods for fast execution (~10s total)

## Fixture

The YAML fixture `fixtures/manifests/bundles/qa106-long-running-agent.yaml` remains available for manual smoke testing with `mock_long_running` agent (`sleep 120`), though the primary verification is now through integration tests.
