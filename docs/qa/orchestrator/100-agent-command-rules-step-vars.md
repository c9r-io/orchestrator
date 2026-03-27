---
self_referential_safe: true
---

# QA-100: Agent Command Rules + Step Vars

Validates FR-084: agent `command_rules` (CEL conditional command selection), step `step_vars` (temporary pipeline variable overlay), and `command_rule_index` audit column.

> Advanced scenarios (6-8) are in [100b-agent-command-rules-step-vars-advanced.md](100b-agent-command-rules-step-vars-advanced.md).

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

## Checklist

- [ ] S1: command_rules serde roundtrip
- [ ] S2: command_rules CEL validation
- [ ] S3: command rule CEL evaluation with pipeline vars
- [ ] S4: resolve_agent_command behavior
- [ ] S5: step_vars overlay and restore
