#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
cd "$REPO_ROOT"

ORCH="orchestrator"
DB="data/agent_orchestrator.db"
PASS=0
FAIL=0
SKIP=0

green() { printf '\033[32m%s\033[0m\n' "$*"; }
red()   { printf '\033[31m%s\033[0m\n' "$*"; }
yellow(){ printf '\033[33m%s\033[0m\n' "$*"; }
bold()  { printf '\033[1m%s\033[0m\n' "$*"; }

pass() { PASS=$((PASS+1)); green "  PASS: $1"; }
fail() { FAIL=$((FAIL+1)); red   "  FAIL: $1 — $2"; }
skip() { SKIP=$((SKIP+1)); yellow "  SKIP: $1 — $2"; }

extract_task_id() {
  grep -oE '[0-9a-f-]{36}' | head -1
}

wait_task() {
  local tid="$1" max="${2:-30}" i=0
  while [ $i -lt $max ]; do
    local st
    st=$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${tid}'" 2>/dev/null || echo "unknown")
    case "$st" in
      completed|failed|paused) return 0 ;;
    esac
    sleep 1
    i=$((i+1))
  done
  return 1
}

bold "======================================"
bold " Self-Bootstrap Workflow QA Tests"
bold "======================================"
echo ""

# --- Setup ---
bold "[Setup] Cleaning stale state..."
rm -f fixtures/ticket/auto_*.md
mkdir -p fixtures/bootstrap-qa
echo "# Bootstrap QA target" > fixtures/bootstrap-qa/bootstrap-check.md

QA_PROJECT="qa-bootstrap-$(whoami)-$(date +%Y%m%d%H%M%S)"
$ORCH init --force > /dev/null 2>&1 || true

bold "[Setup] Applying self-bootstrap-test.yaml..."
$ORCH delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
$ORCH apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml --project "${QA_PROJECT}" > /dev/null 2>&1
echo ""

# ============================================================
bold "Scenario 1: Basic Bootstrap Workflow (plan->implement->build->test)"
bold "--------------------------------------------------------------"

TASK_ID=$($ORCH task create \
  --name "bootstrap-basic" \
  --goal "Test basic bootstrap pipeline" \
  --project "${QA_PROJECT}" \
  --workspace bootstrap-ws \
  --workflow bootstrap_basic \
  --no-start 2>&1 | extract_task_id)

if [ -z "$TASK_ID" ]; then
  fail "S1" "task create returned no ID"
else
  echo "  Task ID: ${TASK_ID}"
  $ORCH task start "${TASK_ID}" > /dev/null 2>&1 || true
  wait_task "${TASK_ID}" 30

  S1_STATUS=$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}'" 2>/dev/null)
  if [ "$S1_STATUS" = "completed" ]; then
    # Check that extended steps ran
    S1_STEPS=$(sqlite3 "$DB" "SELECT json_extract(payload_json, '$.step') FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' ORDER BY created_at" 2>/dev/null)
    echo "  Steps executed: $(echo "$S1_STEPS" | tr '\n' ' ')"

    HAS_PLAN=$(echo "$S1_STEPS" | grep -c "plan" || true)
    HAS_IMPLEMENT=$(echo "$S1_STEPS" | grep -c "implement" || true)
    HAS_BUILD=$(echo "$S1_STEPS" | grep -c "build" || true)
    HAS_TEST=$(echo "$S1_STEPS" | grep -c "test" || true)

    if [ "$HAS_PLAN" -ge 1 ] && [ "$HAS_IMPLEMENT" -ge 1 ] && [ "$HAS_BUILD" -ge 1 ] && [ "$HAS_TEST" -ge 1 ]; then
      pass "S1: All 4 steps executed, task completed"
    else
      fail "S1" "Missing steps: plan=$HAS_PLAN implement=$HAS_IMPLEMENT build=$HAS_BUILD test=$HAS_TEST"
    fi
  else
    fail "S1" "Expected completed, got: ${S1_STATUS}"
    echo "  Logs:"
    $ORCH task logs "${TASK_ID}" 2>/dev/null | tail -10 || true
  fi
fi
echo ""

