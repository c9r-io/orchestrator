# Self-Bootstrap Survival Smoke Runbook

Date baseline: 2026-02-27
Repository: `/Volumes/Yotta/ai_native_sdlc`
Entry CLI: `./scripts/orchestrator.sh`

This runbook is a reproducible, copy-paste oriented smoke process that validates
the orchestrator's self-bootstrap workflow **and** the 4-layer survival mechanism
end-to-end with real execution evidence. Once verified, it supersedes
`docs/plan/orchestrator-usage-manual-testing.md`.

---

## 1. Goal

Validate that:
- The orchestrator builds, initializes, and accepts manifests
- The self-bootstrap workflow executes the correct step chain
- Pipeline variable propagation works (`{plan_output}` resolved)
- All 4 survival mechanism layers function correctly

Evidence sources:
- CLI output and exit codes
- `events` table (SQLite)
- `command_runs` table (SQLite)
- File system artifacts (`.stable`, logs)
- Watchdog stdout

---

## 2. Preconditions

Run from repo root:

```bash
cd /Volumes/Yotta/ai_native_sdlc
```

### 2.1 Build and Verify CLI

```bash
cd core && cargo build --release && cd ..

./scripts/orchestrator.sh --help
./scripts/orchestrator.sh task --help
```

Expected: help output prints without error.

### 2.2 Clean Runtime State

```bash
# Cold-start safe reset
rm -f data/agent_orchestrator.db config/default.yaml
./scripts/orchestrator.sh init -f
```

Runtime data locations:
- SQLite DB: `data/agent_orchestrator.db`
- Logs: `data/logs/`

### 2.3 Apply Self-Bootstrap Resources

```bash
./scripts/orchestrator.sh manifest validate -f docs/workflow/self-bootstrap.yaml
./scripts/orchestrator.sh apply -f docs/workflow/self-bootstrap.yaml

./scripts/orchestrator.sh get workspace
./scripts/orchestrator.sh get workflow
./scripts/orchestrator.sh get agent
```

Expected:
- workspace `self` (self_referential: true)
- workflow `self-bootstrap` (steps: plan → qa_doc_gen → implement → self_test → qa_testing → ticket_fix → align_tests → doc_governance → loop_guard)
- agents: `architect`, `coder`, `tester`, `reviewer`

### 2.4 Create QA Project

```bash
QA_PROJECT="qa-survival-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --from-workspace self --force
```

---

## 3. Layer 3: Self-Referential Enforcement

### 3.1 Hard Error — Missing Checkpoint Strategy

```bash
cat > /tmp/smoke-unsafe.yaml <<'YAML'
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: smoke-unsafe
spec:
  root_path: "."
  qa_targets: [docs/qa]
  ticket_dir: docs/ticket
  self_referential: true
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: smoke-unsafe-wf
spec:
  steps:
    - id: plan
      type: plan
      required_capability: plan
      enabled: true
  safety:
    checkpoint_strategy: none
    auto_rollback: true
YAML

./scripts/orchestrator.sh apply -f /tmp/smoke-unsafe.yaml

./scripts/orchestrator.sh task create --project "${QA_PROJECT}" \
  -w smoke-unsafe -W smoke-unsafe-wf \
  --no-start -g "smoke: expect hard error"

TASK_ID=$(./scripts/orchestrator.sh task list -o json | jq -r 'sort_by(.created_at) | last | .id')
./scripts/orchestrator.sh task start "$TASK_ID" 2>&1 | tee /tmp/smoke-unsafe-out.txt
```

Expected:
- Output contains `[SELF_REF_UNSAFE]` and `checkpoint_strategy is 'none'`
- Task does NOT start (status remains `pending`)

### 3.2 Warning — Auto-Rollback Disabled + No Self-Test Step

