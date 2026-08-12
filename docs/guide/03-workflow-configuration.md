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
```

> `store_inputs`, `store_outputs`, `step_vars` and the `store_put` post-action were removed.
> A manifest carrying any of them is rejected with `[legacy_pipeline_variables_removed]`.
> Steps read and write stores directly instead — see
> [Persistent Store](05-advanced-features.md#persistent-store-wp01).

### Step Execution Modes

A step can execute in one of four modes, resolved automatically:

| Mode | Trigger | Description |
|------|---------|-------------|
| **Builtin** | `builtin: self_test` or known id | Handled by the engine internally |
| **Agent** | `required_capability: plan` | Dispatched to a matching agent |
| **Command** | `command: "cargo check"` | Direct shell execution, no agent |
| **Chain** | `chain_steps: [...]` | Sequential child-step container with inherited pipeline vars |

If you don't specify `builtin` or `required_capability`, the engine infers from the step `id`:

- Known builtin IDs (`init_once`, `loop_guard`, `ticket_scan`, `self_test`, `self_restart`, `item_select`) → auto-builtin
- Known agent IDs (`plan`, `implement`, `qa`, `fix`, etc.) → auto-capability
- **Anything else → `required_capability = <the step id verbatim>`**

That third rule is the one to know about. The convention registry (`crates/orchestrator-config/src/config/step_conventions.rs`) accepts *any* step ID; an ID it does not recognize is not an error, it becomes a capability requirement named after itself. So a typo in `id:` does not fail validation — `loop_gaurd` silently stops being the loop guard builtin and starts demanding an agent with a `loop_gaurd` capability, which is a different failure, later, in a different place.

FR-166 evaluated making `type:` mandatory and decided against it: every existing workflow relies on inference, and the flag day would be larger than the defect. The mitigation is to write the rule down where authors read it, and to say plainly what it costs. **If a step's behaviour matters, state `builtin` or `required_capability` explicitly rather than relying on its ID** — inference is a convenience for the well-known IDs above, not a contract for the rest.

Chain runtime contract:

- The parent step is a container; it does not directly run its own agent or command once `chain_steps` is present.
- Child steps run serially and inherit the current `pipeline_vars`.
- Child outputs should be promoted via normal `captures` and pipeline variables, not hidden special cases.
- A child step applies its own `behavior.on_failure` first; the parent step then applies its own `behavior.on_failure` to the aggregated chain result.

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

> **Defaults when omitted:** `mode: host`, `fs_mode: inherit`, `network_mode: inherit`.

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

#### Sandbox Capability Matrix

| Feature | macOS (Seatbelt) | Linux (native) | Notes |
|---------|:----------------:|:--------------:|-------|
| `mode: sandbox` | Yes | Yes | Linux requires `ip`/`nft` and root |
| `fs_mode: inherit` | Yes | Yes | |
| `fs_mode: workspace_readonly` | Yes | **No** | Linux requires `fs_mode: inherit` [^1] |
| `fs_mode: workspace_rw_scoped` | Yes | **No** | Linux requires `fs_mode: inherit` [^1] |
| `network_mode: deny` | Yes | Yes | |
| `network_mode: allowlist` | **No** | Yes | macOS fails fast with `reason_code=unsupported_backend_feature` |
| `writable_paths` | Yes | **No** | Requires non-inherit `fs_mode` [^1] |
| Resource limits (`max_memory_mb`, etc.) | Yes | Yes | |

[^1]: Linux `linux_native` currently requires `fs_mode: inherit` until a filesystem isolation backend is implemented. Run `orchestrator check` to detect this at preflight time.

> **Tip:** Run `orchestrator check` to detect platform-specific gaps before runtime.
> `orchestrator manifest validate` checks structural correctness; `orchestrator check` additionally detects platform-specific runtime gaps.

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
    - type: spawn_task             # spawn a child task (WP02)
      goal: "verify-changes"
      workflow: verify_workflow
    - type: generate_items         # generate dynamic items (WP03)
      from_var: candidates
```

## Where Failures Go

A step that fails does not necessarily fail its task, and a task that completes
does not necessarily mean nothing went wrong. This section states the full
chain: what a nonzero exit code does, how a task's terminal status is derived,
and which events reach the attention inbox.

### What a nonzero exit code does

There are two execution paths with different failure semantics:

- **Agent (driver) steps** — every step executed by an Agent with a typed
  driver. A nonzero exit code fails output validation directly, the work item
  becomes `unresolved`, and the task ends `failed`. This is not configurable.
- **Builtin and direct-command steps** (`agent: builtin`, engine-owned
  commands) — output validation passes as long as the output itself is valid,
  *even when the exit code is nonzero*. What happens next is decided by
  `on_failure`, and the default `continue` changes nothing: the item's status
  is untouched, the task can end `completed`, and the only trace is a
  `step_finished` event with `success: false` — which the attention inbox
  turns into a `step_failed` item (see the routing table below).

