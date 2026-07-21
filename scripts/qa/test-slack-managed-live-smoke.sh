#!/usr/bin/env bash

set -euo pipefail

fail() {
  printf 'FR-114 live smoke: FAIL (%s)\n' "$1" >&2
  exit 1
}

for command in curl date jq mktemp; do
  command -v "$command" >/dev/null 2>&1 || fail "missing required command: $command"
done

: "${ORCHESTRATOR_BIN:?set ORCHESTRATOR_BIN}"
: "${SLACK_LIVE_DAEMON_DATA:?set SLACK_LIVE_DAEMON_DATA}"
: "${SLACK_LIVE_PROJECT:?set SLACK_LIVE_PROJECT}"
: "${SLACK_LIVE_CONNECTION_ID:?set SLACK_LIVE_CONNECTION_ID}"
: "${SLACK_LIVE_CHANNEL_ID:?set SLACK_LIVE_CHANNEL_ID}"
: "${SLACK_LIVE_ACTOR_ID:?set SLACK_LIVE_ACTOR_ID}"
: "${SLACK_LIVE_DRIVER_BOT_TOKEN:?set SLACK_LIVE_DRIVER_BOT_TOKEN}"
: "${SLACK_LIVE_IMPLEMENT_SKILL_MARKER:?set SLACK_LIVE_IMPLEMENT_SKILL_MARKER}"
: "${SLACK_LIVE_DOCS_SKILL_MARKER:?set SLACK_LIVE_DOCS_SKILL_MARKER}"

IMPLEMENT_REACTION="${SLACK_LIVE_IMPLEMENT_REACTION:-eyes}"
DOCS_REACTION="${SLACK_LIVE_DOCS_REACTION:-white_check_mark}"
TIMEOUT_SECONDS="${SLACK_LIVE_TIMEOUT_SECONDS:-90}"
RUN_ID="fr114-smoke-$(date -u +%Y%m%dT%H%M%SZ)"
PRIVATE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/${RUN_ID}.XXXXXX")"
AUTH_CONFIG="$PRIVATE_ROOT/curl-auth.conf"
MESSAGE_TIMESTAMPS=()
POSTED_TS=""

umask 077
chmod 700 "$PRIVATE_ROOT"
printf 'header = "Authorization: Bearer %s"\n' "$SLACK_LIVE_DRIVER_BOT_TOKEN" >"$AUTH_CONFIG"
chmod 600 "$AUTH_CONFIG"

orch() {
  ORCHESTRATORD_DATA_DIR="$SLACK_LIVE_DAEMON_DATA" "$ORCHESTRATOR_BIN" "$@"
}

slack_api() {
  local method="$1"
  local payload="$2"
  local response="$3"
  curl --silent --show-error --fail-with-body \
    --config "$AUTH_CONFIG" \
    --header 'Content-Type: application/json; charset=utf-8' \
    --data-binary "@$payload" \
    "https://slack.com/api/$method" >"$response" || fail "Slack API transport error"
  jq -e '.ok == true' "$response" >/dev/null || fail "Slack API rejected a test-driver operation"
}

delete_message() {
  local ts="$1"
  local payload="$PRIVATE_ROOT/delete.json"
  local response="$PRIVATE_ROOT/delete-response.json"
  jq -n --arg channel "$SLACK_LIVE_CHANNEL_ID" --arg ts "$ts" \
    '{channel:$channel,ts:$ts}' >"$payload"
  curl --silent --show-error \
    --config "$AUTH_CONFIG" \
    --header 'Content-Type: application/json; charset=utf-8' \
    --data-binary "@$payload" \
    'https://slack.com/api/chat.delete' >"$response" 2>/dev/null || true
}

cleanup() {
  local ts
  for ts in "${MESSAGE_TIMESTAMPS[@]:-}"; do
    [[ -n "$ts" ]] && delete_message "$ts"
  done
  find "$PRIVATE_ROOT" -type f -delete 2>/dev/null || true
  find "$PRIVATE_ROOT" -depth -type d -empty -delete 2>/dev/null || true
}
trap cleanup EXIT

