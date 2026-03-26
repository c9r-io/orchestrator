# QA Loop Template

> **Purpose**: QA test → fix → retest cycle — demonstrates multi-step workflows and capability-based agent selection.

## Use Cases

- Automated QA testing against project documentation or code
- Automatic ticket creation, issue fixing, and regression verification
- Standard QA → ticket_scan → fix → retest pipeline

## Prerequisites

- `orchestratord` is running
- Database initialized (`orchestrator init`)
- Project has `docs/qa/` and `docs/ticket/` directories (can be empty)

## Steps

### 1. Deploy Resources

```bash
orchestrator apply -f docs/workflow/qa-loop.yaml --project qa-loop
```

### 2. Create and Run a Task

```bash
orchestrator task create \
  --name "qa-run" \
  --goal "Run QA cycle" \
  --workflow qa_loop \
  --project qa-loop
```

### 3. Inspect Results

```bash
orchestrator task list --project qa-loop
orchestrator task logs <task_id>
```

## Workflow Steps

```
qa (qa-agent) → ticket_scan (builtin) → fix (fix-agent) → retest (fix-agent)
```

1. **qa** — Scans documents under `qa_targets`, executes test scenarios
2. **ticket_scan** — Built-in step, scans `ticket_dir` for ticket files
3. **fix** — Resolves issues found during QA
4. **retest** — Regression verification to confirm fixes

### Capability Matching

- `qa-agent` has `qa` capability → assigned to the qa step
- `fix-agent` has `fix` + `retest` capabilities → assigned to fix and retest steps

## Customization Guide

### Enable Loop Mode

Change `loop.mode` from `once` to `fixed` with `max_cycles` for multi-round iteration:

```yaml
loop:
  mode: fixed
  max_cycles: 3
```

### Add a Loop Guard

Append a loop guard step so an agent decides whether to continue:

```yaml
- id: loop_guard
  type: loop_guard
  required_capability: review
  enabled: true
  repeatable: true
```

### Replace with Real Agents

See [Hello World Customization Guide](/en/showcases/hello-world#replace-with-a-real-agent).

## Further Reading

- [Full QA Execution](/en/showcases/full-qa-execution) — Production-grade full QA workflow with CEL prehook safety filtering
- [Workflow Configuration](/en/guide/workflow-configuration) — Step execution model and loop strategies