The three `on_failure` actions, for builtin/direct steps whose exit code is
nonzero:

| Action | Effect |
|---|---|
| `continue` (default) | No status change. The step's failure is recorded as an event and an inbox item, nothing else. |
| `set_status` | The item's status is overwritten with `status:` and the segment continues. A status of `unresolved` or `qa_failed` will fail the task at loop end. |
| `early_return` | The item's status is set and the current segment terminates immediately. |

### How a task ends

At the end of each scheduling loop the task's terminal status is derived from
its items, never from exit codes directly:

- `failed` — if `unresolved + stale_pending > 0` (items in status
  `unresolved` or `qa_failed`, plus stale pending items).
- `completed` — otherwise.

Exit codes reach this derivation only through item status: directly for driver
steps, through `on_failure` or finalize rules for builtin steps.

### What reaches the attention inbox

The attention projector turns durable task events into inbox items. The table
below is generated from the projector source
(`crates/orchestrator-scheduler/src/service/attention.rs`) and checked by
`scripts/qa/test-attention-routing-doc.sh`; a row here that the code does not
declare — or an arm the table misses — fails CI. Event types with no row are
deliberately not routed.

<!-- attention-routing:begin -->
| Source event(s) | Condition | Inbox kind | Severity |
|---|---|---|---|
| approval_required, approval_requested | - | `approval_required` | intervention |
| agent_question, decision_required | - | `agent_question` | intervention |
| retry_exhausted | - | `retry_exhausted` | intervention |
| policy_blocked | - | `policy_blocked` | intervention |
| sandbox_denied, sandbox_network_blocked, sandbox_resource_exceeded | - | `sandbox_denied` | intervention |
| budget_threshold, budget_exhausted | - | `budget_threshold` | attention |
| step_timeout, task_stalled | - | `stalled` | intervention |
| task_failed | - | `task_failed` | intervention |
| degenerate_loop, degenerate_cycle, degenerate_cycle_detected | - | `degenerate_loop` | intervention |
| step_failed, output_validation_failed | - | `step_failed` | intervention |
| task_spawn_failed | - | `task_spawn_failed` | intervention |
| step_finished, chain_step_finished, dynamic_step_finished | payload.success == false | `step_failed` | intervention |
| step_finished, chain_step_finished, dynamic_step_finished | confidence < 0.5 | `low_confidence` | attention |
<!-- attention-routing:end -->

### What task completion clears — and what it preserves

Terminal and resolution events resolve open inbox items:

<!-- attention-resolution:begin -->
| Trigger event(s) | Scope | Preserved kinds | Resolution reason |
|---|---|---|---|
| task_completed, task_finished | whole task | low_confidence, step_failed, task_spawn_failed | task_completed |
| resume_executed | whole task | (none) | condition_cleared |
| step_finished, chain_step_finished, dynamic_step_finished (success != false) | matching step | n/a | condition_cleared |
<!-- attention-resolution:end -->

Task completion sweeps *condition* items (approvals, stalls, questions): a
task that ended cannot still be waiting. It does **not** sweep *evidence*
items — `step_failed`, `low_confidence`, and `task_spawn_failed` record
something that already happened and stay visible until a human resolves them,
a retry of the step succeeds, or the task is explicitly resumed. A green task
with a failed builtin step therefore still shows the failure in the inbox.

### Source-side inbox items

Some items are not projected from task events at all: webhook and source
failures materialize directly, carry no task id, and are never swept by task
completion. The current kinds (also generated and drift-checked):

<!-- attention-external-kinds:begin -->
- `inbox_projection_gap` — events were not projected while the inbox was disabled for the project; one merged item per project, written when the inbox is re-enabled.
- `source_auth_failed` — webhook deliveries for a configured trigger are failing signature or secret verification; merged per trigger, auto-resolved by the first successful delivery.
- `source_automation_binding_ambiguous` — a source reaction could not select exactly one binding.
- `source_automation_configuration_invalid` — a matched source reaction could not be reserved.
- `source_automation_needs_attention` — a source automation route is blocked and needs an operator.
- `source_connection_provisioning_attention` — dedicated Slack app provisioning needs an operator.
- `source_connection_reauthorization_required` — a managed source connection must be reauthorized.
- `source_connection_revoked` — the provider revoked a managed connection.
- `source_route_missing` — webhook deliveries name a trigger the project does not have; merged per project, the unknown name appears only as a digest.
- `source_routing_ambiguous` — a source event matched more than one routing target.
<!-- attention-external-kinds:end -->

### When the inbox is disabled

Setting `attention_inbox_enabled: false` in a project's RuntimePolicy stops
new materialization but does not stop the projector cursor: events arriving
during the disabled window are counted per project, and re-enabling the inbox
surfaces one `inbox_projection_gap` item stating how many events (and which id
range) were never projected. Silent loss is not an option; review the task
history for the gap window if the count is nonzero.

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
  stop_when_no_unresolved: false   # false = always run all cycles (default: true)
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
