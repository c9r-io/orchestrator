# Orchestrator - Agent Collaboration & Communication

**Module**: orchestrator
**Scope**: Validate structured agent-to-agent communication, message bus, artifact parsing
**Scenarios**: 5
**Priority**: High

---

## Background

This document tests the new agent collaboration features including:
- AgentOutput with structured data (artifacts, confidence, quality_score)
- MessageBus for agent-to-agent communication
- Artifact parsing from agent stdout/stderr
- Enhanced template rendering with upstream outputs

Entry point: `orchestrator task <command>` with configured agents

---

## Scenario 1: AgentOutput Structure Validation

### Preconditions

- Orchestrator binary built and available
- Test agent configured with JSON output capability
- Full config must be bootstrapped first: `orchestrator config bootstrap --from fixtures/test-workflow-execution.yaml --force`
- Then apply agent-specific manifests on top of the bootstrapped config

### Steps

1. Create test agent that outputs structured JSON:
   ```yaml
   agents:
     json-agent:
       capabilities: [qa]
       metadata:
         cost: 10
       templates:
         qa: 'echo "{\"kind\": \"ticket\", \"severity\": \"high\", \"category\": \"bug\", \"content\": {\"title\": \"test\"}}"'
   ```

2. Run task with qa workflow:
   ```bash
   orchestrator task create --name "output-test" --workflow qa_only
   orchestrator task start --latest
   ```

3. Check logs for parsed artifacts:
   ```bash
   cat data/logs/qa-*-stdout.log
   ```

### Expected

- AgentOutput contains: `exit_code`, `stdout`, `stderr`, `artifacts`, `confidence`, `quality_score`
- Artifacts array contains parsed Ticket artifact with severity and category
- Confidence is 1.0 for successful execution (exit_code=0)

---

## Scenario 2: Artifact Parsing from Plain Text

### Preconditions

- Orchestrator running
- Test agent configured with plain text ticket markers
- Full config must be bootstrapped first: `orchestrator config bootstrap --from fixtures/test-workflow-execution.yaml --force`
- Then apply agent-specific manifests on top of the bootstrapped config

### Steps

1. Configure agent with ticket marker output:
   ```yaml
   agents:
     marker-agent:
       capabilities: [qa]
       metadata:
         cost: 10
       templates:
         qa: 'echo "[TICKET: severity=critical, category=security]"'
   ```

2. Execute task:
   ```bash
   orchestrator task create --name "marker-test" --workflow qa_only
   orchestrator task start --latest
   ```

3. Verify artifact extraction

### Expected

- Plain text `[TICKET: severity=critical, category=security]` is parsed
- ArtifactKind::Ticket created with Severity::Critical and category "security"
- Multiple markers in output create multiple artifacts

---

## Scenario 3: MessageBus Integration

### Preconditions

- Orchestrator binary built and available
- `config bootstrap` is required after `init` for debug and task commands

### Steps

1. Use the new debug command to check MessageBus:
   ```bash
   orchestrator debug --component messagebus
   ```

2. Check for message_bus in source code:
   ```bash
   grep -n "message_bus" core/src/main.rs | head
   ```

3. Run a task and check logs for message events:
   ```bash
   # Create and run a task
   orchestrator task create --name "msg-test" --goal "Test" --no-start
   orchestrator task start --latest
   
   # Check logs
   orchestrator task logs {task_id}
   ```

### Expected

- `orchestrator debug --component messagebus` shows MessageBus debug info
- `message_bus` field exists in InnerState (grep output)
- Logs show message_bus related events if multiple agents communicate

### CLI Command Reference

The orchestrator provides a debug command:

```bash
# Show all debug options
orchestrator debug

# Show MessageBus information
orchestrator debug --component messagebus

# Show active configuration
orchestrator debug --component config

# Show runtime state
orchestrator debug --component state
```

### Important Note

> **WARNING for QA Engineers**: MessageBus is an internal component. Use `orchestrator debug --component messagebus` to verify its status. The actual message passing happens internally and is logged via task logs.

---

## Scenario 4: Enhanced Template Rendering

### Preconditions

- Test agent with template using new placeholders

### Steps

1. Configure agent with enhanced template:
   ```yaml
   agents:
     template-agent:
       capabilities: [qa]
       templates:
         qa: 'echo "phase={phase} cycle={cycle}"'
   ```

2. Verify template rendering supports:
   - `{phase}` - current phase name
   - `{cycle}` - current cycle number
   - `{upstream[0].exit_code}` - upstream output access
   - `{shared_state.key}` - shared state access

### Expected

- Template renders basic placeholders correctly
- Enhanced placeholders available in AgentContext::render_template()

---

## Scenario 5: StepPrehookContext Extended Fields

### Preconditions

- Workflow with prehook configured

### Steps

1. Check StepPrehookContext structure:
   ```bash
   grep -A 20 "struct StepPrehookContext" core/src/main.rs
   ```

2. Verify new fields exist:
   - `qa_confidence: Option<f32>`
   - `qa_quality_score: Option<f32>`
   - `fix_has_changes: Option<bool>`
   - `upstream_artifacts: Vec<ArtifactSummary>`

### Expected

- StepPrehookContext includes all new structured fields
- Fields can be used in CEL prehook expressions

---

## Cleanup

```bash
# Delete test tasks
orchestrator task delete <task_id> --force

# Clear test logs
rm -f data/logs/qa-*.log
```

---

## Notes

- Requires Rust build with collab module
- Use `cargo test collab` to verify collaboration module tests
- Artifacts can be JSON or plain text with markers
- MessageBus enables future multi-agent协作 patterns
