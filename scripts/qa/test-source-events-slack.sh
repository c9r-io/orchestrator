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
GRPC_BIND="${GRPC_BIND:-127.0.0.1:19198}"
WEBHOOK_BIND="${WEBHOOK_BIND:-127.0.0.1:19199}"
PASS=0
FAIL=0
DAEMON_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in curl jq mktemp openssl sqlite3; do
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
  if [[ "${KEEP_QA:-0}" == "1" ]]; then
    echo "QA_ROOT=$QA_ROOT" >&2
    echo "QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-source-events-slack.sh"

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/docs/qa/orchestrator" "$QA_ROOT/docs/ticket"
printf '# Source event QA target\n' > "$QA_ROOT/docs/qa/orchestrator/source.md"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$GRPC_BIND" --webhook-bind "$WEBHOOK_BIND" --workers 1 \
    > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"

for _ in {1..40}; do
  "$ORCH" task list -o json >/dev/null 2>&1 && break
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  echo "isolated daemon failed to start" >&2
  cat "$QA_ROOT/daemon.log" >&2
  exit 1
fi

PROJECT="qa-source-events"
"$ORCH" apply --project "$PROJECT" \
  -f "$REPO_ROOT/fixtures/manifests/bundles/source-events-fixture.yaml" >/dev/null

"$ORCH" source ingest --project "$PROJECT" \
  --file "$REPO_ROOT/fixtures/source-events/generic-message.json" >/dev/null
"$ORCH" source ingest --project "$PROJECT" \
  --file "$REPO_ROOT/fixtures/source-events/generic-message.json" >/dev/null

GENERIC_JSON="$QA_ROOT/generic.json"
for _ in {1..40}; do
  "$ORCH" source list --project "$PROJECT" -o json > "$GENERIC_JSON"
  [[ "$(jq -r 'map(select(.provider == "fixture"))[0].routing_state // ""' "$GENERIC_JSON")" == "routed" ]] && break
  sleep 0.25
done
DB="$QA_ROOT/data/agent_orchestrator.db"
if [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_events WHERE provider='fixture';")" -eq 1 ]] && \
   [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_bindings WHERE provider='fixture';")" -eq 1 ]] && \
   [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE project_id='$PROJECT';")" -eq 1 ]]; then
  pass "non-Slack duplicate fixture creates one event, task, and binding"
else
  fail "non-Slack idempotency or provider-neutral routing counts differ"
fi

SLACK_SECRET="qa-slack-signing-secret"
SLACK_MANIFEST="$QA_ROOT/slack-source.yaml"
cat > "$SLACK_MANIFEST" <<EOF
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: slack-source-secret
spec:
  data:
    signing: $SLACK_SECRET
---
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: slack-source
spec:
  event:
    source: webhook
    webhook:
      provider: slack
      installationId: slack-installation
      timestampToleranceSecs: 300
      actorRoles:
        U-operator: operator
      secret:
        fromRef: slack-source-secret
  action:
    workflow: source-fixture
    workspace: source-fixture
  concurrencyPolicy: Allow
EOF
"$ORCH" apply --project "$PROJECT" -f "$SLACK_MANIFEST" >/dev/null

slack_signature() {
  local timestamp="$1"
  local body="$2"
  printf 'v0:%s:%s' "$timestamp" "$body" | \
    openssl dgst -sha256 -hmac "$SLACK_SECRET" | awk '{print "v0="$NF}'
}

post_slack() {
  local timestamp="$1"
  local body="$2"
  local signature
  signature="$(slack_signature "$timestamp" "$body")"
  curl -sS -o "$QA_ROOT/response.json" -w '%{http_code}' -X POST \
    "http://$WEBHOOK_BIND/source/slack/$PROJECT/slack-source" \
    -H 'Content-Type: application/json' \
    -H "X-Slack-Request-Timestamp: $timestamp" \
    -H "X-Slack-Signature: $signature" \
    --data-binary "$body"
}

NOW="$(date +%s)"
ROOT_TS="$NOW.000001"
ROOT_BODY="{\"type\":\"event_callback\",\"event_id\":\"Ev-root\",\"event\":{\"type\":\"message\",\"user\":\"U-operator\",\"channel\":\"C-source\",\"ts\":\"$ROOT_TS\",\"text\":\"Start source process\"}}"
ROOT_CODE_1="$(post_slack "$NOW" "$ROOT_BODY")"
ROOT_CODE_2="$(post_slack "$NOW" "$ROOT_BODY")"
if [[ "$ROOT_CODE_1" == "200" ]] && [[ "$ROOT_CODE_2" == "200" ]]; then
  pass "valid Slack event is accepted and provider retry is acknowledged"
else
  fail "valid Slack event was not accepted (HTTP $ROOT_CODE_1/$ROOT_CODE_2)"
fi

