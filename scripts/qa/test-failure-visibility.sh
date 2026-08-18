#!/usr/bin/env bash
#
# QA 213 (FR-162): the failure-visibility contract, end to end.
#
# Scenario 1 — task completion must not sweep failure evidence, including when
# the failure and the completion land in the same projection batch (one SQLite
# transaction guarantees one batch).
# Scenario 2 — webhook auth failures materialize as one merged
# source_auth_failed item per trigger, unknown-trigger 404s as one
# source_route_missing item per project with the hostile name digested,
# unknown projects allocate nothing, and the first successful delivery
# auto-resolves the auth item.
# Scenario 3 — a disabled attention inbox advances the cursor but records the
# dropped range; re-enabling surfaces one inbox_projection_gap item.
# Scenario 4 — every summary in the inbox stays free of payload content.

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_daemon.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19213}"
WEBHOOK_ADDR="${WEBHOOK_ADDR:-127.0.0.1:19214}"
PASS=0
FAIL=0
DAEMON_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in jq sqlite3 mktemp curl openssl; do
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
  gate_daemon_stop "$DAEMON_PID" || true
  DAEMON_PID=""
  rm -rf "$QA_ROOT" "$QA_HOME"
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-failure-visibility.sh"

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/fixtures/qa" "$QA_ROOT/fixtures/ticket"
printf '# Failure visibility deterministic target\n' > "$QA_ROOT/fixtures/qa/visibility.md"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --webhook-bind "$WEBHOOK_ADDR" --workers 1 > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"

if ! gate_daemon_wait_ready "$ORCH"; then
  echo "isolated daemon failed to start" >&2
  cat "$QA_ROOT/daemon.log" >&2
  exit 1
fi
# The key file is a second fact and stays a second wait. Readiness reports the
# keyring as loaded from the database; this gate needs the file on disk, which
# is what its later assertions read.
for _ in {1..20}; do
  [[ -f "$ORCHESTRATORD_DATA_DIR/secrets/secretstore.key" ]] && break
  sleep 0.25
done

PROJECT="qa-fr162"
DB="$QA_ROOT/data/agent_orchestrator.db"
"$ORCH" apply --project "$PROJECT" \
  -f "$REPO_ROOT/fixtures/manifests/bundles/process-timeline-failure.yaml" >/dev/null

echo "--- Scenario 1: completion does not sweep failure evidence ---"

CREATE_OUTPUT="$(
  cd "$QA_ROOT"
  "$ORCH" task create \
    --project "$PROJECT" \
    --workspace default \
    --workflow timeline_failure \
    --target-file fixtures/qa/visibility.md \
    --goal "exercise the FR-162 failure-visibility contract" \
    --no-start
)"
# FR-146: `| head -1` under pipefail kills grep and ends the gate with no summary line.
TASK_IDS="$(grep -oE '[0-9a-f-]{36}' <<< "$CREATE_OUTPUT" || true)"
TASK_ID="${TASK_IDS%%$'\n'*}"
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

# The acceptance's negative fixture: a pending condition item and the terminal
# task_completed event commit in ONE transaction, so the projector applies the
# condition upsert and the completion sweep in the same batch. The condition
# must resolve with reason task_completed; the evidence must stay open.
sqlite3 "$DB" <<SQL
BEGIN;
INSERT INTO events(task_id,task_item_id,event_type,payload_json,created_at)
VALUES('$TASK_ID',NULL,'approval_requested','{"step_id":"vis-approval"}',datetime('now'));
INSERT INTO events(task_id,task_item_id,event_type,payload_json,created_at)
VALUES('$TASK_ID',NULL,'task_completed','{}',datetime('now'));
COMMIT;
SQL

APPROVAL_STATE=""
for _ in {1..30}; do
  APPROVAL_STATE="$(sqlite3 "$DB" \
    "SELECT state FROM attention_items WHERE task_id='$TASK_ID' AND kind='approval_required';")"
  [[ "$APPROVAL_STATE" == "resolved" ]] && break
  sleep 0.25
done
if [[ "$APPROVAL_STATE" == "resolved" ]]; then
  pass "same-batch completion sweeps the pending condition item"
else
  fail "condition item did not resolve on task completion (state: ${APPROVAL_STATE:-absent})"
fi

APPROVAL_REASON="$(sqlite3 "$DB" \
  "SELECT resolution_json FROM attention_items WHERE task_id='$TASK_ID' AND kind='approval_required';")"
