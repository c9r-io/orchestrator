# Command Rules Template

> **Purpose**: Conditional agent command selection + per-step variable overlay — demonstrates `command_rules` and `step_vars`.

## Use Cases

- Agent selects different commands based on runtime state (e.g. first step creates a session, subsequent steps resume it)
- Certain steps need isolated variable environments (e.g. QA needs a fresh session, unaffected by prior context)
- Same agent uses different tools or parameters across different workflow steps

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
  --goal "Demonstrate command rules and step_vars" \
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
init_session (default cmd) → continue_session (rule[0]: resume) → independent_review (rule[1]: fresh, via step_vars)
```

1. **init_session** — No `session_id` variable → all rules fail → uses default `command`
2. **continue_session** — `session_id` is set → `command_rules[0]` matches → uses resume command
3. **independent_review** — `step_vars: { fresh_session: "true" }` → `command_rules[1]` matches → uses fresh session command

### Key Feature: command_rules

```yaml
kind: Agent
spec:
  command: echo 'default command'   # fallback
  command_rules:
    - when: "vars.session_id != ''"      # CEL condition
      command: echo 'resume session'      # used when matched
    - when: "vars.fresh_session == 'true'"
      command: echo 'fresh session'
```

**Matching semantics**: Rules are evaluated in order; the first `when` that returns true wins. If none match, the default `command` is used.

**CEL context**: The `vars` map contains current pipeline variables, including all captured step outputs and `step_vars` overlays.

**Audit trail**: The matched rule index is recorded in `command_runs.command_rule_index` (NULL = default command).

### Key Feature: step_vars

```yaml
- id: independent_review
  step_vars:
    fresh_session: "true"    # only applies to this step
```

**Semantics**: Before step execution, `step_vars` are merged into a shallow copy of pipeline variables. After execution, original values are restored. Other steps see unchanged variables.

## Customization Guide

### Real Agent with Session Reuse

```yaml
command: claude -p "{prompt}" --session-id new --verbose --output-format stream-json
command_rules:
  - when: "vars.session_id != ''"
    command: claude -p "{prompt}" --resume {session_id} --verbose --output-format stream-json
  - when: "vars.fresh_session == 'true'"
    command: claude -p "{prompt}" --session-id new --verbose --output-format stream-json
```

### Tool Switching by Step Type

```yaml
command_rules:
  - when: "vars.step_type == 'test'"
    command: cargo test --workspace 2>&1
  - when: "vars.step_type == 'lint'"
    command: cargo clippy --workspace -- -D warnings 2>&1
```

## Further Reading

- [Plan & Execute Template](/en/showcases/plan-execute) — StepTemplate and multi-agent collaboration basics
- [Self-Bootstrap Execution](/en/showcases/self-bootstrap-execution-template) — Production session reuse workflow
- [CEL Prehooks](/en/guide/cel-prehooks) — CEL expression syntax reference
