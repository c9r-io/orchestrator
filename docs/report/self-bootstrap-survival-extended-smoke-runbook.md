# Self-Bootstrap Survival Extended Smoke Runbook

Date baseline: 2026-02-27
Repository: `/Volumes/Yotta/ai_native_sdlc`
Entry CLI: `orchestrator`

This runbook is a **high-cost, high-confidence** extension of
`docs/report/self-bootstrap-survival-smoke-runbook.md`.

Use it when you need stronger proof that the orchestrator's
**real LLM-backed self-modify** and **real self-health** safeguards work under
live execution, not just under deterministic shell-backed smoke fixtures.

It is intentionally slower, more invasive, and less deterministic than the
default smoke runbook. It should be treated as an **extended validation** pass,
not the default fast gate.

---

## 1. Goal

Validate the following with real execution evidence:

- A real LLM-backed `implement` phase can modify repository code
- The modification can intentionally introduce a compile-breaking change
- The builtin `self_test` step detects that break immediately
- The workflow halts before any downstream recovery or governance steps
- The repository can be restored cleanly after the destructive validation

Evidence sources:
- `events` table (SQLite)
- `command_runs` table (SQLite)
- `data/logs/<task-id>/` phase logs
- The actual diff applied to `core/src/lib.rs`

---

## 2. When To Use

Use this runbook only when one of these is true:

- Before a high-risk release touching scheduler, workflow execution, runner, or self-test behavior
- After refactors affecting agent templating, command execution, or pipeline step ordering
- When you need stronger proof than the default survival smoke
- When debugging regressions in self-bootstrap safety guarantees

Do **not** use this as a routine inner-loop smoke:

- It depends on external LLM execution
- It mutates real source files on purpose
- It can take minutes instead of seconds
- It is more sensitive to model drift and upstream provider availability

---

## 3. Safety Model

This runbook is destructive by design, but controlled:

- It writes exactly one known compile-breaking line
- It limits the modification to one file: `core/src/lib.rs`
- It forces the agent to stop after the single append
- It validates that `self_test` fails on the broken state
- It then removes the injected line and re-runs `cargo check`

The destructive marker used throughout this runbook is:

```rust
fn _smoke_break() { let x: i32 = "bad"; }
```

If cleanup fails, stop and restore `core/src/lib.rs` before continuing any other work.

---

## 4. Preconditions

Run from repo root:

```bash
cd /Volumes/Yotta/ai_native_sdlc
```

### 4.1 Build And Baseline

```bash
cargo build --release -p orchestratord -p orchestrator-cli && cargo check -p agent-orchestrator
```

Expected:
- Release build succeeds
- `cargo check` succeeds on a clean codebase

### 4.2 Initialize Orchestrator

```bash
rm -f data/agent_orchestrator.db config/default.yaml
orchestrator init -f
orchestrator apply -f docs/workflow/self-bootstrap.yaml
```

### 4.3 Create Dedicated QA Project

```bash
QA_PROJECT="qa-llm-selfbreak-${USER}-$(date +%Y%m%d%H%M%S)"
orchestrator qa project create "${QA_PROJECT}" --from-workspace self --force
```

---

## 5. Extended Scenario: Real LLM Self-Modify Must Be Caught By self_test

### 5.1 Apply Temporary LLM-Backed Breaker Agent

```bash
cat > /tmp/smoke-llm-selfbreak.yaml <<'YAML'
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: smoke-llm-breaker
spec:
  capabilities:
    - implement_break
  templates:
    implement_break: >-
      opencode run
      "You are performing a controlled smoke test for the Agent Orchestrator repository at {source_tree}.

      Make exactly one intentional breaking change for validation:
      append this exact line to the end of {source_tree}/core/src/lib.rs:
      fn _smoke_break() { let x: i32 = \"bad\"; }

      Requirements:
      1. Do not edit any other file
      2. Do not remove or fix the injected line
      3. Do not run cargo check, cargo test, or any repair step
      4. Stop after the single append succeeds

      Return a short confirmation describing the edit you made."
      --model minimax-coding-plan/MiniMax-M2.5-highspeed
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: smoke-llm-selfbreak
spec:
  steps:
    - id: implement
      type: implement
      required_capability: implement_break
      enabled: true
      repeatable: false
      tty: false
    - id: self_test
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

orchestrator apply -f /tmp/smoke-llm-selfbreak.yaml
```

### 5.2 Create And Start The Destructive Validation Task

