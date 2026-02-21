# Orchestrator - Workflow Execution with Mock Agents

**Module**: orchestrator
**Scope**: Validate complete workflow execution with various mock agent configurations
**Scenarios**: 6
**Priority**: High

---

## Background

This document tests complete workflow execution using mock bash agents. The orchestrator should execute each phase (qa, fix, retest) and handle the output correctly.

### Mock Agent Templates Reference

#### Basic Echo Agent
```yaml
agents:
  mock_echo:
    metadata:
      name: mock_echo
    capabilities:
    - qa
    - fix
    - retest
    templates:
      qa: "echo 'qa-phase: {rel_path}'"
      fix: "echo 'fix-phase: {ticket_paths}'"
      retest: "echo 'retest-phase: {rel_path}'"
```

#### Sleep Agent (for timing tests)
```yaml
agents:
  mock_sleep:
    metadata:
      name: mock_sleep
    capabilities:
    - qa
    - fix
    templates:
      qa: "sleep 0.5 && echo 'qa-complete'"
      fix: "sleep 0.5 && echo 'fix-complete'"
```

#### Multi-line Output Agent
```yaml
agents:
  mock_multiline:
    metadata:
      name: mock_multiline
    capabilities:
    - qa
    templates:
      qa: |
        echo "=== QA Started ==="
        echo "Testing: {rel_path}"
        echo "=== QA Complete ==="
```

#### Error Agent (for failure testing)
```yaml
agents:
  mock_fail:
    metadata:
      name: mock_fail
    capabilities:
    - qa
    - fix
    templates:
      qa: "echo 'QA failed' && exit 1"
      fix: "echo 'Fix attempted' && exit 0"
```

#### File Writer Agent (for file-based tests)
```yaml
agents:
  mock_writer:
    metadata:
      name: mock_writer
    capabilities:
    - qa
    - fix
    templates:
      qa: "echo 'result: {rel_path}' > /tmp/qa-result.txt"
      fix: "echo 'fixed: {rel_path}' > /tmp/fix-result.txt"
```

#### Conditional Agent (with environment variables)
```yaml
agents:
  mock_conditional:
    metadata:
      name: mock_conditional
    capabilities:
    - qa
    templates:
      qa: |
        if [ -f /tmp/skip_qa ]; then
          echo "SKIPPED"
          exit 0
        fi
        echo "QA executed for {rel_path}"
```

#### Loop Guard Agent
```yaml
agents:
  mock_loop_guard:
    metadata:
      name: mock_loop_guard
    capabilities: []
    templates:
      default: |
        if [ {unresolved_items} -eq 0 ]; then
          echo "stop"
        else
          echo "continue"
        fi
```

---

## Scenario 1: qa_only Workflow

### Preconditions

- Config with mock_echo agent and qa_only workflow
- QA target files exist

### Steps

1. Create task with qa_only workflow:
   ```bash
   orchestrator task create \
     --name "qa-only-test" \
     --goal "Test QA only workflow" \
     --workflow qa_only \
     --no-start
   ```

2. Start task:
   ```bash
   orchestrator task start {task_id}
   ```

3. Check result:
   ```bash
   orchestrator task info {task_id}
   orchestrator task logs {task_id}
   ```

### Expected

- QA phase executes for each target file
- Task completes with qa_passed or similar status
- Logs show "qa-phase: {rel_path}" for each file

---

## Scenario 2: qa_fix Workflow

### Preconditions

- Config with mock agents and qa_fix workflow
- No tickets exist initially

### Steps

1. Create task with qa_fix workflow:
   ```bash
   orchestrator task create \
     --name "qa-fix-test" \
     --goal "Test QA and fix workflow" \
     --workflow qa_fix \
     --no-start
   ```

2. Start task:
   ```bash
   orchestrator task start {task_id}
   ```

3. Check result:
   ```bash
   orchestrator task info {task_id}
   ```

### Expected

- QA phase runs first
- If QA passes with no tickets, fix phase is skipped
- Task status reflects final outcome

---

## Scenario 3: qa_fix_retest Workflow

### Preconditions

- Config with mock agents and qa_fix_retest workflow

### Steps

1. Create task:
   ```bash
   orchestrator task create \
     --name "qa-fix-retest-test" \
     --goal "Test full workflow" \
     --workflow qa_fix_retest \
     --no-start
   ```

2. Start task:
   ```bash
   orchestrator task start {task_id}
   ```

3. Check execution:
   ```bash
   orchestrator task info {task_id}
   ```

### Expected

- QA runs → Fix runs → Retest runs
- All three phases execute in order
- Final status reflects retest result

---

## Scenario 4: Workflow with Ticket Creation

### Preconditions

- Config with mock_fail agent (fails QA)
- Empty ticket directory

### Steps

1. Create config with failing agent:
   ```bash
   # Update config to use mock_fail agent
   ```

2. Create task:
   ```bash
   orchestrator task create \
     --name "ticket-test" \
     --goal "Test ticket creation" \
     --workflow qa_fix \
     --no-start
   ```

3. Start task:
   ```bash
   orchestrator task start {task_id}
   ```

4. Check for tickets:
   ```bash
   ls docs/ticket/
   ```

### Expected

- QA fails
- Ticket is created in docs/ticket/
- Fix phase may process the ticket

---

## Scenario 5: Loop Mode Testing

### Preconditions

- Config with loop enabled (mode: infinite)
- Loop guard agent configured

### Steps

1. Create config with infinite loop:
   ```yaml
   workflows:
     infinite_qa:
       steps:
         - id: qa
           required_capability: qa
           enabled: true
           repeatable: true
         - id: check_done
           builtin: loop_guard
           enabled: true
           is_guard: true
           repeatable: true
       loop:
         mode: infinite
         guard:
           enabled: true
           stop_when_no_unresolved: true
       finalize:
         rules: []
   ```

2. Create and start task:
   ```bash
   orchestrator task create --workflow infinite_qa --no-start
   orchestrator task start {task_id}
   ```

3. Manually stop or let loop complete:
   ```bash
   # Watch task status
   orchestrator task info {task_id}
   ```

### Expected

- Task loops until condition is met
- Loop guard controls iteration count

---

## Scenario 6: Multiple Target Files

### Preconditions

- Multiple QA target files configured

### Steps

1. Create task targeting multiple files:
   ```bash
   orchestrator task create \
     --name "multi-file-test" \
     --goal "Test multiple files" \
     --target-file docs/qa/orchestrator/01-cli-agent-orchestration.md \
     --target-file docs/qa/orchestrator/02-cli-task-lifecycle.md \
     --no-start
   ```

2. Start task:
   ```bash
   orchestrator task start {task_id}
   ```

3. Check task items:
   ```bash
   orchestrator task info {task_id}
   ```

### Expected

- Each target file creates a separate task item
- QA runs for each file
- Progress shows X/Y items completed

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | qa_only Workflow | ☐ | | | |
| 2 | qa_fix Workflow | ☐ | | | |
| 3 | qa_fix_retest Workflow | ☐ | | | |
| 4 | Ticket Creation | ☐ | | | |
| 5 | Loop Mode | ☐ | | | |
| 6 | Multiple Target Files | ☐ | | | |
