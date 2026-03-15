# Orchestrator - Agent Collaboration Mainline Validation

**Module**: orchestrator
**Scope**: Validate structured AgentOutput handling, MessageBus publication, artifact behavior, template context, and prehook fields
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates collaboration-related behavior after scheduler mainline integration:

- phase output validation and normalization into `AgentOutput`
- event and MessageBus publication for phase execution results
- artifact persistence into run records
- template placeholders in scheduler execution path
- structured prehook context fields availability

Entry point: `orchestrator`

### Environment Isolation Setup

```bash
orchestrator delete project/qa-collab --force
orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project qa-collab
```

> **Note**: Use `delete project/<name> --force` + `apply --project` to isolate fixture agents
> from global/bootstrap agents. Project-scoped agent selection ensures only
> fixture-defined agents participate. Auto-ticket files are cleaned during reset.

---

## Scenario 1: Structured AgentOutput Persistence

### Preconditions
- Runtime initialized.
- At least one task has executed `qa`/`fix`/`retest`/`guard`.

### Goal
Verify scheduler stores structured run payload and validation status.

### Steps
1. Execute a task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-collab \
     --name "agentoutput-mainline" \
     --goal "structured output" \
     --workspace default \
     --workflow qa_only \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
   ```
2. Inspect command run structured fields:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT phase, validation_status, substr(output_json,1,120), substr(artifacts_json,1,120) FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}') ORDER BY started_at DESC LIMIT 10;"
   ```

### Expected
- `validation_status` is populated per run.
- `output_json` stores serialized `AgentOutput`.
- `artifacts_json` stores artifact payload for parsed artifacts.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `task create failed: multiple workflows exist in project; specify the workflow flag explicitly` | `echo-workflow.yaml` defines multiple workflows (`qa_only`, `qa_fix`, `qa_fix_retest`, `loop_test`) so implicit workflow resolution is ambiguous | Pass the workflow flag with value `qa_only` in Scenario 1, or choose the specific workflow under test |

### Expected Data State
```sql
SELECT COUNT(*)
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
  AND validation_status IN ('passed','failed')
  AND output_json <> '{}';
-- Expected: count >= 1
```

---

## Scenario 2: Strict Phase Validation Behavior

### Preconditions
- Reset previous QA state — `delete project/<name> --force` clears task data, config, and auto-tickets.
- **Important**: This scenario requires the `plain-text-agent.yaml` fixture
  which defines an agent that produces non-JSON output. The base
  `echo-workflow.yaml` fixture must be applied first to provide the Workspace
  resource.

### Goal
Verify non-JSON output is rejected for strict phases.

### Steps
1. Reset and apply into project scope (two fixtures — base workspace + plain-text agent):
   ```bash
   orchestrator delete project/qa-strict --force
   orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project qa-strict
   orchestrator apply -f fixtures/manifests/bundles/plain-text-agent.yaml --project qa-strict
   ```

2. Create and run task:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-strict \
     --name "strict-validation-test" \
     --goal "Test strict phase validation" \
     --workspace default \
     --workflow plain_text_test \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}"
   ```

3. Query validation failure events:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload_json FROM events
      WHERE task_id='${TASK_ID}' AND event_type='output_validation_failed'
      ORDER BY id DESC LIMIT 5;"
   ```

4. Query agent selection distribution:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT agent_id, validation_status, COUNT(*)
      FROM command_runs
      WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '${TASK_ID}')
      GROUP BY agent_id, validation_status;"
   ```

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| `plain_text_agent` never selected | Fixture not applied with `--project`; global agents participate in selection | Use `apply -f ... --project <name>` to scope agents |
| No `output_validation_failed` events | Only JSON-producing agents were selected | Verify only project agents exist via `describe agent` |
| Workspace not found error | `plain-text-agent.yaml` does not define a Workspace resource | Apply `echo-workflow.yaml` first to provide the Workspace |

### Expected
- `plain_text_agent` appears in `command_runs` with `validation_status = 'failed'`
- `output_validation_failed` event appears for non-JSON strict-phase output
- Other fixture agents (e.g. `mock_echo`) appear with `validation_status = 'passed'`

### Expected Data State
```sql
SELECT phase, validation_status
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
ORDER BY started_at DESC;
-- Expected: plain_text_agent rows are marked failed; other agent rows are marked passed
```

---

## Scenario 3: MessageBus Publication Observability

### Preconditions
- Runtime initialized.

### Goal
Verify phase result publication is observable through persisted events.

### Steps
1. Run a task with at least one phase execution.
2. Query publication events:
   ```bash
   sqlite3 data/agent_orchestrator.db "SELECT event_type, payload_json FROM events WHERE task_id='{task_id}' AND event_type IN ('phase_output_published','bus_publish_failed') ORDER BY id DESC LIMIT 10;"
   ```
3. Check debug component output:
   ```bash
   orchestrator debug --component messagebus
   ```

### Expected
- `phase_output_published` appears on successful publish path.
- `bus_publish_failed` appears only on degraded publish path.
- MessageBus debug component reports implementation at `src/collab.rs`.

### Expected Data State
```sql
SELECT COUNT(*)
FROM events
WHERE task_id = '{task_id}'
  AND event_type = 'phase_output_published';
