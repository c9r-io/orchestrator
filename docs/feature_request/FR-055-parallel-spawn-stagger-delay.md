# FR-055: Parallel Spawn Stagger Delay

- **Status**: open
- **Created**: 2026-03-16
- **Priority**: high

---

## 1. Problem

When `max_parallel > 1`, the scheduler dispatches all available items into a `JoinSet`
in a tight loop with no inter-spawn delay. Each agent subprocess (`claude -p ...`) loads
plugins, MCP servers, and initializes API connections during startup. When N agents start
simultaneously:

1. **MCP port/lock contention** — multiple Node.js MCP servers (context7, playwright)
   compete for TCP ports or file locks, causing initialization deadlocks.
2. **API rate limiting** — N concurrent API handshakes may trigger rate limits on the
   upstream LLM provider.
3. **I/O spike** — simultaneous plugin loading, git operations, and filesystem scans
   saturate disk I/O, making all processes slow to start.

**Observed impact**: In a full-QA regression test (`full-qa-execution.md`), 4 parallel
`claude -p` agents all hung during initialization (0 bytes stdout for 900s), triggering
`step_stall_killed` (exit=-7) for every item. No AI credits were consumed — the agents
never reached the API call phase. See `docs/ticket/20260316-stall-kill-qa-testing.md`.

---

## 2. Proposal

Add an optional `stagger_delay_ms` field to both the **Workflow spec** (global default)
and the **Step spec** (per-step override), following the same resolution pattern as
`max_parallel`:

```
step.stagger_delay_ms  >  workflow.stagger_delay_ms  >  0 (no delay)
```

When set, the scheduler inserts a `tokio::time::sleep(Duration::from_millis(delay))`
between successive `join_set.spawn()` calls in the parallel dispatch loop.

### 2.1 YAML Surface

**Workflow level** (global default for all item-scoped steps):

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: full-qa
spec:
  max_parallel: 4
  stagger_delay_ms: 3000        # <-- NEW: 3s between agent spawns

  steps:
    - id: qa_testing
      scope: item
      # inherits stagger_delay_ms: 3000 from workflow
      ...

    - id: ticket_fix
      scope: item
      max_parallel: 2
      stagger_delay_ms: 1000    # <-- per-step override: 1s
      ...
```

**Step level** (per-step override):

```yaml
- id: qa_testing
  scope: item
  max_parallel: 4
  stagger_delay_ms: 5000       # 5s between spawns for heavy steps
```

### 2.2 Resolution Order

```text
step.stagger_delay_ms  ??  workflow.stagger_delay_ms  ??  0
```

- Only applies when `max_parallel > 1` (parallel path).
- Value of `0` means no delay (backward compatible default).
- The delay is inserted **after** each `join_set.spawn()`, not before the first one.

---

## 3. Implementation Plan

### 3.1 Config Layer (`orchestrator-config`)

**File: `crates/orchestrator-config/src/cli_types.rs`**

1. Add `stagger_delay_ms: Option<u64>` to `WorkflowYamlSpec` (next to `max_parallel`,
   line ~649).
2. Add `stagger_delay_ms: Option<u64>` to `StepYamlSpec` (next to `max_parallel`,
   line ~787).

**File: `crates/orchestrator-config/src/config/workflow.rs`**

3. Add `stagger_delay_ms: Option<u64>` to `ExecutionPlan`.

### 3.2 Config Conversion (`core`)

**File: `core/src/resource/workflow/workflow_convert.rs`**

4. Map `WorkflowYamlSpec.stagger_delay_ms` to `ExecutionPlan.stagger_delay_ms`.
5. Map `StepYamlSpec.stagger_delay_ms` to `StepConfig.stagger_delay_ms`.

### 3.3 Segment Builder

**File: `crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs`**

6. Add `stagger_delay_ms: u64` to `ScopeSegment` struct (line ~36).
7. In `build_scope_segments()`, resolve:
   ```rust
   let stagger_delay_ms = if scope == StepScope::Item {
       step.stagger_delay_ms
           .or(task_ctx.execution_plan.stagger_delay_ms)
           .unwrap_or(0)
   } else {
       0
   };
   ```

### 3.4 Parallel Dispatch Loop

**File: `crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs`**

8. In `execute_item_segment()`, after `join_set.spawn()` (line ~383), add:
   ```rust
   dispatched_count += 1;
   if stagger_delay_ms > 0 && dispatched_count < items.len() {
       tokio::time::sleep(std::time::Duration::from_millis(stagger_delay_ms)).await;
   }
   ```
   Only sleeps between spawns (not after the last one).

### 3.5 Workflow YAML Unknown-Field Guard

**File: `core/src/resource/workflow/workflow_convert.rs`**

9. Ensure `stagger_delay_ms` is NOT flagged as an unknown field by the unknown-field
   warning system (FR-051).

---

## 4. Test Plan

### 4.1 Unit Tests

| Test | Location | Assertion |
|------|----------|-----------|
| Default is 0 | `config/workflow.rs` | `ExecutionPlan::default().stagger_delay_ms == None` |
| Step override wins | `segment.rs` | Step=2000, workflow=5000 -> segment gets 2000 |
| Workflow fallback | `segment.rs` | Step=None, workflow=3000 -> segment gets 3000 |
| No delay when sequential | `segment.rs` | max_parallel=1 -> stagger_delay_ms forced to 0 |
| YAML round-trip | `workflow_convert.rs` | Serialize -> deserialize preserves value |

### 4.2 Integration Tests

| Test | Assertion |
|------|-----------|
| 4 items, stagger=500ms | First 4 spawns are ~500ms apart (within tolerance) |
| stagger=0 (default) | No measurable delay between spawns |
| Unknown-field warning | `stagger_delay_ms` in YAML does NOT trigger warning |

### 4.3 Manual Validation

Re-run `full-qa-execution.md` with `stagger_delay_ms: 3000` in `full-qa.yaml` and
verify:
1. Agent processes start ~3s apart (check `step_spawned` event timestamps).
2. All agents produce stdout (init JSON) within the first heartbeat interval.
3. No `step_stall_killed` events.
4. AI credits are consumed normally.

---

## 5. Backward Compatibility

- Default is `0` (no delay) — existing workflows are unaffected.
- No schema-breaking changes; the field is optional with `skip_serializing_if`.
- No CLI flag changes needed.

---

## 6. Related

- **Ticket**: `docs/ticket/20260316-stall-kill-qa-testing.md` — stall-kill root cause
- **FR-053**: Parallel dispatch completeness guard (ensures all items are dispatched)
- **FR-054**: Incremental item progress display
- `STALL_AUTO_KILL_CONSECUTIVE_HEARTBEATS` — separate concern; stagger reduces the
  *cause* (init hang) rather than tuning the *symptom* (kill threshold)
