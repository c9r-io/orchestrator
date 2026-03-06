# WP03: Dynamic Items + Selection — Multi-Candidate Exploration

## Problem

Items are currently bound to QA target files, discovered at task creation time. This means:
- Every item follows the same execution path
- No way to explore multiple implementation strategies in parallel
- No way to compare candidates and select the best one
- The workflow is single-path: plan one thing, implement one thing, test one thing

For evolutionary self-improvement, you need: try N approaches → evaluate each → keep the best.

## Goal

Allow workflow steps to **generate items dynamically** at runtime, execute them in parallel as competing candidates, and **select winners** based on evaluation criteria. This turns the single-path workflow into a multi-path exploration engine.

## Dependencies

- **WP01 (Persistent Store)**: Candidate scores and comparison baselines are stored across items and tasks
- **Existing**: Parallel item execution (`max_parallel` in segments) already works

## Design

### 1. Dynamic Item Source

Currently items come from `qa_targets` files. Add a new item source: **step output**.

```yaml
- id: generate_candidates
  type: plan
  scope: task
  command: |
    echo '{"candidates":[
      {"id":"approach-a","label":"Optimize with caching","strategy":"memoize hot paths"},
      {"id":"approach-b","label":"Optimize with batching","strategy":"batch DB writes"},
      {"id":"approach-c","label":"Optimize with indexing","strategy":"add covering indexes"}
    ]}'
  captures:
    - regex: '(?s)(.*)'
      var: candidates_json
  post_actions:
    - generate_items:
        from_var: candidates_json
        json_path: "$.candidates"
        mapping:
          item_id: "$.id"
          label: "$.label"                # display name
          vars:                            # per-item pipeline vars
            strategy: "$.strategy"
        replace: true                      # replace static items with dynamic ones
```

After this step, the task has 3 items instead of whatever was in `qa_targets`. Each item carries its own `strategy` pipeline var.

### 2. Per-Item Variable Isolation

Each dynamic item gets its own pipeline_vars fork:

```
Task pipeline_vars (shared):  { goal, plan_output, ... }
  ├── Item A vars:  { strategy: "memoize hot paths", ...item A captures... }
  ├── Item B vars:  { strategy: "batch DB writes", ...item B captures... }
  └── Item C vars:  { strategy: "add covering indexes", ...item C captures... }
```

Item-scoped steps already get a cloned `pipeline_vars`. Dynamic items extend this with per-item vars from the mapping.

### 3. Candidate Evaluation Step

After item-scoped steps produce results, a task-scoped **selection step** compares them:

```yaml
- id: implement_candidate
  type: implement
  scope: item
  max_parallel: 3
  # Each item runs implement with its own {strategy} var

- id: test_candidate
  type: test
  scope: item
  max_parallel: 3
  command: "cargo bench -- {strategy} 2>&1"
  captures:
    - regex: 'time:\s+(\d+)ms'
      var: bench_time_ms

- id: select_best
  type: evaluate
  scope: task
  builtin: item_select
  config:
    strategy: min                         # minimize the metric
    metric_var: bench_time_ms             # which pipeline var to compare
    tie_break: first                      # if equal, pick first
    store_result:
      namespace: evolution
      key: "winner_{{task_id}}"
```

The `item_select` builtin:
1. Collects `bench_time_ms` from all items' pipeline_vars
2. Selects the item with the minimum value
3. Sets the winner's pipeline_vars as the task-level pipeline_vars
4. Marks non-winners with a terminal status (`eliminated`)
5. Persists the result to store (if configured)

### 4. Selection Strategies

```yaml
builtin: item_select
config:
  strategy: min | max | threshold | weighted

  # min/max: pick the item with lowest/highest metric value
  metric_var: bench_time_ms

  # threshold: keep all items above/below a threshold
  threshold: 100

  # weighted: score by multiple metrics
  weights:
    bench_time_ms: -0.5      # lower is better (negative weight)
    test_pass_rate: 1.0       # higher is better
    code_complexity: -0.3     # lower is better
```

### 5. Item Lifecycle for Dynamic Items

```
generate_items → pending
  ├── item-scoped steps → running → captures collected
  ├── item_select → winner selected
  │     ├── winner → qa_passed (continues to subsequent steps)
  │     └── others → eliminated (terminal, skipped for remaining steps)
  └── remaining task-scoped steps run with winner's context
```