```bash
cat > /tmp/smoke-warn.yaml <<'YAML'
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: smoke-warn
spec:
  root_path: "."
  qa_targets: [docs/qa]
  ticket_dir: docs/ticket
  self_referential: true
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: smoke-warn-wf
spec:
  steps:
    - id: plan
      type: plan
      required_capability: plan
      enabled: true
  safety:
    checkpoint_strategy: git_tag
    auto_rollback: false
YAML

./scripts/orchestrator.sh apply -f /tmp/smoke-warn.yaml

./scripts/orchestrator.sh task create --project "${QA_PROJECT}" \
  -w smoke-warn -W smoke-warn-wf \
  --no-start -g "smoke: expect warnings"

TASK_ID2=$(./scripts/orchestrator.sh task list -o json | jq -r 'sort_by(.created_at) | last | .id')
./scripts/orchestrator.sh task start "$TASK_ID2" 2>/tmp/smoke-warn-stderr.txt &
TASK_PID=$!
sleep 3
kill "$TASK_PID" 2>/dev/null; wait "$TASK_PID" 2>/dev/null
cat /tmp/smoke-warn-stderr.txt
```

Expected:
- Task DOES start (not blocked)
- Stderr contains `[warn]` with `auto_rollback is disabled`
- Stderr contains `[warn]` with `has no 'self_test' step`

---

## 4. Layer 1: Binary Snapshot

```bash
rm -f .stable

./scripts/orchestrator.sh task create --project "${QA_PROJECT}" \
  -w self -W self-bootstrap \
  --no-start \
  -g "SURVIVAL SMOKE: binary snapshot verification"

TASK_ID=$(./scripts/orchestrator.sh task list -o json | jq -r 'sort_by(.created_at) | last | .id')

# Start inline so the task actually begins even when no background worker is running
./scripts/orchestrator.sh task start "$TASK_ID" >/tmp/smoke-snapshot-stdout.txt 2>/tmp/smoke-snapshot-stderr.txt &
START_PID=$!
sleep 5
./scripts/orchestrator.sh task pause "$TASK_ID"
kill "$START_PID" 2>/dev/null; wait "$START_PID" 2>/dev/null

# Verify .stable was created
ls -la .stable
ls -la core/target/release/agent-orchestrator

# Query events
sqlite3 data/agent_orchestrator.db "
SELECT event_type,
       json_extract(payload_json, '\$.cycle') AS cycle,
       json_extract(payload_json, '\$.path') AS path,
       json_extract(payload_json, '\$.tag') AS tag
FROM events
WHERE task_id='${TASK_ID}'
  AND event_type IN ('checkpoint_created','binary_snapshot_created')
ORDER BY id;
"
```

Expected:
- `.stable` file exists, same size as the release binary
- Events show `checkpoint_created` (with git tag) followed by `binary_snapshot_created` (with `.stable` path)
- Both reference `cycle: 1`

---

## 5. Layer 2: Self-Test Gate

### 5.1 Pass — Clean Codebase

```bash
# Verify codebase is clean first
cd core && cargo check && cargo test --lib && cd ..

cat > /tmp/smoke-selftest.yaml <<'YAML'
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: smoke-selftest
spec:
  steps:
    - id: implement
      type: implement
      required_capability: implement
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

./scripts/orchestrator.sh apply -f /tmp/smoke-selftest.yaml

./scripts/orchestrator.sh task create --project "${QA_PROJECT}" \
  -w self -W smoke-selftest \
  --no-start \
  -g "SURVIVAL SMOKE: self_test should pass on clean codebase"

TASK_ID=$(./scripts/orchestrator.sh task list -o json | jq -r 'sort_by(.created_at) | last | .id')
./scripts/orchestrator.sh task start "$TASK_ID"

# Query step events
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
- Step order: `implement` started/finished → `self_test` started/finished
- `self_test` `step_finished` has `exit_code: 0`, `success: true`

### 5.2 Fail — Broken Code

```bash
# Inject compile error
echo 'fn _smoke_break() { let x: i32 = "bad"; }' >> core/src/lib.rs

./scripts/orchestrator.sh task create --project "${QA_PROJECT}" \
  -w self -W smoke-selftest \
  --no-start \
  -g "SURVIVAL SMOKE: self_test should fail on broken code"

