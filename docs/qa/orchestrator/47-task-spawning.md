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

## Database Schema Reference

### Table: tasks (M8 additions)

| Column | Type | Notes |
|--------|------|-------|
| parent_task_id | TEXT | FK to parent task (nullable) |
| spawn_reason | TEXT | `"spawn_task"` or `"spawn_tasks"` |
| spawn_depth | INTEGER NOT NULL DEFAULT 0 | Depth in spawn tree |

Index: `idx_tasks_parent_id ON tasks(parent_task_id)`

---

## Scenario 1: Single Task Spawn via PostAction

### Preconditions
- A workflow exists with a step that has `post_actions: [{type: "spawn_task", goal: "improve {area}"}]`
- A parent task is running with pipeline variable `area = "authentication"`

### Goal
Verify that `SpawnTask` post-action creates a child task with resolved goal template and correct lineage.

### Steps
1. Configure a workflow step with SpawnTask post-action:
   ```yaml
   post_actions:
     - type: spawn_task
       goal: "improve {area}"
       inherit:
         workspace: true
         project: true
   ```
2. Run the parent task through the step that triggers the spawn
3. Query the database for spawned children

### Expected
- A new task row exists in `tasks` with `parent_task_id = {parent_task_id}`
- The child task's goal is `"improve authentication"` (template resolved)
- `spawn_reason = "spawn_task"`
- `spawn_depth = parent_spawn_depth + 1`
- A `task_spawned` event is emitted with `child_task_id`

### Expected Data State
```sql
SELECT id, goal, parent_task_id, spawn_reason, spawn_depth
FROM tasks WHERE parent_task_id = '{parent_task_id}';
-- Expected: 1 row, goal = 'improve authentication', spawn_reason = 'spawn_task', spawn_depth = 1
```

---

## Scenario 2: Batch Task Spawn from JSON Pipeline Variable

### Preconditions
- A workflow step has `post_actions: [{type: "spawn_tasks", from_var: "goals_output", json_path: "$.goals", mapping: {goal: "$.description"}, max_tasks: 3}]`
- Pipeline variable `goals_output` contains a JSON array with 5 goal objects

### Goal
Verify that `SpawnTasks` post-action creates up to `max_tasks` children from a JSON array.

### Steps
1. Set pipeline variable:
   ```json
   {"goals": [
     {"description": "fix auth bug"},
     {"description": "add logging"},
     {"description": "update deps"},
     {"description": "refactor db"},
     {"description": "add tests"}
   ]}
   ```
2. Run the parent task through the step that triggers the batch spawn
3. Query database for spawned children

### Expected
- Exactly 3 child tasks created (capped by `max_tasks: 3`)
- Each child has `parent_task_id` pointing to the parent
- Each child has `spawn_reason = "spawn_tasks"`
- A `tasks_spawned` event is emitted with `child_task_ids` array of length 3

### Expected Data State
```sql
SELECT COUNT(*) FROM tasks WHERE parent_task_id = '{parent_task_id}';
-- Expected: 3

SELECT goal FROM tasks WHERE parent_task_id = '{parent_task_id}' ORDER BY created_at;
-- Expected: 'fix auth bug', 'add logging', 'update deps'
```

---

## Scenario 3: Spawn Depth Limit Enforcement

### Preconditions
- SafetyConfig has `max_spawn_depth: 2`
- A task at spawn_depth=2 attempts to spawn a child

### Goal
Verify that spawn depth limits are enforced and excess spawns are rejected with a warning.

### Steps
1. Configure safety:
   ```yaml
   safety:
     max_spawn_depth: 2
   ```
2. Create a task chain: root (depth 0) → child (depth 1) → grandchild (depth 2)
3. Have the grandchild attempt to spawn another task

### Expected
- The spawn attempt at depth 2 is **skipped** (not an error, just a warning)
- Log message: `"spawn_task skipped: depth limit"` or `"spawn_tasks skipped: depth limit"`
- No new task row is created
- The parent step continues execution normally (spawn failure does not halt the step)

### Expected Data State
```sql
SELECT MAX(spawn_depth) FROM tasks WHERE parent_task_id IS NOT NULL;
-- Expected: 2 (no depth-3 tasks exist)
```

---

## Scenario 4: Spawn Inherits Workspace and Project

### Preconditions
- Parent task has `workspace_id = "ws-1"`, `project_id = "proj-1"`, `workflow_id = "wf-1"`
- SpawnTaskAction has `inherit: {workspace: true, project: true}`

### Goal
Verify that child tasks inherit parent's workspace and project when configured.

### Steps
1. Configure SpawnTaskAction:
   ```yaml
   post_actions:
     - type: spawn_task
       goal: "child task"
       inherit:
         workspace: true
         project: true
   ```
2. Execute the spawn
3. Query child task

### Expected
- Child task has `workspace_id = "ws-1"` and `project_id = "proj-1"`
- Child task name starts with `"spawn:"` prefix

### Expected Data State
```sql
SELECT workspace_id, project_id, workflow_id FROM tasks WHERE parent_task_id = '{parent_task_id}';
-- Expected: workspace_id = 'ws-1', project_id = 'proj-1', workflow_id = 'wf-1'
```

---

## Scenario 5: Spawn with Custom Workflow Override

### Preconditions
- Parent task uses `workflow_id = "wf-parent"`
- SpawnTaskAction specifies `workflow: "wf-child"`

### Goal
Verify that spawned child can use a different workflow than the parent.

### Steps
1. Configure:
   ```yaml
   post_actions:
     - type: spawn_task
       goal: "run with different workflow"
       workflow: "wf-child"
   ```
2. Execute the spawn
3. Query child task

### Expected
- Child task has `workflow_id = "wf-child"` (overridden, not inherited)
- Parent task retains `workflow_id = "wf-parent"`

### Expected Data State
```sql
SELECT workflow_id FROM tasks WHERE parent_task_id = '{parent_task_id}';
-- Expected: 'wf-child'
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Single task spawn via PostAction | ✅ | 2026-03-07 | claude | Code path verified: spawn.rs:11-64, apply.rs:105-135. Unit tests: resolve_template, validate_spawn_depth |
| 2 | Batch task spawn from JSON pipeline variable | ✅ | 2026-03-07 | claude | Code path verified: spawn.rs:67-148, apply.rs:136-167. max_tasks cap + per-item workflow override |
| 3 | Spawn depth limit enforcement | ✅ | 2026-03-07 | claude | Code path verified: spawn.rs:151-165. Tests: at_limit, within_limit, no_limit |
| 4 | Spawn inherits workspace and project | ✅ | 2026-03-07 | claude | Code path verified: spawn.rs:32-40. SpawnInherit defaults workspace=true, project=true |
| 5 | Spawn with custom workflow override | ✅ | 2026-03-07 | claude | Code path verified: spawn.rs:24-27 (single), spawn.rs:94-99 (batch per-item) |
