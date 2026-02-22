# Orchestrator - Capability-Driven Agent Orchestration

**Module**: orchestrator
**Scope**: Validate new capability-driven orchestration features (cost preference, repeatable steps, guard steps)
**Scenarios**: 5
**Priority**: High

---

## Background

This document tests the upgraded orchestration features:
- **Capability-driven selection**: Agents declare capabilities, steps request capabilities
- **Agent-level selection strategy**: Each agent configures its own selection strategy via `selection.strategy`
- **Repeatable steps**: Steps can be marked repeatable (run every cycle) or one-time
- **Guard steps**: Steps can be marked as guards that terminate the workflow loop
- **Builtin steps**: init_once, ticket_scan, loop_guard are builtin behaviors

> **Note**: The workflow step's `cost_preference` field is deprecated (kept for backward compatibility). Agent selection now uses the agent's own `selection.strategy` as the primary configuration.

### New Config Format Reference

#### Agent with Capabilities and Selection Strategy
```yaml
agents:
  fast_agent:
    metadata:
      name: fast_agent
      cost: 20  # 1-100, lower = cheaper/faster
    capabilities:
    - qa
    - fix
    selection:
      strategy: performance_first  # cost_based, success_rate_weighted, performance_first, adaptive, load_balanced, capability_aware
      weights:  # optional, for adaptive strategy
        cost: 0.2
        success_rate: 0.3
        performance: 0.3
        load: 0.2
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

#### Step with Capability (No longer uses cost_preference)
```yaml
workflows:
  my_workflow:
    steps:
    - id: qa_test
      required_capability: qa
      repeatable: true
      is_guard: false
    - id: check_done
      builtin: loop_guard
      is_guard: true
      repeatable: true
```

> **Deprecation Notice**: The `cost_preference` field in workflow steps is deprecated. Configure agent selection strategy at the agent level using `selection.strategy`.

---

## Scenario 1: Capability-Driven Agent Selection

### Preconditions

- Orchestrator binary built
- Database reset (fresh state)
- Two agents with different capabilities configured
- Use `config bootstrap --from` for flat config fixtures (not `apply -f`; apply expects manifest format with `apiVersion: orchestrator.dev/v1`)

### Goal

Validate that when a step requires a capability, only agents with that capability are selected.

### Steps

1. Reset to fresh state:
   ```bash
   rm -f data/agent_orchestrator.db
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

3. Bootstrap config:
   ```bash
   ./core/target/release/agent-orchestrator config bootstrap --from /tmp/capability-test.yaml --force
   ```

4. Create task:
   ```bash
   ./core/target/release/agent-orchestrator task create \
     --name "capability-test" \
     --goal "Test capability selection" \
     --workspace default \
     --workflow test_capability
   ```

5. Get task info and check which agent was used:
   ```bash
   ./core/target/release/agent-orchestrator task info {task_id}
   ./core/target/release/agent-orchestrator task logs {task_id}
   ```

### Expected

- QA step uses `agent_qa_only` (the only agent with qa capability)
- Fix step uses `agent_fix_only` (the only agent with fix capability)
- Logs show respective echo outputs

---

## Scenario 2: Agent Selection Strategy - Performance First

### Preconditions

- Two agents with same capability but different selection strategies
- Use `config bootstrap --from` for flat config fixtures (not `apply -f`; apply expects manifest format with `apiVersion: orchestrator.dev/v1`)

### Goal

Validate that agents with `performance_first` strategy are prioritized.

### Steps

1. Create config with agents having different selection strategies:
   ```bash
   cat > /tmp/selection-perf-test.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   defaults:
     workspace: default
     workflow: selection_test
   workspaces:
     default:
       root_path: .
       qa_targets: []
       ticket_dir: docs/ticket
   agents:
     fast_agent:
       metadata:
         name: fast_agent
         cost: 20
       capabilities:
       - qa
       - fix
       selection:
         strategy: performance_first
       templates:
         qa: "echo 'fast-qa'"
         fix: "echo 'fast-fix'"
   
     quality_agent:
       metadata:
         name: quality_agent
         cost: 80
       capabilities:
       - qa
       - fix
       selection:
         strategy: success_rate_weighted
       templates:
         qa: "echo 'quality-qa'"
         fix: "echo 'quality-fix'"
   
   workflows:
     selection_test:
       steps:
       - id: do_qa
         required_capability: qa
         enabled: true
         repeatable: false

       - id: do_fix
         required_capability: fix
         enabled: true
         repeatable: false

       loop:
         mode: once
   EOF
   ```