# ============================================================
bold "Scenario 2: Build Failure Triggers Fix via Prehook"
bold "--------------------------------------------------------------"

TASK_ID=$($ORCH task create \
  --name "bootstrap-fix" \
  --goal "Test build failure triggers fix" \
  --project "${QA_PROJECT}" \
  --workspace bootstrap-ws \
  --workflow bootstrap_with_fix \
  --no-start 2>&1 | extract_task_id)

if [ -z "$TASK_ID" ]; then
  fail "S2" "task create returned no ID"
else
  echo "  Task ID: ${TASK_ID}"
  $ORCH task start "${TASK_ID}" > /dev/null 2>&1 || true
  wait_task "${TASK_ID}" 30

  # Check build step finished with failure
  S2_BUILD_OK=$(sqlite3 "$DB" "SELECT json_extract(payload_json, '$.success') FROM events WHERE task_id='${TASK_ID}' AND event_type='step_finished' AND json_extract(payload_json, '$.step')='build'" 2>/dev/null || echo "null")
  S2_BUILD_ERR=$(sqlite3 "$DB" "SELECT json_extract(payload_json, '$.build_errors') FROM events WHERE task_id='${TASK_ID}' AND event_type='step_finished' AND json_extract(payload_json, '$.step')='build'" 2>/dev/null || echo "0")

  # Check fix step was triggered
  S2_FIX_STARTED=$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' AND json_extract(payload_json, '$.step')='fix'" 2>/dev/null || echo "0")

  echo "  Build success: ${S2_BUILD_OK}, build_errors: ${S2_BUILD_ERR}, fix started: ${S2_FIX_STARTED}"

  if [ "$S2_BUILD_OK" = "0" ] || [ "$S2_BUILD_OK" = "false" ]; then
    if [ "$S2_FIX_STARTED" -ge 1 ]; then
      pass "S2: Build failed, fix triggered by prehook"
    else
      fail "S2" "Build failed but fix step not triggered (prehook issue)"
    fi
  else
    fail "S2" "Expected build failure, got success=${S2_BUILD_OK}"
  fi
fi
echo ""

# ============================================================
bold "Scenario 3: Successful Build Skips Fix Step"
bold "--------------------------------------------------------------"

TASK_ID=$($ORCH task create \
  --name "bootstrap-skip-fix" \
  --goal "Test successful build skips fix" \
  --project "${QA_PROJECT}" \
  --workspace bootstrap-ws \
  --workflow bootstrap_skip_fix \
  --no-start 2>&1 | extract_task_id)

if [ -z "$TASK_ID" ]; then
  fail "S3" "task create returned no ID"
else
  echo "  Task ID: ${TASK_ID}"
  $ORCH task start "${TASK_ID}" > /dev/null 2>&1 || true
  wait_task "${TASK_ID}" 30

  S3_STATUS=$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}'" 2>/dev/null)
  S3_FIX_SKIPPED=$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_skipped' AND json_extract(payload_json, '$.step')='fix'" 2>/dev/null || echo "0")
  S3_FIX_STARTED=$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' AND json_extract(payload_json, '$.step')='fix'" 2>/dev/null || echo "0")

  echo "  Status: ${S3_STATUS}, fix_skipped: ${S3_FIX_SKIPPED}, fix_started: ${S3_FIX_STARTED}"

  if [ "$S3_FIX_SKIPPED" -ge 1 ] && [ "$S3_FIX_STARTED" -eq 0 ]; then
    pass "S3: Fix step correctly skipped when build/test succeeded"
  elif [ "$S3_FIX_STARTED" -ge 1 ]; then
    fail "S3" "Fix step should not have started (build/test passed)"
  else
    fail "S3" "Expected step_skipped event for fix, found none"
  fi
fi
echo ""

# ============================================================
bold "Scenario 4: Checkpoint Created at Cycle Start"
bold "--------------------------------------------------------------"

TASK_ID=$($ORCH task create \
  --name "bootstrap-checkpoint" \
  --goal "Test checkpoint creation" \
  --project "${QA_PROJECT}" \
  --workspace bootstrap-ws \
  --workflow bootstrap_checkpoint \
  --no-start 2>&1 | extract_task_id)

