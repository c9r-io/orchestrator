#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_daemon.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
GRPC_BIND="${GRPC_BIND:-127.0.0.1:19313}"
WEBHOOK_BIND="${WEBHOOK_BIND:-127.0.0.1:19314}"
FAKE_SLACK_BIND="${FAKE_SLACK_BIND:-127.0.0.1:19315}"
# The pin is "the previous release", not a fixed point in history: the 0.3.1-era
# pin outlived its schema-compatibility window (an old daemon against a
# schema-37 database answered the disabled-fixture webhook with 500), and the
# rule recorded in QA 161 is to advance it to the prior release commit at each
# release, qualified by running this gate.
PREVIOUS_REF="${FR113_PREVIOUS_REF:-58166a9f6172fa2ea77ea36677ed0db94184beba}"
PROJECT="qa-slack-skill-release"
FIXTURE="$REPO_ROOT/fixtures/manifests/bundles/slack-skill-automation-release-fixture.yaml"
SIGNING_SECRET="qa-slack-release-signing-secret"
VALID_TOKEN="qa-slack-release-valid-token"
INVALID_TOKEN="qa-slack-release-invalid-token"
PASS=0
DAEMON_PID=""
SLACK_PID=""
PREVIOUS_TREE=""

pass() {
  echo "  PASS: $1"
  PASS=$((PASS + 1))
}

fail() {
  echo "  FAIL: $1" >&2
  exit 1
}

for command in cargo curl git jq mktemp openssl python3 rg sqlite3; do
  command -v "$command" >/dev/null 2>&1 || fail "missing required command: $command"
done

if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  fail "fresh debug binaries are required; build orchestratord and orchestrator-cli first"
fi

QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
HOST_HOME="$HOME"
HOST_CARGO_HOME="${CARGO_HOME:-$HOST_HOME/.cargo}"
HOST_RUSTUP_HOME="${RUSTUP_HOME:-$HOST_HOME/.rustup}"
DB="$QA_ROOT/runtime/agent_orchestrator.db"
# A whole second cargo target tree, so the previous release builds without
# invalidating the working tree's. It is a **cache, not scratch**: cleanup() must
# never remove it, because rebuilding it costs the whole previous release.
# Measured 2026-08-12: 7.9 GB for one ref, inside a 100 GB target/. It is
# invisible to `git status` (.gitignore ignores target/), which is how a tree this
# size went unremarked while a 41 MB $TMPDIR root was noticed.
#
# One tree per distinct PREVIOUS_REF, and nothing used to prune them: the count
# grows by one per release the pin is advanced to, at ~8 GB each, forever. So keep
# the tree this run needs and the newest other one — the newest is kept because
# advancing the pin should not throw away the tree the previous pin built, which
# is the one a bisect would come back to — and remove the rest.
PREVIOUS_TARGET="$REPO_ROOT/target/fr113-previous-${PREVIOUS_REF:0:12}"
prune_previous_targets() {
  local keep_newest="" candidate
  # -mindepth/-maxdepth 1 so this can only ever see the siblings it created.
  # Newest is found with `[[ -nt ]]` rather than `stat`, whose format flag differs
  # between BSD and GNU, and rather than parsing `ls -t`, whose output cannot be
  # split safely when $REPO_ROOT contains a space.
  while IFS= read -r candidate; do
    if [[ -n "$candidate" && ( -z "$keep_newest" || "$candidate" -nt "$keep_newest" ) ]]; then
      keep_newest="$candidate"
    fi
  done < <(find "$REPO_ROOT/target" -mindepth 1 -maxdepth 1 -type d \
    -name 'fr113-previous-*' 2>/dev/null)
  while IFS= read -r candidate; do
    if [[ -n "$candidate" && "$candidate" != "$PREVIOUS_TARGET" && "$candidate" != "$keep_newest" ]]; then
      echo "  pruning stale previous-release target tree: $candidate" >&2
      rm -rf "$candidate"
    fi
  done < <(find "$REPO_ROOT/target" -mindepth 1 -maxdepth 1 -type d \
    -name 'fr113-previous-*' 2>/dev/null)
  # An `if` whose condition is false is the loop's last status, so without this
  # the function returns 1 on a tree that needs no pruning and `set -e` takes the
  # gate down before it has asserted anything.
  return 0
}
prune_previous_targets

