#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19101}"
PASS=0
FAIL=0
DAEMON_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in jq sqlite3 mktemp rg; do
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

stop_daemon() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
    DAEMON_PID=""
  fi
}

cleanup() {
  stop_daemon
  if [[ "${KEEP_QA:-0}" == "1" ]]; then
    echo "QA_ROOT=$QA_ROOT" >&2
    echo "QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT

wait_for_daemon() {
  for _ in {1..60}; do
    "$ORCH" task list -o json >/dev/null 2>&1 && return 0
    sleep 0.25
  done
  return 1
}

has_action_audit_migration() {
  local database="$1"
  local latest migration table columns
  latest="$(sqlite3 "$database" "SELECT COALESCE(MAX(version),0) FROM schema_migrations;")"
  migration="$(sqlite3 "$database" "SELECT COUNT(*) FROM schema_migrations WHERE version=31 AND name='m0031_control_action_audit';")"
  table="$(sqlite3 "$database" "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='control_action_audit';")"
  columns="$(sqlite3 "$database" "SELECT COUNT(*) FROM pragma_table_info('control_action_audit') WHERE name IN ('request_id','project_id','action','status','request_hash','created_at','completed_at');")"
  [[ "$latest" -ge 31 && "$migration" -eq 1 && "$table" -eq 1 && "$columns" -eq 7 ]]
}

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/fixtures/qa" "$QA_ROOT/fixtures/ticket"
printf '# Canonical action audit deterministic target\n' > "$QA_ROOT/fixtures/qa/action-audit.md"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --webhook-bind none --workers 1 \
    > daemon-tcp.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(cat "$QA_ROOT/daemon.pid")"
if ! wait_for_daemon; then
  echo "isolated TCP daemon failed to start" >&2
  sed 's/^/  /' "$QA_ROOT/daemon-tcp.log" >&2
  exit 1
fi

PROJECT="qa-action-audit"
"$ORCH" apply --project "$PROJECT" \
  -f "$REPO_ROOT/fixtures/manifests/bundles/process-timeline-failure.yaml" >/dev/null
POLICY="$QA_ROOT/action-audit-policy.yaml"
printf '%s\n' \
  'apiVersion: orchestrator.dev/v2' \
  'kind: RuntimePolicy' \
  'metadata:' \
  '  name: default' \
  'spec:' \
  '  action_audit_mode: enforced' \
  '  runner:' \
  '    shell: /bin/bash' \
  '    shell_arg: -lc' \
  '    policy: allowlist' \
  '    executor: shell' \
  '    allowed_shells: [/bin/bash, /bin/sh, sh]' \
  '    allowed_shell_args: [-lc, -c]' \
  '  resume:' \
  '    auto: false' > "$POLICY"
"$ORCH" apply --project "$PROJECT" -f "$POLICY" >/dev/null

CREATE_OUTPUT="$(
  cd "$QA_ROOT"
  "$ORCH" task create --project "$PROJECT" --workspace default --workflow timeline_failure \
    --target-file fixtures/qa/action-audit.md --goal "exercise canonical action audit" --no-start
)"
TASK_ID="$(printf '%s\n' "$CREATE_OUTPUT" | grep -oE '[0-9a-f-]{36}' | head -1)"
[[ -n "$TASK_ID" ]] || { echo "task creation returned no task id" >&2; exit 1; }

"$ORCH" task start "$TASK_ID" >/dev/null 2>&1 || true
for _ in {1..80}; do
  TASK_STATUS="$("$ORCH" task info "$TASK_ID" -o json | jq -r '.task.status')"
  [[ "$TASK_STATUS" =~ ^(completed|failed|cancelled)$ ]] && break
  sleep 0.25
done

INBOX="$QA_ROOT/inbox.json"
for _ in {1..40}; do
  "$ORCH" attention list --project "$PROJECT" --kind step_failed -o json > "$INBOX"
  jq -e '.items | length > 0' "$INBOX" >/dev/null && break
  sleep 0.25
done
ITEM_ID="$(jq -r '.items[0].id // empty' "$INBOX")"
VERSION="$(jq -r '.items[0].version // empty' "$INBOX")"
[[ -n "$ITEM_ID" && -n "$VERSION" ]] || { echo "attention item was not materialized" >&2; exit 1; }

