#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
GRPC_BIND="${GRPC_BIND:-127.0.0.1:19222}"
WEBHOOK_BIND="${WEBHOOK_BIND:-127.0.0.1:19223}"
FAKE_SLACK_BIND="${FAKE_SLACK_BIND:-127.0.0.1:19224}"
PASS=0
FAIL=0
DAEMON_PID=""
SLACK_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() {
  echo "  FAIL: $1" >&2
  FAIL=$((FAIL + 1))
  exit 1
}

for command in curl jq mktemp openssl python3 sqlite3; do
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
  if [[ -n "$SLACK_PID" ]]; then
    kill "$SLACK_PID" 2>/dev/null || true
    wait "$SLACK_PID" 2>/dev/null || true
  fi
  if [[ "${KEEP_QA:-0}" == "1" ]]; then
    echo "QA_ROOT=$QA_ROOT" >&2
    echo "QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/runtime"
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
export ORCHESTRATOR_SLACK_API_BASE_URL="http://$FAKE_SLACK_BIND/api/"
unset ORCHESTRATOR_SOCKET
mkdir -p "$QA_ROOT/docs/qa/orchestrator" "$QA_ROOT/docs/ticket"

python3 -u - "$FAKE_SLACK_BIND" "$QA_ROOT/slack-requests.log" <<'PY' &
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

host, port = sys.argv[1].rsplit(":", 1)
log_path = sys.argv[2]

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)
        query = parse_qs(parsed.query)
        channel = query.get("channel", [""])[0]
        message_ts = query.get("message_ts", [""])[0]
        authorized = self.headers.get("Authorization") == "Bearer qa-source-routing-fake-token"
        with open(log_path, "a", encoding="utf-8") as log:
            log.write(json.dumps({
                "path": parsed.path,
                "channel": channel,
                "message_ts": message_ts,
                "authorized": authorized,
            }) + "\n")
        if parsed.path != "/api/chat.getPermalink" or not authorized:
            payload = {"ok": False, "error": "invalid_auth"}
        else:
            compact_ts = message_ts.replace(".", "")
            payload = {
                "ok": True,
                "permalink": f"https://qa-workspace.slack.com/archives/{channel}/p{compact_ts}",
            }
        body = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass

ThreadingHTTPServer((host, int(port)), Handler).serve_forever()
PY
SLACK_PID=$!

for _ in {1..40}; do
  if curl -sS "http://$FAKE_SLACK_BIND/api/chat.getPermalink?channel=health&message_ts=0" \
      >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

start_daemon() {
  local max_role="${1:-admin}"
  (
    cd "$QA_ROOT"
    "$ORCHD" --foreground --bind "$GRPC_BIND" --webhook-bind "$WEBHOOK_BIND" --workers 1 \
      --uds-max-role "$max_role" > daemon.log 2>&1 &
    echo $! > daemon.pid
  )
  DAEMON_PID="$(<"$QA_ROOT/daemon.pid")"
  for _ in {1..80}; do
    "$ORCH" task list -o json >/dev/null 2>&1 && return 0
    sleep 0.25
  done
  echo "isolated daemon failed to start" >&2
  sed -n '1,240p' "$QA_ROOT/daemon.log" >&2
  return 1
}

stop_daemon() {
  kill "$DAEMON_PID" 2>/dev/null || true
  wait "$DAEMON_PID" 2>/dev/null || true
  DAEMON_PID=""
}

start_read_only_daemon() {
  (
    cd "$QA_ROOT"
    "$ORCHD" --foreground --webhook-bind "$WEBHOOK_BIND" --workers 1 \
      --uds-max-role read-only > daemon.log 2>&1 &
    echo $! > daemon.pid
  )
  DAEMON_PID="$(<"$QA_ROOT/daemon.pid")"
  for _ in {1..80}; do
    if env -u ORCHESTRATOR_CONTROL_PLANE_CONFIG \
        ORCHESTRATOR_SOCKET="$ORCHESTRATORD_DATA_DIR/orchestrator.sock" \
        "$ORCH" task list -o json >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.25
  done
  echo "isolated read-only UDS daemon failed to start" >&2
  sed -n '1,240p' "$QA_ROOT/daemon.log" >&2
  return 1
}