cleanup() {
  gate_daemon_stop "$DAEMON_PID" || true
  DAEMON_PID=""
  if [[ -n "$SLACK_PID" ]]; then
    kill "$SLACK_PID" 2>/dev/null || true
    wait "$SLACK_PID" 2>/dev/null || true
  fi
  if [[ -n "$PREVIOUS_TREE" && -d "$PREVIOUS_TREE" ]]; then
    git -C "$REPO_ROOT" worktree remove --force "$PREVIOUS_TREE" >/dev/null 2>&1 || true
  fi
  # gate_scratch_has_evidence: a retained root that holds nothing is
  # indistinguishable from a gate that ran and produced no findings. Either root
  # holding something retains both, because both are removed below.
  if [[ "${KEEP_QA:-0}" == "1" ]] &&
    { gate_scratch_has_evidence "$QA_ROOT" || gate_scratch_has_evidence "$QA_HOME"; }; then
    echo "FR-113 vertical logs retained at: $QA_ROOT" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-slack-skill-automation-vertical.sh"

export HOME="$QA_HOME"
export CARGO_HOME="$HOST_CARGO_HOME"
export RUSTUP_HOME="$HOST_RUSTUP_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/runtime"
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
export ORCHESTRATOR_SLACK_API_BASE_URL="http://$FAKE_SLACK_BIND/api/"
unset ORCHESTRATOR_SOCKET
mkdir -p "$QA_ROOT/docs/qa/orchestrator" "$QA_ROOT/docs/ticket"

python3 -u - "$FAKE_SLACK_BIND" "$QA_ROOT/fake-slack.log" "$VALID_TOKEN" <<'PY' &
import hashlib
import json
import sys
from collections import defaultdict
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

host, port = sys.argv[1].rsplit(":", 1)
log_path = sys.argv[2]
valid_token = sys.argv[3]
requests = defaultdict(int)

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)
        query = parse_qs(parsed.query)
        channel = query.get("channel", [""])[0]
        message_ts = query.get("message_ts", [""])[0]
        key = f"{channel}:{message_ts}"
        requests[key] += 1
        authorized = self.headers.get("Authorization") == f"Bearer {valid_token}"
        status = 200
        headers = {}
        if parsed.path != "/api/chat.getPermalink" or not authorized:
            payload = {"ok": False, "error": "invalid_auth"}
            outcome = "invalid_auth"
        elif message_ts.endswith("000300") and requests[key] == 1:
            status = 429
            headers["Retry-After"] = "2"
            payload = {"ok": False, "error": "ratelimited"}
            outcome = "rate_limited"
        else:
            compact_ts = message_ts.replace(".", "")
            payload = {
                "ok": True,
                "permalink": f"https://qa-release-workspace.slack.com/archives/{channel}/p{compact_ts}",
            }
            outcome = "ok"
        with open(log_path, "a", encoding="utf-8") as log:
            log.write(json.dumps({
                "coordinate_hash": hashlib.sha256(key.encode("utf-8")).hexdigest()[:12],
                "outcome": outcome,
                "attempt": requests[key],
            }) + "\n")
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        for name, value in headers.items():
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass

ThreadingHTTPServer((host, int(port)), Handler).serve_forever()
PY
SLACK_PID=$!

for _ in {1..50}; do
  if curl -sS "http://$FAKE_SLACK_BIND/api/chat.getPermalink?channel=health&message_ts=0" \
      >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done
kill -0 "$SLACK_PID" 2>/dev/null || fail "fake Slack API failed to start"