DB="$QA_ROOT/data/agent_orchestrator.db"
"$ORCH" attention claim "$ITEM_ID" --expected-version "$VERSION" \
  --idempotency-key qa-audit-success > "$QA_ROOT/success.out"
"$ORCH" audit list --project "$PROJECT" --action attention.claim -o json > "$QA_ROOT/audit.json"
SUCCESS_REQUEST_ID="$(jq -r 'map(select(.idempotency_key == "qa-audit-success"))[0].request_id' "$QA_ROOT/audit.json")"
"$ORCH" audit get "$SUCCESS_REQUEST_ID" --project "$PROJECT" -o json > "$QA_ROOT/audit-get.json"

JOIN_COUNT="$(sqlite3 "$DB" "
SELECT COUNT(*)
FROM control_action_audit a
JOIN attention_actions d ON d.request_id=a.request_id
JOIN control_plane_audit t ON t.request_id=a.request_id
JOIN events e ON e.request_id=a.request_id
WHERE a.request_id='$SUCCESS_REQUEST_ID' AND a.status='succeeded';
")"
POLICY_MODE="$(sqlite3 "$DB" "SELECT json_extract(spec_json,'$.action_audit_mode') FROM resources WHERE kind='RuntimePolicy' AND project='$PROJECT' AND name='default';")"
if [[ "$JOIN_COUNT" -ge 1 && "$POLICY_MODE" == "enforced" ]] && jq -e '.[0].status == "succeeded" and (.[0].request_hash | length) == 64' "$QA_ROOT/audit-get.json" >/dev/null; then
  pass "current client succeeds in enforced mode and joins all request-id evidence"
else
  fail "successful mutation request-id join is incomplete"
fi

VERSION_AFTER="$("$ORCH" attention get "$ITEM_ID" -o json | jq -r '.version')"
DOMAIN_ROWS_BEFORE="$(sqlite3 "$DB" "SELECT COUNT(*) FROM attention_actions WHERE idempotency_key='qa-audit-success';")"
set +e
"$ORCH" attention claim "$ITEM_ID" --expected-version "$VERSION" \
  --idempotency-key qa-audit-success > "$QA_ROOT/duplicate.out" 2>&1
DUPLICATE_STATUS=$?
"$ORCH" attention claim "$ITEM_ID" --expected-version "$VERSION_AFTER" \
  --idempotency-key qa-audit-success > "$QA_ROOT/conflict.out" 2>&1
CONFLICT_STATUS=$?
set -e
DOMAIN_ROWS_AFTER="$(sqlite3 "$DB" "SELECT COUNT(*) FROM attention_actions WHERE idempotency_key='qa-audit-success';")"
CONFLICT_REQUEST_ID="$(grep -oE 'req-[A-Za-z0-9_.:-]+' "$QA_ROOT/conflict.out" | tail -1 || true)"
if [[ "$DUPLICATE_STATUS" -ne 0 && "$CONFLICT_STATUS" -ne 0 ]] && \
   [[ "$DOMAIN_ROWS_BEFORE" == "$DOMAIN_ROWS_AFTER" ]] && \
   [[ -n "$CONFLICT_REQUEST_ID" ]] && \
   [[ "$(sqlite3 "$DB" "SELECT status || ':' || error_code FROM control_action_audit WHERE request_id='$CONFLICT_REQUEST_ID';")" == "failed:idempotency_conflict" ]]; then
  pass "matching duplicate has no second side effect and changed retry is durably rejected"
else
  fail "duplicate/conflict retry contract or durable error correlation failed"
fi

set +e
"$ORCH" attention resolve "$ITEM_ID" --expected-version "$VERSION" --reason "stale QA" \
  --idempotency-key qa-audit-stale > "$QA_ROOT/stale.out" 2>&1
STALE_STATUS=$?
set -e
STALE_REQUEST_ID="$(grep -oE 'req-[A-Za-z0-9_.:-]+' "$QA_ROOT/stale.out" | tail -1 || true)"
if [[ "$STALE_STATUS" -ne 0 && -n "$STALE_REQUEST_ID" ]] && \
   [[ "$(sqlite3 "$DB" "SELECT status FROM control_action_audit WHERE request_id='$STALE_REQUEST_ID';")" == "failed" ]] && \
   [[ "$("$ORCH" attention get "$ITEM_ID" -o json | jq -r '.version')" == "$VERSION_AFTER" ]]; then
  pass "stale version records terminal failure without mutating domain state"
