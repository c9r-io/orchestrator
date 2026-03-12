# Design Doc: Integration Test Coverage (FR-023)

## Overview

Added a dedicated integration test crate (`crates/integration-tests/`) that exercises
gRPC round-trips through an in-process daemon server backed by real `InnerState` and
SQLite persistence. This closes the gap between unit tests (per-module) and production
deployment, verifying that CLI → daemon → core paths work end-to-end.

## Architecture

### Test Harness (`crates/integration-tests/src/lib.rs`)

- **`TestOrchestratorServer`**: Implements the full `OrchestratorService` trait, mirroring
  the daemon's thin delegation pattern (`crates/daemon/src/server/`) but without control-plane
  authentication or shutdown rejection. Each RPC delegates directly to
  `agent_orchestrator::service::*` functions.

- **`TestHarness`**: Encapsulates test lifecycle:
  1. Creates `TestState` (isolated temp directory, SQLite DB, config)
  2. Applies YAML manifests via `apply_manifests()`
  3. Binds `TcpListener` on `127.0.0.1:0` (random port)
  4. Spawns tonic `Server` with `serve_with_incoming_shutdown`
  5. Returns connected `OrchestratorServiceClient`
  6. Provides `seed_qa_file()` for tests that need QA document targets
  7. Exposes `state()` for driving task execution via `start_task_blocking()`

### Cross-Crate Visibility

- **`test-harness` feature** on `agent-orchestrator` crate: gates `pub mod test_utils`
  with `#[cfg(any(test, feature = "test-harness"))]`, allowing the integration test crate
  to import `TestState`.
- **`pub(crate)` → `pub`** on `TestState` struct and methods (only when feature enabled).
- Clippy deny attributes (`panic`, `unwrap_used`, `expect_used`) relaxed for the
  `test-harness` feature since test utilities use these idiomatically.

### Test Execution Strategy

- Each test uses `tokio::time::timeout(30s, ...)` to prevent hangs.
- Tests are designed for serial execution (`--test-threads=1`) to avoid port/DB conflicts.
- Task execution is driven directly via `start_task_blocking()` (deterministic, no worker loop).
- Total execution time: ~1.6s for all 9 tests.

## Covered Scenarios

| # | Scenario | Test File | Test Function |
|---|----------|-----------|---------------|
| 1 | task create → start → complete | `lifecycle.rs` | `task_create_start_complete` |
| 2 | task pause → resume | `lifecycle.rs` | `task_pause_resume` |
| 3 | agent cordon → drain → uncordon | `agent_drain.rs` | `agent_cordon_drain_uncordon` |
| 4 | workflow with failing step | `workflow_loop.rs` | `workflow_failing_step` |
| 5 | workflow with prehook skip | `workflow_loop.rs` | `workflow_prehook_skip` |
| 6 | multi-cycle loop execution | `workflow_loop.rs` | `multi_cycle_loop` |
| 7 | gRPC API round-trip | `grpc_compat.rs` | `ping_roundtrip`, `task_crud_roundtrip`, `apply_get_describe_roundtrip` |

## Design Decisions

1. **Separate crate vs daemon integration tests**: Chosen because the daemon is binary-only.
   A separate crate avoids dual-target friction and keeps test infrastructure isolated.

2. **In-process gRPC vs subprocess**: In-process avoids process management complexity and
   port allocation races. The tradeoff (shared process space) is acceptable for tests.

3. **Duplicated `OrchestratorService` impl**: The test server mirrors the daemon's delegation
   but skips auth/shutdown logic. This duplication (~600 lines) is intentional — it tests the
   real gRPC serialization path without depending on daemon internals.

4. **Secret RPCs stubbed as `Unimplemented`**: Secret key operations require filesystem key
   material that TestState doesn't provision. These are tested via dedicated unit tests.