if grep -q task_completed <<< "$APPROVAL_REASON"; then
  pass "swept condition carries resolution reason task_completed"
else
  fail "swept condition reason is not task_completed ($APPROVAL_REASON)"
fi

"$ORCH" attention list --project "$PROJECT" --kind step_failed -o json > "$INBOX"
if jq -e '.items | any(.kind == "step_failed" and .state == "open")' "$INBOX" >/dev/null; then
  pass "failure evidence stays visible after the completion event"
else
  fail "completion swept the failure evidence"
fi

echo ""
echo "--- Scenario 2: webhook failures reach the inbox ---"

TMP="$QA_ROOT/manifest.yaml"
cat > "$TMP" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: vis-signing
spec:
  data:
    key: vis-secret-value
EOF
"$ORCH" apply --project "$PROJECT" -f "$TMP" >/dev/null

cat > "$TMP" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: auth-vis
spec:
  event:
    source: webhook
    webhook:
      secret:
        fromRef: vis-signing
  action:
    workflow: timeline_failure
    workspace: default
EOF
"$ORCH" apply --project "$PROJECT" -f "$TMP" >/dev/null

BODY='{"probe":"fr162"}'
BAD_SIG="$(echo -n "$BODY" | openssl dgst -sha256 -hmac "wrong-secret" | awk '{print $NF}')"
for _ in 1 2 3; do
  RESP="$(curl -s -o /dev/null -w "%{http_code}" -X POST \
    "http://${WEBHOOK_ADDR}/webhook/${PROJECT}/auth-vis" \
    -d "$BODY" -H "X-Webhook-Signature: sha256=${BAD_SIG}" 2>/dev/null || echo "000")"
  [[ "$RESP" == "401" ]] || fail "bad signature returned HTTP $RESP (expected 401)"
done

AUTH_ITEMS=0
for _ in {1..30}; do
  "$ORCH" attention list --project "$PROJECT" --kind source_auth_failed -o json > "$INBOX"
  AUTH_ITEMS="$(jq '.items | length' "$INBOX")"
  [[ "$AUTH_ITEMS" -ge 1 ]] && break
  sleep 0.25
done
if [[ "$AUTH_ITEMS" -eq 1 ]] &&
  jq -e '.items[0].state == "open"' "$INBOX" >/dev/null; then
  pass "repeated bad signatures merge into one open source_auth_failed item"
else
  fail "expected exactly one open source_auth_failed item, got $AUTH_ITEMS"
fi

GOOD_SIG="$(echo -n "$BODY" | openssl dgst -sha256 -hmac "vis-secret-value" | awk '{print $NF}')"
RESP="$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  "http://${WEBHOOK_ADDR}/webhook/${PROJECT}/auth-vis" \
  -d "$BODY" -H "X-Webhook-Signature: sha256=${GOOD_SIG}" 2>/dev/null || echo "000")"
if [[ "$RESP" == "200" ]]; then
  pass "valid signature fires the trigger (HTTP 200)"
else
  fail "valid signature returned HTTP $RESP (expected 200)"
fi

AUTH_STATE=""
for _ in {1..30}; do
  "$ORCH" attention list --project "$PROJECT" --kind source_auth_failed --state resolved -o json > "$INBOX"
  AUTH_STATE="$(jq -r '.items[0].state // empty' "$INBOX")"
  [[ "$AUTH_STATE" == "resolved" ]] && break
  sleep 0.25
done
if [[ "$AUTH_STATE" == "resolved" ]]; then
  pass "first successful delivery auto-resolves the auth-failure item"
else
  fail "auth-failure item did not auto-resolve after a good delivery"
fi

HOSTILE="ghost-trigger-fr162-secret-name"
RESP="$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  "http://${WEBHOOK_ADDR}/webhook/${PROJECT}/${HOSTILE}" -d '{}' 2>/dev/null || echo "000")"
[[ "$RESP" == "404" ]] || fail "unknown trigger returned HTTP $RESP (expected 404)"
ROUTE_ITEMS=0
for _ in {1..30}; do
  "$ORCH" attention list --project "$PROJECT" --kind source_route_missing -o json > "$INBOX"
  ROUTE_ITEMS="$(jq '.items | length' "$INBOX")"
  [[ "$ROUTE_ITEMS" -ge 1 ]] && break
  sleep 0.25