if [ -z "$TASK_ID" ]; then
  fail "S4" "task create returned no ID"
else
  echo "  Task ID: ${TASK_ID}"
  $ORCH task start "${TASK_ID}" > /dev/null 2>&1 || true
  wait_task "${TASK_ID}" 30

  S4_CP_EVENT=$(sqlite3 "$DB" "SELECT json_extract(payload_json, '$.tag') FROM events WHERE task_id='${TASK_ID}' AND event_type='checkpoint_created'" 2>/dev/null || echo "")
  S4_TAG_EXISTS=$(git tag -l "checkpoint/${TASK_ID}/1" 2>/dev/null)

  echo "  Checkpoint event tag: ${S4_CP_EVENT}"
  echo "  Git tag exists: ${S4_TAG_EXISTS:-none}"

  if [ -n "$S4_CP_EVENT" ]; then
    if [ -n "$S4_TAG_EXISTS" ]; then
      pass "S4: Checkpoint created — event and git tag both present"
    else
      fail "S4" "Checkpoint event found but git tag missing"
    fi
  else
    fail "S4" "No checkpoint_created event found"
  fi

  # Clean up checkpoint tag
  git tag -l "checkpoint/${TASK_ID}/*" 2>/dev/null | xargs -r git tag -d 2>/dev/null || true
fi
echo ""

# ============================================================
bold "Scenario 5: Self-Bootstrap Manifest Applies Successfully"
bold "--------------------------------------------------------------"

S5_DRYRUN=$($ORCH apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --project "${QA_PROJECT}" --dry-run 2>&1) || true
S5_APPLY=$($ORCH apply -f fixtures/manifests/bundles/self-bootstrap-mock.yaml --project "${QA_PROJECT}" 2>&1) || true

echo "  Dry-run output: $(echo "$S5_DRYRUN" | head -5)"

# Check resources exist
S5_WS=$($ORCH get workspaces --project "${QA_PROJECT}" 2>&1 | grep -c "self" || true)
S5_AGENTS=$($ORCH get agents --project "${QA_PROJECT}" 2>&1)
S5_ARCHITECT=$(echo "$S5_AGENTS" | grep -c "architect" || true)
S5_CODER=$(echo "$S5_AGENTS" | grep -c "coder" || true)
S5_TESTER=$(echo "$S5_AGENTS" | grep -c "tester" || true)
S5_REVIEWER=$(echo "$S5_AGENTS" | grep -c "reviewer" || true)
S5_WF=$($ORCH get workflows --project "${QA_PROJECT}" 2>&1 | grep -c "self-bootstrap" || true)

echo "  Workspace 'self': ${S5_WS}, agents: architect=${S5_ARCHITECT} coder=${S5_CODER} tester=${S5_TESTER} reviewer=${S5_REVIEWER}, workflow: ${S5_WF}"

if [ "$S5_WS" -ge 1 ] && [ "$S5_ARCHITECT" -ge 1 ] && [ "$S5_CODER" -ge 1 ] && [ "$S5_TESTER" -ge 1 ] && [ "$S5_REVIEWER" -ge 1 ] && [ "$S5_WF" -ge 1 ]; then
  pass "S5: Self-bootstrap manifest applied — 4 agents (architect/coder/tester/reviewer) + workflow registered"
else
  fail "S5" "Some resources missing after apply"
fi

# Re-apply test fixture to reset state (S5 adds claude-code agent with multiline templates)
bold "[Reset] Re-applying test fixture after S5..."
$ORCH delete "project/${QA_PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
$ORCH apply -f fixtures/manifests/bundles/self-bootstrap-test.yaml --project "${QA_PROJECT}" > /dev/null 2>&1
echo ""

# ============================================================
bold "Scenario 6: Full Simplified SDLC Pipeline (plan→qa_doc_gen→implement→qa_testing→align_tests→doc_governance)"
bold "--------------------------------------------------------------"