### 6. Workflow Example — Evolutionary Self-Improvement

```yaml
steps:
  - id: analyze
    type: plan
    scope: task
    # Analyze codebase, propose 3 improvement strategies
    post_actions:
      - generate_items:
          from_var: plan_output
          json_path: "$.strategies"
          mapping:
            item_id: "$.id"
            label: "$.name"
            vars:
              approach: "$.description"
              target_files: "$.files"

  - id: implement
    type: implement
    scope: item
    max_parallel: 3
    # Each candidate implements its approach independently

  - id: test
    type: test
    scope: item
    max_parallel: 3
    command: "cargo test --lib 2>&1 && cargo bench 2>&1"

  - id: select
    type: evaluate
    scope: task
    builtin: item_select
    config:
      strategy: weighted
      weights:
        test_pass_count: 1.0
        bench_time_ms: -0.5

  - id: apply_winner
    type: implement
    scope: task
    # Apply the winning candidate's changes to the main branch

  - id: self_test
    type: self_test
    scope: task
    builtin: self_test
```

### 7. Engine Support

#### New types

```rust
pub struct GenerateItemsAction {
    pub from_var: String,
    pub json_path: String,
    pub mapping: DynamicItemMapping,
    pub replace: bool,
}

pub struct DynamicItemMapping {
    pub item_id: String,       // json_path to id field
    pub label: String,         // json_path to label field
    pub vars: HashMap<String, String>,  // key → json_path for per-item vars
}

pub struct ItemSelectConfig {
    pub strategy: SelectionStrategy,
    pub metric_var: Option<String>,
    pub weights: Option<HashMap<String, f64>>,
    pub threshold: Option<f64>,
    pub store_result: Option<StoreTarget>,
}

pub enum SelectionStrategy {
    Min,
    Max,
    Threshold,
    Weighted,
}
```

#### Integration points

1. **Post-action processing** (`apply.rs`): `generate_items` creates new `task_items` rows, replaces static items if `replace: true`
2. **Item executor**: Inject per-item vars into the item's pipeline_vars clone
3. **New builtin** (`item_select`): Collect metrics across items, apply selection, mark winners/losers
4. **Loop engine**: After selection, subsequent task-scoped steps use winner's context

### 8. Git Worktree Integration (Future)

For code changes, each candidate should work in an isolated branch:

```yaml
- id: implement
  type: implement
  scope: item
  max_parallel: 3
  isolation: worktree           # each item gets its own git worktree
```

This is a natural extension but not required for the initial implementation. Items can start with non-code exploration (strategy evaluation, benchmark comparison) before adding code isolation.

## Files to Change

| File | Change |
|------|--------|
| `core/src/config/step.rs` | Parse `generate_items` post_action, `item_select` builtin config |
| `core/src/scheduler/item_executor/apply.rs` | Execute `generate_items` post_action |
| `core/src/scheduler/item_executor/dispatch.rs` | Handle `item_select` builtin |
| `core/src/scheduler/loop_engine.rs` | Dynamic item injection between segments |
| `core/src/task_ops.rs` | `create_dynamic_items()` for runtime item creation |
| `core/src/dto.rs` | Extend TaskItemRow with `dynamic_vars_json` |
| `core/src/migration.rs` | Migration N: `dynamic_vars_json` column on task_items |

## Verification

```bash
# Unit tests
cargo test --lib -- config::step::tests::parse_generate_items
cargo test --lib -- dispatch::tests::item_select_min_strategy
cargo test --lib -- dispatch::tests::item_select_weighted_strategy

# Integration: 3-candidate workflow
./orchestrator apply -f fixtures/manifests/bundles/dynamic-items-test.yaml
TASK=$(./orchestrator task create --workflow evolutionary_test --goal "test candidate selection")
./orchestrator task start $TASK

# Verify: 3 items created dynamically
./orchestrator task info $TASK -o json | jq '.items | length'
# Expected: 3

# Verify: 1 winner, 2 eliminated
./orchestrator task info $TASK -o json | jq '[.items[] | .status] | group_by(.) | map({(.[0]): length}) | add'
# Expected: {"qa_passed": 1, "eliminated": 2}

# Verify: winner's context propagated
./orchestrator store get evolution "winner_${TASK}"
```