-- Expected: count >= 1 for successful phase execution
```

---

## Scenario 4: Scheduler Template Placeholders

### Preconditions
- An agent whose `command` field contains template placeholders must be applied.
- **Important**: The default `echo-workflow.yaml` fixture uses a hardcoded
  `echo` command with no placeholders, so it cannot validate placeholder
  rendering. You must also apply `fixtures/template-agent.yaml`, which
  contains `{phase}` and `{cycle}` placeholders in its command.

### Goal
Verify scheduler path renders supported placeholders into concrete values
before command execution.

### Steps
1. Reset and apply fixtures (base workspace + placeholder-bearing agent):
   ```bash
   orchestrator delete project/qa-template --force
   orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project qa-template
   orchestrator apply -f fixtures/template-agent.yaml --project qa-template
   ```
2. Create and run a task that will select the template agent:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-template \
     --name "placeholder-render-test" \
     --goal "Validate placeholder rendering" \
     --workspace default \
     --workflow qa_only \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
   ```
3. Inspect rendered command records:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT agent_id, command FROM command_runs
      WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id='${TASK_ID}')
      ORDER BY started_at DESC LIMIT 5;"
   ```

### Supported Placeholders

The template engine (`core/src/qa_utils.rs`) supports these placeholders:

| Placeholder | Source |
|---|---|
| `{rel_path}` | Workspace-relative target path |
| `{ticket_paths}` | Space-joined ticket file paths |
| `{phase}` | Current step phase name |
| `{task_id}` | Task identifier |
| `{cycle}` | Loop cycle number |
| `{unresolved_items}` | Count of unresolved items |

### Expected
- For `template-agent` rows: command text contains concrete rendered values
  (e.g., `phase=qa` rather than literal `{phase}`).
- For `mock_echo` rows: command is unchanged (no placeholders to render).
- Guard command path supports `{task_id}` and `{cycle}`.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---|---|---|
| All commands are identical hardcoded `echo` strings | Only `mock_echo` was selected; `template-agent` fixture not applied | Apply `fixtures/template-agent.yaml` into the project |
| `template-agent` never selected | Agent lacks required capability for the workflow step | Verify `template-agent.yaml` lists the `qa` capability |

### Expected Data State
```sql
SELECT agent_id, command
FROM command_runs
WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '{task_id}')
ORDER BY started_at DESC
LIMIT 5;
-- Expected: template-agent rows show rendered values (e.g., 'phase=qa cycle=1')
-- Expected: mock_echo rows show the original hardcoded echo command
-- Expected: no rows contain literal unresolved '{phase}' or '{cycle}' strings
```

---

## Scenario 5: StepPrehookContext Structured Fields

### Preconditions
- Workflow prehook expressions are enabled.

### Goal
Verify structured prehook fields are available for CEL expressions.

### Steps
1. Check type definition:
   ```bash
   rg -n "struct StepPrehookContext|qa_confidence|qa_quality_score|fix_has_changes|upstream_artifacts" core/src/config.rs
   ```
2. Run workflow with prehook expression that references at least one structured field.

### Expected
- Context definition includes structured fields used by collaboration flow.
- CEL prehook can evaluate without missing-field errors.

### Expected Data State
```sql
SELECT event_type, payload_json
FROM events
WHERE task_id = '{task_id}'
  AND event_type IN ('step_started','step_skipped','step_finished')
ORDER BY id DESC
LIMIT 20;
-- Expected: prehook-driven branch/skip behavior recorded without context-resolution errors
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Structured AgentOutput Persistence | ☐ | | | |
| 2 | Strict Phase Validation Behavior | ☐ | | | |
| 3 | MessageBus Publication Observability | ☐ | | | |
| 4 | Scheduler Template Placeholders | ☐ | | | |
| 5 | StepPrehookContext Structured Fields | ☐ | | | |
