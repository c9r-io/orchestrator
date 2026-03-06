# WP02: Task Spawning — Step-Driven Task Creation

## Problem

Goals are static: set once at `task create`, never changed. A workflow cannot:
- Discover new improvement targets and act on them
- Decompose a large goal into sub-tasks
- Chain workflows (output of one task feeds into goal of another)
- Implement a "meta-planner" that directs the system's evolution

This means every improvement cycle requires human intervention to define the next goal.

## Goal

Allow workflow steps to **spawn new tasks** as a post-action. A step's output (e.g., a list of goals produced by a meta-planner agent) becomes the input for new tasks, enabling autonomous goal discovery and cascading workflows.

## Dependency

- **WP01 (Persistent Store)**: Spawned tasks need cross-task context. The parent task writes context to the store; child tasks read it via `store_inputs`.

## Design

### 1. Post-Action: `spawn_tasks`

```yaml
- id: discover_goals
  type: plan
  scope: task
  command: |
    echo '{"goals":[
      {"goal":"optimize DB query performance","priority":"high","workflow":"self-bootstrap"},
      {"goal":"add retry logic to agent runner","priority":"medium","workflow":"self-bootstrap"}
    ]}'
  captures:
    - regex: '(?s)(.*)'
      var: discovered_goals
  post_actions:
    - spawn_tasks:
        from_var: discovered_goals       # pipeline var containing JSON array
        json_path: "$.goals"             # path to array of goal objects
        mapping:
          goal: "$.goal"                 # required: where to find the goal text
          workflow: "$.workflow"          # optional: override workflow (default: current)
          priority: "$.priority"         # optional: metadata
        inherit:
          workspace: true                # inherit parent task's workspace
          project: true                  # inherit parent task's project
          target_files: true             # inherit parent task's target files
        max_tasks: 5                     # safety cap
        queue: true                      # create as pending, don't auto-start
```

### 2. Simplified Syntax — Single Task Spawn

For common case of spawning one follow-up task:

```yaml
post_actions:
  - spawn_task:
      goal: "Fix issues found in QA: {qa_summary}"
      workflow: ticket-fix-workflow
      inherit:
        workspace: true
        project: true
```

### 3. Task Lineage

Track parent-child relationships for observability:

```sql
-- Extend tasks table
ALTER TABLE tasks ADD COLUMN parent_task_id TEXT;
ALTER TABLE tasks ADD COLUMN spawn_reason TEXT;

CREATE INDEX idx_tasks_parent ON tasks(parent_task_id);
```

This enables:
- Tracing goal derivation chains
- Aggregating metrics across related tasks
- Preventing unbounded spawning (depth limits)

### 4. Safety Controls

Unbounded task spawning is dangerous. Multiple layers of protection:

#### a. Workflow-level limits

```yaml
safety:
  max_spawned_tasks: 10          # per task execution
  max_spawn_depth: 3             # no grandchild beyond depth 3
  spawn_cooldown_seconds: 60     # minimum time between spawn batches
```

#### b. Engine-enforced limits

```rust
const MAX_SPAWN_PER_TASK: usize = 20;     // hard cap regardless of YAML
const MAX_SPAWN_DEPTH: usize = 5;          // hard cap on lineage depth
```

#### c. Queue-only mode (default)

Spawned tasks are created with status `pending` — they don't auto-start. A human or a separate scheduler decides when to run them. This prevents runaway chains.

#### d. Spawn budget per project

```yaml
# In project config
projects:
  my-project:
    spawn_budget:
      max_pending_spawned: 20    # max pending spawned tasks at any time
      daily_limit: 50            # max tasks spawned per day
```

### 5. Engine Support

#### Config parsing

Extend `PostAction` enum:

```rust
pub enum PostAction {
    // ... existing variants
    SpawnTask(SpawnTaskAction),
    SpawnTasks(SpawnTasksAction),
}

pub struct SpawnTaskAction {
    pub goal: String,              // may contain {var} templates
    pub workflow: Option<String>,
    pub inherit_workspace: bool,
    pub inherit_project: bool,
    pub inherit_target_files: bool,
}

pub struct SpawnTasksAction {
    pub from_var: String,
    pub json_path: String,
    pub mapping: SpawnMapping,
    pub inherit: SpawnInherit,
    pub max_tasks: usize,
    pub queue: bool,
}
```

#### Execution

In `apply.rs` post_actions processing:

1. Resolve `from_var` from pipeline_vars
2. Parse JSON, extract array via json_path
3. For each entry (up to max_tasks):
   - Build `CreateTaskPayload` from mapping + inherited fields
   - Set `parent_task_id` and `spawn_reason`
   - Call `create_task_impl()` with status `pending`
4. Log spawned task IDs as event

#### Events

```json
{
  "event_type": "tasks_spawned",
  "payload": {
    "step": "discover_goals",
    "parent_task_id": "abc-123",
    "spawned": [
      {"task_id": "def-456", "goal": "optimize DB query performance"},
      {"task_id": "ghi-789", "goal": "add retry logic to agent runner"}
    ],
    "depth": 1
  }
}
```

### 6. CLI Support

```bash
# List spawned tasks
./orchestrator task list --parent abc-123
./orchestrator task tree abc-123          # show full lineage tree

# Start spawned tasks
./orchestrator task start-spawned abc-123  # start all pending children

# Inspect spawn budget
./orchestrator project spawn-budget my-project
```

### 7. Store Integration

Parent tasks should write context for children:

```yaml
- id: discover_goals
  type: plan
  post_actions:
    - store_put:
        namespace: spawn_context
        key: "parent_{{task_id}}"
        value_from: analysis_summary
    - spawn_tasks:
        from_var: discovered_goals
        # ...
```

Child workflows read parent context:

```yaml
- id: plan
  type: plan
  store_inputs:
    - namespace: spawn_context
      key: "parent_{{parent_task_id}}"
      into_var: parent_context
```

## Files to Change

| File | Change |
|------|--------|
| `core/src/migration.rs` | Migration 8: parent_task_id + spawn_reason columns |
| `core/src/config/step.rs` | Parse `spawn_task` / `spawn_tasks` post_actions |
| `core/src/config/safety.rs` | Parse spawn safety limits |
| `core/src/scheduler/item_executor/apply.rs` | Execute spawn post_actions |
| `core/src/task_ops.rs` | Accept parent_task_id in CreateTaskPayload |
| `core/src/dto.rs` | Extend CreateTaskPayload, TaskSummary |
| `core/src/cli/task.rs` | `task list --parent`, `task tree` subcommands |

## Verification

```bash
# Unit tests
cargo test --lib -- config::step::tests::parse_spawn_task_post_action
cargo test --lib -- apply::tests::spawn_tasks_from_output

# Integration
./orchestrator apply -f fixtures/manifests/bundles/spawn-test.yaml
PARENT=$(./orchestrator task create --workflow goal_discoverer --goal "find improvements")
./orchestrator task start $PARENT

# Verify children created
./orchestrator task list --parent $PARENT
# Should show N pending tasks with derived goals

# Verify lineage
./orchestrator task tree $PARENT

# Verify safety cap
# (workflow with max_tasks: 2 should only create 2 even if output has 10)
```