wait_for_active_key() {
  for _ in {1..60}; do
    if "$ORCH" secret key status -o json 2>/dev/null | jq -e '.active_key != null' >/dev/null; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

wait_for_source_state() {
  local external_event_id="$1"
  local expected="$2"
  for _ in {1..80}; do
    local state
    state="$(sqlite3 "$DB" "SELECT routing_state FROM source_events WHERE external_event_id='$external_event_id';")"
    [[ "$state" == "$expected" ]] && return 0
    sleep 0.25
  done
  return 1
}

slack_signature() {
  local timestamp="$1"
  local body="$2"
  printf 'v0:%s:%s' "$timestamp" "$body" | \
    openssl dgst -sha256 -hmac "$SLACK_SIGNING_SECRET" | awk '{print "v0="$NF}'
}

post_reaction() {
  local external_event_id="$1"
  local timestamp="$2"
  local message_ts="$3"
  local body signature
  body="{\"type\":\"event_callback\",\"event_id\":\"$external_event_id\",\"event\":{\"type\":\"reaction_added\",\"user\":\"U_OPERATOR\",\"reaction\":\"agent-implement\",\"item\":{\"type\":\"message\",\"channel\":\"C_QA_ROUTING\",\"ts\":\"$message_ts\"},\"event_ts\":\"$timestamp.000200\"}}"
  signature="$(slack_signature "$timestamp" "$body")"
  curl -sS -o "$QA_ROOT/$external_event_id-response.txt" -w '%{http_code}' -X POST \
    "http://$WEBHOOK_BIND/source/slack/$PROJECT/slack-routing" \
    -H 'Content-Type: application/json' \
    -H "X-Slack-Request-Timestamp: $timestamp" \
    -H "X-Slack-Signature: $signature" \
    --data-binary "$body"
}

PROJECT="qa-source-routing"
SLACK_SIGNING_SECRET="qa-source-routing-signing-secret"
FIXTURE="$REPO_ROOT/fixtures/manifests/bundles/source-task-routing-fixture.yaml"
DB="$QA_ROOT/runtime/agent_orchestrator.db"

start_daemon admin
wait_for_active_key
"$ORCH" apply --project "$PROJECT" -f "$FIXTURE" >/dev/null

NOW="$(date +%s)"
MESSAGE_TS="$NOW.000100"
FIRST_CODE="$(post_reaction Ev-route-first "$NOW" "$MESSAGE_TS")"
if [[ "$FIRST_CODE" == "200" ]] && wait_for_source_state Ev-route-first routed; then
  pass "signed Slack reaction is durably acknowledged and routed asynchronously"
else
  fail "first signed Slack route did not complete (HTTP $FIRST_CODE)"
fi

"$ORCH" source list --project "$PROJECT" -o json > "$QA_ROOT/source-list.json"
SOURCE_ID="$(jq -r '.[] | select(.external_event_id == "Ev-route-first") | .id' "$QA_ROOT/source-list.json")"
"$ORCH" source get "$SOURCE_ID" -o json > "$QA_ROOT/source-get.json"
"$ORCH" source route "$SOURCE_ID" -o json > "$QA_ROOT/route.json"
TASK_ID="$(jq -r '.task_id' "$QA_ROOT/route.json")"
PERMALINK="https://qa-workspace.slack.com/archives/C_QA_ROUTING/p${MESSAGE_TS//./}"

if jq -e --arg task "$TASK_ID" '
    .routing_state == "routed" and
    .routed_task_id == $task and
    .automation_status == "routed" and
    .automation_binding_name == "slack-implement" and
    .automation_template_name == "implement-from-slack" and
    (.automation_template_hash | test("^[0-9a-f]{64}$"))
  ' "$QA_ROOT/source-get.json" >/dev/null &&
  jq -e --arg task "$TASK_ID" --arg permalink "$PERMALINK" '
    .status == "routed" and .task_id == $task and .permalink == $permalink and
    .binding_name == "slack-implement" and .template_name == "implement-from-slack" and
    (.binding_revision | test("^[0-9a-f]{64}$")) and
    (.request_id | length > 0)
  ' "$QA_ROOT/route.json" >/dev/null &&
  [[ "$(sqlite3 "$DB" "SELECT goal FROM tasks WHERE id='$TASK_ID';")" == \
      "\$docs: inspect $PERMALINK" ]]; then
  pass "safe source summary, protected route, and canonical task goal agree"
else
  fail "source/route/task public projections differ"
fi

ROUTE_ID="$(jq -r '.id' "$QA_ROOT/route.json")"
REQUEST_ID="$(jq -r '.request_id' "$QA_ROOT/route.json")"
if [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_automation_routes WHERE id='$ROUTE_ID' AND task_id='$TASK_ID' AND request_id='$REQUEST_ID' AND status='routed';")" -eq 1 ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_routing_attempts WHERE source_event_id='$SOURCE_ID' AND automation_route_id='$ROUTE_ID' AND task_id='$TASK_ID';")" -eq 1 ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_bindings WHERE task_id='$TASK_ID' AND binding_type='automation';")" -eq 1 ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM control_action_audit WHERE request_id='$REQUEST_ID' AND action='source.automation.create_task' AND status='succeeded' AND result_id='$TASK_ID';")" -eq 1 ]] &&
  ! sqlite3 "$DB" "SELECT binding_snapshot_json,template_snapshot_json,credential_store,credential_key FROM source_automation_routes WHERE id='$ROUTE_ID';" | grep -q 'qa-source-routing-fake-token'; then
  pass "event, attempt, route, binding, audit, and task form one token-free provenance chain"
