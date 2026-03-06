# WP04: Invariant Constraints — Tamper-Proof Safety Boundaries

## Problem

In a self-modifying workflow, all evaluation logic (QA steps, test commands, prehook conditions) lives inside the same workflow that the `implement` step can modify. This creates a critical vulnerability:

**The system can weaken its own evaluation criteria to make low-quality changes appear to pass.**

Example attack path:
1. Cycle 1: implement step modifies `self_test` command to always return 0
2. Cycle 2: self_test passes trivially, broken code ships

Current `self_test` builtin partially addresses this (it's hardcoded to run `cargo check` + `cargo test`), but:
- Custom test steps in the workflow are still mutable
- QA documents that define pass criteria can be modified by implement
- Prehook conditions can be weakened
- Metric baselines can be overwritten

## Goal

Introduce **Invariant Constraints** — safety assertions that are:
1. **Declared separately** from the mutable workflow steps
2. **Enforced by the engine** at defined checkpoints
3. **Immutable** during workflow execution (steps cannot modify them)
4. **Fail-safe** — invariant violation halts the task, not just logs a warning

## Dependencies

- **WP01 (Persistent Store)**: Invariants can reference stored baselines (e.g., "test count must not decrease from stored value")

## Design

### 1. Invariant Declaration

Invariants are declared in the `safety` block of a workflow, separate from `steps`:

```yaml
safety:
  max_consecutive_failures: 3
  auto_rollback: false
  checkpoint_strategy: git_tag

  invariants:
    - name: core_tests_pass
      description: "All library unit tests must pass"
      command: "cd core && cargo test --lib 2>&1"
      expect_exit: 0
      immutable: true

    - name: test_count_no_regression
      description: "Test count must not decrease from baseline"
      command: "cd core && cargo test --lib 2>&1 | grep -oP '\\d+ passed' | grep -oP '\\d+'"
      capture_as: current_test_count
      assert: "int(current_test_count) >= int(store('baselines', 'min_test_count'))"
      immutable: true

    - name: no_unsafe_code
      description: "No new unsafe blocks introduced"
      command: "grep -r 'unsafe {' core/src/ | wc -l"
      capture_as: unsafe_count
      assert: "int(unsafe_count) <= int(store('baselines', 'max_unsafe_blocks'))"
      immutable: true

    - name: binary_size_bounded
      description: "Release binary must not exceed 50MB"
      command: "stat -f%z core/target/release/agent-orchestrator 2>/dev/null || echo 0"
      capture_as: binary_size
      assert: "int(binary_size) < 52428800"
```

### 2. Invariant Properties

| Property | Type | Description |
|----------|------|-------------|
| `name` | string | Unique identifier |
| `description` | string | Human-readable explanation |
| `command` | string | Shell command to run (read-only check) |
| `expect_exit` | int | If set, command must exit with this code |
| `capture_as` | string | Pipeline var name for command stdout |
| `assert` | CEL expr | Boolean expression that must evaluate to true |
| `immutable` | bool | If true, engine guarantees this invariant cannot be modified by workflow steps |
| `check_at` | list | When to check: `before_cycle`, `after_implement`, `before_restart`, `before_complete` |
| `on_violation` | enum | `halt` (default), `rollback`, `warn` |

### 3. Check Points

Invariants are evaluated at configurable checkpoints during workflow execution:

```
┌─ Cycle Start ─────────────────────────────────────────────────────┐
│  [before_cycle] ← invariant check                                 │
│                                                                    │
│  plan → implement                                                  │
│                  [after_implement] ← invariant check               │
│                                                                    │
│  self_test → self_restart                                          │
│              [before_restart] ← invariant check                    │
│                                                                    │
│  qa_testing → align_tests → doc_governance                         │
│                                                                    │
│  [before_complete] ← invariant check                               │
└────────────────────────────────────────────────────────────────────┘
```

Default check_at: `["after_implement", "before_complete"]`

### 4. Immutability Enforcement

When `immutable: true`:

1. **File hash locking**: At task start, the engine computes a hash of the invariant definition. Before each check, it verifies the hash hasn't changed. If the workflow somehow modified the config, the check uses the original definition.

2. **In-memory pinning**: Immutable invariants are loaded into an `Arc<Vec<Invariant>>` at task start and never re-read from the mutable config during execution.

3. **Protected files** (optional): Invariants can declare files that implement steps must not modify:

```yaml
invariants:
  - name: golden_tests
    command: "cargo test --lib -- golden::"
    immutable: true
    protected_files:
      - "core/tests/golden/**"
      - "fixtures/golden/**"
```

The engine checks `git diff` after implement steps — if protected files are modified, the invariant is immediately violated.

### 5. Violation Response

```yaml
on_violation: halt       # Stop the task immediately, status = invariant_violated
on_violation: rollback   # Git reset to pre-implement state, retry with different approach
on_violation: warn       # Log warning + event, continue execution
```

#### Halt (default)

```rust
if !invariant_passed {
    state.db_writer.set_task_status(task_id, "invariant_violated", true).await?;
    state.db_writer.insert_event(task_id, None, "invariant_violated", &json!({
        "invariant": invariant.name,
        "description": invariant.description,
        "actual": captured_value,
        "assertion": invariant.assert_expr,
    }).to_string()).await?;
    return Ok(CycleSegmentOutcome::Halt);
}
```

#### Rollback

```rust
if !invariant_passed && invariant.on_violation == OnViolation::Rollback {
    // git stash or git checkout to pre-implement state
    rollback_to_checkpoint(state, task_id).await?;
    state.db_writer.insert_event(task_id, None, "invariant_rollback", &payload).await?;
    // Continue to next cycle with clean state
}
```

### 6. Engine Support

#### New types

```rust
pub struct Invariant {
    pub name: String,
    pub description: String,
    pub command: Option<String>,
    pub expect_exit: Option<i32>,
    pub capture_as: Option<String>,
    pub assert_expr: Option<String>,
    pub immutable: bool,
    pub check_at: Vec<InvariantCheckPoint>,
    pub on_violation: OnViolation,
    pub protected_files: Vec<String>,
}

pub enum InvariantCheckPoint {
    BeforeCycle,
    AfterImplement,
    BeforeRestart,
    BeforeComplete,
}

pub enum OnViolation {
    Halt,
    Rollback,
    Warn,
}
```

#### Integration points

1. **Config parsing**: Parse `safety.invariants` from workflow YAML
2. **Task startup**: Pin immutable invariants into `Arc<Vec<Invariant>>`
3. **Loop engine**: Insert invariant check calls at configured checkpoints
4. **New module** `core/src/scheduler/invariant.rs`: Invariant evaluation logic
5. **CEL context**: Extend with `store()` function and `capture_as` vars
6. **Events**: New event types `invariant_checked`, `invariant_violated`, `invariant_rollback`
7. **Task status**: New terminal status `invariant_violated`

### 7. CLI Support

```bash
# List invariants for a workflow
./orchestrator workflow invariants self-bootstrap

# Check invariants manually (dry run)
./orchestrator invariant check --workflow self-bootstrap

# View violation history
./orchestrator task events <id> --type invariant_violated
```

### 8. Relationship to Existing Safety

| Mechanism | Scope | Mutable? | Purpose |
|-----------|-------|----------|---------|
| `self_test` builtin | Hardcoded cargo check+test | No | Binary correctness |
| `max_consecutive_failures` | Task level | Via config | Prevent infinite loops |
| `.stable` checkpoint | Binary level | No | Binary rollback |
| **Invariant Constraints** | Arbitrary assertions | `immutable: true` = no | Tamper-proof quality gates |

Invariants complement existing safety — they don't replace `self_test` or `.stable`, they add a declarative, extensible layer on top.

## Files to Change

| File | Change |
|------|--------|
| `core/src/config/safety.rs` | Parse `invariants` in safety block |
| `core/src/scheduler/invariant.rs` (new) | Invariant evaluation engine |
| `core/src/scheduler/loop_engine.rs` | Insert invariant checks at checkpoints |
| `core/src/scheduler/item_executor/dispatch.rs` | After-implement invariant check |
| `core/src/prehook.rs` | Extend CEL with invariant capture vars |
| `core/src/migration.rs` | New task status `invariant_violated` (no schema change, just a new status string) |
| `core/src/cli/workflow.rs` | `workflow invariants` subcommand |

## Verification

```bash
# Unit tests
cargo test --lib -- invariant::tests
cargo test --lib -- config::safety::tests::parse_invariants

# Integration: invariant passes
./orchestrator apply -f fixtures/manifests/bundles/invariant-test.yaml
TASK=$(./orchestrator task create --workflow invariant_pass --goal "test passing invariant")
./orchestrator task start $TASK
./orchestrator task info $TASK -o json | jq '.task.status'
# Expected: "completed"

# Integration: invariant violation halts task
TASK2=$(./orchestrator task create --workflow invariant_fail --goal "test failing invariant")
./orchestrator task start $TASK2
./orchestrator task info $TASK2 -o json | jq '.task.status'
# Expected: "invariant_violated"

# Verify violation event
sqlite3 data/agent_orchestrator.db \
  "SELECT payload_json FROM events WHERE task_id='${TASK2}' AND event_type='invariant_violated';"
# Expected: JSON with invariant name and actual vs expected values

# Integration: immutable invariant cannot be weakened
# (workflow that tries to modify its own invariant command)
TASK3=$(./orchestrator task create --workflow invariant_tamper_test --goal "test immutability")
./orchestrator task start $TASK3
# Verify the original invariant definition was used, not the modified one
```