TASK_ID=$($ORCH task create \
  --name "sdlc-full" \
  --goal "Test full SDLC pipeline" \
  --project "${QA_PROJECT}" \
  --workspace bootstrap-ws \
  --workflow sdlc_full_pipeline \
  --no-start 2>&1 | extract_task_id)

if [ -z "$TASK_ID" ]; then
  fail "S6" "task create returned no ID"
else
  echo "  Task ID: ${TASK_ID}"
  $ORCH task start "${TASK_ID}" > /dev/null 2>&1 || true
  wait_task "${TASK_ID}" 30

  S6_STATUS=$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}'" 2>/dev/null)
  S6_STEPS=$(sqlite3 "$DB" "SELECT json_extract(payload_json, '$.step') FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' ORDER BY created_at" 2>/dev/null)
  echo "  Status: ${S6_STATUS}"
  echo "  Steps executed: $(echo "$S6_STEPS" | tr '\n' ' ')"

  if [ "$S6_STATUS" != "completed" ]; then
    fail "S6" "Expected completed, got: ${S6_STATUS}"
    sqlite3 "$DB" "SELECT event_type, json_extract(payload_json, '$.step') AS step, json_extract(payload_json, '$.reason') AS reason FROM events WHERE task_id='${TASK_ID}' ORDER BY created_at" 2>/dev/null | tail -15
  else
    HAS_PLAN=$(echo "$S6_STEPS" | grep -c "^plan$" || true)
    HAS_QA_DOC_GEN=$(echo "$S6_STEPS" | grep -c "qa_doc_gen" || true)
    HAS_IMPLEMENT=$(echo "$S6_STEPS" | grep -c "implement" || true)
    HAS_QA_TESTING=$(echo "$S6_STEPS" | grep -c "qa_testing" || true)
    HAS_ALIGN=$(echo "$S6_STEPS" | grep -c "align_tests" || true)
    HAS_DOC_GOV=$(echo "$S6_STEPS" | grep -c "doc_governance" || true)

    # ticket_fix should be skipped (no tickets)
    S6_TF_SKIPPED=$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_skipped' AND json_extract(payload_json, '$.step')='ticket_fix'" 2>/dev/null || echo "0")

    MISSING=""
    [ "$HAS_PLAN" -lt 1 ] && MISSING="${MISSING} plan"
    [ "$HAS_QA_DOC_GEN" -lt 1 ] && MISSING="${MISSING} qa_doc_gen"
    [ "$HAS_IMPLEMENT" -lt 1 ] && MISSING="${MISSING} implement"
    [ "$HAS_QA_TESTING" -lt 1 ] && MISSING="${MISSING} qa_testing"
    [ "$HAS_ALIGN" -lt 1 ] && MISSING="${MISSING} align_tests"
    [ "$HAS_DOC_GOV" -lt 1 ] && MISSING="${MISSING} doc_governance"

    if [ -z "$MISSING" ] && [ "$S6_TF_SKIPPED" -ge 1 ]; then
      pass "S6: Full SDLC pipeline — 6 steps executed, ticket_fix correctly skipped"
    else
      fail "S6" "Missing steps:${MISSING}, ticket_fix_skipped=${S6_TF_SKIPPED}"
    fi
  fi
fi
echo ""

# ============================================================
bold "Scenario 7: QA Testing → Ticket Fix Chain"
bold "--------------------------------------------------------------"

# Clean tickets before this scenario
rm -f fixtures/ticket/auto_s7_*.md

TASK_ID=$($ORCH task create \
  --name "sdlc-qa-ticket" \
  --goal "Test QA ticket chain" \
  --project "${QA_PROJECT}" \
  --workspace bootstrap-ws \
  --workflow sdlc_qa_ticket_chain \
  --no-start 2>&1 | extract_task_id)

if [ -z "$TASK_ID" ]; then
  fail "S7" "task create returned no ID"