else
  fail "durable provenance chain or credential redaction differs"
fi

stop_daemon
start_daemon admin
SECOND_NOW="$(date +%s)"
SECOND_CODE="$(post_reaction Ev-route-after-restart "$SECOND_NOW" "$MESSAGE_TS")"
if [[ "$SECOND_CODE" == "200" ]] && wait_for_source_state Ev-route-after-restart routed; then
  SECOND_SOURCE_ID="$(sqlite3 "$DB" "SELECT id FROM source_events WHERE external_event_id='Ev-route-after-restart';")"
  "$ORCH" source route "$SECOND_SOURCE_ID" -o json > "$QA_ROOT/route-after-restart.json"
  if [[ "$(jq -r '.id' "$QA_ROOT/route-after-restart.json")" == "$ROUTE_ID" ]] &&
    [[ "$(jq -r '.task_id' "$QA_ROOT/route-after-restart.json")" == "$TASK_ID" ]] &&
    [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE project_id='$PROJECT';")" -eq 1 ]] &&
    [[ "$(wc -l < "$QA_ROOT/slack-requests.log" | tr -d ' ')" -eq 2 ]]; then
    pass "different delivery after restart converges on one route, task, and provider lookup"
  else
    fail "restart replay created divergent route/task/provider work"
  fi
else
  fail "restart replay did not complete (HTTP $SECOND_CODE)"
fi

sed 's/reactionRouting: bindings/reactionRouting: disabled/' "$FIXTURE" \
  > "$QA_ROOT/source-routing-disabled.yaml"
"$ORCH" apply --project "$PROJECT" -f "$QA_ROOT/source-routing-disabled.yaml" >/dev/null
DISABLED_NOW="$(date +%s)"
DISABLED_MESSAGE_TS="$((DISABLED_NOW + 1)).000100"
DISABLED_CODE="$(post_reaction Ev-route-disabled "$DISABLED_NOW" "$DISABLED_MESSAGE_TS")"
if [[ "$DISABLED_CODE" == "200" ]] && wait_for_source_state Ev-route-disabled ignored &&
  [[ "$(sqlite3 "$DB" "SELECT last_error_code FROM source_events WHERE external_event_id='Ev-route-disabled';")" == \
      "reaction_routing_not_enabled" ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_automation_routes WHERE project_id='$PROJECT';")" -eq 1 ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE project_id='$PROJECT';")" -eq 1 ]] &&
  [[ "$(wc -l < "$QA_ROOT/slack-requests.log" | tr -d ' ')" -eq 2 ]]; then
  pass "feature disable blocks new provider/task work and preserves existing evidence"
else
  fail "feature-disable compatibility boundary differs"
fi

stop_daemon
start_read_only_daemon
if ! env -u ORCHESTRATOR_CONTROL_PLANE_CONFIG \
    ORCHESTRATOR_SOCKET="$ORCHESTRATORD_DATA_DIR/orchestrator.sock" \
    "$ORCH" source route "$SOURCE_ID" -o json > "$QA_ROOT/read-only-route.log" 2>&1 &&
  grep -qi 'permission denied\|requires role\|UDS policy restricts' "$QA_ROOT/read-only-route.log" &&
  env -u ORCHESTRATOR_CONTROL_PLANE_CONFIG \
    ORCHESTRATOR_SOCKET="$ORCHESTRATORD_DATA_DIR/orchestrator.sock" \
    "$ORCH" source get "$SOURCE_ID" -o json > "$QA_ROOT/read-only-source.json" &&
  ! grep -q 'qa-workspace.slack.com' "$QA_ROOT/read-only-source.json"; then
  pass "read-only users receive safe route metadata but cannot retrieve the permalink"
else
  fail "role-aware permalink boundary differs"
fi

echo ""
echo "Slack reaction task routing QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
