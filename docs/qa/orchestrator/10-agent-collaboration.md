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

Entry point: `./scripts/orchestrator.sh task <command>` with configured agents

### Project Isolation Setup

```bash
QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/echo-workflow.yaml
```

---

## Scenario 1: AgentOutput Structure Validation

### Preconditions

- Orchestrator binary built and available
- Test agent configured with JSON output capability
- Full config must be applied first: `orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml`
- Then apply agent-specific manifests on top of the applied config

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
   ./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "output-test" --workflow qa_only
   ./scripts/orchestrator.sh task start --latest
   ```

3. Check logs for parsed artifacts:
   ```bash
   cat data/logs/qa-*-stdout.log
   ```

### Expected

- The scheduler captures stdout to file, reads it back, and passes it through `parse_artifacts_from_output()` from the collab module
- If the stdout contains valid JSON with `kind` field (e.g., `{"kind": "ticket", "severity": "high", ...}`), it is parsed into an `Artifact` struct
- An `artifacts_parsed` event is emitted with the count of parsed artifacts
- The `AgentOutput` struct (with `confidence`, `quality_score`) exists in `collab.rs` but is **not used in the main scheduler execution path** — the scheduler uses `RunResult` (exit_code, stdout_path, stderr_path, success, duration_ms) instead
- `confidence` and `quality_score` are available in `ItemFinalizeContext` but always `None` (not populated from agent output)

> **Note**: Artifact parsing IS implemented in the scheduler. Check for `artifacts_parsed` events in the database. The `AgentOutput` struct with `confidence`/`quality_score` is designed for future integration but not yet wired into the main execution path.

---

## Scenario 2: Artifact Parsing from Plain Text

### Preconditions

- Orchestrator running
- Test agent configured with plain text ticket markers
- Full config must be applied first: `orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml`
- Then apply agent-specific manifests on top of the applied config

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
   ./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "marker-test" --workflow qa_only
   ./scripts/orchestrator.sh task start --latest
   ```

3. Verify artifact extraction

### Expected

- Plain text `[TICKET: severity=critical, category=security]` IS parsed by `parse_ticket_from_line()` in `collab.rs`
- `ArtifactKind::Ticket` created with `Severity::Critical` and category "security"
- Multiple markers in output create multiple artifacts
- The scheduler reads stdout after execution and calls `parse_artifacts_from_output()` which handles both JSON and plain text ticket markers
- Verify by checking for `artifacts_parsed` events in the events table

> **Note**: Plain text artifact parsing IS implemented and called from the scheduler. The parsing supports both JSON format (`{"kind": "ticket", ...}`) and plain text markers (`[TICKET: severity=..., category=...]`).

---

## Scenario 3: MessageBus Integration

### Preconditions

- Orchestrator binary built and available
- `apply` is required after `init` for debug and task commands

### Steps

1. Use the new debug command to check MessageBus:
   ```bash
   ./scripts/orchestrator.sh debug --component messagebus
   ```

2. Check for message_bus in source code:
   ```bash
   grep -n "message_bus" core/src/main.rs | head
   ```

3. Run a task and check logs for message events:
   ```bash
   # Create and run a task
   ./scripts/orchestrator.sh task create --project "${QA_PROJECT}" --name "msg-test" --goal "Test" --no-start
   ./scripts/orchestrator.sh task start --latest
   
   # Check logs
   ./scripts/orchestrator.sh task logs {task_id}
   ```

### Expected

- `./scripts/orchestrator.sh debug --component messagebus` shows MessageBus debug info
- `message_bus` field exists in InnerState (grep output)
- Logs show message_bus related events if multiple agents communicate

### CLI Command Reference

The orchestrator provides a debug command:

```bash
# Show all debug options
./scripts/orchestrator.sh debug

# Show MessageBus information
./scripts/orchestrator.sh debug --component messagebus

# Show active configuration
./scripts/orchestrator.sh debug --component config

# Show runtime state
./scripts/orchestrator.sh debug --component state
```

### Important Note

> **WARNING for QA Engineers**: MessageBus is an internal component. Use `./scripts/orchestrator.sh debug --component messagebus` to verify its status. The actual message passing happens internally and is logged via task logs.

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

- `{phase}` and `{cycle}` are now rendered in capability step templates via `run_phase_with_rotation`
- `{rel_path}` and `{ticket_paths}` continue to work as before
- Guard step templates support `{task_id}` and `{cycle}` (already implemented)
- `AgentContext::render_template()` in `collab.rs` supports additional placeholders (`{task_id}`, `{item_id}`, `{workspace_root}`, `{upstream[i].exit_code}`, etc.) but is used in the collab module, not the main scheduler path

> **Note**: The main scheduler path (`run_phase_with_rotation`) now supports `{rel_path}`, `{ticket_paths}`, `{phase}`, and `{cycle}`. The richer `AgentContext::render_template()` is available in the collab module for future integration.

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
./scripts/orchestrator.sh task delete <task_id> --force

# Clear test logs
rm -f data/logs/qa-*.log
```

---

## Notes

- Requires Rust build with collab module
- Use `cargo test collab` to verify collaboration module tests
- Artifacts can be JSON or plain text with markers
- MessageBus enables future multi-agent协作 patterns
