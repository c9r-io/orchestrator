#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
GRPC_BIND="${GRPC_BIND:-127.0.0.1:19317}"
WEBHOOK_BIND="${WEBHOOK_BIND:-127.0.0.1:19318}"
FAKE_SLACK_BIND="${FAKE_SLACK_BIND:-127.0.0.1:19319}"
FIXTURE="$REPO_ROOT/fixtures/manifests/bundles/non-code-workspace-fixture.yaml"
QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
DAEMON_PID=""
SLACK_PID=""
PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then kill "$DAEMON_PID" 2>/dev/null || true; wait "$DAEMON_PID" 2>/dev/null || true; fi
  if [[ -n "$SLACK_PID" ]]; then kill "$SLACK_PID" 2>/dev/null || true; wait "$SLACK_PID" 2>/dev/null || true; fi
  if [[ "$FAIL" -gt 0 || "${KEEP_FR117_QA:-0}" == "1" ]]; then
    echo "FR-117 QA retained at QA_ROOT=$QA_ROOT QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-non-code-workspace.sh"

for command in curl jq mktemp openssl python3 sqlite3; do
  command -v "$command" >/dev/null 2>&1 || { echo "missing required command: $command" >&2; exit 1; }
done
if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "debug binaries not found; run: cargo build -p orchestratord -p orchestrator-cli" >&2
  exit 1
fi

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/runtime"
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
export ORCHESTRATOR_SLACK_API_BASE_URL="http://$FAKE_SLACK_BIND/api/"
unset ORCHESTRATOR_SOCKET
mkdir -p "$ORCHESTRATORD_DATA_DIR" "$QA_ROOT/global-skills/warehouse" "$QA_ROOT/workspace/evidence"
WORK_ROOT="$(cd "$QA_ROOT/workspace" && pwd -P)"
printf 'Use inventory evidence and require human approval before promising stock.\n' > "$QA_ROOT/global-skills/warehouse/SKILL.md"
printf 'sku=widget-a available=3\n' > "$QA_ROOT/workspace/inventory.txt"
cat > "$ORCHESTRATORD_DATA_DIR/file-sharing.yaml" <<EOF
fileSharing:
  globalSkills:
    - path: $QA_ROOT/global-skills
  shareableRoots:
    - $QA_ROOT
EOF

python3 -u - "$FAKE_SLACK_BIND" <<'PY' &
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

host, port = sys.argv[1].rsplit(":", 1)
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)
        query = parse_qs(parsed.query)
        channel = query.get("channel", [""])[0]
        message_ts = query.get("message_ts", [""])[0]
        ok = parsed.path == "/api/chat.getPermalink" and self.headers.get("Authorization") == "Bearer qa-non-code-fake-token"
        payload = {"ok": ok}
        if ok:
            payload["permalink"] = f"https://qa-workspace.slack.com/archives/{channel}/p{message_ts.replace('.', '')}"
        else:
            payload["error"] = "invalid_auth"
        body = json.dumps(payload).encode()
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

(
  cd "$QA_ROOT/workspace"
  "$ORCHD" --foreground --bind "$GRPC_BIND" --webhook-bind "$WEBHOOK_BIND" --workers 1 --uds-max-role admin > "$QA_ROOT/daemon.log" 2>&1 &
  echo $! > "$QA_ROOT/daemon.pid"
)
DAEMON_PID="$(<"$QA_ROOT/daemon.pid")"
for _ in {1..80}; do
  "$ORCH" task list -o json >/dev/null 2>&1 && break
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  sed -n '1,240p' "$QA_ROOT/daemon.log" >&2
  exit 1
fi

PROJECT="qa-non-code"
DB="$ORCHESTRATORD_DATA_DIR/agent_orchestrator.db"
"$ORCH" apply --project "$PROJECT" -f "$FIXTURE" >/dev/null
pass "task workspaces, scoped sandbox, Slack route, and global Skill configuration apply"

NOW="$(date +%s)"
MESSAGE_TS="$NOW.000100"
BODY="{\"type\":\"event_callback\",\"event_id\":\"Ev-fr117\",\"event\":{\"type\":\"reaction_added\",\"user\":\"U_OPERATOR\",\"reaction\":\"warehouse-check\",\"item\":{\"type\":\"message\",\"channel\":\"C_WAREHOUSE\",\"ts\":\"$MESSAGE_TS\"},\"event_ts\":\"$NOW.000200\"}}"
SIGNATURE="$(printf 'v0:%s:%s' "$NOW" "$BODY" | openssl dgst -sha256 -hmac 'qa-non-code-signing-secret' | awk '{print "v0="$NF}')"
HTTP_CODE="$(curl -sS -o "$QA_ROOT/webhook-response.txt" -w '%{http_code}' -X POST \
  "http://$WEBHOOK_BIND/source/slack/$PROJECT/slack-warehouse" \
  -H 'Content-Type: application/json' \
  -H "X-Slack-Request-Timestamp: $NOW" \
  -H "X-Slack-Signature: $SIGNATURE" --data-binary "$BODY")"