start_daemon() {
  local daemon_bin="${1:-$ORCHD}"
  local cli_bin="${2:-$ORCH}"
  local log_name="${3:-daemon-current.log}"
  (
    cd "$QA_ROOT"
    "$daemon_bin" --foreground --bind "$GRPC_BIND" --webhook-bind "$WEBHOOK_BIND" \
      --workers 1 --uds-max-role admin > "$log_name" 2>&1 &
    echo $! > daemon.pid
  )
  DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"
  gate_daemon_wait_ready "$cli_bin" && return 0
  sed -n '1,200p' "$QA_ROOT/$log_name" >&2
  fail "isolated daemon failed readiness"
}

stop_daemon() {
  # Restart site: the daemons share ORCHESTRATORD_DATA_DIR, so wait for the
  # daemon's own pidfile to be released before the next start races it.
  local rc=0
  gate_daemon_stop "$DAEMON_PID" "$ORCHESTRATORD_DATA_DIR/daemon.pid" || rc=$?
  DAEMON_PID=""
  return "$rc"
}

wait_for_active_key() {
  for _ in {1..80}; do
    "$ORCH" secret key status -o json 2>/dev/null | jq -e '.active_key != null' >/dev/null && return 0
    sleep 0.25
  done
  return 1
}

wait_for_event_state() {
  local external_event_id="$1"
  local expected="$2"
  for _ in {1..160}; do
    local state
    state="$(sqlite3 "$DB" "SELECT routing_state FROM source_events WHERE external_event_id='$external_event_id';")"
    [[ "$state" == "$expected" ]] && return 0
    sleep 0.25
  done
  return 1
}

route_id_for_event() {
  local external_event_id="$1"
  sqlite3 "$DB" \
    "SELECT r.id FROM source_automation_routes r JOIN source_events e ON e.id=r.source_event_id WHERE e.external_event_id='$external_event_id' LIMIT 1;"
}

wait_for_route_state() {
  local route_id="$1"
  local expected="$2"
  for _ in {1..200}; do
    local state
    state="$(sqlite3 "$DB" "SELECT status FROM source_automation_routes WHERE id='$route_id';")"
    [[ "$state" == "$expected" ]] && return 0
    sleep 0.25
  done
  return 1
}

wait_for_route_for_event() {
  local external_event_id="$1"
  for _ in {1..120}; do
    local route_id
    route_id="$(route_id_for_event "$external_event_id")"
    [[ -n "$route_id" ]] && {
      printf '%s' "$route_id"
      return 0
    }
    sleep 0.25
  done
  return 1
}

wait_for_task_completed() {
  local task_id="$1"
  for _ in {1..200}; do
    local state
    state="$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='$task_id';")"
    [[ "$state" == "completed" ]] && return 0
    [[ "$state" == "failed" ]] && return 1
    sleep 0.25
  done
  return 1
}

slack_signature() {
  local timestamp="$1"
  local body="$2"
  printf 'v0:%s:%s' "$timestamp" "$body" | \
    openssl dgst -sha256 -hmac "$SIGNING_SECRET" | awk '{print "v0="$NF}'
}

post_reaction() {
  local external_event_id="$1"
  local reaction="$2"
  local message_ts="$3"
  local output_file="$4"
  local timestamp body signature code
  timestamp="$(date +%s)"
  body="{\"type\":\"event_callback\",\"event_id\":\"$external_event_id\",\"event\":{\"type\":\"reaction_added\",\"user\":\"U_OPERATOR\",\"reaction\":\"$reaction\",\"item\":{\"type\":\"message\",\"channel\":\"C_QA_RELEASE\",\"ts\":\"$message_ts\"},\"event_ts\":\"$timestamp.000900\"}}"
  signature="$(slack_signature "$timestamp" "$body")"
  code="$(curl -sS -o "$output_file.body" -w '%{http_code}' -X POST \
    "http://$WEBHOOK_BIND/source/slack/$PROJECT/slack-release" \
    -H 'Content-Type: application/json' \
    -H "X-Slack-Request-Timestamp: $timestamp" \
    -H "X-Slack-Signature: $signature" \
    --data-binary "$body")"
  printf '%s' "$code" > "$output_file.code"
}

