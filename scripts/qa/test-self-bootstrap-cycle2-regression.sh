#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

ORCH="./target/release/orchestrator"
DB="data/agent_orchestrator.db"
WORKFLOW_FILE=""
TASK_ID=""

cleanup() {
  if [ -n "$WORKFLOW_FILE" ] && [ -f "$WORKFLOW_FILE" ]; then
    rm -f "$WORKFLOW_FILE"
  fi
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

info() {
  echo "[cycle2-regression] $*"
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

require_cmd sqlite3
require_cmd cargo

info "Building release CLI"
(cd core && cargo build --release >/dev/null)

if [ ! -f "$DB" ]; then
  info "Initializing orchestrator database"
  $ORCH init >/dev/null
fi

mkdir -p fixtures/bootstrap-qa
printf '# Bootstrap QA target\n' > fixtures/bootstrap-qa/bootstrap-check.md

info "Applying deterministic fixture resources"
$ORCH apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml >/dev/null

WORKFLOW_FILE="$(mktemp "${TMPDIR:-/tmp}/qa-cycle2-regression.XXXXXX.yaml")"
cat > "$WORKFLOW_FILE" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: qa_cycle2_validation_deterministic
spec:
  steps:
    - id: plan
      type: plan
      scope: task
      enabled: true
      repeatable: false
      command: "printf '%s\n' '{\"confidence\":0.95,\"quality_score\":0.90,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"plan-ok\",\"description\":\"plan generated\",\"severity\":\"info\"}]}]}'"
    - id: qa_doc_gen
      type: qa_doc_gen
      scope: task
      enabled: true
      repeatable: false
      command: "printf '%s\n' '{\"confidence\":0.92,\"quality_score\":0.89,\"artifacts\":[{\"kind\":\"qa_doc\",\"files\":[\"docs/qa/self-bootstrap/04-cycle2-validation-and-runtime-timestamps.md\"]}]}'"
    - id: implement
      type: implement
      scope: task
      enabled: true
      repeatable: true
      command: "printf '%s\n' '{\"confidence\":0.91,\"quality_score\":0.87,\"artifacts\":[{\"kind\":\"code_change\",\"files\":[\"core/src/scheduler/item_executor.rs\"]}]}'"
    - id: qa_testing
      type: qa_testing
      scope: item
      enabled: true
      repeatable: true
      command: "printf '%s\n' '{\"confidence\":0.97,\"quality_score\":0.94,\"artifacts\":[{\"kind\":\"qa_result\",\"findings\":[]}]}'"
      prehook:
        engine: cel
        when: "is_last_cycle"
        reason: "QA deferred to final cycle"
    - id: ticket_fix
      type: ticket_fix
      scope: item
      enabled: true
      repeatable: true
      command: "printf '%s\n' '{\"confidence\":0.88,\"quality_score\":0.84,\"artifacts\":[{\"kind\":\"fix\",\"files\":[]}]}'"
      prehook:
        engine: cel
        when: "is_last_cycle && active_ticket_count > 0"
        reason: "Only fix when tickets exist on final cycle"
    - id: align_tests
      type: align_tests
      scope: task
      enabled: true
      repeatable: true
      command: "printf '%s\n' '{\"confidence\":0.93,\"quality_score\":0.90,\"artifacts\":[{\"kind\":\"test_alignment\",\"findings\":[]}]}'"
      prehook:
        engine: cel
        when: "is_last_cycle"
        reason: "Align deferred to final cycle"
    - id: doc_governance
      type: doc_governance
      scope: task
      enabled: true
      repeatable: true
      command: "printf '%s\n' '{\"confidence\":0.94,\"quality_score\":0.91,\"artifacts\":[{\"kind\":\"doc_audit\",\"findings\":[]}]}'"
      prehook:
        engine: cel
        when: "is_last_cycle"
        reason: "Docs deferred to final cycle"
    - id: loop_guard
      type: loop_guard
      enabled: true
      repeatable: true
      is_guard: true
      builtin: loop_guard
  loop:
    mode: fixed
    max_cycles: 2
    enabled: true
    stop_when_no_unresolved: false
  safety:
    max_consecutive_failures: 3
    auto_rollback: false
    checkpoint_strategy: none
EOF

info "Applying deterministic regression workflow"
$ORCH apply -f "$WORKFLOW_FILE" >/dev/null

TASK_ID="$($ORCH task create \
  --workspace bootstrap-ws \
  --workflow qa_cycle2_validation_deterministic \
  --target-file fixtures/bootstrap-qa/bootstrap-check.md \
  --goal "qa validate cycle2 regression deterministic" \
  --no-start | grep -oE '[0-9a-f-]{36}' | head -1)"

[ -n "$TASK_ID" ] || fail "task creation returned no task id"

info "Running task $TASK_ID"
$ORCH task start "$TASK_ID" >/dev/null

TASK_ROW="$(sqlite3 "$DB" "SELECT status, started_at, completed_at FROM tasks WHERE id='${TASK_ID}';")"
TASK_STATUS="$(printf '%s' "$TASK_ROW" | cut -d'|' -f1)"
TASK_STARTED_AT="$(printf '%s' "$TASK_ROW" | cut -d'|' -f2)"
TASK_COMPLETED_AT="$(printf '%s' "$TASK_ROW" | cut -d'|' -f3)"

[ "$TASK_STATUS" = "completed" ] || fail "expected completed task, got: $TASK_STATUS"
[ -n "$TASK_STARTED_AT" ] || fail "task.started_at is empty"
[ -n "$TASK_COMPLETED_AT" ] || fail "task.completed_at is empty"

QA_STARTED="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' AND json_extract(payload_json,'$.step')='qa_testing' AND json_extract(payload_json,'$.cycle')=2;")"
ALIGN_STARTED="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' AND json_extract(payload_json,'$.step')='align_tests' AND json_extract(payload_json,'$.cycle')=2;")"
DOC_STARTED="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' AND json_extract(payload_json,'$.step')='doc_governance' AND json_extract(payload_json,'$.cycle')=2;")"
TICKET_SKIP="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_skipped' AND json_extract(payload_json,'$.step')='ticket_fix' AND json_extract(payload_json,'$.reason')='prehook_false';")"
VALIDATION_MISSING="$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='item_validation_missing';")"
ITEM_NULL_TS="$(sqlite3 "$DB" "SELECT COUNT(*) FROM task_items WHERE task_id='${TASK_ID}' AND (started_at IS NULL OR completed_at IS NULL);")"

[ "$QA_STARTED" -ge 1 ] || fail "qa_testing did not start in cycle 2"
[ "$ALIGN_STARTED" -ge 1 ] || fail "align_tests did not start in cycle 2"
[ "$DOC_STARTED" -ge 1 ] || fail "doc_governance did not start in cycle 2"
[ "$TICKET_SKIP" -ge 1 ] || fail "ticket_fix was not explicitly skipped by prehook"
[ "$VALIDATION_MISSING" -eq 0 ] || fail "item_validation_missing was emitted"
[ "$ITEM_NULL_TS" -eq 0 ] || fail "one or more task_items have null started_at/completed_at"

info "PASS"
echo "task_id=$TASK_ID"
echo "task_status=$TASK_STATUS"
echo "qa_started_cycle2=$QA_STARTED"
echo "ticket_fix_skipped=$TICKET_SKIP"
echo "task_started_at=$TASK_STARTED_AT"
echo "task_completed_at=$TASK_COMPLETED_AT"
