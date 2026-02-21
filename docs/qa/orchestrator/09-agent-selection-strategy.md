# Orchestrator - Agent Selection Strategy

**Module**: orchestrator
**Scope**: Validate intelligent agent selection with multi-factor scoring
**Scenarios**: 5
**Priority**: High

---

## Background

This document tests the new agent selection strategy that uses multi-factor scoring including cost, success rate, performance, load, and health state.

Entry point: `orchestrator task <command>` with configured agents

---

## Scenario 1: Multi-Factor Scoring Selection

### Preconditions

- Orchestrator binary available
- At least 2 agents configured with same capability (e.g., `qa`)
- Agents have different `cost` metadata values

### Steps

1. Configure agents with different costs in `config/default.yaml`:
   ```yaml
   agents:
     agent-low-cost:
       metadata:
         cost: 20
       capabilities: [qa]
     agent-high-cost:
       metadata:
         cost: 80
       capabilities: [qa]
   ```

2. Create and run a task with `qa` capability:
   ```bash
   orchestrator task create --name "scoring-test" --workflow qa_only
   orchestrator task start --latest
   ```

3. Check agent selection via event log:
   ```bash
   # Look for "agent_selected" events in logs
   cat data/logs/*.log | grep "agent_selected"
   ```

### Expected

- Agent selection considers multiple factors, not just cost
- Selection event shows strategy "adaptive"
- Metrics tracked for selected agent

---

## Scenario 2: Runtime Metrics Collection

### Preconditions

- Orchestrator running with task execution

### Steps

1. Execute several task cycles to generate metrics:
   ```bash
   orchestrator task create --name "metrics-test" --workflow qa_fix_retest
   orchestrator task start --latest
   ```

2. Query metrics via internal state (check event emissions):
   ```bash
   # Monitor for success/failure events
   ```

3. Check agent health after multiple runs:
   ```bash
   orchestrator agent health
   ```

### Expected

- Agent metrics collected: `total_runs`, `successful_runs`, `avg_duration_ms`
- Metrics update after each execution
- Health state reflects recent performance

---

## Scenario 3: Capability-Aware Health

### Preconditions

- Multiple agents with different capability sets
- One agent supports both `qa` and `fix`

### Steps

1. Configure agent with capabilities:
   ```yaml
   agents:
     multi-cap-agent:
       capabilities: [qa, fix]
     qa-only-agent:
       capabilities: [qa]
   ```

2. Force failure on `qa` capability:
   - Configure `qa` template to return non-zero exit code

3. Run `qa` phase multiple times until agent marked diseased

4. Run `fix` phase (different capability)

### Expected

- Agent marked diseased for `qa` capability
- Agent still available for `fix` capability
- Health state shows per-capability tracking

---

## Scenario 4: Retry Avoids Duplicate Selection

### Preconditions

- Multiple agents support same capability
- At least one agent set to always fail

### Steps

1. Configure agents where first fails:
   ```yaml
   agents:
     failing-agent:
       capabilities: [qa]
       templates:
         qa: "exit 1"
     healthy-agent:
       capabilities: [qa]
       templates:
         qa: "echo success"
   ```

2. Create task and start:
   ```bash
   orchestrator task create --name "retry-test" --workflow qa_only
   orchestrator task start --latest
   ```

3. Observe retry behavior in logs

### Expected

- First agent selected, fails
- Second retry does NOT select same failed agent
- Retry cycles through different agents
- Event log shows `attempt: 1`, `attempt: 2`, etc.

---

## Scenario 5: Load Balancing

### Preconditions

- Multiple agents with same capability

### Steps

1. Monitor load during concurrent operations

2. Check load metrics in selection events:
   ```bash
   # Check event payload for "current_load" field
   ```

3. Verify load decrements after task completion

### Expected

- Selection considers current load
- Higher load agents receive lower scores
- Load updates before and after execution

---

## Cleanup

```bash
# Delete test tasks
orchestrator task delete <task_id> --force
```

---

## Notes

- This feature requires Rust build with `metrics` module
- Use `cargo test metrics` to verify metrics module tests pass
- Event logging provides observability into selection decisions
