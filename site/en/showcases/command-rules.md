# Command Rules Template

> **Purpose**: Conditional agent command selection + per-step variable overlay — demonstrates `command_rules` and `step_vars`.

## Use Cases

- Same agent uses different commands across steps (e.g. default / verbose / quick modes)
- Certain steps need isolated variable environments (e.g. QA clears session context)
- Agent selects different tools based on runtime state (session exists, current cycle, etc.)

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
  --name "mode-demo" \
  --goal "Demonstrate command rules" \
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
default_analysis (default cmd) → verbose_analysis (rule[0]) → quick_review (rule[1])
```

1. **default_analysis** — No step_vars → `run_mode` absent → no rule matches → default command
2. **verbose_analysis** — `step_vars: { run_mode: "verbose" }` → rule[0] matches → verbose command
3. **quick_review** — `step_vars: { run_mode: "quick" }` → rule[1] matches → quick command

Each step's echo output differs (`default-mode` / `verbose-mode` / `quick-mode`), verifiable via `task logs`.

### Key Feature: command_rules

```yaml
kind: Agent
spec:
  command: echo 'default mode'        # fallback
  command_rules:
    - when: "run_mode == \"verbose\""  # CEL condition (direct variable name)
      command: echo 'verbose mode'     # used when matched
    - when: "run_mode == \"quick\""
      command: echo 'quick mode'
```

**CEL variable access**: Pipeline variables are injected as top-level CEL names — write `run_mode == "verbose"` directly, **no** `vars.` prefix needed.

**Matching semantics**: Rules evaluated in order; first `when` returning true wins. If none match, the default `command` is used.

**Audit trail**: Matched rule index recorded in `command_runs.command_rule_index` (NULL = default, 0 = first rule).

### Key Feature: step_vars

```yaml
- id: verbose_analysis
  step_vars:
    run_mode: "verbose"    # only applies to this step
```

**Semantics**: Before step execution, `step_vars` merge into a shallow copy of pipeline variables. After execution, original values are restored. Other steps see unchanged variables.

**Typical uses**:
- Control command_rules matching (as in this template)
- Clear a session ID to force a new session (`step_vars: { session_id: "" }`)
- Inject step-specific config (timeout, log level, etc.)

## Customization Guide

### Real Agent with Session Reuse

```yaml
# Default: create new session
command: claude -p "{prompt}" --session-id new --output-format stream-json
command_rules:
  # Has session → resume it
  - when: "loop_session_id != \"\""
    command: claude -p "{prompt}" --resume {loop_session_id} --output-format stream-json
```

QA steps clear the session via step_vars for independent analysis:
```yaml
- id: qa_testing
  step_vars:
    loop_session_id: ""    # blocks session reuse
```

### Tool Switching by Step Type

```yaml
command_rules:
  - when: "step_type == \"test\""
    command: cargo test --workspace 2>&1
  - when: "step_type == \"lint\""
    command: cargo clippy --workspace -- -D warnings 2>&1
```

## Further Reading

- [Plan & Execute Template](/en/showcases/plan-execute) — StepTemplate and multi-agent collaboration basics
- [Self-Bootstrap Execution](/en/showcases/self-bootstrap-execution-template) — Production session reuse workflow
- [CEL Prehooks](/en/guide/cel-prehooks) — CEL expression syntax reference