task_id_for_route() {
  sqlite3 "$DB" "SELECT task_id FROM source_automation_routes WHERE id='$1';"
}

start_daemon
wait_for_active_key || fail "daemon did not create an active encryption key"
"$ORCH" apply --project "$PROJECT" -f "$FIXTURE" >/dev/null

NOW="$(date +%s)"
IMPLEMENT_TS="$NOW.000100"
# Deliberately use the same message for both badges. Distinct reviewed bindings
# must create distinct tasks without weakening message/reaction deduplication.
DOCS_TS="$IMPLEMENT_TS"
post_reaction Ev-release-implement agent-implement "$IMPLEMENT_TS" "$QA_ROOT/implement"
post_reaction Ev-release-docs agent-docs "$DOCS_TS" "$QA_ROOT/docs"
if [[ "$(<"$QA_ROOT/implement.code")" == "200" && "$(<"$QA_ROOT/docs.code")" == "200" ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_events WHERE external_event_id IN ('Ev-release-implement','Ev-release-docs');")" -eq 2 ]]; then
  pass "signed Slack deliveries are persisted before provider acknowledgement completes"
else
  fail "signed Slack deliveries were not durably acknowledged"
fi

wait_for_event_state Ev-release-implement routed || fail "implement badge did not route"
wait_for_event_state Ev-release-docs routed || fail "docs badge did not route"
IMPLEMENT_ROUTE="$(wait_for_route_for_event Ev-release-implement)"
DOCS_ROUTE="$(wait_for_route_for_event Ev-release-docs)"
IMPLEMENT_TASK="$(task_id_for_route "$IMPLEMENT_ROUTE")"
DOCS_TASK="$(task_id_for_route "$DOCS_ROUTE")"
wait_for_task_completed "$IMPLEMENT_TASK" || fail "implement workflow task did not complete"
wait_for_task_completed "$DOCS_TASK" || fail "docs workflow task did not complete"
IMPLEMENT_URL="https://qa-release-workspace.slack.com/archives/C_QA_RELEASE/p${IMPLEMENT_TS//./}"
DOCS_URL="https://qa-release-workspace.slack.com/archives/C_QA_RELEASE/p${DOCS_TS//./}"
if [[ "$IMPLEMENT_ROUTE" != "$DOCS_ROUTE" && "$IMPLEMENT_TASK" != "$DOCS_TASK" ]] &&
  [[ "$(sqlite3 "$DB" "SELECT workflow_id FROM tasks WHERE id='$IMPLEMENT_TASK';")" == "slack-release-implement" ]] &&
  [[ "$(sqlite3 "$DB" "SELECT workflow_id FROM tasks WHERE id='$DOCS_TASK';")" == "slack-release-docs" ]] &&
  [[ "$(sqlite3 "$DB" "SELECT goal FROM tasks WHERE id='$IMPLEMENT_TASK';")" == "\$ticket-fix $IMPLEMENT_URL" ]] &&
  [[ "$(sqlite3 "$DB" "SELECT goal FROM tasks WHERE id='$DOCS_TASK';")" == "\$qa-doc-gen $DOCS_URL" ]]; then
  pass "two badges on one message select distinct Skill, template, workflow, route, and completed task results"
else
  fail "two-badge Skill/workflow routing differs"
fi

if [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_routing_attempts WHERE automation_route_id IN ('$IMPLEMENT_ROUTE','$DOCS_ROUTE') AND task_id IS NOT NULL;")" -eq 2 ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_bindings WHERE task_id IN ('$IMPLEMENT_TASK','$DOCS_TASK') AND binding_type='automation';")" -eq 2 ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM control_action_audit WHERE result_id IN ('$IMPLEMENT_TASK','$DOCS_TASK') AND action='source.automation.create_task' AND status='succeeded';")" -eq 2 ]] &&
  ! sqlite3 "$DB" "SELECT normalized_payload_json,binding_snapshot_json,template_snapshot_json FROM source_events LEFT JOIN source_automation_routes ON source_events.id=source_automation_routes.source_event_id;" | rg -F "$VALID_TOKEN" >/dev/null; then
  pass "source, route, task, binding, and audit retain one credential-free provenance chain"
