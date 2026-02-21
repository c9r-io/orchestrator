# Orchestrator - Capability-Driven Agent Orchestration

**Module**: orchestrator
**Scope**: Validate new capability-driven orchestration features (cost preference, repeatable steps, guard steps)
**Scenarios**: 6
**Priority**: High

---

## Background

This document tests the upgraded orchestration features:
- **Capability-driven selection**: Agents declare capabilities, steps request capabilities
- **Cost preference**: Steps can prefer performance (low cost), quality (high cost), or balance
- **Repeatable steps**: Steps can be marked repeatable (run every cycle) or one-time
- **Guard steps**: Steps can be marked as guards that terminate the workflow loop
- **Builtin steps**: init_once, ticket_scan, loop_guard are builtin behaviors

### New Config Format Reference

#### Agent with Capabilities and Cost
```yaml
agents:
  fast_agent:
    metadata:
      name: fast_agent
      cost: 20  # 1-100, lower = cheaper/faster
    capabilities:
    - qa
    - fix
    templates:
      qa: "echo 'fast qa'"
      fix: "echo 'fast fix'"
  
  slow_agent:
    metadata:
      name: slow_agent
      cost: 80  # higher = more expensive/higher quality
    capabilities:
    - fix
    templates:
      fix: "echo 'slow but quality fix'"
```

#### Step with Capability and Cost Preference
```yaml
workflows:
  my_workflow:
    steps:
    - id: qa_test
      required_capability: qa
      cost_preference: performance  # performance | quality | balance
      repeatable: true
      is_guard: false
    - id: check_done
      builtin: loop_guard
      is_guard: true
      repeatable: true
```

---

## Scenario 1: Capability-Driven Agent Selection

### Preconditions

- Orchestrator binary built
- Database reset (fresh state)
- Two agents with different capabilities configured

### Goal

Validate that when a step requires a capability, only agents with that capability are selected.

### Steps

1. Reset to fresh state:
   ```bash
   rm -f orchestrator/data/agent_orchestrator.db
   ```

2. Create test config with capability-based agents:
   ```bash
   cat > /tmp/capability-test.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   defaults:
     workspace: default
     workflow: test_capability
   workspaces:
     default:
       root_path: .
       qa_targets: []
       ticket_dir: docs/ticket
   agents:
     agent_qa_only:
       metadata:
         name: agent_qa_only
         cost: 30
       capabilities:
       - qa
       templates:
         qa: "echo 'qa-from-agent-qa-only'"
     
     agent_fix_only:
       metadata:
         name: agent_fix_only
         cost: 50
       capabilities:
       - fix
       templates:
         fix: "echo 'fix-from-agent-fix-only'"
   
   workflows:
     test_capability:
       steps:
       - id: run_qa
         required_capability: qa
         enabled: true
         repeatable: false

       - id: run_fix
         required_capability: fix
         enabled: true
         repeatable: false

       loop:
         mode: once
   EOF
   ```

3. Apply config:
   ```bash
   cd orchestrator
   ./src-tauri/target/release/agent-orchestrator apply -f /tmp/capability-test.yaml
   ```

4. Create task:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task create \
     --name "capability-test" \
     --goal "Test capability selection" \
     --workspace default \
     --workflow test_capability
   ```

5. Get task info and check which agent was used:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task info {task_id}
   ./src-tauri/target/release/agent-orchestrator task logs {task_id}
   ```

### Expected

- QA step uses `agent_qa_only` (the only agent with qa capability)
- Fix step uses `agent_fix_only` (the only agent with fix capability)
- Logs show respective echo outputs

---

## Scenario 2: Cost Preference - Performance

### Preconditions

- Two agents with same capability but different costs
- Cost preference set to "performance"

### Goal

Validate that when cost_preference is "performance", the lower-cost agent is selected.

### Steps

1. Create config with two agents having same capability but different costs:
   ```bash
   cat > /tmp/cost-perf-test.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   defaults:
     workspace: default
     workflow: cost_test
   workspaces:
     default:
       root_path: .
       qa_targets: []
       ticket_dir: docs/ticket
   agents:
     cheap_agent:
       metadata:
         name: cheap_agent
         cost: 20
       capabilities:
       - qa
       - fix
       templates:
         qa: "echo 'cheap-qa'"
         fix: "echo 'cheap-fix'"
     
     expensive_agent:
       metadata:
         name: expensive_agent
         cost: 80
       capabilities:
       - qa
       - fix
       templates:
         qa: "echo 'expensive-qa'"
         fix: "echo 'expensive-fix'"
   
   workflows:
     cost_test:
       steps:
       - id: do_qa
         required_capability: qa
         cost_preference: performance
         enabled: true
         repeatable: false

       - id: do_fix
         required_capability: fix
         cost_preference: performance
         enabled: true
         repeatable: false

       loop:
         mode: once
   EOF
   ```

2. Apply and test:
   ```bash
   cd orchestrator
   ./src-tauri/target/release/agent-orchestrator apply -f /tmp/cost-perf-test.yaml
   ./src-tauri/target/release/agent-orchestrator task create \
     --name "cost-perf-test" \
     --goal "Test performance cost" \
     --workspace default \
     --workflow cost_test
   ```

