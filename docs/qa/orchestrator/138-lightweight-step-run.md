---
self_referential_safe: true
---
# QA: FR-090 Lightweight Step Run

Verifies the three phases of FR-090: step filtering on `task create`, synchronous `orchestrator run`, and direct assembly mode.

## S1: Step filter validation rejects unknown step IDs

**Steps:**
1. `grep 'unknown step id' core/src/task_ops.rs`
2. Verify the validation logic checks each step ID against the execution plan.
3. `cargo test --lib -p agent-orchestrator -- create_task`

**Expected:** Validation code present; all existing task creation tests pass.

## S2: `--step` and `--set` flags exist on `task create`

**Steps:**
1. `cargo run -- task create --help 2>&1 | grep -E '\-\-step|\-\-set'`

**Expected:** Both `--step` and `--set` appear in CLI help.

## S3: step_filter and initial_vars persisted in tasks table

**Steps:**
1. `grep 'step_filter_json' core/src/persistence/migration_steps.rs`
2. `grep 'initial_vars_json' core/src/persistence/migration_steps.rs`
3. `grep 'step_filter_json' core/src/task_ops.rs`

**Expected:** Migration m0023 adds both columns; task creation INSERT includes both.

## S4: TaskRuntimeContext loads step_filter from DB

**Steps:**
1. `grep 'step_filter' crates/orchestrator-scheduler/src/scheduler/runtime.rs`
2. `grep 'step_filter' crates/orchestrator-config/src/config/execution.rs`

**Expected:** `step_filter: Option<HashSet<String>>` field in TaskRuntimeContext; parsed from `step_filter_json` column.

## S5: build_scope_segments respects task-level step_filter

**Steps:**
1. `grep 'step_filter' crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs`

**Expected:** Segment builder skips steps not in the filter set.

## S6: initial_vars injected into pipeline_vars

**Steps:**
1. `grep 'initial_vars' crates/orchestrator-scheduler/src/scheduler/runtime.rs`

**Expected:** initial_vars_json parsed and merged into pipeline_vars.vars with `entry().or_insert()`.

## S7: `orchestrator run` command exists

**Steps:**
1. `cargo run -- run --help 2>&1 | head -20`

**Expected:** Shows help with --workflow, --step, --set, --detach, --template, --agent-capability, --profile flags.

## S8: RunStep gRPC endpoint registered

**Steps:**
1. `grep 'RunStep' crates/proto/orchestrator.proto`
2. `grep 'run_step' crates/daemon/src/server/mod.rs`

**Expected:** RunStep RPC defined in proto; dispatched in server impl.

## S9: Direct assembly mode validates template and capability

**Steps:**
1. `grep 'step template.*not found' core/src/task_ops.rs`
2. `grep 'no agent.*has capability' core/src/task_ops.rs`

**Expected:** Both validation error messages present in `create_run_step_task`.

## S10: All tests pass

**Steps:**
1. `cargo test --workspace`
2. `cargo clippy --workspace --all-targets -- -D warnings`

**Expected:** Zero failures, zero warnings.