else
  fail "release provenance or credential boundary differs"
fi

DUPLICATE_TS="$NOW.000250"
POST_PIDS=()
for index in 1 2 3 4; do
  post_reaction "Ev-release-duplicate-$index" agent-implement "$DUPLICATE_TS" "$QA_ROOT/duplicate-$index" &
  POST_PIDS+=("$!")
done
for post_pid in ${POST_PIDS[@]+"${POST_PIDS[@]}"}; do
  wait "$post_pid" || fail "concurrent duplicate delivery process failed"
done
for index in 1 2 3 4; do
  [[ "$(<"$QA_ROOT/duplicate-$index.code")" == "200" ]] || fail "concurrent duplicate delivery $index was not acknowledged"
done
wait_for_event_state Ev-release-duplicate-1 routed || fail "concurrent duplicate identity did not converge"
DUPLICATE_ROUTE="$(sqlite3 "$DB" "SELECT id FROM source_automation_routes WHERE message_ts='$DUPLICATE_TS' AND reaction='agent-implement' LIMIT 1;")"
[[ -n "$DUPLICATE_ROUTE" ]] || fail "concurrent duplicate identity did not reserve a route"
DUPLICATE_TASK="$(task_id_for_route "$DUPLICATE_ROUTE")"
wait_for_task_completed "$DUPLICATE_TASK" || fail "deduplicated task did not complete"
if [[ "$(sqlite3 "$DB" "SELECT COUNT(DISTINCT id) FROM source_automation_routes WHERE message_ts='$DUPLICATE_TS' AND reaction='agent-implement';")" -eq 1 ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(DISTINCT task_id) FROM source_automation_routes WHERE message_ts='$DUPLICATE_TS' AND reaction='agent-implement';")" -eq 1 ]]; then
  pass "concurrent deliveries converge to one message/badge/binding route and task identity"
else
  fail "concurrent delivery created duplicate route or task state"
fi

RATE_TS="$NOW.000300"
post_reaction Ev-release-rate-limit agent-implement "$RATE_TS" "$QA_ROOT/rate-limit"
[[ "$(<"$QA_ROOT/rate-limit.code")" == "200" ]] || fail "rate-limit fixture delivery was not acknowledged"
RATE_ROUTE="$(wait_for_route_for_event Ev-release-rate-limit)"
wait_for_route_state "$RATE_ROUTE" retrying || fail "429 response did not create a durable retry checkpoint"
stop_daemon
start_daemon "$ORCHD" "$ORCH" daemon-after-rate-restart.log
wait_for_route_state "$RATE_ROUTE" routed || fail "retry checkpoint did not converge after daemon restart"
RATE_TASK="$(task_id_for_route "$RATE_ROUTE")"
wait_for_task_completed "$RATE_TASK" || fail "restarted retry task did not complete"
if [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_automation_route_attempts WHERE route_id='$RATE_ROUTE';")" -ge 2 ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE id='$RATE_TASK';")" -eq 1 ]]; then
  pass "Slack rate limit and daemon restart resume from one durable route checkpoint"
else
  fail "rate-limit restart evidence differs"
fi

sed "s/$VALID_TOKEN/$INVALID_TOKEN/" "$FIXTURE" > "$QA_ROOT/invalid-credential.yaml"
"$ORCH" apply --project "$PROJECT" -f "$QA_ROOT/invalid-credential.yaml" >/dev/null
INVALID_TS="$NOW.000400"
post_reaction Ev-release-invalid-credential agent-docs "$INVALID_TS" "$QA_ROOT/invalid-credential"
[[ "$(<"$QA_ROOT/invalid-credential.code")" == "200" ]] || fail "invalid-credential fixture delivery was not acknowledged"
INVALID_ROUTE="$(wait_for_route_for_event Ev-release-invalid-credential)"
wait_for_route_state "$INVALID_ROUTE" needs_attention || fail "invalid credential did not enter Attention"
INVALID_VERSION="$(sqlite3 "$DB" "SELECT version FROM source_automation_routes WHERE id='$INVALID_ROUTE';")"
if "$ORCH" attention list --project "$PROJECT" --state open -o json > "$QA_ROOT/attention-open.json" &&
  jq -e '.items | length > 0' "$QA_ROOT/attention-open.json" >/dev/null &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM attention_items WHERE source_route_id='$INVALID_ROUTE' AND state='open';")" -eq 1 ]]; then
  pass "permanent provider failure is actionable through Attention"