done
if [[ "$ROUTE_ITEMS" -eq 1 ]] &&
  jq -e --arg name "$HOSTILE" \
    '.items[0] | (.summary + .title + .dedupe_key) | contains($name) | not' "$INBOX" >/dev/null; then
  pass "unknown trigger surfaces source_route_missing without the hostile name"
else
  fail "source_route_missing item missing or leaks the delivery's trigger name"
fi

RESP="$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  "http://${WEBHOOK_ADDR}/webhook/ghost-project-fr162/whatever" -d '{}' 2>/dev/null || echo "000")"
GHOST_ROWS="$(sqlite3 "$DB" "SELECT COUNT(*) FROM attention_items WHERE project_id='ghost-project-fr162';")"
if [[ "$GHOST_ROWS" == "0" ]]; then
  pass "deliveries naming an unknown project allocate nothing (HTTP $RESP)"
else
  fail "unknown project allocated $GHOST_ROWS attention row(s)"
fi

echo ""
echo "--- Scenario 3: disabled inbox leaves a named gap ---"

runtime_policy() {
  cat > "$TMP" <<EOF
apiVersion: orchestrator.dev/v2
kind: RuntimePolicy
metadata:
  name: default
spec:
  runner:
    shell: /bin/bash
    shell_arg: -lc
    policy: allowlist
    allowed_shells: [/bin/bash, /bin/sh, sh]
    allowed_shell_args: [-lc, -c]
  resume:
    auto: false
  attention_inbox_enabled: $1
EOF
  "$ORCH" apply --project "$PROJECT" -f "$TMP" >/dev/null
}
runtime_policy false

CURSOR_BEFORE="$(sqlite3 "$DB" "SELECT last_event_id FROM attention_projector_state WHERE projector='builtin';")"
sqlite3 "$DB" "INSERT INTO events(task_id,task_item_id,event_type,payload_json,created_at)
VALUES('$TASK_ID',NULL,'agent_question','{\"step_id\":\"vis-gap\"}',datetime('now'));"

CURSOR_AFTER="$CURSOR_BEFORE"
for _ in {1..30}; do
  CURSOR_AFTER="$(sqlite3 "$DB" "SELECT last_event_id FROM attention_projector_state WHERE projector='builtin';")"
  [[ "$CURSOR_AFTER" -gt "$CURSOR_BEFORE" ]] && break
  sleep 0.25
done
QUESTION_ROWS="$(sqlite3 "$DB" \
  "SELECT COUNT(*) FROM attention_items WHERE task_id='$TASK_ID' AND kind='agent_question';")"
if [[ "$CURSOR_AFTER" -gt "$CURSOR_BEFORE" ]] && [[ "$QUESTION_ROWS" == "0" ]]; then
  pass "disabled inbox advances the cursor without materializing the event"
else
  fail "disabled-inbox semantics broken (cursor $CURSOR_BEFORE->$CURSOR_AFTER, rows $QUESTION_ROWS)"
fi

runtime_policy true

GAP_COUNT=0
for _ in {1..30}; do
  "$ORCH" attention list --project "$PROJECT" --kind inbox_projection_gap -o json > "$INBOX"
  GAP_COUNT="$(jq '.items | length' "$INBOX")"
  [[ "$GAP_COUNT" -ge 1 ]] && break
  sleep 0.25
done
if [[ "$GAP_COUNT" -eq 1 ]] &&
  jq -e '.items[0].state == "open" and (.items[0].summary | test("[1-9][0-9]* task events"))' "$INBOX" >/dev/null; then
  pass "re-enabling surfaces one open inbox_projection_gap item with the dropped count"
else
  fail "expected one open inbox_projection_gap item with a nonzero count, got $GAP_COUNT"
fi
GAP_ROWS="$(sqlite3 "$DB" "SELECT COUNT(*) FROM attention_projection_gaps;")"
if [[ "$GAP_ROWS" == "0" ]]; then
  pass "gap accounting row is cleared by the flush"
else
  fail "gap accounting row survived the flush ($GAP_ROWS rows)"
fi

echo ""
echo "--- Scenario 4: redaction sweep ---"

"$ORCH" attention list --project "$PROJECT" -o json > "$INBOX"
if jq -e '.items | all((.summary + .title | test("secret-value|token=|stdout|stderr|wrong-secret"; "i")) | not)' "$INBOX" >/dev/null; then
  pass "no inbox summary or title carries payload or secret content"
else
  fail "an inbox summary or title carries unsafe raw text"
fi

echo ""
echo "Failure Visibility QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
