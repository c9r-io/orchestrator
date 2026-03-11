# 03 - Workflow Configuration

This chapter covers how to design workflows: step definitions, execution scopes, loop policies, finalize rules, and safety configuration.

## Workflow Structure

A workflow is defined under `spec` with three main sections:

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: my_workflow
spec:
  steps: [...]        # ordered list of steps
  loop: {...}         # loop policy
  finalize: {...}     # item terminal-state rules (optional)
  safety: {...}       # safety limits (optional)
  max_parallel: 4     # default parallelism for item-scoped segments (optional)
```

## Step Definition

Each step is a unit of work in the workflow pipeline.

### Complete Field Reference

```yaml
- id: plan                          # (required) unique step identifier
  type: plan                        # (optional) step type — defaults to id value
  scope: task                       # (optional) "task" or "item" — defaults based on id
  enabled: true                     # (required) whether this step runs
  repeatable: true                  # (optional) can re-run in subsequent cycles (default: true)
  required_capability: plan         # (optional) agent capability needed (auto-inferred from id)
  template: plan                    # (optional) StepTemplate name for prompt injection
  builtin: self_test                # (optional) builtin step handler name
  command: "cargo check"            # (optional) direct shell command (no agent needed)
  is_guard: false                   # (optional) marks loop-termination guard steps
  tty: false                        # (optional) allocate TTY for interactive agents
  max_parallel: 2                   # (optional) per-step parallelism override
  timeout_secs: 600                 # (optional) per-step timeout in seconds
  cost_preference: balance          # (optional) "performance" | "quality" | "balance"
  prehook: {...}                    # (optional) conditional execution — see chapter 04
  behavior: {...}                   # (optional) on_failure, captures, post_actions
  store_inputs: [...]               # (optional) read from workflow stores before execution
  store_outputs: [...]              # (optional) write to workflow stores after execution
```

### Step Execution Modes

A step can execute in one of four modes, resolved automatically:

| Mode | Trigger | Description |
|------|---------|-------------|
| **Builtin** | `builtin: self_test` or known id | Handled by the engine internally |
| **Agent** | `required_capability: plan` | Dispatched to a matching agent |
| **Command** | `command: "cargo check"` | Direct shell execution, no agent |
| **Chain** | `chain_steps: [...]` | Sequential sub-step execution |

If you don't specify `builtin` or `required_capability`, the engine infers from the step `id`:

- Known builtin IDs (`init_once`, `loop_guard`, `ticket_scan`, `self_test`, `self_restart`, `item_select`) → auto-builtin
- Known agent IDs (`plan`, `implement`, `qa`, `fix`, etc.) → auto-capability

### Execution Profiles

`execution_profile` selects the runtime boundary for an agent step:

- if omitted, the step uses implicit `host`
- only agent steps may set this field
- the referenced profile must exist in the same project

Recommended defaults:

- `implement` / `ticket_fix` -> sandbox profile
- `qa_testing` -> host profile

Example:

```yaml
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: sandbox_write
spec:
  mode: sandbox
  fs_mode: workspace_rw_scoped
  writable_paths:
    - src
    - docs
  network_mode: deny
```

```yaml
- id: implement
  type: implement
  required_capability: implement
  execution_profile: sandbox_write
```

Runtime notes:

- On the current macOS backend, `network_mode: deny` may surface as DNS failure or connection failure; both map to `sandbox_network_blocked`.
- On Linux `linux_native`, `network_mode: allowlist` is supported when the daemon runs as `root`, `ip` and `nft` are present, and the profile uses `fs_mode: inherit`.
- Sandbox events now carry a stable `reason_code`; use that for automation before falling back to free-form `stderr_excerpt`.
- `network_target` is best-effort metadata and may be empty for some error shapes.
- `network_mode: allowlist` still is not supported on macOS; it fails fast with `reason_code=unsupported_backend_feature` instead of silently degrading.
- `network_mode: allowlist` entries must be exact hostname/IP values with an optional port, for example `api.example.com`, `api.example.com:443`, `10.203.0.1`, or `[::1]:8443`.

### Known Step IDs

| ID | Default Scope | Default Mode | Description |
|----|--------------|--------------|-------------|
| `init_once` | task | builtin | One-time initialization |
| `plan` | task | agent | Implementation planning |
| `qa_doc_gen` | task | agent | Generate QA test documents |
| `implement` | task | agent | Code generation |
| `self_test` | task | builtin | `cargo check` + `cargo test --lib` |
| `self_restart` | task | builtin | Rebuild binary + restart process |
| `review` | task | agent | Code review |
| `build` | task | agent | Build step |
| `test` | task | agent | Test step |
| `lint` | task | agent | Lint step |
| `align_tests` | task | agent | Align tests after refactoring |
| `doc_governance` | task | agent | Audit QA doc quality |
| `git_ops` | task | agent | Git operations |
| `qa` | item | agent | QA execution (per file) |
| `qa_testing` | item | agent | QA scenario execution (per file) |
| `ticket_scan` | item | builtin | Scan for active tickets |
| `ticket_fix` | item | agent | Fix QA tickets |
| `fix` | item | agent | Apply fixes |
| `retest` | item | agent | Re-test after fix |
| `evaluate` | task | agent | Evaluate results |
| `item_select` | task | builtin | WP03: Select items by strategy |
| `loop_guard` | task | builtin | Loop termination check |
| `smoke_chain` | task | agent | Chained smoke test |

### Execution Scope

Steps execute in one of two scopes:

- **`task` scope**: Runs **once per cycle**. Used for planning, implementing, testing.
- **`item` scope**: Runs **once per task item** (QA file). Used for QA testing, ticket fixing.

Steps are grouped into contiguous **scope segments**. Within an item-scoped segment, items can execute in parallel up to `max_parallel`.

```
┌─── Task Segment ────────────┐  ┌── Item Segment ──┐  ┌── Task Segment ──┐
plan + implement + self_test     qa_testing + ticket_fix  align_tests + doc_governance
```

## Behavior Configuration

The `behavior` block controls what happens on step success/failure and how to extract results.

### on_failure / on_success

```yaml
behavior:
  on_failure:
    action: continue       # default — keep going
  # OR
  on_failure:
    action: set_status
    status: "build_failed"
  # OR
  on_failure:
    action: early_return
    status: "aborted"

  on_success:
    action: continue       # default
  # OR
  on_success:
    action: set_status
    status: "verified"
