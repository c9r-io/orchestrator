---
self_referential_safe: true
---

# Orchestrator - Task Spawning (WP02)

**Module**: orchestrator
**Scope**: SpawnTask / SpawnTasks post-actions, spawn depth safety, task lineage tracking
**Scenarios**: 5
**Priority**: High

---

## Background

WP02 adds task spawning as a PostAction. Steps can create child tasks from their output:

- `SpawnTask`: Spawn a single child task with a goal template (`{var}` substitution from pipeline vars)
- `SpawnTasks`: Spawn a batch of children from a JSON array in a pipeline variable

Child tasks record `parent_task_id`, `spawn_reason`, and `spawn_depth` in the `tasks` table. SafetyConfig provides `max_spawn_depth` to prevent runaway spawning.

---

## Scenario 1: Single Task Spawn via PostAction

### Goal
Verify that `SpawnTask` post-action creates a child task with resolved goal template and correct lineage.

### Steps

1. **Unit test** — verify goal template resolution:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_resolve_template
   cargo test -p orchestrator-scheduler --lib test_resolve_template_no_vars
   ```

2. **Unit test** — verify spawn execution creates child task:
   ```bash
   cargo test -p orchestrator-scheduler --lib execute_spawn_task_creates_child_task_and_increments_depth
   ```

3. **Unit test** — verify config serde:
   ```bash
   cargo test -p orchestrator-config --lib test_spawn_task_action_minimal
   ```

### Expected
- Template `"improve {area}"` resolves to `"improve authentication"` when `area = "authentication"`
- Child task has `parent_task_id`, `spawn_reason = "spawn_task"`, `spawn_depth = parent + 1`
- SpawnTaskAction config serializes/deserializes correctly

---

## Scenario 2: Batch Task Spawn from JSON Pipeline Variable

### Goal
Verify that `SpawnTasks` post-action creates up to `max_tasks` children from a JSON array.

### Steps

1. **Unit test** — verify batch spawn with limit:
   ```bash
   cargo test -p orchestrator-scheduler --lib execute_spawn_tasks_creates_batch_children_skips_missing_goal_and_honors_limit
   ```

2. **Unit test** — verify error on missing source variable:
   ```bash
   cargo test -p orchestrator-scheduler --lib execute_spawn_tasks_errors_when_source_variable_is_missing
   ```

3. **Unit test** — verify config serde:
   ```bash
   cargo test -p orchestrator-config --lib test_spawn_tasks_action_defaults
   cargo test -p orchestrator-config --lib test_spawn_tasks_action_full
   ```

### Expected
- Batch spawn creates up to `max_tasks` children, skipping items with missing goal
- Missing source variable returns an error
- SpawnTasksAction config defaults and full config serialize correctly

---

## Scenario 3: Spawn Depth Limit Enforcement

### Goal
Verify that spawn depth limits are enforced and excess spawns are rejected.

### Steps

1. **Unit test** — verify depth validation:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_validate_spawn_depth_within_limit
   cargo test -p orchestrator-scheduler --lib test_validate_spawn_depth_at_limit
   cargo test -p orchestrator-scheduler --lib test_validate_spawn_depth_no_limit
   ```

### Expected
- Spawn within limit: allowed
- Spawn at limit: rejected (depth = max_spawn_depth is at the boundary)
- No limit configured (None): allowed at any depth

---

## Scenario 4: Spawn Inherits Workspace and Project

### Goal
Verify that child tasks inherit parent's workspace and project when configured.

### Steps

1. **Unit test** — verify workspace inheritance:
   ```bash
   cargo test -p orchestrator-scheduler --lib execute_spawn_task_without_workspace_inheritance_uses_default_workspace
   ```

2. **Code review** — verify SpawnInherit defaults:
   ```bash
   rg -n "SpawnInherit|inherit.*workspace|inherit.*project" crates/orchestrator-scheduler/src/scheduler/spawn.rs
   ```

### Expected
- Without workspace inheritance, child uses default workspace
- SpawnInherit defaults: `workspace=true`, `project=true`

---

## Scenario 5: Spawn with Custom Workflow Override

### Goal
Verify that spawned child can use a different workflow than the parent.

### Steps

1. **Unit test** — verify duplicate task detection (spawn runner):
   ```bash
   cargo test -p orchestrator-scheduler --lib spawn_task_runner_returns_early_for_duplicate_task
   ```

2. **Code review** — verify workflow override in spawn config:
   ```bash
   rg -n "workflow.*override|SpawnTaskAction.*workflow" crates/orchestrator-scheduler/src/scheduler/spawn.rs crates/orchestrator-config/src/config/spawn.rs
   ```

### Expected
- Duplicate spawn attempt returns early without error
- Child task can specify a different `workflow` than parent

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Single task spawn via PostAction | ✅ | 2026-03-31 | claude | Unit tests: test_resolve_template, test_resolve_template_no_vars, execute_spawn_task_creates_child_task_and_increments_depth, test_spawn_task_action_minimal — all PASS |
| 2 | Batch task spawn from JSON pipeline variable | ✅ | 2026-03-31 | claude | Unit tests: execute_spawn_tasks_creates_batch_children_skips_missing_goal_and_honors_limit, execute_spawn_tasks_errors_when_source_variable_is_missing, test_spawn_tasks_action_defaults, test_spawn_tasks_action_full — all PASS |
| 3 | Spawn depth limit enforcement | ✅ | 2026-03-31 | claude | Unit tests: test_validate_spawn_depth_within_limit, test_validate_spawn_depth_at_limit, test_validate_spawn_depth_no_limit — all PASS |
| 4 | Spawn inherits workspace and project | ✅ | 2026-03-31 | claude | Unit test: execute_spawn_task_without_workspace_inheritance_uses_default_workspace PASS. Code review: SpawnInherit::default() → workspace=true, project=true |
| 5 | Spawn with custom workflow override | ✅ | 2026-03-31 | claude | Unit test: spawn_task_runner_returns_early_for_duplicate_task PASS. Code review: action.workflow optional override confirmed in spawn.rs lines 8-10, 49-51 |