post_and_react() {
  local reaction="$1"
  local label="$2"
  local post_payload="$PRIVATE_ROOT/post-$label.json"
  local post_response="$PRIVATE_ROOT/post-$label-response.json"
  local reaction_payload="$PRIVATE_ROOT/reaction-$label.json"
  local reaction_response="$PRIVATE_ROOT/reaction-$label-response.json"

  jq -n --arg channel "$SLACK_LIVE_CHANNEL_ID" --arg text "$RUN_ID $label synthetic message" \
    '{channel:$channel,text:$text}' >"$post_payload"
  slack_api chat.postMessage "$post_payload" "$post_response"
  POSTED_TS="$(jq -er '.ts' "$post_response")"
  MESSAGE_TIMESTAMPS+=("$POSTED_TS")

  jq -n --arg channel "$SLACK_LIVE_CHANNEL_ID" --arg timestamp "$POSTED_TS" --arg name "$reaction" \
    '{channel:$channel,timestamp:$timestamp,name:$name}' >"$reaction_payload"
  slack_api reactions.add "$reaction_payload" "$reaction_response"
}

connection="$PRIVATE_ROOT/connection.json"
baseline="$PRIVATE_ROOT/tasks-before.json"
tasks="$PRIVATE_ROOT/tasks-after.json"

orch source connection get "$SLACK_LIVE_CONNECTION_ID" \
  --project "$SLACK_LIVE_PROJECT" -o json >"$connection"
jq -e '.state == "active" and .delivery_lag == 0' "$connection" >/dev/null \
  || fail "connection is not active and caught up"

orch source binding simulate \
  --project "$SLACK_LIVE_PROJECT" \
  --installation "$(jq -er '.installation_id' "$connection")" \
  --reaction "$IMPLEMENT_REACTION" \
  --channel "$SLACK_LIVE_CHANNEL_ID" \
  --actor "$SLACK_LIVE_ACTOR_ID" -o json >"$PRIVATE_ROOT/simulate-implement.json"
jq -e '.match.status == "matched" and .mutation_performed == false and .network_performed == false' \
  "$PRIVATE_ROOT/simulate-implement.json" >/dev/null || fail "implement binding simulation did not match"

orch task list --project "$SLACK_LIVE_PROJECT" -o json >"$baseline"
baseline_count="$(jq 'length' "$baseline")"

post_and_react "$IMPLEMENT_REACTION" implement
implement_ts="$POSTED_TS"
post_and_react "$DOCS_REACTION" docs

deadline=$(( $(date +%s) + TIMEOUT_SECONDS ))
while (( $(date +%s) < deadline )); do
  orch task list --project "$SLACK_LIVE_PROJECT" -o json >"$tasks"
  current_count="$(jq 'length' "$tasks")"
  new_terminal="$(jq --slurpfile before "$baseline" \
    '[.[] | select(.id as $id | all($before[0][]; .id != $id)) | select(.status == "completed")] | length' "$tasks")"
  if (( current_count == baseline_count + 2 && new_terminal == 2 )); then
    break
  fi
  sleep 2
done

current_count="$(jq 'length' "$tasks")"
(( current_count == baseline_count + 2 )) || fail "expected exactly two new tasks"
jq -e --slurpfile before "$baseline" --arg marker "$SLACK_LIVE_IMPLEMENT_SKILL_MARKER" \
  'any(.[] | select(.id as $id | all($before[0][]; .id != $id)); .goal | contains($marker))' "$tasks" >/dev/null \
  || fail "implement Skill marker missing from new task"
jq -e --slurpfile before "$baseline" --arg marker "$SLACK_LIVE_DOCS_SKILL_MARKER" \
  'any(.[] | select(.id as $id | all($before[0][]; .id != $id)); .goal | contains($marker))' "$tasks" >/dev/null \
  || fail "docs Skill marker missing from new task"

# A remove/add retry on the same Slack message must converge to the existing task.
jq -n --arg channel "$SLACK_LIVE_CHANNEL_ID" --arg timestamp "$implement_ts" --arg name "$IMPLEMENT_REACTION" \
  '{channel:$channel,timestamp:$timestamp,name:$name}' >"$PRIVATE_ROOT/retry-reaction.json"
slack_api reactions.remove "$PRIVATE_ROOT/retry-reaction.json" "$PRIVATE_ROOT/remove-response.json"
slack_api reactions.add "$PRIVATE_ROOT/retry-reaction.json" "$PRIVATE_ROOT/readd-response.json"
sleep 8
orch task list --project "$SLACK_LIVE_PROJECT" -o json >"$tasks"
(( $(jq 'length' "$tasks") == baseline_count + 2 )) || fail "duplicate reaction created another task"

orch source automation status --project "$SLACK_LIVE_PROJECT" -o json >"$PRIVATE_ROOT/automation-status.json"
jq -e '.backlog_count == 0 and .active_leases == 0 and .needs_attention_count == 0' \
  "$PRIVATE_ROOT/automation-status.json" >/dev/null || fail "automation did not quiesce cleanly"

printf 'FR-114 live smoke: PASS (two routed tasks, duplicate converged, backlog empty)\n'