else
  echo "  Task ID: ${TASK_ID}"
  $ORCH task start "${TASK_ID}" > /dev/null 2>&1 || true
  wait_task "${TASK_ID}" 30

  S7_STATUS=$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}'" 2>/dev/null)
  S7_STEPS=$(sqlite3 "$DB" "SELECT json_extract(payload_json, '$.step') FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' ORDER BY created_at" 2>/dev/null)
  S7_QA_TESTING=$(echo "$S7_STEPS" | grep -c "qa_testing" || true)
  S7_TICKET_FIX=$(echo "$S7_STEPS" | grep -c "ticket_fix" || true)

  echo "  Status: ${S7_STATUS}, qa_testing ran: ${S7_QA_TESTING}, ticket_fix ran: ${S7_TICKET_FIX}"
  echo "  Steps: $(echo "$S7_STEPS" | tr '\n' ' ')"

  if [ "$S7_QA_TESTING" -ge 1 ] && [ "$S7_TICKET_FIX" -ge 1 ]; then
    pass "S7: QA testing → ticket_fix chain executed"
  else
    fail "S7" "qa_testing=${S7_QA_TESTING} ticket_fix=${S7_TICKET_FIX}"
    sqlite3 "$DB" "SELECT event_type, json_extract(payload_json, '$.step') AS step, json_extract(payload_json, '$.reason') AS reason FROM events WHERE task_id='${TASK_ID}' ORDER BY created_at" 2>/dev/null | tail -15
  fi
fi
# Clean up any leftover tickets
rm -f fixtures/ticket/auto_s7_*.md
echo ""

# ============================================================
bold "Scenario 8: Clean QA Testing → Ticket Fix Skipped"
bold "--------------------------------------------------------------"

# Ensure no tickets exist
rm -f fixtures/ticket/auto_*.md

TASK_ID=$($ORCH task create \
  --name "sdlc-ticket-skip" \
  --goal "Test ticket fix skip when no tickets" \
  --project "${QA_PROJECT}" \
  --workspace bootstrap-ws \
  --workflow sdlc_ticket_skip \
  --no-start 2>&1 | extract_task_id)

if [ -z "$TASK_ID" ]; then
  fail "S8" "task create returned no ID"
else
  echo "  Task ID: ${TASK_ID}"
  $ORCH task start "${TASK_ID}" > /dev/null 2>&1 || true
  wait_task "${TASK_ID}" 30

  S8_STATUS=$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}'" 2>/dev/null)
  S8_QA_STARTED=$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' AND json_extract(payload_json, '$.step')='qa_testing'" 2>/dev/null || echo "0")
  S8_TF_SKIPPED=$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_skipped' AND json_extract(payload_json, '$.step')='ticket_fix'" 2>/dev/null || echo "0")
  S8_TF_STARTED=$(sqlite3 "$DB" "SELECT COUNT(*) FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' AND json_extract(payload_json, '$.step')='ticket_fix'" 2>/dev/null || echo "0")

  echo "  Status: ${S8_STATUS}, qa_testing started: ${S8_QA_STARTED}, ticket_fix skipped: ${S8_TF_SKIPPED}, ticket_fix started: ${S8_TF_STARTED}"

  if [ "$S8_QA_STARTED" -ge 1 ] && [ "$S8_TF_SKIPPED" -ge 1 ] && [ "$S8_TF_STARTED" -eq 0 ]; then
    pass "S8: Ticket fix correctly skipped when no active tickets"
  elif [ "$S8_TF_STARTED" -ge 1 ]; then
    fail "S8" "Ticket fix should not have started (no tickets)"
  else
    fail "S8" "qa_testing=${S8_QA_STARTED} ticket_fix_skipped=${S8_TF_SKIPPED}"
    sqlite3 "$DB" "SELECT event_type, json_extract(payload_json, '$.step') AS step, json_extract(payload_json, '$.reason') AS reason FROM events WHERE task_id='${TASK_ID}' ORDER BY created_at" 2>/dev/null | tail -15
  fi
fi
echo ""

# ============================================================
bold "Scenario 9: Pipeline Variable Propagation ({source_tree} rendered in step commands)"
bold "--------------------------------------------------------------"

TASK_ID=$($ORCH task create \
  --name "sdlc-pipeline-vars" \
  --goal "Test pipeline variable propagation" \
  --project "${QA_PROJECT}" \
  --workspace bootstrap-ws \
  --workflow sdlc_pipeline_vars \
  --no-start 2>&1 | extract_task_id)

