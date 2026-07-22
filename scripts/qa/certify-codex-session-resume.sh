#!/usr/bin/env bash

set -euo pipefail

EXPECTED_VERSION="${CODEX_RESUME_EXPECTED_VERSION:-0.144.5}"
SOURCE_CODEX_HOME="${CODEX_RESUME_SOURCE_HOME:-${CODEX_HOME:-$HOME/.codex}}"
FIRST_ANCHOR="ORCH_RESUME_ANCHOR_ALPHA"
RESUME_ANCHOR="ORCH_RESUME_ANCHOR_BETA:$FIRST_ANCHOR"
QA_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/orch-codex-resume.XXXXXX")"
PASS=0

pass() {
  echo "  PASS: $1"
  PASS=$((PASS + 1))
}

cleanup() {
  # The isolated home contains a temporary authentication copy and must never
  # be retained, including after a failed provider call.
  find "$QA_ROOT" -depth -delete 2>/dev/null || true
}
trap cleanup EXIT INT TERM

for command in codex jq mktemp; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 2
  }
done

AUTH_FILE="$SOURCE_CODEX_HOME/auth.json"
[[ -f "$AUTH_FILE" ]] || {
  echo "Codex authentication file not found: $AUTH_FILE" >&2
  exit 2
}

ACTUAL_VERSION="$(codex --version | awk '{print $2}')"
[[ "$ACTUAL_VERSION" == "$EXPECTED_VERSION" ]] || {
  echo "expected codex-cli $EXPECTED_VERSION, found $ACTUAL_VERSION" >&2
  echo "Recertify the recorded fixture before changing the pinned version." >&2
  exit 2
}
pass "codex-cli version is pinned to $EXPECTED_VERSION"

CODEX_TEST_HOME="$QA_ROOT/codex-home"
WORK_DIR="$QA_ROOT/work"
mkdir -p "$CODEX_TEST_HOME" "$WORK_DIR"
cp "$AUTH_FILE" "$CODEX_TEST_HOME/auth.json"
chmod 700 "$CODEX_TEST_HOME" "$WORK_DIR"
chmod 600 "$CODEX_TEST_HOME/auth.json"

FIRST_OUT="$QA_ROOT/first.jsonl"
FIRST_ERR="$QA_ROOT/first.stderr"
RESUME_OUT="$QA_ROOT/resume.jsonl"
RESUME_ERR="$QA_ROOT/resume.stderr"

if ! (
  cd "$WORK_DIR"
  CODEX_HOME="$CODEX_TEST_HOME" codex exec \
    --json \
    --ignore-user-config \
    --ignore-rules \
    --sandbox read-only \
    --skip-git-repo-check \
    -- "Reply with exactly: $FIRST_ANCHOR"
) >"$FIRST_OUT" 2>"$FIRST_ERR"; then
  echo "initial codex exec failed; stderr was captured in the disposable QA directory" >&2
  exit 1
fi

THREAD_ID="$(jq -r 'select(.type == "thread.started") | .thread_id' "$FIRST_OUT" | head -n 1)"
FIRST_TEXT="$(jq -r 'select(.type == "item.completed" and .item.type == "agent_message") | .item.text' "$FIRST_OUT" | tail -n 1)"
[[ -n "$THREAD_ID" && "$THREAD_ID" != "null" ]] || {
  echo "initial stream did not contain thread.started.thread_id" >&2
  exit 1
}
[[ "$FIRST_TEXT" == "$FIRST_ANCHOR" ]] || {
  echo "initial assistant response did not match the certification anchor" >&2
  exit 1
}
pass "initial exec exposes a thread id and the expected assistant event"

if ! (
  cd "$WORK_DIR"
  CODEX_HOME="$CODEX_TEST_HOME" codex exec resume "$THREAD_ID" \
    --json \
    --ignore-user-config \
    --ignore-rules \
    --skip-git-repo-check \
    -- 'What exact anchor did you reply with in the previous turn? Reply exactly: ORCH_RESUME_ANCHOR_BETA:<previous anchor>'
) >"$RESUME_OUT" 2>"$RESUME_ERR"; then
  echo "codex exec resume failed; stderr was captured in the disposable QA directory" >&2
  exit 1
fi

RESUME_THREAD_ID="$(jq -r 'select(.type == "thread.started") | .thread_id' "$RESUME_OUT" | head -n 1)"
RESUME_TEXT="$(jq -r 'select(.type == "item.completed" and .item.type == "agent_message") | .item.text' "$RESUME_OUT" | tail -n 1)"
[[ "$RESUME_THREAD_ID" == "$THREAD_ID" ]] || {
  echo "resume returned a different thread id" >&2
  exit 1
}
[[ "$RESUME_TEXT" == "$RESUME_ANCHOR" ]] || {
  echo "resume did not inherit the first-turn anchor" >&2
  exit 1
}
pass "resume preserves thread identity and prior-turn context"

EXPECTED_EVENTS="thread.started,turn.started,item.completed,turn.completed"
FIRST_EVENTS="$(jq -r '.type' "$FIRST_OUT" | paste -sd, -)"
RESUME_EVENTS="$(jq -r '.type' "$RESUME_OUT" | paste -sd, -)"
[[ "$FIRST_EVENTS" == "$EXPECTED_EVENTS" && "$RESUME_EVENTS" == "$EXPECTED_EVENTS" ]] || {
  echo "Codex JSONL schema drifted: first=$FIRST_EVENTS resume=$RESUME_EVENTS" >&2
  exit 1
}
pass "initial and resumed JSONL event sequences match the recorded protocol"

if [[ "${CODEX_RESUME_PRINT_FIXTURE:-0}" == "1" ]]; then
  FIRST_JSON="$(jq -s --arg session '<SESSION_ID>' '
    map(if .type == "thread.started" then .thread_id = $session else . end)
    | map(if .item.id then .item.id = "<FIRST_ITEM_ID>" else . end)
  ' "$FIRST_OUT")"
  RESUME_JSON="$(jq -s --arg session '<SESSION_ID>' '
    map(if .type == "thread.started" then .thread_id = $session else . end)
    | map(if .item.id then .item.id = "<RESUME_ITEM_ID>" else . end)
  ' "$RESUME_OUT")"
  jq -n \
    --arg version "$ACTUAL_VERSION" \
    --arg recorded_at "2026-07-22" \
    --argjson first "$FIRST_JSON" \
    --argjson resume "$RESUME_JSON" \
    '{
      schema_version: 1,
      provider: "codex",
      transport: "cli",
      codex_cli_version: $version,
      recorded_at: $recorded_at,
      session_placeholder: "<SESSION_ID>",
      first_events: $first,
      resume_events: $resume
    }'
fi

echo "Codex session resume live certification: $PASS passed, 0 failed"