```

### captures

Extract values from step results into pipeline variables:

```yaml
behavior:
  captures:
    - var: build_output
      source: stdout       # stdout | stderr | exit_code | failed_flag | success_flag
```

### post_actions

Run actions after a step completes:

```yaml
behavior:
  post_actions:
    - type: create_ticket          # create a failure ticket
    - type: scan_tickets           # scan ticket directory
    - type: store_put              # write to workflow store (WP01)
      store: context
      key: finding
      from_var: plan_output
    - type: spawn_task             # spawn a child task (WP02)
      goal: "verify-changes"
      workflow: verify_workflow
    - type: generate_items         # generate dynamic items (WP03)
      from_var: candidates
```

## Loop Policy

The loop policy controls how many cycles a workflow runs.

```yaml
loop:
  mode: once              # run one cycle and stop (default)
```

```yaml
loop:
  mode: fixed             # run exactly N cycles
  max_cycles: 2
  enabled: true
  stop_when_no_unresolved: false   # false = always run all cycles
```

```yaml
loop:
  mode: infinite          # run until guard stops or max_cycles hit
  max_cycles: 10          # safety cap
```

### Loop Modes

| Mode | Behavior |
|------|----------|
| `once` | Single cycle, then stop |
| `fixed` | Exactly `max_cycles` cycles |
| `infinite` | Repeat until `loop_guard` step decides to stop, capped by `max_cycles` |

The `loop_guard` builtin step should be the last step in infinite/fixed workflows. It evaluates whether unresolved items remain and decides whether to continue.

## Finalize Rules

Finalize rules determine the terminal status of each task item at the end of a cycle. They use CEL expressions (same engine as prehooks).

```yaml
finalize:
  rules:
    - id: qa_passed_no_tickets
      engine: cel
      when: "active_ticket_count == 0 && qa_ran"
      status: qa_passed
      reason: "QA passed with no active tickets"

    - id: fix_verified
      engine: cel
      when: "fix_ran && retest_success"
      status: fix_verified
      reason: "Fix applied and retest passed"

    - id: fallback_pending
      engine: cel
      when: "true"
      status: pending
      reason: "Default fallback"
```

Rules are evaluated in order; the first match wins. See [Chapter 04](04-cel-prehooks.md) for finalize-context variables.

## Safety Configuration

The `safety` block protects against runaway or destructive workflows.

```yaml
safety:
  max_consecutive_failures: 3     # auto-rollback after N failures (default: 3)
  auto_rollback: true             # enable automatic rollback
  checkpoint_strategy: git_tag    # none | git_tag | git_stash
  binary_snapshot: true           # snapshot binary at cycle start (self-bootstrap)
  step_timeout_secs: 1800         # global step timeout (30 min)
  max_spawned_tasks: 10           # WP02: max child tasks per parent
  max_spawn_depth: 3              # WP02: max parent→child→grandchild depth
  invariants:                     # WP04: immutable safety assertions
    - id: no_delete_main
      check:
        command: "git branch --list main | wc -l"
        expect: "1"
      on_violation: abort
```

## Putting It Together

A complete self-bootstrap-style workflow:

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: self-bootstrap
spec:
  max_parallel: 4

  steps:
    # ── Task segment: plan → implement → self_test ──
    - id: plan
      scope: task
      template: plan
      enabled: true
      repeatable: false

    - id: implement
      scope: task
      template: implement
      enabled: true

    - id: self_test
      scope: task
      builtin: self_test
      enabled: true

    # ── Item segment: qa_testing → ticket_fix ──
    - id: qa_testing
      scope: item
      template: qa_testing
      enabled: true
      prehook:
        engine: cel
        when: "is_last_cycle"
        reason: "QA deferred to final cycle"

    - id: ticket_fix
      scope: item
      template: ticket_fix
      enabled: true
      max_parallel: 2
      prehook:
        engine: cel
        when: "is_last_cycle && active_ticket_count > 0"

    # ── Loop guard ──
    - id: loop_guard
      builtin: loop_guard
      enabled: true
      is_guard: true

  loop:
    mode: fixed
    max_cycles: 2

  safety:
    max_consecutive_failures: 3
    auto_rollback: true
    checkpoint_strategy: git_tag
```

## Next Steps

- [04 - CEL Prehooks](04-cel-prehooks.md) — dynamic step gating and all available variables
- [05 - Advanced Features](05-advanced-features.md) — CRDs, stores, task spawning