if [ -z "$TASK_ID" ]; then
  fail "S9" "task create returned no ID"
else
  echo "  Task ID: ${TASK_ID}"
  $ORCH task start "${TASK_ID}" > /dev/null 2>&1 || true
  wait_task "${TASK_ID}" 30

  S9_STATUS=$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}'" 2>/dev/null)

  # Verify the steps all ran
  S9_STEPS=$(sqlite3 "$DB" "SELECT json_extract(payload_json, '$.step') FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' ORDER BY created_at" 2>/dev/null)
  HAS_PLAN=$(echo "$S9_STEPS" | grep -c "^plan$" || true)
  HAS_IMPLEMENT=$(echo "$S9_STEPS" | grep -c "implement" || true)
  HAS_ALIGN=$(echo "$S9_STEPS" | grep -c "align_tests" || true)
  echo "  Status: ${S9_STATUS}"
  echo "  Steps: $(echo "$S9_STEPS" | tr '\n' ' ')"

  if [ "$S9_STATUS" = "completed" ] && [ "$HAS_PLAN" -ge 1 ] && [ "$HAS_IMPLEMENT" -ge 1 ] && [ "$HAS_ALIGN" -ge 1 ]; then
    pass "S9: Pipeline variable propagation — plan→implement→align_tests completed"
  else
    fail "S9" "status=${S9_STATUS} plan=${HAS_PLAN} implement=${HAS_IMPLEMENT} align=${HAS_ALIGN}"
  fi
fi
echo ""

# ============================================================
bold "Scenario 10: Align Tests as Safety Net After Implement"
bold "--------------------------------------------------------------"

TASK_ID=$($ORCH task create \
  --name "sdlc-align-safety" \
  --goal "Test align_tests covers build+test+lint after implement" \
  --project "${QA_PROJECT}" \
  --workspace bootstrap-ws \
  --workflow sdlc_align_after_implement \
  --no-start 2>&1 | extract_task_id)

if [ -z "$TASK_ID" ]; then
  fail "S10" "task create returned no ID"
else
  echo "  Task ID: ${TASK_ID}"
  $ORCH task start "${TASK_ID}" > /dev/null 2>&1 || true
  wait_task "${TASK_ID}" 30

  S10_STATUS=$(sqlite3 "$DB" "SELECT status FROM tasks WHERE id='${TASK_ID}'" 2>/dev/null)
  S10_STEPS=$(sqlite3 "$DB" "SELECT json_extract(payload_json, '$.step') FROM events WHERE task_id='${TASK_ID}' AND event_type='step_started' ORDER BY created_at" 2>/dev/null)

  HAS_IMPLEMENT=$(echo "$S10_STEPS" | grep -c "implement" || true)
  HAS_QA_TESTING=$(echo "$S10_STEPS" | grep -c "qa_testing" || true)
  HAS_ALIGN=$(echo "$S10_STEPS" | grep -c "align_tests" || true)
  HAS_DOC_GOV=$(echo "$S10_STEPS" | grep -c "doc_governance" || true)

  echo "  Status: ${S10_STATUS}"
  echo "  Steps: $(echo "$S10_STEPS" | tr '\n' ' ')"

  if [ "$S10_STATUS" = "completed" ] && [ "$HAS_IMPLEMENT" -ge 1 ] && [ "$HAS_QA_TESTING" -ge 1 ] && [ "$HAS_ALIGN" -ge 1 ] && [ "$HAS_DOC_GOV" -ge 1 ]; then
    pass "S10: implement→qa_testing→align_tests→doc_governance — align_tests as safety net"
  else
    fail "S10" "status=${S10_STATUS} implement=${HAS_IMPLEMENT} qa=${HAS_QA_TESTING} align=${HAS_ALIGN} doc=${HAS_DOC_GOV}"
  fi
fi
echo ""

# ============================================================
bold "======================================"
bold " Results: ${PASS} passed, ${FAIL} failed, ${SKIP} skipped"
bold "======================================"

if [ "$FAIL" -gt 0 ]; then
  exit 1
fi
exit 0
