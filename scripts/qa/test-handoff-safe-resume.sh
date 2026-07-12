#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19197}"
PASS=0
FAIL=0
DAEMON_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in jq sqlite3 mktemp; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "debug binaries not found; run: cargo build -p orchestratord -p orchestrator-cli" >&2
  exit 1
fi

QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  rm -rf "$QA_ROOT" "$QA_HOME"
}
trap cleanup EXIT

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/fixtures/qa" "$QA_ROOT/fixtures/ticket"
printf '# Handoff deterministic target\n' > "$QA_ROOT/fixtures/qa/handoff.md"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(cat "$QA_ROOT/daemon.pid")"

for _ in {1..40}; do
  "$ORCH" task list -o json >/dev/null 2>&1 && break
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  echo "isolated daemon failed to start" >&2
  cat "$QA_ROOT/daemon.log" >&2
  exit 1
fi

PROJECT="qa-handoff-safe-resume"
"$ORCH" apply --project "$PROJECT" \
  -f "$REPO_ROOT/fixtures/manifests/bundles/handoff-safe-resume.yaml" >/dev/null

create_task() {
  local workflow="$1"
  local name="$2"
  local output
  output="$(
    cd "$QA_ROOT"
    "$ORCH" task create --project "$PROJECT" --workspace default --workflow "$workflow" \
      --target-file fixtures/qa/handoff.md --goal "deterministic handoff and resume QA" \
      --name "$name" --no-start
  )"
  printf '%s\n' "$output" | grep -oE '[0-9a-f-]{36}' | head -1
}

TASK_ID="$(create_task handoff_failure handoff-failure)"
[[ -n "$TASK_ID" ]] || { echo "task creation returned no task id" >&2; exit 1; }
"$ORCH" task start "$TASK_ID" >/dev/null 2>&1 || true
for _ in {1..60}; do
  STATUS="$("$ORCH" task info "$TASK_ID" -o json | jq -r '.task.status')"
  [[ "$STATUS" =~ ^(completed|failed|cancelled)$ ]] && break
  sleep 0.25
done

DB="$QA_ROOT/data/agent_orchestrator.db"
ITEM_ID="$(sqlite3 "$DB" "SELECT id FROM task_items WHERE task_id='$TASK_ID' LIMIT 1;")"
sqlite3 "$DB" "
INSERT INTO events(task_id,task_item_id,event_type,payload_json,created_at)
VALUES('$TASK_ID','$ITEM_ID','qa_test_failed',
       '{\"step_id\":\"qa\",\"changed_files\":[\"src/lib.rs\"],\"test_evidence\":\"1 failed\",\"provider_token\":\"qa-secret-token\"}',
       datetime('now'));
"

"$ORCH" handoff generate "$TASK_ID" -o json > "$QA_ROOT/handoff-a.json"
CURSOR="$(jq -r '.source_event_cursor' "$QA_ROOT/handoff-a.json")"
"$ORCH" handoff generate "$TASK_ID" --cursor "$CURSOR" -o json > "$QA_ROOT/handoff-b.json"
if [[ "$(jq -r '.content_hash' "$QA_ROOT/handoff-a.json")" == "$(jq -r '.content_hash' "$QA_ROOT/handoff-b.json")" ]] && \
   [[ "$(jq -r '.id' "$QA_ROOT/handoff-a.json")" == "$(jq -r '.id' "$QA_ROOT/handoff-b.json")" ]]; then
  pass "same cursor returns the same immutable handoff and content hash"
else
  fail "same-cursor handoff projection changed"
fi

if jq -e '.briefing.changed_files == ["src/lib.rs"] and .briefing.failure != null and (.briefing.test_evidence | length) > 0' "$QA_ROOT/handoff-a.json" >/dev/null && \
   ! rg -q 'qa-secret-token' "$QA_ROOT/handoff-a.json" "$QA_ROOT/daemon.log"; then
  pass "briefing includes bounded failure/test/file evidence without provider tokens"
else
  fail "briefing evidence or redaction is incorrect"
fi

"$ORCH" resume boundaries "$TASK_ID" -o json > "$QA_ROOT/boundaries.json"
BOUNDARY_ID="$(jq -r 'map(select(.step_id == "qa"))[0].id' "$QA_ROOT/boundaries.json")"
if jq -e 'map(select(.step_id == "qa"))[0] | .replay_safe == true and .side_effect_class == "workspace_only"' "$QA_ROOT/boundaries.json" >/dev/null; then
  pass "declared workspace-only boundary is replay-safe"
else
  fail "workspace-only boundary classification is incorrect"
fi

