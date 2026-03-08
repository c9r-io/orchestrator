# Self-Bootstrap Survival Self-Repair Extended Smoke Runbook

Date baseline: 2026-02-27
Repository: `/Volumes/Yotta/ai_native_sdlc`
Entry CLI: `orchestrator`

This runbook is a **high-cost, high-confidence** validation of the orchestrator's
**real self-repair loop**.

It extends:

- `docs/report/self-bootstrap-survival-smoke-runbook.md`
- `docs/report/self-bootstrap-survival-extended-smoke-runbook.md`

Its purpose is to prove a stronger statement than "the system can detect a bad
self-modification":

- a controlled, deterministic fault can simulate the kind of bad edit a model might accidentally make,
- a real self-health gate can fail,
- a real repair phase can restore the code,
- and a follow-up self-test can pass again.

This is not a default smoke gate. It is slower, more invasive, and more
provider-dependent than the standard survival smoke.

---

## 1. Goal

Validate, with real execution evidence, that:

- A controlled `implement` phase can inject a realistic compile-breaking change
- A real `self_test` phase fails on that change
- A real LLM-backed `ticket_fix` phase repairs the exact change
- A second `self_test` phase passes after the repair
- The repository is left in a clean, compilable state

Evidence sources:

- `events` table (SQLite)
- `command_runs` table (SQLite)
- `data/logs/<task-id>/`
- `core/src/lib.rs` before/after state

---

## 2. When To Use

Use this runbook when:

- You need stronger proof of autonomous recovery, not just failure detection
- You changed scheduler phase ordering, retry logic, self-test behavior, or repair routing
- You are evaluating release readiness for self-bootstrap / self-healing claims

Do not use this as a routine fast smoke:

- It depends on external LLM behavior
- It mutates source code on purpose
- It can take several minutes
- It has more nondeterminism than shell-backed smoke checks

---

## 3. Safety Model

This runbook uses a tightly scoped destructive marker:

```rust
fn _smoke_break() { let x: i32 = "bad"; }
```

Constraints:

- The break is limited to `core/src/lib.rs`
- The break is a single appended line
- The repair target is unambiguous
- The workflow uses `checkpoint_strategy: git_tag`
- The runbook still performs explicit cleanup and final `cargo check`

If cleanup fails, stop immediately and restore `core/src/lib.rs` before running
any other task.

---

## 4. Preconditions

Run from repo root:

```bash
cd /Volumes/Yotta/ai_native_sdlc
```

### 4.1 Baseline Build

```bash
cargo build --release -p orchestratord -p orchestrator-cli && cargo check -p agent-orchestrator
```

Expected:

- Release build succeeds
- `cargo check` succeeds before the test starts

### 4.2 Fresh Orchestrator State

```bash
rm -f data/agent_orchestrator.db config/default.yaml
orchestrator init -f
orchestrator apply -f docs/workflow/self-bootstrap.yaml
```

### 4.3 Dedicated QA Project

```bash
QA_PROJECT="qa-llm-selfrepair-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator qa project create "${QA_PROJECT}" --from-workspace self --force
```

---

## 5. Scenario Design

This runbook uses a purpose-built temporary workflow instead of the default
`self-bootstrap` loop. That keeps the validation narrow and the evidence easier
to interpret.

The sequence is:

1. `implement`:
   a deterministic breaker agent appends the destructive marker
2. `self_test_fail`:
   builtin `self_test` must fail
3. `ticket_fix`:
   a real LLM-backed repair agent removes the destructive marker
4. `self_test_recover`:
   builtin `self_test` must pass
5. `loop_guard`:
   terminate

This structure proves detection and recovery in one controlled task.

---

## 6. Apply Temporary Repair Validation Resources

