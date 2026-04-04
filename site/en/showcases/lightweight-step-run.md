# Lightweight Step Run

> **Harness Engineering template**: this showcase demonstrates one concrete capability slice of orchestrator as a control plane for agent-first software delivery.
>
> **Purpose**: Three lightweight execution modes — step filtering, synchronous run, and direct assembly — enabling point-shot single-step execution without creating a full workflow.

## Use Cases

- Re-run a single step from a multi-step workflow (e.g., run only `fix` for a specific ticket)
- Inject ad-hoc variables into a step (e.g., specify a ticket path)
- Synchronously wait for execution results instead of polling asynchronously
- Execute a StepTemplate + Agent capability directly without an existing workflow

## Prerequisites

- `orchestratord` is running (`orchestratord --foreground --workers 2`)
- Database initialized (`orchestrator init`)
- A multi-step workflow is deployed (e.g., `plan-execute`)

## Steps

### 1. Deploy a Multi-Step Workflow

Using the plan-execute template as an example:

```bash
orchestrator apply -f docs/workflow/plan-execute.yaml --project demo
```

Verify resources are loaded:

```bash
orchestrator get workflows --project demo
orchestrator get agents --project demo
```

### 2. Phase 1 — Step Filtering (`--step` + `--set`)

Execute only specific steps from a workflow, with optional variable injection:

```bash
# Only execute the implement step
orchestrator task create \
  --workflow plan-execute \
  --project demo \
  --step implement

# Inject a pipeline variable
orchestrator task create \
  --workflow plan-execute \
  --project demo \
  --step fix \
  --set ticket_paths=docs/ticket/T-0042.md

# Multiple steps (executed in workflow order)
orchestrator task create \
  --workflow plan-execute \
  --project demo \
  --step plan --step implement
```

Inspect results:

```bash
orchestrator task list --project demo
orchestrator task logs <task_id>
```

**Error handling**: specifying a non-existent step ID returns a clear error:

```bash
orchestrator task create --workflow plan-execute --step nonexistent
# Error: unknown step id 'nonexistent' in --step filter; available steps: plan, implement, verify
```

### 3. Phase 2 — Synchronous Execution (`orchestrator run`)

The `run` command creates a task, follows logs in real time, and exits with a status code:

```bash
# Synchronous — logs stream to terminal
orchestrator run \
  --workflow plan-execute \
  --project demo \
  --step implement \
  --set goal="Fix concurrency issue in login module"

# Background mode (falls back to task create)
orchestrator run \
  --workflow plan-execute \
  --project demo \
  --step implement \
  --detach
```

Behavior:
1. Creates task, automatically follows logs until completion
2. Streams agent output to the terminal in real time
3. Exits with code 0 (completed) or 1 (failed)
4. `--detach` prints the task ID and returns immediately

### 4. Phase 3 — Direct Assembly Mode (No Workflow)

Execute a StepTemplate + Agent capability directly, without an existing workflow:

```bash
# Specify template and agent capability
orchestrator run \
  --template fix-ticket \
  --agent-capability fix \
  --project demo \
  --set ticket_paths=docs/ticket/T-0042.md

# With execution profile override
orchestrator run \
  --template fix-ticket \
  --agent-capability fix \
  --profile host-unrestricted \
  --project demo \
  --set ticket_paths=docs/ticket/T-0042.md
```

Direct assembly mode internally constructs a single-step `TaskExecutionPlan`, reusing StepTemplate, Agent, and ExecutionProfile resources already applied to the workspace.

## Execution Flow

```
Phase 1: task create --step fix --set key=val
         ↓
         Step filter → only execute specified steps
         Variable injection → injected as pipeline variables

Phase 2: orchestrator run --workflow X --step fix
         ↓
         Create task → follow logs → wait for completion → exit code

Phase 3: orchestrator run --template T --agent-capability C
         ↓
         Build single-step plan → create ephemeral task → execute
```

### Key Feature: Step Filtering

Step IDs specified with `--step` are validated server-side against the execution plan. Steps not in the filter are skipped. Filtered steps execute in their original workflow order, and the scope partitioning (task/item) mechanism is unchanged.

### Key Feature: Variable Injection

Variables injected with `--set key=value` are merged into pipeline variables at task start. They can be referenced in StepTemplate prompts (`{key}`) and in prehook CEL expressions.

### Key Feature: Security Guarantees

- ExecutionProfile sandbox enforcement is not bypassed
- All `run` executions produce RunResult records, accessible via `event list`
- Full audit trail, identical to regular task execution

## Customization Guide

### Add Point-Shot Capability to Existing Workflows

No YAML changes needed — `--step` and `--set` work with any existing workflow:

```bash
# Run only the qa step from the sdlc workflow
orchestrator run --workflow sdlc --step qa --project my-project

# Run qa + fix steps
orchestrator run --workflow sdlc --step qa --step fix --project my-project
```

### Create a StepTemplate for Direct Assembly

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: fix-ticket
spec:
  prompt: |
    Fix the issue described in the following ticket.
    Ticket path: {ticket_paths}
    Project goal: {goal}
    Source tree root: {source_tree}
```

Then execute directly:

```bash
orchestrator run \
  --template fix-ticket \
  --agent-capability fix \
  --set ticket_paths=docs/ticket/T-0042.md
```

## Further Reading

- [Plan & Execute Template](/en/showcases/plan-execute) — multi-step workflow for use with `--step` partial execution
- [Hello World Template](/en/showcases/hello-world) — minimal runnable workflow
- [CLI Reference](/en/guide/cli-reference) — full `run` command parameter reference
- [Workflow Configuration](/en/guide/workflow-configuration) — step definitions, scope, loop policy details
