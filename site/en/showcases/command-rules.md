# Command Rules Template

> **Harness Engineering template**: this showcase demonstrates one concrete capability slice of orchestrator as a control plane for agent-first software delivery.
>
> **Purpose**: Agent session reuse and isolation — share session context across steps via `command_rules`, while isolating QA with `step_vars`.

## Use Cases

- AI agents (e.g. Claude Code) support session mode: first step creates a session, subsequent steps `--resume` to retain context
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

| Step | loop_session_id | command_rules | Command Used | Effect |
|------|----------------|---------------|-------------|--------|
| create_session | absent | no match → default | new session | Outputs `session_id`, captured to pipeline vars |
| plan | `"ses-abc-123"` | rule[0] ✓ | resume session | Reuses session context |
| implement | `"ses-abc-123"` | rule[0] ✓ | resume session | Continues from plan |
| qa_testing | `""` (step_vars clears) | no match → default | new session | Independent analysis, no bias |

After qa_testing, `loop_session_id` is restored to `"ses-abc-123"` (step_vars is a temporary overlay).

### Key Mechanism 1: behavior.captures

```yaml
- id: create_session
  behavior:
    captures:
      - var: loop_session_id     # pipeline variable name
        source: stdout           # extract from stdout
        json_path: "$.session_id"  # JSON path selector
```

The agent outputs `{"session_id":"ses-abc-123",...}`, and the capture automatically extracts the `session_id` field into the `loop_session_id` pipeline variable. All subsequent steps can access it.

### Key Mechanism 2: command_rules

```yaml
kind: Agent
spec:
  command: echo 'new session'          # default: create new session
  command_rules:
    - when: "loop_session_id != \"\""  # CEL: session exists
      command: echo 'resumed session'  # match: resume session
```

- Pipeline variables are injected as **top-level CEL names** (write `loop_session_id` directly, no `vars.` prefix)
- Rules evaluated in order; first `true` wins. No match → default `command`
- Matched rule index recorded in `command_runs.command_rule_index` for auditing

### Key Mechanism 3: step_vars

```yaml
- id: qa_testing
  step_vars:
    loop_session_id: ""    # temporarily clear → force new session
```

- Before execution, `step_vars` merge into a shallow copy of pipeline vars
- After execution, original values are restored (`loop_session_id` returns to `"ses-abc-123"`)
- Only affects the current step's input view; global pipeline state unchanged

## Customization Guide

### Real Agent (Claude Code Session)

```yaml
# Default: create new session
command: claude -p "{prompt}" --session-id new --output-format stream-json

command_rules:
  # Has session → resume
  - when: "loop_session_id != \"\""
    command: claude -p "{prompt}" --resume {loop_session_id} --output-format stream-json
```

### More step_vars Isolation Scenarios

```yaml
# Security audit: independent session + extra audit directives
- id: security_audit
  step_vars:
    loop_session_id: ""           # independent session
    audit_mode: "strict"          # inject audit config
```

## Further Reading

- [Plan & Execute Template](/en/showcases/plan-execute) — StepTemplate and variable propagation basics
- [Self-Bootstrap Execution](/en/showcases/self-bootstrap-execution-template) — Production multi-step workflow
- [CEL Prehooks](/en/guide/cel-prehooks) — CEL expression syntax reference