for _ in {1..40}; do
  STATE="$(sqlite3 "$DB" "SELECT routing_state FROM source_events WHERE external_event_id='Ev-root';")"
  [[ "$STATE" == "routed" ]] && break
  sleep 0.25
done
SLACK_TASK="$(sqlite3 "$DB" "SELECT routed_task_id FROM source_events WHERE external_event_id='Ev-root';")"
if [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_events WHERE external_event_id='Ev-root';")" -eq 1 ]] && \
   [[ -n "$SLACK_TASK" ]]; then
  pass "Slack retry remains one durable source event and one task"
else
  fail "Slack duplicate created extra state or did not route"
fi

REPLY_BODY="{\"type\":\"event_callback\",\"event_id\":\"Ev-reply\",\"event\":{\"type\":\"message\",\"user\":\"U-operator\",\"channel\":\"C-source\",\"ts\":\"$NOW.000002\",\"thread_ts\":\"$ROOT_TS\",\"text\":\"More bounded context\"}}"
REPLY_CODE="$(post_slack "$NOW" "$REPLY_BODY")"
[[ "$REPLY_CODE" == "200" ]] || fail "bound Slack reply was rejected (HTTP $REPLY_CODE)"
for _ in {1..40}; do
  REPLY_TASK="$(sqlite3 "$DB" "SELECT routed_task_id FROM source_events WHERE external_event_id='Ev-reply';")"
  [[ -n "$REPLY_TASK" ]] && break
  sleep 0.25
done
if [[ -n "${REPLY_TASK:-}" && -n "$SLACK_TASK" && "$REPLY_TASK" == "$SLACK_TASK" ]]; then
  pass "bound Slack thread routes to the existing process"
else
  fail "bound Slack reply did not retain task correlation"
fi

UNKNOWN_BODY="{\"type\":\"event_callback\",\"event_id\":\"Ev-unknown-cancel\",\"event\":{\"type\":\"message\",\"user\":\"U-unknown\",\"channel\":\"C-source\",\"ts\":\"$NOW.000003\",\"thread_ts\":\"$ROOT_TS\",\"text\":\"/orchestrator cancel\"}}"
UNKNOWN_CODE="$(post_slack "$NOW" "$UNKNOWN_BODY")"
[[ "$UNKNOWN_CODE" == "200" ]] || fail "unknown actor event was not durably accepted (HTTP $UNKNOWN_CODE)"
for _ in {1..40}; do
  UNKNOWN_STATE="$(sqlite3 "$DB" "SELECT routing_state FROM source_events WHERE external_event_id='Ev-unknown-cancel';")"
  [[ "$UNKNOWN_STATE" == "failed" ]] && break
  sleep 0.25
done
if [[ "$(sqlite3 "$DB" "SELECT last_error_code FROM source_events WHERE external_event_id='Ev-unknown-cancel';")" == "actor_not_authorized" ]] && \
   [[ "$(sqlite3 "$DB" "SELECT status || ':' || resolved_role FROM source_command_actions WHERE source_event_id=(SELECT id FROM source_events WHERE external_event_id='Ev-unknown-cancel');")" == "failed:read_only" ]]; then
  pass "unknown privileged actor fails closed with a command audit record"
else
  fail "unknown privileged actor was not rejected and audited"
fi

STALE="$((NOW - 301))"
STALE_CODE="$(post_slack "$STALE" "$ROOT_BODY")"
if [[ "$STALE_CODE" == "401" ]]; then
  pass "stale Slack timestamp is rejected"
else
  fail "stale Slack timestamp returned HTTP $STALE_CODE"
fi

TAMPERED_CODE="$(curl -sS -o /dev/null -w '%{http_code}' -X POST \
  "http://$WEBHOOK_BIND/source/slack/$PROJECT/slack-source" \
  -H 'Content-Type: application/json' \
  -H "X-Slack-Request-Timestamp: $NOW" \
  -H 'X-Slack-Signature: v0=00' \
  --data-binary "$ROOT_BODY")"
if [[ "$TAMPERED_CODE" == "401" ]]; then
  pass "tampered Slack signature is rejected"
else
  fail "tampered Slack signature was accepted"
fi

OVERSIZED="$QA_ROOT/oversized.json"
dd if=/dev/zero bs=1024 count=300 2>/dev/null | tr '\0' 'a' > "$OVERSIZED"
OVERSIZED_CODE="$(curl -sS -o /dev/null -w '%{http_code}' -X POST \
  "http://$WEBHOOK_BIND/source/slack/$PROJECT/slack-source" \
  -H 'Content-Type: application/json' \
  --data-binary "@$OVERSIZED")"
if [[ "$OVERSIZED_CODE" == "413" ]]; then
  pass "Slack body over 256 KiB is rejected"
else
  fail "oversized Slack body returned HTTP $OVERSIZED_CODE"
fi

echo ""
echo "Source events and Slack QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