```bash
cat > /tmp/smoke-llm-selfrepair.yaml <<'YAML'
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: smoke-breaker
spec:
  capabilities:
    - implement_break
  templates:
    implement_break: >-
      sh -lc 'printf "\nfn _smoke_break() { let x: i32 = \"bad\"; }\n" >> "{source_tree}/core/src/lib.rs"'
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: smoke-llm-repairer
spec:
  capabilities:
    - repair_break
  templates:
    repair_break: >-
      opencode run
      "You are performing a controlled self-repair smoke test for the Agent Orchestrator repository at {source_tree}.

      Repair exactly one known issue:
      remove this exact line from {source_tree}/core/src/lib.rs if it exists:
      fn _smoke_break() { let x: i32 = \"bad\"; }

      Requirements:
      1. Do not edit any other file
      2. Only remove the exact injected line
      3. Do not add refactors, formatting-only changes, or extra fixes
      4. After removing the line, stop immediately
      5. Do not run cargo check or cargo test yourself

      Return a short confirmation describing the repair you made."
      --model minimax-coding-plan/MiniMax-M2.5-highspeed
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: smoke-llm-selfrepair
spec:
  steps:
    - id: implement
      type: implement
      required_capability: implement_break
      enabled: true
      repeatable: false
      tty: false
    - id: self_test_fail
      type: self_test
      enabled: true
      repeatable: false
      tty: false
    - id: ticket_fix
      type: ticket_fix
      required_capability: repair_break
      enabled: true
      repeatable: false
      tty: false
    - id: self_test_recover
      type: self_test
      enabled: true
      repeatable: false
      tty: false
    - id: loop_guard
      type: loop_guard
      enabled: true
      repeatable: true
      is_guard: true
      builtin: loop_guard
  loop:
    mode: once
    enabled: true
    stop_when_no_unresolved: true
  safety:
    checkpoint_strategy: git_tag
    auto_rollback: true
YAML

orchestrator apply -f /tmp/smoke-llm-selfrepair.yaml
```

Expected:

- `agent/smoke-breaker` created
- `agent/smoke-llm-repairer` created
- `workflow/smoke-llm-selfrepair` created

---

## 7. Execute The Self-Repair Validation

```bash
orchestrator task create --project "${QA_PROJECT}" \
  -n "llm-selfrepair-$(date +%s)" \
  -w self -W smoke-llm-selfrepair \
  --no-start \
  -g "LLM self-repair smoke: break core/src/lib.rs, fail self_test, repair, then pass self_test" \
  -t core/src/lib.rs

TASK_ID=$(orchestrator task list -o json | jq -r 'sort_by(.created_at) | last | .id')
orchestrator task start "$TASK_ID"
```

If the task takes too long in your environment, allow up to 5 minutes before
declaring the run inconclusive.

---

## 8. Verify The Destructive Edit Happened

```bash
sqlite3 data/agent_orchestrator.db "
SELECT phase, agent_id, exit_code, validation_status, command
FROM command_runs
WHERE task_item_id=(SELECT id FROM task_items WHERE task_id='${TASK_ID}' ORDER BY order_no LIMIT 1)
  AND phase='implement'
ORDER BY started_at;
"
```

Expected:

- One `implement` row exists
- `agent_id='smoke-breaker'`
- `exit_code=0`
- The command appends the `_smoke_break` marker into `core/src/lib.rs`

---

## 9. Verify The First self_test Failed

```bash
sqlite3 data/agent_orchestrator.db "
SELECT event_type,
       json_extract(payload_json, '\$.step') AS step,
       json_extract(payload_json, '\$.exit_code') AS exit_code,
       json_extract(payload_json, '\$.success') AS success
FROM events
WHERE task_id='${TASK_ID}'
  AND event_type IN ('step_started','step_finished')
  AND json_extract(payload_json, '\$.step') IN ('implement','self_test_fail','ticket_fix','self_test_recover')
ORDER BY id;
"
```

Expected (minimum sequence):

- `implement` starts and finishes with success
- `self_test_fail` starts and finishes with `exit_code != 0`, `success: false`

Note:

- Some builds record builtin `self_test` as the generic step type `self_test` in payloads rather than the custom step id.
- If the step id is normalized internally, confirm order using event chronology and the surrounding `command_runs`.