else
  fail "route Attention projection is missing"
fi

"$ORCH" apply --project "$PROJECT" -f "$FIXTURE" >/dev/null
"$ORCH" source template preview docs-from-slack --project "$PROJECT" --provider slack \
  --installation T_QA_RELEASE --message-url "https://qa-release-workspace.slack.com/archives/C_QA_RELEASE/p0000000000400" \
  --reaction agent-docs --target-id "C_QA_RELEASE:$INVALID_TS" -o json > "$QA_ROOT/preview.json"
"$ORCH" source binding simulate --project "$PROJECT" --installation T_QA_RELEASE \
  --reaction agent-docs --channel C_QA_RELEASE --actor U_OPERATOR -o json > "$QA_ROOT/simulate.json"
if jq -e '.goal | startswith("$qa-doc-gen ")' "$QA_ROOT/preview.json" >/dev/null &&
  jq -e '.status == "matched" and .binding_id == "slack-docs" and .template_ref == "docs-from-slack"' "$QA_ROOT/simulate.json" >/dev/null; then
  pass "credential fix is previewed and simulated without provider or task mutation"
else
  fail "preview or simulation after credential repair differs"
fi

"$ORCH" source automation replay "$INVALID_ROUTE" --expected-version "$INVALID_VERSION" \
  --reason "FR-113 credential rotation verified" --idempotency-key fr113-invalid-auth-replay \
  --adopt-current-config -o json > "$QA_ROOT/replay.json"
wait_for_route_state "$INVALID_ROUTE" routed || fail "reviewed credential replay did not route"
INVALID_TASK="$(task_id_for_route "$INVALID_ROUTE")"
wait_for_task_completed "$INVALID_TASK" || fail "replayed credential task did not complete"
if "$ORCH" attention list --project "$PROJECT" --state resolved -o json > "$QA_ROOT/attention-resolved.json" &&
  jq -e '.items | length > 0' "$QA_ROOT/attention-resolved.json" >/dev/null &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM attention_items WHERE source_route_id='$INVALID_ROUTE' AND state='resolved';")" -eq 1 ]] &&
  [[ "$(sqlite3 "$DB" "SELECT generation FROM source_automation_routes WHERE id='$INVALID_ROUTE';")" -eq 2 ]]; then
  pass "reviewed replay adopts current configuration, creates one task, and resolves Attention"
else
  fail "replay generation or Attention resolution differs"
fi

FR113_LIVE_E2E=1 FR113_PROJECT="$PROJECT" FR113_ROUTE_ID="$IMPLEMENT_ROUTE" \
  FR113_TASK_ID="$IMPLEMENT_TASK" \
  cargo test -p orchestrator-gui live_slack_skill_release_crosses_tauri_provenance_boundary \
    -- --nocapture 2>&1 | tee "$QA_ROOT/tauri-bridge.log"
rg -q 'FR113_TAURI_OK=1' "$QA_ROOT/tauri-bridge.log" || fail "real Tauri provenance bridge did not pass"
pass "production Tauri commands expose role-safe route and Process Workspace provenance"