```bash
orchestrator task create --project "${QA_PROJECT}" \
  -n "llm-selfbreak-$(date +%s)" \
  -w self -W smoke-llm-selfbreak \
  --no-start \
  -g "LLM self-modify smoke: intentionally append compile-breaking line and let self_test catch it" \
  -t core/src/lib.rs

TASK_ID=$(orchestrator task list -o json | jq -r 'sort_by(.created_at) | last | .id')
orchestrator task start "$TASK_ID"
```

### 5.3 Verify The Real LLM Agent Performed The Modification

```bash
sqlite3 data/agent_orchestrator.db "
SELECT phase, agent_id, exit_code, validation_status, command
FROM command_runs
WHERE task_item_id=(SELECT id FROM task_items WHERE task_id='${TASK_ID}' ORDER BY order_no LIMIT 1)
ORDER BY started_at;
"
```

Expected:
- `implement` row exists
- `agent_id='smoke-llm-breaker'`
- `exit_code=0`
- The `command` contains `opencode run`

### 5.4 Verify The Self-Health Gate Failed Immediately After The LLM Edit

```bash
sqlite3 data/agent_orchestrator.db "
SELECT event_type,
       json_extract(payload_json, '\$.step') AS step,
       json_extract(payload_json, '\$.exit_code') AS exit_code,
       json_extract(payload_json, '\$.success') AS success
FROM events
WHERE task_id='${TASK_ID}'
  AND event_type IN ('step_started','step_finished')
ORDER BY id;
"
```

Expected:
- `implement` starts and finishes first
- `implement` finishes with `exit_code: 0`, `success: true`
- `self_test` starts next
- `self_test` finishes with `exit_code != 0`, `success: false`

### 5.5 Verify The Actual File Was Broken

```bash
tail -5 core/src/lib.rs
```

Expected:
- The `_smoke_break` line is present at the end of the file

### 5.6 Cleanup And Restore Build Health

```bash
python - <<'PY'
from pathlib import Path
p = Path("core/src/lib.rs")
needle = '\nfn _smoke_break() { let x: i32 = "bad"; }\n'
text = p.read_text()
idx = text.rfind(needle)
assert idx != -1, "marker not found"
p.write_text(text[:idx] + text[idx + len(needle):])
PY

cd core && cargo check && cd ..
```

Expected:
- The injected line is removed
- `cargo check` succeeds again

### 5.7 Remove Temporary Resources

```bash
orchestrator task delete "$TASK_ID" -f
orchestrator delete workflow/smoke-llm-selfbreak -f
orchestrator delete agent/smoke-llm-breaker -f
rm -f /tmp/smoke-llm-selfbreak.yaml
```

---

## 6. Acceptance Checklist

| # | Check | Status |
|---|-------|--------|
| 1 | Release build succeeds before the test | ☐ |
| 2 | Clean baseline `cargo check` passes before the test | ☐ |
| 3 | Temporary LLM-backed agent and workflow apply successfully | ☐ |
| 4 | Real `implement` phase executes through `opencode run` | ☐ |
| 5 | Real LLM agent modifies `core/src/lib.rs` | ☐ |
| 6 | `self_test` fails immediately after the LLM modification | ☐ |
| 7 | No downstream fix/governance phases run in this workflow | ☐ |
| 8 | Cleanup removes the injected line and `cargo check` passes | ☐ |
| 9 | Temporary task/workflow/agent are deleted | ☐ |

---

## 7. Relationship To Default Smoke

This runbook complements, but does not replace:

- `docs/report/self-bootstrap-survival-smoke-runbook.md`

Recommended policy:

- Run the default survival smoke on every significant refactor or pre-release pass
- Run this extended smoke only for high-risk changes or when stronger proof is needed

---

## 8. Failure Handling

If this scenario fails:

- If the LLM agent edits the wrong file or makes extra changes:
  - Treat as a validation failure of the test harness, not proof of broken self-health
  - Create a ticket and tighten the prompt before trusting the result
- If `implement` succeeds but `self_test` also succeeds:
  - Treat as a critical regression in self-health enforcement
- If cleanup cannot remove the injected marker:
  - Stop immediately and restore `core/src/lib.rs` before any further testing
- If the provider is unavailable or times out:
  - Mark the run as inconclusive, not failed

---

## 9. Notes

- This is intentionally provider-dependent. Model behavior can drift.
- The prompt is constrained to keep the mutation narrow and auditable.
- The canonical success condition is not the agent's prose output; it is:
  - the actual file diff,
  - the `command_runs` record,
  - and the `self_test` failure event.
