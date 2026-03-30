---
self_referential_safe: true
---

# QA 134: QA Doctor Observability CLI

## FR Reference

FR-088

## Prerequisites

Build check: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` — all tests pass, no clippy warnings.

## Verification Scenarios

### Scenario 1: Proto definition exists

**Steps:**
1. `rg 'rpc QaDoctor' crates/proto/orchestrator.proto`

**Expected:** `rpc QaDoctor(QaDoctorRequest) returns (QaDoctorResponse);` is defined in the service block.

### Scenario 2: Response message fields

**Steps:**
1. `rg 'message QaDoctorResponse' -A 5 crates/proto/orchestrator.proto`

**Expected:** Response contains three fields: `task_execution_metrics_total` (uint64), `task_execution_metrics_last_24h` (uint64), `task_completion_rate` (double).

### Scenario 3: Core query function handles empty table

**Steps:**
1. `rg 'if total > 0' core/src/qa_doctor.rs`

**Expected:** The `qa_doctor_stats` function returns `0.0` for `task_completion_rate` when total is 0, avoiding division by zero.

### Scenario 4: CLI subcommand parses

**Steps:**
1. `cargo run -- qa doctor --help 2>&1 | head -10`

**Expected:** Help text shows `orchestrator qa doctor` with `-o` / `--output` flag accepting `table` and `json`.

### Scenario 5: JSON output structure

**Steps:**
1. `rg 'observability' crates/cli/src/commands/qa.rs`

**Expected:** JSON output nests metrics under an `"observability"` key with fields `task_execution_metrics_total`, `task_execution_metrics_last_24h`, `task_completion_rate`.

### Scenario 6: Table output format

**Steps:**
1. `rg 'task_execution_metrics_total' crates/cli/src/commands/qa.rs`

**Expected:** Table output prints a header row (`METRIC VALUE`) followed by three data rows, one per metric.

### Scenario 7: Daemon handler wiring

**Steps:**
1. `rg 'qa_doctor' crates/daemon/src/server/mod.rs`

**Expected:** The `OrchestratorService` trait impl delegates `qa_doctor` to `system::qa_doctor`.

### Scenario 8: Core module exported

**Steps:**
1. `rg 'pub mod qa_doctor' core/src/lib.rs`

**Expected:** `qa_doctor` module is publicly exported from the core crate.
