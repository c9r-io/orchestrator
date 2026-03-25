# Infinite Evolution Loop: Alternating Evolution and Bootstrap Iterations

> **Recommended usage**: Open this project in an AI coding agent (Claude Code, OpenCode, Codex, etc.) and ask the agent to read and execute this plan. The agent can autonomously handle resource deployment, task creation, execution monitoring, and result verification.


## Background

self-evolution and self-bootstrap have each completed end-to-end verification:

- **self-evolution**: Multi-path competitive exploration, automatically selecting the optimal solution (verified 2026-03-08)
- **self-bootstrap**: Single-path iterative refinement, repeatedly self-testing until stable (verified 2026-02-28)

The next goal: combine both into an **evolution -> bootstrap -> evolution -> bootstrap -> ...** infinite iteration loop, enabling the orchestrator to continuously evolve autonomously.

---

## Prerequisite: Git Commit Mechanism

### Problem

Currently, agent code changes land directly in the working directory without automatic commits. Changes from multiple iterations mix together, making them impossible to distinguish, review, or roll back.

### Solution

Use existing command steps to orchestrate git operations in workflow YAML, **with no core code changes required**:

```yaml
- id: git_commit
  command: |
    cd {workspace_root}
    git add -A
    git diff --cached --quiet && echo "nothing to commit" || \
    git commit -m "[orchestrator] {task_name} cycle {cycle} — {step_id}"
```

Key design points:

1. **Commit timing**: Placed after `self_test` passes, with a prehook ensuring execution only when tests pass
2. **Commit message**: Includes task_name, cycle number, and workflow name for traceability
3. **Empty commit protection**: `git diff --cached --quiet` prevents errors when there are no changes
4. **Branch strategy**: Operate on a feature branch (see next section), never directly on main
5. **Rollback support**: Checkpoints are associated with git commits; failures can use `git revert`

### Automatic Feature Branch Management

Also implemented via command steps, placed in `init_once` or the first step of cycle 1:

```yaml
- id: init_once
  command: |
    cd {workspace_root}
    git checkout -b auto/{task_name} 2>/dev/null || git checkout auto/{task_name}
```

After the task completes, humans decide whether to merge into main.

---

## Orchestration Approach

### Approach A: Alternating Within a Single Workflow (Recommended Starting Point)

One workflow contains complete evolution + bootstrap segments, driven by a loop mode through multiple rounds:

```
Cycle N:
  [Evolution Segment]
  evo_plan → generate_items → evo_implement (×2) → evo_benchmark (×2) → select_best
  → evo_apply_winner → self_test → git_commit

  [Bootstrap Segment]
  plan → implement → self_test → align_tests → self_test → git_commit

  [Convergence Check]
  loop_guard: Check if this round's diff is small enough / tests are all green / no new clippy warnings
```

All steps can be expressed using existing workflow primitives:
- The evolution segment's `generate_items` post-action generates candidate items
- item-scoped steps implement and evaluate in parallel
- `captures` extracts benchmark scores into pipeline variables
- prehook CEL expressions control conditional execution

**Pros**: Compact; state passes naturally within pipeline vars; no cross-workflow communication needed
**Cons**: Workflow YAML is longer

### Approach B: Workflow Cascading (via Trigger Resources)

Two independent workflows trigger each other via Trigger resources (FR-039, already implemented):

```yaml
# Trigger: start bootstrap after evolution completes
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: evo-to-bootstrap
  project: self-evolution
spec:
  event:
    source: task_completed
    filter:
      workflow: self-evolution
  action:
    workspace: self
    workflow: self-bootstrap
    goal: "Refine the output from the previous evolution round"
  concurrency_policy: Forbid

# Trigger: start next evolution round after bootstrap completes
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bootstrap-to-evo
  project: self-evolution
spec:
  event:
    source: task_completed
    filter:
      workflow: self-bootstrap
  action:
    workspace: self
    workflow: self-evolution
    goal: "Explore the next improvement direction"
  concurrency_policy: Forbid
```

**Pros**: Each workflow evolves independently; clear responsibilities; can be tested separately
**Cons**: Cross-workflow state transfer requires Store resources as intermediary

### Recommendation

Start with Approach A to validate the concept, then split into Approach B once stable.

---

## Convergence Conditions

An infinite loop requires reasonable stopping conditions to avoid meaningless idle cycling.

**FR-043** has been filed (`docs/feature_request/FR-043-convergence-expression.md`) to add CEL expression-driven convergence checks to `loop_guard`:

```yaml
loop:
  mode: infinite
  max_cycles: 10          # hard upper limit safety valve
  convergence_expr:
    engine: cel
    when: "delta_lines < 5 && cycle >= 2"
    reason: "code diff converged"
```

Before FR-043 is implemented, existing mechanisms can approximate this:
- `max_cycles` hard stop
- `loop_guard` builtin's `stop_when_no_unresolved` flag
- prehook to conditionally skip unnecessary steps

Reference convergence dimensions:

1. **Diff convergence**: Diff line count falls below threshold for N consecutive rounds (e.g., < 5 lines)
2. **Score convergence**: Gap between two candidates' benchmark scores in the evolution phase falls below threshold
3. **Test stability**: No new/fixed tests for N consecutive rounds
4. **Budget cap**: Maximum cycle count or maximum agent invocation count
5. **Manual interrupt**: `task pause` can intervene at any time

---

## Automatic Topic Discovery

Implemented via agent step + `spawn_tasks` post-action, **with no core code changes required**:

```yaml
- id: discover_topics
  required_capability: plan
  template: topic_discovery    # prompt guides agent to analyze codebase for improvement points
  behavior:
    post_actions:
      - type: spawn_tasks
        from_var: discover_output
        json_path: "$.topics"
        mapping:
          goal: "$.description"
          workflow: "self-evolution"
          name: "$.slug"
        max_tasks: 3
```

The agent analyzes the codebase and outputs a JSON list -> `spawn_tasks` automatically creates sub-tasks. Combined with Trigger resources, this forms a continuous discovery loop.

---

## Implementation Priority

| Priority | Task | Implementation Method | Dependencies |
|----------|------|----------------------|--------------|
| P0 | Git commit mechanism | command step (pure YAML) | None |
| P0 | Automatic feature branch management | command step (pure YAML) | None |
| P1 | Single workflow alternating orchestration (Approach A) | workflow YAML authoring | Git commit |
| P1 | Convergence condition expression (FR-043) | Core code change | loop_guard CEL extension |
| P2 | Workflow cascading trigger (Approach B) | Trigger resource (pure YAML) | Already available (FR-039) |
| P2 | Automatic topic discovery | agent step + spawn_tasks (pure YAML) | topic_discovery template |

> **Note**: Among P0/P1, only FR-043 requires core code changes; everything else can be implemented through workflow YAML orchestration.

---

## Expected Outcome

Once complete, a single command launches continuous autonomous evolution:

```bash
orchestrator task create \
  -n "continuous-evolution" \
  -w self -W self-evolve-bootstrap \
  --project self-evolution \
  -g "Continuously improve orchestrator code quality, performance, and features"
```

The engine will autonomously loop: explore new solutions -> select the best -> refine implementation -> commit -> explore the next improvement -> ...

Until convergence conditions are met or a human intervenes.
