---
self_referential_safe: true
---

# QA-100b: Agent Command Rules + Step Vars (Advanced)

Continuation of [QA-100](100-agent-command-rules-step-vars.md). Covers DB migration, YAML manifest parsing for `command_rules`, and `step_vars` in workflow YAML.

## Scenario 6: DB migration m0021

**Steps:**
```bash
cargo test -p agent-orchestrator -- all_migrations_count
```

**Expected:** 21 migrations registered, including m0021_command_rule_index_column.

## Scenario 7: YAML manifest with command_rules parses correctly

**Steps:**
```bash
rg command_rules fixtures/ --files-with-matches  # check if fixture exists
# Or validate inline:
echo '
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: test-agent
spec:
  command: "echo {prompt}"
  command_rules:
    - when: "loop_session_id != \"\""
      command: "echo --resume {prompt}"
  capabilities: [plan]
' | orchestrator manifest validate -f -
```

**Expected:** Manifest validates without errors.

## Scenario 8: step_vars in workflow YAML parses correctly

> **Prerequisite:** `manifest validate` performs semantic validation (including agent capability checks). An agent with a capability matching the step `type` must be enabled in the project. If no agents are configured, use the unit-test path instead.

**Steps (unit-test — always works):**
```bash
cargo test -p orchestrator-scheduler -- step_vars
```

**Steps (CLI — requires enabled agent with `plan` capability):**
```bash
echo '
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: test-wf
spec:
  steps:
    - id: isolated_step
      type: plan
      scope: item
      step_vars:
        loop_session_id: ""
' | orchestrator manifest validate -f - --project <PROJECT_WITH_PLAN_AGENT>
```

**Expected:** Tests pass / manifest validates — `step_vars` key is accepted and parsed correctly.

| Symptom | Cause | Fix |
|---------|-------|-----|
| `no agent supports capability for step 'X'` | No enabled agent has the required capability | Use the unit-test path, or apply an agent manifest with that capability first |

## Checklist

- [x] S6: DB migration m0021
- [x] S7: YAML manifest with command_rules parses correctly
- [x] S8: step_vars in workflow YAML parses correctly