else
  fail "stale mutation was not failed and correlated atomically"
fi

if ! rg -q 'action-audit\.md|exercise canonical action audit|canonical_request|provider_token|terminal_input' \
  "$QA_ROOT/audit.json" "$QA_ROOT/audit-get.json"; then
  pass "audit query output contains bounded hashes and references, not request bodies"
else
  fail "audit query output contains request-body or secret-bearing fields"
fi

stop_daemon
unset ORCHESTRATOR_CONTROL_PLANE_CONFIG
export ORCHESTRATOR_SOCKET="$ORCHESTRATORD_DATA_DIR/orchestrator.sock"
(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --uds-max-role read-only --webhook-bind none --workers 1 \
    > daemon-uds.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(cat "$QA_ROOT/daemon.pid")"
if ! wait_for_daemon; then
  echo "isolated read-only UDS daemon failed to start" >&2
  sed 's/^/  /' "$QA_ROOT/daemon-uds.log" >&2
  exit 1
fi

set +e
"$ORCH" attention resolve "$ITEM_ID" --expected-version "$VERSION_AFTER" \
  --reason "must be denied" --idempotency-key qa-audit-denied > "$QA_ROOT/denied.out" 2>&1
DENIED_STATUS=$?
set -e
DENIED_REQUEST_ID="$(grep -oE 'req-[A-Za-z0-9_.:-]+' "$QA_ROOT/denied.out" | tail -1 || true)"
if [[ "$DENIED_STATUS" -ne 0 && -n "$DENIED_REQUEST_ID" ]] && \
   [[ "$(sqlite3 "$DB" "SELECT status || ':' || error_code FROM control_action_audit WHERE request_id='$DENIED_REQUEST_ID';")" == "denied:authorization_denied" ]] && \
   [[ "$(sqlite3 "$DB" "SELECT authz_result FROM control_plane_audit WHERE request_id='$DENIED_REQUEST_ID' ORDER BY id DESC LIMIT 1;")" == "denied" ]]; then
  pass "read-only UDS denial returns a request ID joined to terminal denial evidence"
else
  sed 's/^/    /' "$QA_ROOT/denied.out" >&2
  fail "authorization denial request ID or durable evidence is missing"
fi

"$ORCH" audit list --project "$PROJECT" --status denied -o json > "$QA_ROOT/denied-audit.json"
if jq -e --arg id "$DENIED_REQUEST_ID" 'any(.request_id == $id and .status == "denied")' "$QA_ROOT/denied-audit.json" >/dev/null && \
   has_action_audit_migration "$DB"; then
  pass "project-scoped audit query and migration-31 capability are available under read-only role"
else
  fail "audit query RBAC, project filter, or migration schema check failed"
fi

stop_daemon
SCHEMA31_DB="$QA_ROOT/schema-31.db"
SCHEMA32_DB="$QA_ROOT/schema-32.db"
FUTURE_DB="$QA_ROOT/schema-future.db"
MISSING31_DB="$QA_ROOT/schema-missing-31.db"
for target in "$SCHEMA31_DB" "$SCHEMA32_DB" "$FUTURE_DB" "$MISSING31_DB"; do
  sqlite3 "$DB" ".backup '$target'"
done
sqlite3 "$SCHEMA31_DB" "DELETE FROM schema_migrations WHERE version > 31;"
sqlite3 "$FUTURE_DB" \
  "INSERT INTO schema_migrations(version,name,applied_at) VALUES(33,'m0033_future_additive_fixture',datetime('now'));"
sqlite3 "$MISSING31_DB" "DELETE FROM schema_migrations WHERE version=31;"
if has_action_audit_migration "$SCHEMA31_DB" && \
   has_action_audit_migration "$SCHEMA32_DB" && \
   has_action_audit_migration "$FUTURE_DB" && \
   ! has_action_audit_migration "$MISSING31_DB"; then
  pass "migration identity accepts schema 31, 32, and future additive versions but rejects missing 31"
else
  fail "migration identity/capability matrix is incorrect"
fi

echo ""
echo "Control-plane action audit QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
