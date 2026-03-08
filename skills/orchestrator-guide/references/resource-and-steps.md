# Resource Model & Step Configuration

## Table of Contents
- [Workspace](#workspace)
- [Agent](#agent)
- [StepTemplate](#steptemplate)
- [Pipeline Variables](#pipeline-variables)
- [Step Fields](#step-fields)
- [Known Step IDs](#known-step-ids)
- [Execution Scope](#execution-scope)
- [Behavior Configuration](#behavior-configuration)
- [Loop Policy](#loop-policy)

## Workspace

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: my-project
spec:
  root_path: "."
  qa_targets: [docs/qa]
  ticket_dir: docs/ticket
  self_referential: false   # true enables 4-layer survival mechanism
```

## Agent

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: coder
spec:
  capabilities: [implement, ticket_fix, align_tests]
  command: "claude --print -p '{prompt}'"
  metadata:
    cost: 100               # lower = preferred in selection
```

Selection priority: capability match (required) → cost (lower preferred). Agents are strictly project-scoped — only agents applied to the target project participate in selection.

## StepTemplate

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: plan
spec:
  description: "Implementation planning"
  prompt: "Create a plan for: {goal}. Project: {source_tree}. Diff: {diff}"
```

## Pipeline Variables

| Variable | Description |
|----------|-------------|
| `{goal}` | Task goal string |
| `{source_tree}` | Workspace root path |
| `{workspace_root}` | Absolute workspace path |
| `{diff}` | Current git diff |
| `{rel_path}` | Current item relative path (item-scoped) |
| `{qa_file_path}` | QA file path for current item |
| `{plan_output_path}` | Plan step output file path |
| `{ticket_paths}` | Active ticket paths |
| `{ticket_dir}` | Ticket directory |
| `{task_id}` | Current task ID |
| `{task_item_id}` | Current task item ID |
| `{cycle}` | Current cycle number |
| `{workspace}` | Workspace ID |
| `{project}` | Project ID |
| `{workflow}` | Workflow ID |
| `{prev_stdout}` | Previous step stdout |
| `{prev_stderr}` | Previous step stderr |
| `{<step_id>_output}` | Output from named step |
| `{prompt}` | Resolved prompt (in Agent command) |

Values >4096 bytes spill to disk as `{<key>_path}`.

## Step Fields

```yaml
- id: plan                          # (required) unique identifier
  type: plan                        # (optional) defaults to id
  scope: task                       # (optional) task | item, inferred from id
  enabled: true                     # (required)
  repeatable: true                  # (optional, default: true)
  required_capability: plan         # (optional) auto-inferred from id
  template: plan                    # (optional) StepTemplate name
  builtin: self_test                # (optional) builtin handler
  command: "cargo check"            # (optional) direct shell command
  is_guard: false                   # (optional) loop guard marker
  tty: false                        # (optional) allocate TTY
  max_parallel: 2                   # (optional) per-step parallelism
  timeout_secs: 600                 # (optional) per-step timeout
  cost_preference: balance          # (optional) performance | quality | balance
  prehook: {engine: cel, when: "...", reason: "..."}
  behavior: {on_failure: ..., captures: [...], post_actions: [...]}
  store_inputs: [{store: X, key: Y, as_var: Z}]
  store_outputs: [{store: X, key: Y, from_var: Z}]
```

## Known Step IDs

### Task-scoped (run once per cycle)
`init_once`(builtin), `plan`, `qa_doc_gen`, `implement`, `self_test`(builtin), `self_restart`(builtin), `review`, `build`, `test`, `lint`, `align_tests`, `doc_governance`, `git_ops`, `evaluate`, `item_select`(builtin), `loop_guard`(builtin), `smoke_chain`

### Item-scoped (fan out per QA file)
`qa`, `qa_testing`, `ticket_scan`(builtin), `ticket_fix`, `fix`, `retest`

### Builtin steps (engine-handled, no agent needed)
`init_once`, `loop_guard`, `ticket_scan`, `self_test`, `self_restart`, `item_select`

## Execution Scope

Steps are grouped into contiguous **scope segments**:

```
┌── Task Segment ──────────┐  ┌── Item Segment ────┐  ┌── Task Segment ─────┐
plan + implement + self_test  qa_testing + ticket_fix  align_tests + doc_gov
```

Item segments run items in parallel up to `max_parallel`.

## Behavior Configuration

```yaml
behavior:
  on_failure:
    action: continue          # continue | set_status | early_return
  on_success:
    action: continue
  captures:
    - var: build_output
      source: stdout          # stdout | stderr | exit_code | failed_flag | success_flag
  post_actions:
    - type: create_ticket
    - type: scan_tickets
    - type: store_put         # WP01
      store: context
      key: finding
      from_var: plan_output
    - type: spawn_task        # WP02
      goal: "verify"
      workflow: verify_wf
    - type: spawn_tasks       # WP02 (multiple)
      from_var: task_list
      workflow: child_wf
    - type: generate_items    # WP03
      from_var: candidates
```

## Loop Policy

```yaml
loop:
  mode: once                  # once | fixed | infinite
  max_cycles: 2              # required for fixed/infinite
  enabled: true
  stop_when_no_unresolved: false
```

| Mode | Behavior |
|------|----------|
| `once` | Single cycle |
| `fixed` | Exactly max_cycles cycles |
| `infinite` | Until loop_guard stops or max_cycles reached |
