# 05 - Advanced Features

This chapter covers advanced workflow primitives: Custom Resource Definitions, Persistent Stores, Task Spawning, Dynamic Items, and Invariant Constraints.

## Custom Resource Definitions (CRDs)

CRDs let you define new resource types beyond the built-in Workspace/Agent/Workflow/StepTemplate. This is useful for domain-specific configuration (prompt libraries, evaluation rubrics, etc.).

### Defining a CRD

```yaml
apiVersion: orchestrator.dev/v2
kind: CustomResourceDefinition
metadata:
  name: promptlibraries.extensions.orchestrator.dev
spec:
  kind: PromptLibrary
  plural: promptlibraries
  short_names: [pl]
  group: extensions.orchestrator.dev
  versions:
    - name: v1
      served: true
      schema:
        type: object
        required: [prompts]
        properties:
          prompts:
            type: array
            minItems: 1
            items:
              type: object
              required: [name, template]
              properties:
                name:
                  type: string
                template:
                  type: string
                tags:
                  type: array
                  items:
                    type: string
      cel_rules:
        - rule: "size(self.prompts) > 0"
          message: "at least one prompt is required"
```

### Creating CRD Instances

Once registered, create instances using the CRD's `group/version` as `apiVersion`:

```yaml
apiVersion: extensions.orchestrator.dev/v1
kind: PromptLibrary
metadata:
  name: qa-prompts
  labels:
    team: platform
spec:
  prompts:
    - name: code-review
      template: "Review the following code for {criteria}..."
      tags: [qa, review]
```

### Managing CRDs

```bash
# Apply CRD + instances
./scripts/orchestrator.sh apply -f crd-manifest.yaml

# List instances
./scripts/orchestrator.sh get promptlibraries
./scripts/orchestrator.sh get pl                    # using short name

# Describe
./scripts/orchestrator.sh describe promptlibrary qa-prompts

# Delete
./scripts/orchestrator.sh delete promptlibrary qa-prompts

# Export
./scripts/orchestrator.sh manifest export           # includes CRD resources
```

### CRD Validation

CRDs support two levels of validation:
- **JSON Schema**: `schema` defines structural validation (types, required fields, min/max)
- **CEL Rules**: `cel_rules` define semantic validation (cross-field constraints)

## Persistent Store (WP01)

The Persistent Store provides cross-task memory via a `WorkflowStore` CRD. Data persists across tasks, enabling learning from past runs.

### Defining a Store

```yaml
apiVersion: orchestrator.dev/v2
kind: WorkflowStore
metadata:
  name: context
spec:
  backend: local           # "local" (SQLite) or "command" (shell command)
  schema:
    type: object
    properties:
      value:
        type: string
  retention:
    max_entries: 1000
    ttl_seconds: 86400      # optional: auto-expire after 24h
```

### Reading/Writing from Steps

Steps interact with stores through `store_inputs`, `store_outputs`, and `post_actions`:

```yaml
steps:
  - id: plan
    scope: task
    enabled: true
    command: "echo '{\"confidence\":0.95}'"
    behavior:
      post_actions:
        - type: store_put
          store: context
          key: plan_result
          from_var: plan_output

  - id: implement
    scope: task
    enabled: true
    store_inputs:                # read from store before execution
      - store: context
        key: plan_result
        as_var: inherited_plan
```

### CLI Operations

```bash
# Put a value
./scripts/orchestrator.sh store put context my_key "my_value"

# Get a value
./scripts/orchestrator.sh store get context my_key

# List keys
./scripts/orchestrator.sh store list context

# Delete a key
./scripts/orchestrator.sh store delete context my_key
```

## Task Spawning (WP02)

Steps can spawn child tasks as post-actions, enabling autonomous work decomposition.

### Spawn a Single Task

```yaml
- id: plan
  scope: task
  enabled: true
  behavior:
    post_actions:
      - type: spawn_task
        goal: "verify-changes"
        workflow: verify_workflow
```

### Spawn Multiple Tasks

```yaml
- id: plan
  scope: task
  enabled: true
  behavior:
    post_actions:
      - type: spawn_tasks
        from_var: task_list        # pipeline variable containing JSON array of goals
        workflow: child_workflow
```

### Safety Limits

Task spawning is guarded by safety configuration:

```yaml
safety:
  max_spawned_tasks: 10      # max children per parent
  max_spawn_depth: 3         # max parent → child → grandchild depth
  spawn_cooldown_seconds: 5  # min seconds between spawn bursts
```

## Dynamic Items + Selection (WP03)

Workflow steps can dynamically generate task items at runtime and use tournament-style selection to pick the best candidates.

### Generating Items

```yaml
- id: generate
  scope: task
  enabled: true
  behavior:
    post_actions:
      - type: generate_items
        from_var: candidates       # pipeline variable with JSON array
```

### Item Selection

The `item_select` builtin step selects items using configurable strategies:

```yaml
- id: select_best
  scope: task
  builtin: item_select
  enabled: true
  item_select_config:
    strategy: weighted              # min | max | threshold | weighted
    metric_key: quality_score       # field to compare
    top_k: 3                        # select top N items
    threshold: 0.7                  # minimum score (for threshold strategy)
    weights:                        # field weights (for weighted strategy)
      confidence: 0.4
      quality_score: 0.6
```

| Strategy | Description |
|----------|-------------|
| `min` | Select items with lowest metric value |
| `max` | Select items with highest metric value |
| `threshold` | Select items above/below a threshold |
| `weighted` | Score by weighted combination of fields |

## Invariant Constraints (WP04)

Invariants are immutable safety assertions that cannot be weakened by the workflow itself. They are pinned at task start and enforced by the engine.

```yaml
safety:
  invariants:
    - id: main_branch_exists
      description: "The main branch must always exist"
      check:
        command: "git branch --list main | wc -l"
        expect: "1"
      on_violation: abort           # abort | warn | rollback
      protected_files:              # files that cannot be modified
        - ".github/workflows/*"
        - "Cargo.lock"
      checkpoint_filter:            # only check at specific points
        steps: [implement, self_test]
```

| on_violation | Behavior |
|-------------|----------|
| `abort` | Stop the task immediately |
| `warn` | Log a warning but continue |
| `rollback` | Restore to the last checkpoint |

## Next Steps

- [06 - Self-Bootstrap](06-self-bootstrap.md) — self-modifying workflows and survival mechanisms
- [04 - CEL Prehooks](04-cel-prehooks.md) — dynamic step gating