TASK_ID=$(./scripts/orchestrator.sh task list -o json | jq -r 'sort_by(.created_at) | last | .id')
./scripts/orchestrator.sh task start "$TASK_ID"

# Query evidence
sqlite3 data/agent_orchestrator.db "
SELECT event_type,
       json_extract(payload_json, '\$.step') AS step,
       json_extract(payload_json, '\$.exit_code') AS exit_code,
       json_extract(payload_json, '\$.success') AS success
FROM events
WHERE task_id='${TASK_ID}'
  AND event_type = 'step_finished'
  AND json_extract(payload_json, '\$.step') = 'self_test';
"

# Revert the error
sed -i '' '/_smoke_break/d' core/src/lib.rs
cd core && cargo check && cd ..
```

Expected:
- `step_finished` for `self_test` has `exit_code != 0`, `success: false`
- After reverting, `cargo check` passes clean

---

## 6. Basic Workflow Execution Chain (from V1)

This section validates the end-to-end agent execution chain and pipeline
variable propagation — the same concerns as the original V1 smoke test.

```bash
cat > /tmp/smoke-chain.yaml <<'YAML'
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: smoke-chain
spec:
  steps:
    - id: plan
      type: plan
      required_capability: plan
      enabled: true
      repeatable: false
      tty: false
    - id: qa_doc_gen
      type: qa_doc_gen
      required_capability: qa_doc_gen
      enabled: true
      repeatable: false
      tty: false
    - id: implement
      type: implement
      required_capability: implement
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

./scripts/orchestrator.sh apply -f /tmp/smoke-chain.yaml

./scripts/orchestrator.sh task create --project "${QA_PROJECT}" \
  -n survival-smoke-chain \
  -w self -W smoke-chain \
  --no-start \
  -g "SMOKE CHAIN: verify plan -> qa_doc_gen -> implement -> self_test with plan_output propagation" \
  -t docs/qa/orchestrator/26-self-bootstrap-workflow.md

TASK_ID=$(./scripts/orchestrator.sh task list -o json | jq -r 'sort_by(.created_at) | last | .id')
./scripts/orchestrator.sh task start "$TASK_ID"
```

### 6.1 Validate Step Execution

```bash
sqlite3 data/agent_orchestrator.db "
SELECT id, event_type,
       json_extract(payload_json,'\$.step') AS step,
       json_extract(payload_json,'\$.step_id') AS step_id,
       json_extract(payload_json,'\$.success') AS success,
       json_extract(payload_json,'\$.exit_code') AS exit_code,
       created_at
FROM events
WHERE task_id='${TASK_ID}'
ORDER BY id;
"
```

Expected:
- Events show `plan → qa_doc_gen → implement → self_test` started and finished
- `self_test` finishes with `exit_code: 0`, `success: true`

### 6.2 Validate `plan_output` Propagation

```bash
sqlite3 data/agent_orchestrator.db "
SELECT phase, command
FROM command_runs
WHERE task_item_id=(SELECT id FROM task_items WHERE task_id='${TASK_ID}' ORDER BY order_no LIMIT 1)
  AND phase IN ('qa_doc_gen','implement')
ORDER BY started_at;
"
```

Expected:
- `qa_doc_gen` and `implement` commands contain concrete plan text
- Commands do NOT contain literal `{plan_output}`

### 6.3 Validate Run Details

```bash
sqlite3 data/agent_orchestrator.db "
SELECT id, phase, agent_id, exit_code, validation_status,
       started_at, ended_at, stdout_path, stderr_path
FROM command_runs
WHERE task_item_id=(SELECT id FROM task_items WHERE task_id='${TASK_ID}' ORDER BY order_no LIMIT 1)
ORDER BY started_at;
"
```

Expected:
- Each phase has a `command_runs` row with `exit_code=0` and `validation_status=passed`
- `stdout_path` and `stderr_path` point to existing log files under `data/logs/`

---

## 7. Layer 4: Watchdog

```bash
# Create .stable from known-good binary
cp core/target/release/agent-orchestrator .stable

