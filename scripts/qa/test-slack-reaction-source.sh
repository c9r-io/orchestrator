#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
GRPC_BIND="${GRPC_BIND:-127.0.0.1:19207}"
WEBHOOK_BIND="${WEBHOOK_BIND:-127.0.0.1:19208}"
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
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
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
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/docs/qa/orchestrator" "$QA_ROOT/docs/ticket"
printf '# Slack reaction QA target\n' > "$QA_ROOT/docs/qa/orchestrator/source.md"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$GRPC_BIND" --webhook-bind "$WEBHOOK_BIND" --workers 1 \
    > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(<"$QA_ROOT/daemon.pid")"

for _ in {1..40}; do
  "$ORCH" task list -o json >/dev/null 2>&1 && break
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  echo "isolated daemon failed to start" >&2
  sed -n '1,240p' "$QA_ROOT/daemon.log" >&2
  exit 1
fi

PROJECT="qa-slack-reaction"
"$ORCH" apply --project "$PROJECT" \
  -f "$REPO_ROOT/fixtures/manifests/bundles/source-events-fixture.yaml" >/dev/null

SLACK_SECRET="qa-slack-reaction-signing-secret"
SLACK_MANIFEST="$QA_ROOT/slack-source.yaml"
cat > "$SLACK_MANIFEST" <<EOF
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: slack-reaction-secret
spec:
  data:
    signing: $SLACK_SECRET
---
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: slack-reaction
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
        fromRef: slack-reaction-secret
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
  local response_file="$3"
  local signature
  signature="$(slack_signature "$timestamp" "$body")"
  curl -sS -o "$response_file" -w '%{http_code}' -X POST \
    "http://$WEBHOOK_BIND/source/slack/$PROJECT/slack-reaction" \
    -H 'Content-Type: application/json' \
    -H "X-Slack-Request-Timestamp: $timestamp" \
    -H "X-Slack-Signature: $signature" \
    --data-binary "$body"
}

DB="$QA_ROOT/data/agent_orchestrator.db"
NOW="$(date +%s)"
MESSAGE_TS="$NOW.000100"
EVENT_TS="$NOW.000200"
REACTION_BODY="{\"type\":\"event_callback\",\"event_id\":\"Ev-reaction-message\",\"event\":{\"type\":\"reaction_added\",\"user\":\"U-operator\",\"reaction\":\"agent_fix\",\"item\":{\"type\":\"message\",\"channel\":\"C-source\",\"ts\":\"$MESSAGE_TS\"},\"event_ts\":\"$EVENT_TS\"}}"
REACTION_CODE_1="$(post_slack "$NOW" "$REACTION_BODY" "$QA_ROOT/reaction-1.txt")"
REACTION_CODE_2="$(post_slack "$NOW" "$REACTION_BODY" "$QA_ROOT/reaction-2.txt")"
if [[ "$REACTION_CODE_1" == "200" && "$REACTION_CODE_2" == "200" ]]; then
  pass "signed message reaction is durably acknowledged, including provider retry"
else
  fail "signed message reaction returned HTTP $REACTION_CODE_1/$REACTION_CODE_2"
fi

for _ in {1..40}; do
  STATE="$(sqlite3 "$DB" "SELECT routing_state FROM source_events WHERE external_event_id='Ev-reaction-message';")"
  [[ "$STATE" == "ignored" ]] && break
  sleep 0.25
done
SOURCE_JSON="$QA_ROOT/reaction-source.json"
"$ORCH" source list --project "$PROJECT" -o json > "$SOURCE_JSON"
REACTION_SOURCE_ID="$(jq -r '.[] | select(.external_event_id == "Ev-reaction-message") | .id' "$SOURCE_JSON")"
"$ORCH" source get "$REACTION_SOURCE_ID" -o json > "$QA_ROOT/reaction-get.json"

if jq -e --arg target "C-source:$MESSAGE_TS" '
    .event_type == "reaction_added" and
    .external_actor_id == "U-operator" and
    .occurred_at == .normalized.occurred_at and
    (.occurred_at | endswith(".000200+00:00")) and
    .routing_state == "ignored" and
    .routing_attempts == 1 and
    .last_error_code == "reaction_routing_not_enabled" and
    .normalized.reaction.name == "agent_fix" and
    .normalized.reaction.target.kind == "message" and
    .normalized.reaction.target.external_id == $target and
    .normalized.reaction.target.url == null and
    .normalized.text_summary == null
  ' "$QA_ROOT/reaction-get.json" >/dev/null; then
  pass "public source read exposes bounded typed reaction provenance without body or URL"
else
  fail "typed reaction provenance or fail-safe routing projection differs"
fi

if [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_events WHERE external_event_id='Ev-reaction-message';")" -eq 1 ]] && \
   [[ "$(sqlite3 "$DB" "SELECT routing_attempts FROM source_events WHERE external_event_id='Ev-reaction-message';")" -eq 1 ]] && \
   [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE project_id='$PROJECT';")" -eq 0 ]] && \
   [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_bindings WHERE project_id='$PROJECT';")" -eq 0 ]]; then
  pass "reaction retry creates one source row and no task or binding"
else
  fail "reaction dedupe or non-mutating gate counts differ"
fi

FILE_BODY="{\"type\":\"event_callback\",\"event_id\":\"Ev-reaction-file\",\"event\":{\"type\":\"reaction_added\",\"user\":\"U-operator\",\"reaction\":\"agent_docs\",\"item\":{\"type\":\"file\",\"file\":\"F123\"},\"event_ts\":\"$NOW.000300\"}}"
FILE_CODE="$(post_slack "$NOW" "$FILE_BODY" "$QA_ROOT/file.txt")"
for _ in {1..40}; do
  FILE_STATE="$(sqlite3 "$DB" "SELECT routing_state FROM source_events WHERE external_event_id='Ev-reaction-file';")"
  [[ "$FILE_STATE" == "ignored" ]] && break
  sleep 0.25
done
if [[ "$FILE_CODE" == "200" ]] && \
   [[ "$(sqlite3 "$DB" "SELECT last_error_code FROM source_events WHERE external_event_id='Ev-reaction-file';")" == "unsupported_reaction_target" ]] && \
   [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE project_id='$PROJECT';")" -eq 0 ]]; then
  pass "non-message reaction is queryable and cannot create a task"
else
  fail "non-message reaction was not safely ignored (HTTP $FILE_CODE)"
fi

MISSING_ACTOR_BODY="{\"type\":\"event_callback\",\"event_id\":\"Ev-reaction-missing-actor\",\"event\":{\"type\":\"reaction_added\",\"reaction\":\"agent_fix\",\"item\":{\"type\":\"message\",\"channel\":\"C-source\",\"ts\":\"$MESSAGE_TS\"},\"event_ts\":\"$NOW.000400\"}}"
MISSING_ACTOR_CODE="$(post_slack "$NOW" "$MISSING_ACTOR_BODY" "$QA_ROOT/missing-actor.txt")"
if [[ "$MISSING_ACTOR_CODE" == "400" ]] && \
   [[ "$(<"$QA_ROOT/missing-actor.txt")" == "slack_reaction_missing_actor" ]] && \
   [[ "$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_events WHERE external_event_id='Ev-reaction-missing-actor';")" -eq 0 ]]; then
  pass "missing actor fails closed with a stable error code and no durable row"
else
  fail "missing actor error contract differs (HTTP $MISSING_ACTOR_CODE)"
fi

echo ""
echo "Slack reaction source QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
