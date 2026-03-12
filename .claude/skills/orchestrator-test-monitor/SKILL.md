---
name: orchestrator-test-monitor
description: >
  Monitor and evaluate orchestrator test execution plans end-to-end.
  Use when the user wants to run a test execution plan from docs/plan/,
  observe the orchestrator's full-pipeline processing, and get a final assessment.
  Triggers on: "run test plan", "execute plan", "monitor orchestrator", "test the orchestrator",
  "run execution plan", or any request to observe/evaluate orchestrator behavior on a plan.
  This skill is OBSERVE-ONLY — never intervene in the orchestrator's execution.
---

# Orchestrator Test Monitor

Observe and evaluate the orchestrator's execution of a test plan. **You are a monitor, not a participant.** Never modify code, fix bugs, or intervene in execution. Only observe, report, and assess.

## Workflow

### Phase 1: Plan Selection

1. List all files under `docs/plan/` in the project working directory
2. Present each plan with a one-line summary (read the "Task Goal" or opening section)
3. Ask the user which plan to execute
4. Read the selected plan thoroughly — extract:
   - Expected workflow steps and their order
   - Success criteria
   - Key checkpoints
   - Known anomaly patterns

### Phase 2: Pre-Execution Setup

1. Verify the daemon is running: `pgrep -f orchestratord` or check `data/daemon.pid`
2. If not running, inform the user — do NOT start it yourself
3. Note the current git state: `git status`, `git log --oneline -3`
4. Note existing tasks: `orchestrator task list`
5. Record the baseline state for later comparison

### Phase 3: Guided Task Launch

1. Walk the user through the startup steps described in the selected plan
2. The USER creates the task — you only tell them what command to run per the plan
3. Once the task is created, record the task ID for monitoring

### Phase 4: Live Monitoring (OBSERVE ONLY)

Monitor the task using these commands in a loop:

```
orchestrator task watch <task_id>       # Real-time status panel
orchestrator task trace <task_id>       # Event timeline with anomaly detection
orchestrator task logs --follow <task_id>  # Stream logs
orchestrator task info <task_id>        # Detailed status snapshot
```

#### Monitoring Checklist

For each workflow step, verify:
- [ ] Step started (event emitted)
- [ ] Correct agent was selected
- [ ] Step completed with expected exit code
- [ ] Output matches expected structure (JSON for structured steps)
- [ ] No timeout or stall detected
- [ ] Cycle transitions are correct

#### Suspicious Indicators — Report Immediately

- Step stalled (no progress for >60s without explanation)
- Unexpected step skip or branch
- Agent mismatch (wrong agent for capability)
- Non-zero exit code on critical steps
- Missing expected events in timeline
- Database state inconsistency
- Cycle number not advancing as expected
- Items not generated when expected (evolution workflow)

**On suspicion**: Immediately tell the user what you observed, which step/event triggered concern, and the raw evidence (log line, event, status).

#### Confirmed Anomaly — Record Ticket

If an anomaly is clearly a bug (not just suspicious), create a ticket file:

```
docs/ticket/YYYYMMDD-<short-slug>.md
```

Format:
```markdown
# <Title>

- **Observed during**: <plan name>, step <step>, cycle <N>
- **Severity**: critical | major | minor
- **Symptom**: <what happened>
- **Expected**: <what should have happened>
- **Evidence**: <relevant log lines, events, or DB state>
- **Status**: open
```

Do NOT attempt to fix the anomaly. Only record it.

### Phase 5: Post-Execution Verification

After the task reaches `completed` or `failed`:

1. **Check final task status**:
   ```
   orchestrator task info <task_id>
   orchestrator task trace <task_id>
   ```

2. **Verify success criteria** from the plan:
   - Did all expected steps execute?
   - Did self_test pass (if applicable)?
   - Are cycle counts correct?
   - Did loop_guard terminate properly?

3. **Check artifacts**:
   - `git diff --stat` — were code changes made as expected?
   - Check `docs/qa/` for generated QA documents (if applicable)
   - Check `docs/ticket/` for any auto-generated tickets
   - Verify DB state via sqlite3 if the plan specifies DB checks

4. **Run plan-specific validations**:
   - For self-bootstrap: verify compilation gate passed, QA docs generated
   - For self-evolution: verify candidates generated, benchmarks ran, winner selected

### Phase 6: Assessment Report

Produce a structured assessment:

```
## Test Execution Report

### Plan: <plan name>
### Task ID: <id>
### Duration: <start to end>
### Final Status: <completed/failed>

### Step-by-Step Results
| Step | Status | Duration | Notes |
|------|--------|----------|-------|
| ...  | ...    | ...      | ...   |

### Anomalies Detected
- <list anomalies with severity, or "None">

### Tickets Created
- <list ticket files, or "None">

### Success Criteria Evaluation
| Criterion | Met? | Evidence |
|-----------|------|----------|
| ...       | ...  | ...      |

### Overall Assessment
<Pass/Partial/Fail with explanation>

### Recommendations
<Actionable next steps, if any>
```

## Rules

1. **NEVER** modify source code, config files, or workflow definitions
2. **NEVER** restart the daemon, kill processes, or alter system state
3. **NEVER** run `cargo` commands, `git commit/push`, or any write operation on the codebase
4. **NEVER** fix bugs — only record them as tickets
5. **ALWAYS** report suspicious observations immediately, don't wait
6. **ALWAYS** show raw evidence (log lines, events) when reporting issues
7. If the orchestrator is stuck and you suspect it will not recover, inform the user with evidence and let THEM decide whether to intervene