# Replace binary with broken one
cp core/target/release/agent-orchestrator /tmp/smoke-binary-backup
echo "broken" > core/target/release/agent-orchestrator
chmod +x core/target/release/agent-orchestrator

# Start watchdog with fast poll
WATCHDOG_POLL_INTERVAL=2 WATCHDOG_MAX_FAILURES=3 WATCHDOG_HEALTH_TIMEOUT=2 \
  scripts/watchdog.sh > /tmp/smoke-watchdog.txt 2>&1 &
WATCHDOG_PID=$!

# Wait until restore is logged (avoid racing with an in-flight health probe)
until rg -q "binary restored successfully" /tmp/smoke-watchdog.txt; do
  sleep 1
done

# Verify binary restored and healthy after the restore completes
core/target/release/agent-orchestrator --help >/dev/null 2>&1
sleep 2
core/target/release/agent-orchestrator --help >/dev/null 2>&1

# Check watchdog output
cat /tmp/smoke-watchdog.txt

# Graceful shutdown
kill "$WATCHDOG_PID" 2>/dev/null
wait "$WATCHDOG_PID" 2>/dev/null
tail -1 /tmp/smoke-watchdog.txt

# Restore original binary
cp /tmp/smoke-binary-backup core/target/release/agent-orchestrator
rm -f /tmp/smoke-binary-backup
```

Expected:
- `[watchdog] started`
- `health check failed (1/3)`, `(2/3)`, `(3/3)`
- `3 consecutive failures — triggering restore`
- `[watchdog] binary restored successfully`
- `agent-orchestrator --help` exits 0 after restore
- `[watchdog] shutting down gracefully`

---

## 8. Acceptance Checklist

| # | Section | Check | Status |
|---|---------|-------|--------|
| 1 | §2.1 | CLI builds and `--help` works | ☐ |
| 2 | §2.3 | Manifest validates and resources load | ☐ |
| 3 | §3.1 | `[SELF_REF_UNSAFE]` hard error blocks task start | ☐ |
| 4 | §3.2 | Warnings fire for auto_rollback + missing self_test | ☐ |
| 5 | §4 | `.stable` created at cycle start with event evidence | ☐ |
| 6 | §5.1 | `self_test` passes on clean codebase | ☐ |
| 7 | §5.2 | `self_test` fails on broken code | ☐ |
| 8 | §6.1 | Step chain `plan → qa_doc_gen → implement → self_test` executes | ☐ |
| 9 | §6.2 | `{plan_output}` resolved in downstream commands | ☐ |
| 10 | §6.3 | `command_runs` rows have exit_code=0, log files exist | ☐ |
| 11 | §7 | Watchdog restores binary after 3 failures | ☐ |

---

## 9. Cleanup

```bash
# Delete smoke tasks
./scripts/orchestrator.sh task list -o json | jq -r '.[].id' | while read tid; do
  ./scripts/orchestrator.sh task delete "$tid" -f 2>/dev/null
done

# Remove temp manifests
rm -f /tmp/smoke-unsafe.yaml /tmp/smoke-warn.yaml /tmp/smoke-selftest.yaml /tmp/smoke-chain.yaml

# Remove .stable snapshot
rm -f .stable

# Optional full reset (cold-start safe)
rm -f data/agent_orchestrator.db config/default.yaml
./scripts/orchestrator.sh init -f
```

---

## 10. What This Runbook Supersedes

When all checks pass, this runbook replaces:
- `docs/plan/orchestrator-usage-manual-testing.md` (V1 — basic CLI and workflow smoke)
- `docs/report/self-bootstrap-smoke-runbook.md` (earlier runbook — plan_output propagation focus)

V1 coverage fully absorbed into §2 (prerequisites), §6 (chain execution), and §9 (cleanup).
Earlier runbook coverage absorbed into §6.2 (plan_output propagation).
