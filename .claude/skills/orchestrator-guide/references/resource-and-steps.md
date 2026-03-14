# Resource Model & Step Configuration

## Table of Contents
- [Workspace](#workspace)
- [Agent](#agent)
- [StepTemplate](#steptemplate)
- [ExecutionProfile](#executionprofile)
- [SecretStore & EnvStore](#secretstore--envstore)
- [RuntimePolicy](#runtimepolicy)
- [Trigger](#trigger)
- [Pipeline Variables](#pipeline-variables)
- [Step Fields](#step-fields)
- [Known Step IDs](#known-step-ids)
- [Execution Scope](#execution-scope)
- [Behavior Configuration](#behavior-configuration)
- [Loop Policy](#loop-policy)
- [Item Isolation](#item-isolation)

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
  env:
    - fromRef: claude-opus             # import all keys from SecretStore
    - name: RUST_BACKTRACE
      value: "1"                       # direct literal
    - name: API_KEY
      refValue:                        # import single key
        name: claude-opus
        key: ANTHROPIC_API_KEY
  metadata:
    cost: 100               # lower = preferred in selection
```

Selection priority: capability match (required) → cost (lower preferred) → project-scoped overrides global.

### Agent Lifecycle

```bash
orchestrator agent list                # list agents and state
orchestrator agent cordon <name>       # mark unschedulable
orchestrator agent uncordon <name>     # mark schedulable
orchestrator agent drain <name>        # cordon + wait for in-flight completion
```

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

## ExecutionProfile

Controls sandbox isolation for workflow steps. Referenced by name in step `execution_profile` field.

```yaml
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: sandbox_write
spec:
  mode: sandbox                    # host | sandbox
  fs_mode: workspace_rw_scoped    # inherit | workspace_readonly | workspace_rw_scoped
  writable_paths:                  # paths writable within sandbox
    - docs
    - core/src
  network_mode: inherit            # inherit | deny | allowlist
  network_allowlist:               # required when network_mode=allowlist (Linux only)
    - api.example.com:443
  max_memory_mb: 512               # optional resource limits
  max_cpu_seconds: 60
  max_processes: 4
  max_open_files: 1024
```

### Network Mode Details

| Mode | Behavior | Linux | macOS |
|------|----------|-------|-------|
| `inherit` | No restrictions (default) | Yes | Yes |
| `deny` | Block all outbound | nftables DROP | Seatbelt deny |
| `allowlist` | Only listed targets | nftables per-target | **Not supported** |

Allowlist entry formats: `host`, `host:port`, `ip`, `ip:port`, `[ipv6]:port`.
Invalid: URLs (`https://...`), wildcards (`*.example.com`), paths.

### Production vs QA Profiles

Production profiles live in `docs/workflow/execution-profiles.yaml` with `network_mode: inherit` (agents need API access). QA/fixture profiles in `fixtures/manifests/bundles/` use `network_mode: deny` to test sandbox enforcement.

## SecretStore & EnvStore

Both share the same spec structure. SecretStore values are encrypted at rest.

```yaml
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: claude-opus
spec:
  data:
    ANTHROPIC_API_KEY: "sk-ant-..."
---
apiVersion: orchestrator.dev/v2
kind: EnvStore
metadata:
  name: build-env
spec:
  data:
    CARGO_TERM_COLOR: "always"
```

### Secret Key Management

Encryption keys protect SecretStore data at rest. A fresh `orchestrator init` creates an active `primary` key.

```bash
orchestrator secret key status        # show active key
orchestrator secret key list          # list all keys with state
orchestrator secret key rotate        # rotate to new key (requires active key)
orchestrator secret key revoke <id>   # revoke a key
orchestrator secret key history       # audit trail
```

If all keys are retired/revoked, SecretStore writes are blocked. Re-init the DB to create a new key.

## RuntimePolicy

Singleton resource controlling runner behavior.

```yaml
apiVersion: orchestrator.dev/v2
kind: RuntimePolicy
metadata:
  name: default
spec:
  runner:
    shell: "/bin/bash"
    policy_mode: strict
    redaction_patterns:
      - "sk-ant-[a-zA-Z0-9]+"
  resume:
    auto: true
```

## Trigger

A Trigger enables cron-scheduled or event-driven automatic task creation. Follows the K8s CronJob mental model.

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: nightly-qa
spec:
  cron:
    schedule: "0 0 2 * * *"       # 6-field cron (sec min hour day month weekday)
    timezone: "Asia/Shanghai"      # IANA timezone (optional, default UTC)
  action:
    workflow: full-qa
    workspace: main-workspace
  concurrencyPolicy: Forbid        # Allow | Forbid | Replace
  suspend: false
  historyLimit: 5                  # max completed tasks to retain
```

| Field | Required | Description |
|-------|----------|-------------|
| `cron` | One of cron/event | Cron schedule with optional timezone |
| `event` | One of cron/event | Event-driven trigger (source + filter) |
| `action.workflow` | Yes | Workflow to run when triggered |
| `action.workspace` | Yes | Workspace for the created task |
| `concurrencyPolicy` | No | `Allow` (default), `Forbid` (skip if active), `Replace` (cancel + create) |
| `suspend` | No | Pause trigger without deleting (default: `false`) |
| `historyLimit` | No | Max completed tasks to keep (default: 5) |

### Event Trigger Example

```yaml
spec:
  event:
    source: task_completed
    filter:
      workflow: build-pipeline
  action:
    workflow: deploy
    workspace: prod
```

### Trigger Lifecycle Commands

```bash
orchestrator trigger suspend <name>   # pause trigger
orchestrator trigger resume <name>    # unpause trigger
orchestrator trigger fire <name>      # manually fire (create task now)
orchestrator get triggers             # list all triggers
orchestrator delete trigger/<name>    # remove trigger
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
  execution_profile: sandbox_write  # (optional) ExecutionProfile name
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

## Item Isolation

```yaml
item_isolation:
  strategy: git_worktree      # none (default) | git_worktree
  branch_prefix: "item-"      # prefix for temporary git branches
  cleanup: after_workflow      # after_workflow (default) | never
```

When `git_worktree` is enabled, each item-scoped step runs in an isolated git worktree. This allows parallel item execution without file conflicts.
