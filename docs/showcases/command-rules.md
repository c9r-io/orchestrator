# Driver Session Continuity Template

> **Harness Engineering template**: this showcase demonstrates one concrete capability slice of orchestrator as a control plane for agent-first software delivery.
>
> **Purpose**: Agent session reuse and isolation — a step opts into resuming the provider's session through `behavior.driverRequirements.sessionResume`, and a step that omits it starts fresh.

## Use Cases

- AI agents (e.g. Claude Code) support session mode: an earlier step establishes context, later steps resume it
- Plan and implement steps need shared session context (plan output is prerequisite for implementation)
- QA steps need a fresh session to avoid bias from prior context

## Prerequisites

- `orchestratord` is running
- Database initialized (`orchestrator init`)

## Steps

### 1. Deploy Resources

```bash
orchestrator apply -f docs/workflow/command-rules.yaml --project cmd-rules
```

### 2. Create and Run a Task

```bash
orchestrator task create \
  --name "session-demo" \
  --goal "Demonstrate session reuse" \
  --workflow command_rules \
  --project cmd-rules
```

### 3. Inspect Results

```bash
orchestrator task list --project cmd-rules
orchestrator task logs <task_id>
```

## Workflow Steps

```
create_session (new) → plan (resume) → implement (resume) → qa_testing (new, isolated)
```

### Step-by-Step Breakdown

| Step | `sessionResume` | Session Used | Effect |
|------|----------------|--------------|--------|
| create_session | omitted | new session | Establishes provider context |
| plan | `true` | resumes it | Reuses session context |
| implement | `true` | resumes it | Continues from plan |
| qa_testing | omitted | new session | Independent analysis, no bias |

Isolation is the default. A step is only continuous with an earlier one when it says so.

### Key Mechanism: `behavior.driverRequirements.sessionResume`

```yaml
- id: plan
  type: plan
  required_capability: plan
  template: plan
  behavior:
    driverRequirements:
      sessionResume: true       # resume the provider context
      workspaceAccess: write

- id: qa_testing
  type: qa_review
  required_capability: qa_review
  template: qa_review
  behavior:
    driverRequirements:
      workspaceAccess: write    # no sessionResume → fresh session
```

The provider's session identifier never leaves the daemon. It is not captured from stdout, not
carried in a variable, and not written into any manifest — a step declares the *requirement* and the
driver satisfies it. Apply rejects the workflow if the selected agent's driver cannot resume, with
`[driver_session_resume_required]`, so an unsupported combination fails before the task runs rather
than silently starting a new session.

### Why not the legacy pattern

This template used to capture the session id out of the agent's stdout with `behavior.captures`,
switch commands on it with an agent-level `command_rules` CEL block, and clear it for one step with
`step_vars`. All three of those routed a provider-internal identifier through manifest-visible
coordination state:

- `behavior.captures` was removed by the coordination collapse — `[legacy_coordination_removed]`
- `step_vars` was removed with the rest of the pipeline-variable authoring surface —
  `[legacy_pipeline_variables_removed]`
- `command_rules` still exists on `Agent`, but it is no longer how session continuity is expressed

The typed requirement replaces all of it, which is why isolation is now the default rather than
something a step has to arrange by blanking a variable.

## Customization Guide

### Isolating an additional step

Omit `sessionResume`. There is nothing to clear:

```yaml
- id: security_audit
  type: qa_review
  required_capability: qa_review
  behavior:
    driverRequirements:
      workspaceAccess: write
```

### Passing configuration to a step

Put it in the step's own `StepTemplate` prompt, or read it from a store inside the step's command
with `orchestrator store get <store> <key> --project {project_id}`.

## Further Reading

- [Plan & Execute Template](plan-execute.md) — StepTemplate and variable propagation basics
- [Self-Bootstrap Execution](self-bootstrap-execution-template.md) — Production multi-step workflow
- [Coordination Tools](../guide/coordination-tools.md) — the typed replacements for legacy coordination
- [Error Codes](../guide/error-codes.md) — `legacy_coordination_removed`, `legacy_pipeline_variables_removed`