3. Check logs:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task logs {task_id}
   ```

### Expected

- Both QA and Fix steps use `cheap_agent` (cost=20, lower than expensive_agent's cost=80)
- Logs show "cheap-qa" and "cheap-fix"

---

## Scenario 3: Cost Preference - Quality

### Preconditions

- Same agents as Scenario 2
- Cost preference set to "quality"

### Goal

Validate that when cost_preference is "quality", the higher-cost agent is selected.

### Steps

1. Create config with quality preference:
   ```bash
   cat > /tmp/cost-quality-test.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   defaults:
     workspace: default
     workflow: quality_test
   workspaces:
     default:
       root_path: .
       qa_targets: []
       ticket_dir: docs/ticket
   agents:
     cheap_agent:
       metadata:
         name: cheap_agent
         cost: 20
       capabilities:
       - qa
       templates:
         qa: "echo 'cheap-qa'"
     
     expensive_agent:
       metadata:
         name: expensive_agent
         cost: 80
       capabilities:
       - qa
       templates:
         qa: "echo 'expensive-qa'"
   
   workflows:
     quality_test:
       steps:
       - id: do_qa
         required_capability: qa
         cost_preference: quality
         enabled: true
         repeatable: false

       loop:
         mode: once
   EOF
   ```

2. Apply and test:
   ```bash
   cd orchestrator
   ./src-tauri/target/release/agent-orchestrator apply -f /tmp/cost-quality-test.yaml
   ./src-tauri/target/release/agent-orchestrator task create \
     --name "cost-quality-test" \
     --goal "Test quality cost" \
     --workspace default \
     --workflow quality_test
   ```

3. Check logs:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task logs {task_id}
   ```

### Expected

- QA step uses `expensive_agent` (cost=80, higher than cheap_agent's cost=20)
- Logs show "expensive-qa"

---

## Scenario 4: Repeatable Steps

### Preconditions

- Workflow with repeatable and non-repeatable steps
- Infinite loop mode

### Goal

Validate that repeatable steps run every cycle, while non-repeatable steps run only in the first cycle.

### Steps

1. Create config:
   ```bash
   cat > /tmp/repeatable-test.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   defaults:
     workspace: default
     workflow: repeat_test
   workspaces:
     default:
       root_path: .
       qa_targets: []
       ticket_dir: docs/ticket
   agents:
     test_agent:
       metadata:
         name: test_agent
         cost: 50
       capabilities:
       - qa
       templates:
         qa: "echo 'cycle-{cycle}'"
   
   workflows:
     repeat_test:
       steps:
       - id: one_time
         required_capability: qa
         enabled: true
         repeatable: false

       - id: every_cycle
         required_capability: qa
         enabled: true
         repeatable: true

       loop:
         mode: infinite
         guard:
           enabled: true
           stop_when_no_unresolved: false
   EOF
   ```

2. Apply:
   ```bash
   cd orchestrator
   ./src-tauri/target/release/agent-orchestrator apply -f /tmp/repeatable-test.yaml
   ```

3. Create task and start:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task create \
     --name "repeatable-test" \
     --goal "Test repeatable steps" \
     --workspace default \
     --workflow repeat_test
   ```

4. Wait for 2-3 cycles, then check logs:
   ```bash
   sleep 3
   ./src-tauri/target/release/agent-orchestrator task logs {task_id}
   ```

### Expected

- "one_time" step appears only in cycle 1
- "every_cycle" step appears in every cycle (cycle-1, cycle-2, cycle-3, etc.)

---

## Scenario 5: Guard Steps (is_guard)

### Preconditions

- Workflow with guard step that returns "stop"

### Goal

Validate that when a guard step returns "stop", the workflow loop terminates.

### Steps

1. Create config with guard:
   ```bash
   cat > /tmp/guard-test.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   defaults:
     workspace: default
     workflow: guard_test
   workspaces:
     default:
       root_path: .
       qa_targets: []
       ticket_dir: docs/ticket
   agents:
     test_agent:
       metadata:
         name: test_agent
         cost: 50
       capabilities:
       - qa
       templates:
         qa: "echo 'qa-run'"
         default: "echo 'stop'"
   
   workflows:
     guard_test:
       steps:
       - id: run_qa
         required_capability: qa
         enabled: true
         repeatable: true

       - id: check_stop
         builtin: loop_guard
         enabled: true
         repeatable: true
         is_guard: true
       loop:
         mode: infinite
         guard:
           enabled: true
           stop_when_no_unresolved: true
   EOF
   ```

2. Apply and test:
   ```bash
   cd orchestrator
   ./src-tauri/target/release/agent-orchestrator apply -f /tmp/guard-test.yaml
   ./src-tauri/target/release/agent-orchestrator task create \
     --name "guard-test" \
     --goal "Test guard step" \
     --workspace default \
     --workflow guard_test
   ```

3. Check task status:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task info {task_id}
   ```

### Expected

- Task runs for one cycle
- Guard step (builtin loop_guard) checks for unresolved items
- With stop_when_no_unresolved=true and no tickets, workflow terminates

---

## Scenario 6: Config View Shows New Fields

### Preconditions

- Config with new fields applied

### Goal

Validate that config view correctly displays new fields (capabilities, cost, cost_preference, repeatable, is_guard).

### Steps

1. Apply config with new fields:
   ```bash
   cd orchestrator
   ./src-tauri/target/release/agent-orchestrator apply -f /tmp/cost-perf-test.yaml
   ```

2. View config:
   ```bash
   ./src-tauri/target/release/agent-orchestrator config view -o json | jq '.agents'
   ./src-tauri/target/release/agent-orchestrator config view -o json | jq '.workflows | to_entries[0].value.steps'
   ```

### Expected

- Agents show `metadata.cost` field
- Agents show `capabilities` array
- Steps show `cost_preference`, `repeatable`, `is_guard`, `required_capability`, `builtin` fields

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Capability-Driven Agent Selection | ☐ | | | |
| 2 | Cost Preference - Performance | ☐ | | | |
| 3 | Cost Preference - Quality | ☐ | | | |
| 4 | Repeatable Steps | ☐ | | | |
| 5 | Guard Steps (is_guard) | ☐ | | | |
| 6 | Config View Shows New Fields | ☐ | | | |
