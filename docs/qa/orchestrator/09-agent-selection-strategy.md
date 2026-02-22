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
- Clean start: `rm -f data/agent_orchestrator.db && orchestrator init`
- Config must be bootstrapped (not just init): use `config bootstrap --from` with a complete fixture

### Steps

1. Prepare a config manifest with agents that have different costs (then apply it):
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
- Selection event shows strategy "capability_aware" (default)
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

3. Inspect runtime state after multiple runs:
   ```bash
   orchestrator debug --component state
   ```

### Expected

- Agent metrics collected internally: `total_runs`, `successful_runs`, `avg_duration_ms` are updated via `MetricsCollector` after each `run_phase` call
- Metrics accumulate in-memory in `agent_metrics` map and influence future agent selection scoring
- `debug --component state` shows general runtime state but does not expose per-agent metrics (dedicated metrics debug view is not yet implemented)
- Verify metrics are being used by observing that agent selection becomes less random over repeated runs

> **Note**: Runtime metrics are recorded in-memory via `MetricsCollector::record_success`/`record_failure` and influence the scoring in `calculate_agent_score`. However, there is no dedicated CLI command to inspect raw agent metrics. Verify indirectly by running multiple cycles and checking whether agent selection stabilizes toward the expected strategy.

---

## Scenario 3: Capability-Aware Health

### Preconditions

- Multiple agents with different capability sets
- One agent supports both `qa` and `fix`
- A full config must be bootstrapped first before applying agent-only manifests

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

- Agent marked diseased for `qa` capability after 2+ consecutive failures (via `increment_consecutive_errors` → `mark_agent_diseased`)
- Per-capability health tracked internally via `update_capability_health` (called after each `run_phase`)
- If `multi-cap-agent` is diseased globally but has good `fix` capability health (`success_rate >= 0.5`), it remains available for `fix`
- Verify by checking that the backup agent handles `qa` when primary is diseased, while primary still handles `fix`

> **Note**: Per-capability health IS implemented and functional. The health state is tracked in-memory via `AgentHealthState.capability_health` map. There is no dedicated CLI command to inspect per-capability health state directly — verify via behavioral observation (which agent gets selected for which capability after failures).

---

## Scenario 4: Retry Avoids Duplicate Selection

### Preconditions

- Multiple agents support same capability
- At least one agent set to always fail
- A full config must be bootstrapped first before applying agent-only manifests

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

- Failed items are marked `unresolved` and remain failed — **automatic retry with agent rotation is not implemented**
- Manual retry is available via `orchestrator task retry <task_item_id>`, which resets the item to `pending` status
- Manual retry does not exclude the previously failed agent — the same agent may be selected again
- The `excluded_agents` parameter exists in `select_agent_advanced` but is not populated during retry

> **Note**: Automatic retry with agent rotation is a planned but unimplemented feature. The current behavior is: if an agent fails, the item is finalized as `unresolved`. Use `task retry` to manually re-queue the item. To verify the failing agent is eventually avoided, rely on the health system — after 2+ consecutive failures, the agent is marked diseased and excluded from selection.

---

## Scenario 5: Load Balancing

### Preconditions

- Multiple agents with same capability
- `config bootstrap` (not just `init`) must be done before task commands

### Steps

1. Monitor load during concurrent operations

2. Check load metrics in selection events:
   ```bash
   # Check event payload for "current_load" field
   ```

3. Verify load decrements after task completion

### Expected

- `MetricsCollector::increment_load` is called before each agent execution, `decrement_load` after completion
- Load factor is used in scoring via `load_penalty` in `calculate_agent_score` (weight varies by strategy; `LoadBalanced` uses 0.5 weight)
- Higher load agents receive lower scores during concurrent operations
- Load metrics are tracked in-memory only — no `current_load` field is emitted in event payloads

> **Note**: Load tracking is now wired in the scheduler. `increment_load` is called in `run_phase_with_rotation` and `execute_guard_step` before execution; `decrement_load` is called in `run_phase` after completion. However, load data is not persisted to events — it is used solely for in-memory scoring decisions during agent selection.

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