2. Bootstrap config and test:
   ```bash
   ./core/target/release/agent-orchestrator config bootstrap --from /tmp/selection-perf-test.yaml --force
   ./core/target/release/agent-orchestrator task create \
     --name "selection-perf-test" \
     --goal "Test selection strategy" \
     --workspace default \
     --workflow selection_test
   ```

3. Check logs:
   ```bash
   ./core/target/release/agent-orchestrator task logs {task_id}
   ```

### Expected

- Both QA and Fix steps favor agents with `performance_first` strategy
- Logs show "fast-qa" and "fast-fix"
- Selection events show strategy "performance_first"

---

## Scenario 3: Agent Selection Strategy - Success Rate Weighted

### Preconditions

- Two agents with different selection strategies
- One agent configured with `success_rate_weighted`
- Use `config bootstrap --from` for flat config fixtures (not `apply -f`; apply expects manifest format with `apiVersion: orchestrator.dev/v1`)

### Goal

Validate that agents with higher success rates are prioritized when using `success_rate_weighted` strategy.

### Steps

1. Create config with success_rate_weighted strategy:
   ```bash
   cat > /tmp/selection-quality-test.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   defaults:
     workspace: default
     workflow: quality_selection_test
   workspaces:
     default:
       root_path: .
       qa_targets: []
       ticket_dir: docs/ticket
   agents:
     proven_agent:
       metadata:
         name: proven_agent
         cost: 50
       capabilities:
       - qa
       selection:
         strategy: success_rate_weighted
       templates:
         qa: "echo 'proven-qa'"
   
     new_agent:
       metadata:
         name: new_agent
         cost: 20
       capabilities:
       - qa
       selection:
         strategy: capability_aware
       templates:
         qa: "echo 'new-qa'"
   
   workflows:
     quality_selection_test:
       steps:
       - id: do_qa
         required_capability: qa
         enabled: true
         repeatable: false

       loop:
         mode: once
   EOF
   ```

2. Bootstrap config and test:
   ```bash
   ./core/target/release/agent-orchestrator config bootstrap --from /tmp/selection-quality-test.yaml --force
   ./core/target/release/agent-orchestrator task create \
     --name "selection-quality-test" \
     --goal "Test success rate weighted" \
     --workspace default \
     --workflow quality_selection_test
   ```

3. Check logs:
   ```bash
   ./core/target/release/agent-orchestrator task logs {task_id}
   ```

### Expected

- QA step favors `proven_agent` under `success_rate_weighted` strategy
- Logs show `proven-qa`

---

## Scenario 4: Repeatable Steps

### Preconditions

- Workflow with repeatable and non-repeatable steps
- Infinite loop mode
- Use `config bootstrap --from` for flat config fixtures (not `apply -f`; apply expects manifest format with `apiVersion: orchestrator.dev/v1`)

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

2. Bootstrap config:
   ```bash
   ./core/target/release/agent-orchestrator config bootstrap --from /tmp/repeatable-test.yaml --force
   ```

3. Create task and start:
   ```bash
   ./core/target/release/agent-orchestrator task create \
     --name "repeatable-test" \
     --goal "Test repeatable steps" \
     --workspace default \
     --workflow repeat_test
   ```

4. Wait for 2-3 cycles, then check logs:
   ```bash
   sleep 3
   ./core/target/release/agent-orchestrator task logs {task_id}
   ```

### Expected

- "one_time" step appears only in cycle 1
- "every_cycle" step appears in every cycle (cycle-1, cycle-2, cycle-3, etc.)

---

## Scenario 5: Guard Steps (is_guard)

### Preconditions

- Workflow with guard step that returns "stop"
- Use `config bootstrap --from` for flat config fixtures (not `apply -f`; apply expects manifest format with `apiVersion: orchestrator.dev/v1`)

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

2. Bootstrap config and test:
   ```bash
   ./core/target/release/agent-orchestrator config bootstrap --from /tmp/guard-test.yaml --force
   ./core/target/release/agent-orchestrator task create \
     --name "guard-test" \
     --goal "Test guard step" \
     --workspace default \
     --workflow guard_test
   ```

3. Check task status:
   ```bash
   ./core/target/release/agent-orchestrator task info {task_id}
   ```

### Expected

- Task runs for one cycle
- Guard step (builtin loop_guard) checks for unresolved items
- With stop_when_no_unresolved=true and no tickets, workflow terminates

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Capability-Driven Agent Selection | ☐ | | | |
| 2 | Cost Preference - Performance | ☐ | | | |
| 3 | Cost Preference - Quality | ☐ | | | |
| 4 | Repeatable Steps | ☐ | | | |
| 5 | Guard Steps (is_guard) | ☐ | | | |
