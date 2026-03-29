# Plan-Execute Template

> **Harness Engineering template**: this showcase demonstrates one concrete capability slice of orchestrator as a control plane for agent-first software delivery.
>
> **Purpose**: Plan → implement → verify three-phase iteration — demonstrates StepTemplate, multi-agent collaboration, and variable propagation.

## Use Cases

- Any development task requiring "plan first, implement second, verify last"
- Feature development, bug fixes, refactoring
- Separating planning from execution with different agents for each role

## Prerequisites

- `orchestratord` is running
- Database initialized (`orchestrator init`)

## Steps

### 1. Deploy Resources

```bash
orchestrator apply -f docs/workflow/plan-execute.yaml --project plan-exec
```

### 2. Create and Run a Task

```bash
orchestrator task create \
  --name "my-feature" \
  --goal "Implement user authentication with JWT tokens" \
  --workflow plan_execute \
  --project plan-exec
```

### 3. Inspect Results

```bash
orchestrator task list --project plan-exec
orchestrator task logs <task_id>
```

## Workflow Steps

```
plan (planner) → implement (coder) → verify (coder)
```

1. **plan** — Planner agent generates an implementation plan; output is automatically captured
2. **implement** — Coder agent follows the plan via `{plan_output_path}`
3. **verify** — Coder agent verifies the implementation matches the plan

### Key Feature: StepTemplate

Each step uses an independent StepTemplate to define its prompt, decoupled from the Agent:

```yaml
kind: StepTemplate
metadata:
  name: plan
spec:
  prompt: >-
    You are working on a project at {source_tree}.
    Your task: create a detailed implementation plan for: {goal}.
    ...
```

**Pipeline variables** (auto-injected):
- `{goal}` — the goal specified at task creation
- `{source_tree}` — the Workspace root_path
- `{diff}` — git diff in the current cycle
- `{plan_output_path}` — file path of the plan step's captured output

### Key Feature: Multi-Agent Collaboration

- **planner** agent (capability: `plan`) — focuses on planning
- **coder** agent (capability: `implement`, `verify`) — focuses on coding and verification

The orchestrator matches agents to steps based on `required_capability`.

## Customization Guide

### Add QA Steps

Append a QA testing step after verify:

```yaml
- id: qa_testing
  type: qa_testing
  scope: item
  required_capability: qa
  template: qa_testing
  enabled: true
```

`scope: item` means the step fans out in parallel per QA file.

### Enable 2-Cycle Mode

Cycle 1 for implementation, cycle 2 for regression verification:

```yaml
loop:
  mode: fixed
  max_cycles: 2
```

### Add Prehook Conditional Control

Use CEL expressions to conditionally enable steps:

```yaml
- id: verify
  ...
  prehook:
    engine: cel
    when: "cycle == 2"
    reason: "Only verify in the second cycle"
```

## Further Reading

- [Self-Bootstrap Execution](/en/showcases/self-bootstrap-execution-template) — Production-grade plan-execute-verify workflow (8 StepTemplates + 4 Agents + CEL prehooks)
- [CEL Prehooks](/en/guide/cel-prehooks) — Dynamic control flow via CEL expressions
- [Workflow Configuration](/en/guide/workflow-configuration) — Scope, loop, and safety configuration