"$ORCH" resume plan "$TASK_ID" --boundary "$BOUNDARY_ID" --mode restart_from_boundary -o json > "$QA_ROOT/plan-stale.json"
PLAN_ID="$(jq -r '.id' "$QA_ROOT/plan-stale.json")"
STATE_VERSION="$(jq -r '.expected_state_version' "$QA_ROOT/plan-stale.json")"
CHILDREN_BEFORE="$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE parent_task_id='$TASK_ID';")"
sqlite3 "$DB" "UPDATE tasks SET current_cycle=current_cycle+1, updated_at=datetime('now') WHERE id='$TASK_ID';"
set +e
"$ORCH" resume execute "$PLAN_ID" --expected-state-version "$STATE_VERSION" \
  --reason "stale plan QA" --idempotency-key stale-key > "$QA_ROOT/stale.out" 2>&1
STALE_STATUS=$?
set -e
CHILDREN_AFTER="$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE parent_task_id='$TASK_ID';")"
if [[ "$STALE_STATUS" -ne 0 ]] && rg -q 'stale resume plan' "$QA_ROOT/stale.out" && [[ "$CHILDREN_BEFORE" == "$CHILDREN_AFTER" ]]; then
  pass "stale plan is rejected before task/workspace mutation"
else
  echo "    stale_status=$STALE_STATUS children_before=$CHILDREN_BEFORE children_after=$CHILDREN_AFTER" >&2
  sed 's/^/    /' "$QA_ROOT/stale.out" >&2
  fail "stale plan was not rejected atomically"
fi

"$ORCH" resume boundaries "$TASK_ID" -o json > "$QA_ROOT/boundaries-fresh.json"
BOUNDARY_ID="$(jq -r 'map(select(.step_id == "qa"))[0].id' "$QA_ROOT/boundaries-fresh.json")"
"$ORCH" resume plan "$TASK_ID" --boundary "$BOUNDARY_ID" --mode restart_from_boundary -o json > "$QA_ROOT/plan.json"
PLAN_ID="$(jq -r '.id' "$QA_ROOT/plan.json")"
STATE_VERSION="$(jq -r '.expected_state_version' "$QA_ROOT/plan.json")"
"$ORCH" resume execute "$PLAN_ID" --expected-state-version "$STATE_VERSION" \
  --reason "reviewed deterministic retry" --idempotency-key execute-key -o json > "$QA_ROOT/execution.json"
CHILD_ID="$(jq -r '.child_task_id' "$QA_ROOT/execution.json")"
if sqlite3 "$DB" "SELECT parent_task_id || '|' || spawn_reason FROM tasks WHERE id='$CHILD_ID';" | \
   grep -q "^$TASK_ID|resume_boundary:"; then
  pass "restart creates and enqueues a correlated child without git rollback"
else
  fail "correlated child task was not created"
fi

EXTERNAL_TASK="$(create_task handoff_external_unknown handoff-external)"
"$ORCH" resume boundaries "$EXTERNAL_TASK" -o json > "$QA_ROOT/external-boundaries.json"
EXTERNAL_BOUNDARY="$(jq -r '.[0].id' "$QA_ROOT/external-boundaries.json")"
"$ORCH" resume plan "$EXTERNAL_TASK" --boundary "$EXTERNAL_BOUNDARY" --mode restart_from_boundary -o json > "$QA_ROOT/external-plan.json"
set +e
"$ORCH" resume execute "$(jq -r '.id' "$QA_ROOT/external-plan.json")" \
  --expected-state-version "$(jq -r '.expected_state_version' "$QA_ROOT/external-plan.json")" \
  --reason "must stay denied" --idempotency-key external-key --elevated-confirmation \
  > "$QA_ROOT/external.out" 2>&1
EXTERNAL_STATUS=$?
set -e
if [[ "$EXTERNAL_STATUS" -ne 0 ]] && rg -q 'non-idempotent replay denied' "$QA_ROOT/external.out"; then
  pass "undeclared external replay is denied by default policy"
else
  fail "non-idempotent replay was not denied"
fi

if sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$TASK_ID' AND event_type='resume_executed';" | grep -q '^1$' && \
   sqlite3 "$DB" "SELECT COUNT(*) FROM resume_executions WHERE plan_id='$PLAN_ID' AND status='succeeded';" | grep -q '^1$'; then
  pass "successful state change records execution and audit event"
else
  fail "resume execution audit evidence is missing"
fi

for _ in {1..20}; do
  "$ORCH" attention list --task "$TASK_ID" -o json > "$QA_ROOT/source-attention.json"
  ACTIVE_ATTENTION="$(jq '[.items[] | select(.state != "resolved")] | length' "$QA_ROOT/source-attention.json")"
  [[ "$ACTIVE_ATTENTION" -eq 0 ]] && break
  sleep 0.25
done
if [[ "$ACTIVE_ATTENTION" -eq 0 ]]; then
  pass "Attention resolves only after the durable resume execution event"
else
  fail "source-task Attention remained active after successful resume execution"
fi

echo ""
echo "Handoff and safe resume QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