TASK_ID=""
STATUS=""
for _ in {1..120}; do
  TASK_ID="$(sqlite3 "$DB" "SELECT routed_task_id FROM source_events WHERE external_event_id='Ev-fr117';" 2>/dev/null || true)"
  if [[ -n "$TASK_ID" ]]; then
    STATUS="$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='$TASK_ID';" 2>/dev/null || true)"
    [[ "$STATUS" =~ ^(completed|failed|cancelled)$ ]] && break
  fi
  sleep 0.25
done
if [[ "$HTTP_CODE" == "200" && "$STATUS" == "completed" ]]; then
  pass "signed Slack badge creates and completes a non-code task"
else
  fail "Slack route/task did not complete (http=$HTTP_CODE task=$TASK_ID status=$STATUS)"
fi

PERMALINK="https://qa-workspace.slack.com/archives/C_WAREHOUSE/p${MESSAGE_TS//./}"
EVIDENCE="$QA_ROOT/workspace/evidence/reply-suggestion.txt"
if [[ -f "$EVIDENCE" ]] && rg -q --fixed-strings "$PERMALINK" "$EVIDENCE" && \
   rg -q 'available=3' "$EVIDENCE" && rg -q 'reserve after approval' "$EVIDENCE"; then
  pass "agent receives the Slack message URL, reads inventory, and writes an approval-ready suggestion"
else
  fail "agent evidence is missing Slack, inventory, or reply content"
fi

if rg -q "home=$WORK_ROOT" "$EVIDENCE" && rg -q "xdg_config=$WORK_ROOT/.config" "$EVIDENCE" && \
   rg -q 'skill_read_only=true' "$EVIDENCE" && [[ ! -e "$QA_ROOT/global-skills/warehouse/mutation-denied" ]] && \
   ! rg -q --fixed-strings "$QA_HOME" "$EVIDENCE"; then
  pass "HOME/XDG are redirected and the global Skill remains read-only without host HOME leakage"
else
  fail "HOME isolation or global Skill read-only evidence differs"
fi

ITEM_PATH="$(sqlite3 "$DB" "SELECT qa_file_path FROM task_items WHERE task_id='$TASK_ID';")"
if [[ "$ITEM_PATH" == "__TASK__" ]] && \
   [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='$TASK_ID' AND event_type='driver_finished';")" -ge 1 ]]; then
  pass "single implicit item converges through the driver terminal signal without QA-file finalize"
else
  fail "implicit item or driver convergence evidence is missing"
fi

# Low confidence opens the decision surface even though terminal task cleanup may
# immediately resolve it. Preserve both changes as proof of the human handoff.
for _ in {1..40}; do
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM attention_changes WHERE project_id='$PROJECT' AND attention_item_id IN (SELECT id FROM attention_items WHERE task_id='$TASK_ID');" 2>/dev/null || true)" -ge 1 ]] && break
  sleep 0.25
done
if [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM attention_items WHERE task_id='$TASK_ID' AND kind='low_confidence';")" -eq 1 ]]; then
  pass "low-confidence reply is projected through Attention for human decision"
else
  fail "Attention handoff was not projected"
fi

# FR-146: `| head -1` under pipefail kills rg and ends the gate with no summary line.
EPHEMERAL_IDS="$(cd "$QA_ROOT/workspace" && "$ORCH" task create --project "$PROJECT" --workspace ephemeral-ops --workflow warehouse-reply --name ephemeral --goal cleanup --no-start | rg -o '[0-9a-f-]{36}')"
EPHEMERAL_ID="${EPHEMERAL_IDS%%$'\n'*}"
EPHEMERAL_HOME="$(sqlite3 "$DB" "SELECT workspace_root FROM tasks WHERE id='$EPHEMERAL_ID';")"
MODE="$(stat -f '%Lp' "$EPHEMERAL_HOME" 2>/dev/null || stat -c '%a' "$EPHEMERAL_HOME")"
"$ORCH" task start "$EPHEMERAL_ID" >/dev/null
for _ in {1..80}; do
  EPHEMERAL_STATUS="$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='$EPHEMERAL_ID';")"
  [[ "$EPHEMERAL_STATUS" =~ ^(completed|failed|cancelled)$ ]] && break
  sleep 0.25
done
for _ in {1..40}; do [[ ! -e "$EPHEMERAL_HOME" ]] && break; sleep 0.1; done
if [[ "$MODE" == "700" && "$EPHEMERAL_STATUS" == "completed" && ! -e "$EPHEMERAL_HOME" ]]; then
  pass "omitted work_dir receives a private 0700 HOME that is removed at task completion"
else
  fail "ephemeral HOME lifecycle differs (mode=$MODE status=${EPHEMERAL_STATUS:-unknown} path=$EPHEMERAL_HOME)"
fi

if [[ "$FAIL" -ne 0 ]]; then
  echo "FR-117 QA: $PASS passed, $FAIL failed" >&2
  exit 1
fi
echo "FR-117 QA: $PASS passed, 0 failed"
