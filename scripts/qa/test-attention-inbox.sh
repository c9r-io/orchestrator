#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19196}"
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
printf '# Attention Inbox deterministic target\n' > "$QA_ROOT/fixtures/qa/attention.md"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(cat "$QA_ROOT/daemon.pid")"

for _ in {1..40}; do
  if "$ORCH" task list -o json >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  echo "isolated daemon failed to start" >&2
  cat "$QA_ROOT/daemon.log" >&2
  exit 1
fi

PROJECT="qa-attention-inbox"
"$ORCH" apply --project "$PROJECT" \
  -f "$REPO_ROOT/fixtures/manifests/bundles/process-timeline-failure.yaml" >/dev/null

CREATE_OUTPUT="$(
  cd "$QA_ROOT"
  "$ORCH" task create \
    --project "$PROJECT" \
    --workspace default \
    --workflow timeline_failure \
    --target-file fixtures/qa/attention.md \
    --goal "exercise deterministic Attention Inbox behavior" \
    --no-start
)"
TASK_ID="$(printf '%s\n' "$CREATE_OUTPUT" | grep -oE '[0-9a-f-]{36}' | head -1)"
[[ -n "$TASK_ID" ]] || { echo "task creation returned no task id" >&2; exit 1; }

"$ORCH" task start "$TASK_ID" >/dev/null 2>&1 || true
for _ in {1..60}; do
  STATUS="$("$ORCH" task info "$TASK_ID" -o json | jq -r '.task.status')"
  [[ "$STATUS" =~ ^(completed|failed|cancelled)$ ]] && break
  sleep 0.25
done

INBOX="$QA_ROOT/inbox.json"
for _ in {1..30}; do
  "$ORCH" attention list --project "$PROJECT" -o json > "$INBOX"
  jq -e '.items | any(.kind == "step_failed")' "$INBOX" >/dev/null && break
  sleep 0.25
done

if jq -e '.items | any(.kind == "step_failed" and .state == "open")' "$INBOX" >/dev/null; then
  pass "failed step materialized as an open attention item"
else
  fail "failed step was not materialized"
fi

if jq -e '.items | all((.summary | test("secret|token=|stdout|stderr"; "i")) | not)' "$INBOX" >/dev/null; then
  pass "list summaries exclude raw secrets and command output"
else
  fail "list summary contains unsafe raw text"
fi

DB="$QA_ROOT/data/agent_orchestrator.db"
sqlite3 "$DB" "
INSERT INTO events(task_id,task_item_id,event_type,payload_json,created_at)
SELECT task_id,task_item_id,event_type,payload_json,datetime('now')
FROM events WHERE task_id='$TASK_ID' AND event_type='step_finished' LIMIT 1;
"
for _ in {1..20}; do
  "$ORCH" attention list --project "$PROJECT" --kind step_failed -o json > "$INBOX"
  OCCURRENCES="$(jq -r '.items[0].occurrence_count // 0' "$INBOX")"
  [[ "$OCCURRENCES" -ge 2 ]] && break
  sleep 0.25
done
if [[ "$OCCURRENCES" -ge 2 ]] && [[ "$(jq '.items | length' "$INBOX")" -eq 1 ]]; then
  pass "duplicate failure aggregates into one active item"
else
  fail "duplicate failure did not aggregate"
fi

ITEM_ID="$(jq -r '.items[0].id' "$INBOX")"
VERSION="$(jq -r '.items[0].version' "$INBOX")"
set +e
"$ORCH" attention claim "$ITEM_ID" --expected-version "$VERSION" --idempotency-key qa-claim-a > "$QA_ROOT/claim-a" 2>&1 &
CLAIM_A=$!
"$ORCH" attention claim "$ITEM_ID" --expected-version "$VERSION" --idempotency-key qa-claim-b > "$QA_ROOT/claim-b" 2>&1 &
CLAIM_B=$!
wait "$CLAIM_A"; STATUS_A=$?
wait "$CLAIM_B"; STATUS_B=$?
set -e
if [[ $((STATUS_A + STATUS_B)) -ne 0 ]] && { [[ "$STATUS_A" -eq 0 ]] || [[ "$STATUS_B" -eq 0 ]]; }; then
  pass "only one concurrent claim succeeds for one version"
else
  fail "concurrent claim version gate was not exclusive"
fi

sqlite3 "$DB" "
INSERT INTO events(task_id,task_item_id,event_type,payload_json,created_at)
SELECT task_id,task_item_id,'step_finished','{\"step_id\":\"qa\",\"success\":true}',datetime('now')
FROM events WHERE task_id='$TASK_ID' AND event_type='step_finished' LIMIT 1;
"
for _ in {1..20}; do
  "$ORCH" attention get "$ITEM_ID" -o json > "$QA_ROOT/item.json"
  ITEM_STATE="$(jq -r '.state' "$QA_ROOT/item.json")"
  [[ "$ITEM_STATE" == "resolved" ]] && break
  sleep 0.25
done
if [[ "$ITEM_STATE" == "resolved" ]]; then
  pass "successful step event auto-resolves the originating item"
else
  fail "successful step did not auto-resolve the item"
fi

if sqlite3 "$DB" "SELECT resolution_json FROM attention_items WHERE id='$ITEM_ID';" | grep -q condition_cleared; then
  pass "auto-resolution stores an auditable reason"
else
  fail "auto-resolution reason is missing"
fi

if rg -q '"AttentionList" \| "AttentionGet" \| "AttentionFollow"' "$REPO_ROOT/crates/daemon/src/control_plane.rs" && \
   rg -q '"AttentionClaim" \| "AttentionSnooze" \| "AttentionResolve"' "$REPO_ROOT/crates/daemon/src/control_plane.rs"; then
  pass "RBAC separates read-only reads from operator mutations"
else
  fail "Attention RBAC mapping is incomplete"
fi

echo ""
echo "Attention Inbox QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