if rg -F "$SIGNING_SECRET" "$QA_ROOT"/*.log >/dev/null 2>&1 ||
  rg -F "$VALID_TOKEN" "$QA_ROOT"/*.log >/dev/null 2>&1 ||
  rg -F "$INVALID_TOKEN" "$QA_ROOT"/*.log >/dev/null 2>&1 ||
  rg -F 'qa-release-workspace.slack.com' "$QA_ROOT"/*.log >/dev/null 2>&1; then
  fail "release diagnostics contain a secret, credential, or private message URL"
fi
pass "retained release diagnostics exclude secrets, credentials, URLs, goals, and raw payloads"

sed 's/reactionRouting: bindings/reactionRouting: disabled/' "$FIXTURE" \
  > "$QA_ROOT/automation-disabled.yaml"
"$ORCH" apply --project "$PROJECT" -f "$QA_ROOT/automation-disabled.yaml" >/dev/null
sqlite3 "$DB" ".backup '$QA_ROOT/pre-rollback.backup'"
[[ "$(sqlite3 "$QA_ROOT/pre-rollback.backup" 'PRAGMA quick_check;')" == "ok" ]] || fail "rollback backup failed integrity check"
TASKS_BEFORE="$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE project_id='$PROJECT';")"
ROUTES_BEFORE="$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_automation_routes WHERE project_id='$PROJECT';")"
SOURCES_BEFORE="$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_events WHERE project_id='$PROJECT';")"
stop_daemon

PREVIOUS_TREE="$QA_ROOT/previous-compatible"
git -C "$REPO_ROOT" cat-file -e "$PREVIOUS_REF^{commit}" || fail "previous compatible ref does not exist"
git -C "$REPO_ROOT" worktree add --detach "$PREVIOUS_TREE" "$PREVIOUS_REF" >/dev/null
CARGO_TARGET_DIR="$PREVIOUS_TARGET" cargo build --manifest-path "$PREVIOUS_TREE/Cargo.toml" \
  -p orchestratord -p orchestrator-cli >/dev/null
PREVIOUS_ORCHD="$PREVIOUS_TARGET/debug/orchestratord"
PREVIOUS_ORCH="$PREVIOUS_TARGET/debug/orchestrator"
start_daemon "$PREVIOUS_ORCHD" "$PREVIOUS_ORCH" daemon-previous-compatible.log
"$PREVIOUS_ORCH" task list --project "$PROJECT" -o json > "$QA_ROOT/previous-tasks.json"
"$PREVIOUS_ORCH" source automation list --project "$PROJECT" -o json > "$QA_ROOT/previous-routes.json"
ROLLBACK_TS="$(date +%s).000500"
post_reaction Ev-release-rollback-disabled agent-implement "$ROLLBACK_TS" "$QA_ROOT/rollback-disabled"
[[ "$(<"$QA_ROOT/rollback-disabled.code")" == "200" ]] || fail "previous daemon did not acknowledge disabled fixture"
wait_for_event_state Ev-release-rollback-disabled ignored || fail "previous daemon did not honor disabled reaction writer"
if [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE project_id='$PROJECT';")" -eq "$TASKS_BEFORE" ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_automation_routes WHERE project_id='$PROJECT';")" -eq "$ROUTES_BEFORE" ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_events WHERE project_id='$PROJECT';")" -eq $((SOURCES_BEFORE + 1)) ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM schema_migrations WHERE version IN (33,34);")" -eq 2 ]]; then
  pass "compatible previous daemon preserves additive schema-34 data and keeps automation writers stopped"
else
  fail "binary rollback changed retained routes/tasks or created new automation work"
fi
stop_daemon
start_daemon "$ORCHD" "$ORCH" daemon-current-after-rollback.log
if [[ "$(sqlite3 "$DB" 'PRAGMA quick_check;')" == "ok" ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE project_id='$PROJECT';")" -eq "$TASKS_BEFORE" ]] &&
  [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_automation_routes WHERE project_id='$PROJECT';")" -eq "$ROUTES_BEFORE" ]]; then
  pass "forward recovery reopens the rollback database without deleting created tasks or evidence"
else
  fail "forward recovery did not preserve rollback state"
fi

echo ""
echo "Slack Skill automation vertical QA passed: $PASS gates"