---

## 10. Verify The Real Repair Phase Executed

```bash
sqlite3 data/agent_orchestrator.db "
SELECT phase, agent_id, exit_code, validation_status, command
FROM command_runs
WHERE task_item_id=(SELECT id FROM task_items WHERE task_id='${TASK_ID}' ORDER BY order_no LIMIT 1)
  AND phase='ticket_fix'
ORDER BY started_at;
"
```

Expected:

- One `ticket_fix` row exists
- `agent_id='smoke-llm-repairer'`
- `exit_code=0`
- The command contains `opencode run`

This is the core proof that a real repair phase ran, not a manual external cleanup.

---

## 11. Verify The Second self_test Passed

```bash
sqlite3 data/agent_orchestrator.db "
SELECT event_type,
       json_extract(payload_json, '\$.step') AS step,
       json_extract(payload_json, '\$.exit_code') AS exit_code,
       json_extract(payload_json, '\$.success') AS success
FROM events
WHERE task_id='${TASK_ID}'
  AND event_type='step_finished'
ORDER BY id;
"
```

Expected:

- One failed `self_test` completion appears before the repair
- A later `self_test` completion appears with `exit_code: 0`, `success: true`

This is the core proof that the system did not merely detect breakage; it recovered.

---

## 12. Verify Repository State Is Healthy

```bash
tail -5 core/src/lib.rs
cd core && cargo check && cd ..
```

Expected:

- The `_smoke_break` line is no longer present
- `cargo check` succeeds

---

## 13. Cleanup

```bash
orchestrator task delete "$TASK_ID" -f
orchestrator delete workflow/smoke-llm-selfrepair -f
orchestrator delete agent/smoke-breaker -f
orchestrator delete agent/smoke-llm-repairer -f
rm -f /tmp/smoke-llm-selfrepair.yaml
```

If the repair phase did not fully clean the file, remove the marker manually:

```bash
python - <<'PY'
from pathlib import Path
p = Path("core/src/lib.rs")
needle = '\nfn _smoke_break() { let x: i32 = "bad"; }\n'
text = p.read_text()
idx = text.rfind(needle)
if idx != -1:
    p.write_text(text[:idx] + text[idx + len(needle):])
PY
cd core && cargo check && cd ..
```

---

## 14. Acceptance Checklist

| # | Check | Status |
|---|-------|--------|
| 1 | Baseline release build succeeds | ☐ |
| 2 | Baseline `cargo check` succeeds | ☐ |
| 3 | Temporary breaker/repairer agents apply successfully | ☐ |
| 4 | Deterministic `implement` phase injects the controlled break | ☐ |
| 5 | First `self_test` fails on the injected break | ☐ |
| 6 | Real LLM-backed `ticket_fix` phase runs | ☐ |
| 7 | Second `self_test` passes after repair | ☐ |
| 8 | Final `cargo check` succeeds | ☐ |
| 9 | Temporary task/workflow/agents are deleted | ☐ |

---

## 15. Interpretation

If this runbook passes, you have materially stronger evidence that the
orchestrator can do all of the following under real LLM-driven execution:

- self-modify
- detect self-inflicted breakage
- execute a real LLM-backed repair phase
- re-establish build health

That is a stronger claim than the default survival smoke and a stronger claim
than the destructive-only extended smoke.

It still does **not** prove:

- long-horizon unattended evolution
- robust multi-issue repair
- semantic correctness of arbitrary feature work

It proves a narrow, high-value autonomous repair loop.

---

## 16. Failure Handling

If the run fails:

- If the breaker edits the wrong file or makes unrelated changes:
  - Treat the run as a harness bug and fix the breaker template
- If the first `self_test` does not fail:
  - Treat as a critical regression in self-health enforcement
- If the repair phase does not run:
  - Treat as a workflow wiring or agent selection regression
- If the repair phase runs but the second `self_test` still fails:
  - Treat as a genuine self-repair failure
- If cleanup fails:
  - Restore `core/src/lib.rs` immediately before any further testing
