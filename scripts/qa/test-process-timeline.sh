#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19195}"
PASS=0
FAIL=0
DAEMON_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in jq mktemp; do
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
gate_runlog_arm "scripts/qa/test-process-timeline.sh"

export HOME="$QA_HOME"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/fixtures/qa" "$QA_ROOT/fixtures/ticket"
printf '# Deterministic process timeline target\n' > "$QA_ROOT/fixtures/qa/timeline.md"

(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 > daemon.log 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(cat "$QA_ROOT/daemon.pid")"

for _ in {1..30}; do
  if "$ORCH" task list -o json >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  echo "isolated daemon failed to start" >&2
  "$ORCH" task list -o json >&2 || true
  cat "$QA_ROOT/daemon.log" >&2
  exit 1
fi

PROJECT="qa-process-timeline"
"$ORCH" apply \
  --project "$PROJECT" \
  -f "$REPO_ROOT/fixtures/manifests/bundles/process-timeline-failure.yaml" >/dev/null

CREATE_OUTPUT="$(
  cd "$QA_ROOT"
  "$ORCH" task create \
    --project "$PROJECT" \
    --workspace default \
    --workflow timeline_failure \
    --target-file fixtures/qa/timeline.md \
    --goal "explain a deterministic QA assertion failure" \
    --no-start
)"
# FR-146: `| head -1` under pipefail kills grep and ends the gate with no summary line.
TASK_IDS="$(grep -oE '[0-9a-f-]{36}' <<< "$CREATE_OUTPUT" || true)"
TASK_ID="${TASK_IDS%%$'\n'*}"
if [[ -z "$TASK_ID" ]]; then
  echo "task creation returned no task id: $CREATE_OUTPUT" >&2
  exit 1
fi

"$ORCH" task start "$TASK_ID" >/dev/null 2>&1 || true
for _ in {1..40}; do
  STATUS="$("$ORCH" task info "$TASK_ID" -o json | jq -r '.task.status')"
  if [[ "$STATUS" =~ ^(completed|failed|cancelled)$ ]]; then
    break
  fi
  sleep 0.25
done

TIMELINE="$QA_ROOT/timeline.json"
"$ORCH" task timeline "$TASK_ID" --limit 100 -o json > "$TIMELINE"

for category in goal lifecycle test failure; do
  if jq -e --arg category "$category" '.entries | any(.category == $category)' "$TIMELINE" >/dev/null; then
    pass "timeline contains $category entry"
  else
    fail "timeline is missing $category entry"
  fi
done

if jq -e '(.entries | map(.id)) as $ids | ($ids | length) == ($ids | unique | length)' "$TIMELINE" >/dev/null; then
  pass "timeline IDs are unique"
else
  fail "timeline IDs contain duplicates"
fi

if jq -e '.entries | any(.category == "failure" and (.summary | length > 0) and .command_run_id != null and (.evidence | any(.uri | startswith("orchestrator://runs/"))))' "$TIMELINE" >/dev/null; then
  pass "failure includes a useful reason and command-run evidence"
else
  fail "failure lacks structured reason or command-run evidence"
  jq '.entries[] | select(.category == "failure")' "$TIMELINE" >&2
fi

FIRST_PAGE="$QA_ROOT/page-1.json"
SECOND_PAGE="$QA_ROOT/page-2.json"
"$ORCH" task timeline "$TASK_ID" --limit 2 -o json > "$FIRST_PAGE"
CURSOR="$(jq -r '.next_cursor // empty' "$FIRST_PAGE")"
if [[ -n "$CURSOR" ]]; then
  "$ORCH" task timeline "$TASK_ID" --limit 2 --cursor "$CURSOR" -o json > "$SECOND_PAGE"
  if jq -s -e '([.[0].entries[].id] as $left | [.[1].entries[].id] as $right | [$left[] | select(. as $id | $right | index($id))] | length == 0)' "$FIRST_PAGE" "$SECOND_PAGE" >/dev/null; then
    pass "cursor pages do not overlap"
  else
    fail "cursor pages overlap"
  fi
else
  fail "two-entry page did not return a cursor"
fi

FILTERED="$QA_ROOT/failure-only.json"
"$ORCH" task timeline "$TASK_ID" --category failure -o json > "$FILTERED"
if jq -e '.entries | length > 0 and all(.category == "failure")' "$FILTERED" >/dev/null; then
  pass "category filter returns only failures"
else
  fail "category filter leaked another category"
fi

echo ""
echo "Process timeline QA: $PASS passed, $FAIL failed"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
