---
self_referential_safe: true
---

# QA-100: Agent Command Rules + Step Vars

Validates FR-084: agent `command_rules` (CEL conditional command selection), step `step_vars` (temporary pipeline variable overlay), and `command_rule_index` audit column.

## Scenario 1: command_rules serde roundtrip

**Steps:**
```bash
cargo test -p orchestrator-config -- command_rules
```

**Expected:** All tests pass — `command_rules` serializes/deserializes correctly, empty rules omitted from JSON.

## Scenario 2: command_rules CEL validation

**Steps:**
```bash
cargo test -p agent-orchestrator -- validate_command_rules
```

**Expected:** All tests pass — valid CEL accepted, invalid CEL rejected, empty `when` rejected, missing `{prompt}` rejected.

## Scenario 3: command rule CEL evaluation with pipeline vars

**Steps:**
```bash
cargo test -p agent-orchestrator -- command_rule_cel
```

**Expected:** All tests pass — pipeline vars accessible in CEL, empty var does not match `!= ""`, missing var does not match.

## Scenario 4: resolve_agent_command behavior

**Steps:**
```bash
cargo test -p orchestrator-scheduler -- resolve_command
```

**Expected:** All tests pass — no rules returns default (index None), matching rule returns its command (index Some(0)), no match falls back (index None), first matching rule wins.

## Scenario 5: step_vars overlay and restore

**Steps:**
```bash
cargo test -p orchestrator-scheduler -- step_vars
```

**Expected:** All tests pass — overlay adds/overrides keys, restore removes new keys and reverts overridden keys, captures from step execution survive restore.

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

**Steps:**
```bash
echo '
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: test-wf
spec:
  steps:
    - id: isolated_step
      type: qa_testing
      scope: item
      step_vars:
        loop_session_id: ""
' | orchestrator manifest validate -f -
```

**Expected:** Manifest validates without errors.
