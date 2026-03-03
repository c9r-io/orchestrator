#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

if ! qa_parse_common_args "$@"; then
  qa_print_usage
  exit 1
fi

REPO_ROOT="$(qa_repo_root)"
BINARY="$(qa_binary_path)"
qa_require_binary
cd "$REPO_ROOT"

qa_info "Preparing isolated config for exec interactive simulation..."

mkdir -p "$REPO_ROOT/workspace/default/docs/qa" "$REPO_ROOT/workspace/default/docs/ticket"
cat > "$REPO_ROOT/workspace/default/docs/qa/exec-smoke.md" <<'MD'
# Exec Interactive Smoke
MD

MANIFEST="$REPO_ROOT/workspace/exec-interactive-flow.yaml"
cat > "$MANIFEST" <<'YAML'
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: workspace/default
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: exec-agent
spec:
  command: "echo '{\"confidence\":1.0,\"quality_score\":1.0,\"artifacts\":[]}'"
  capabilities:
    - qa
    - plan
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: exec_interactive_flow
spec:
  steps:
    - id: qa
      type: qa
      required_capability: qa
      enabled: true
      repeatable: true
      tty: false
  loop:
    mode: once
  finalize:
    rules: []
YAML

qa_apply_fixture_additive "$MANIFEST"

qa_resolve_project "qa-exec-interactive"
qa_recreate_project "exec_interactive_flow"

TASK_OUTPUT="$("$BINARY" task create --project "$QA_PROJECT" --workspace "$QA_WORKSPACE" --workflow exec_interactive_flow --name "exec-interactive-$(date +%s)" --goal "simulate interactive exec" --no-start 2>&1)"
TASK_ID="$(qa_extract_task_id "$TASK_OUTPUT")"
if [[ -z "$TASK_ID" ]]; then
  qa_error "Failed to create task: $TASK_OUTPUT"
  exit 2
fi

EDIT_OUT="$("$BINARY" task edit "$TASK_ID" --insert-before qa --step plan --tty 2>&1)"
PLAN_STEP_ID="$(echo "$EDIT_OUT" | sed -n "s/.*inserted step '\([^']*\)'.*/\1/p")"
if [[ -z "$PLAN_STEP_ID" ]]; then
  qa_error "Failed to parse plan step id: $EDIT_OUT"
  exit 3
fi

"$BINARY" task start "$TASK_ID" >/dev/null 2>&1 || true

PIPE_OUT="$(printf 'sim-tty-input\n' | "$BINARY" exec -it "task/$TASK_ID/step/$PLAN_STEP_ID" -- cat 2>&1 || true)"
PIPE_PASS=0
if echo "$PIPE_OUT" | rg -q "sim-tty-input"; then
  PIPE_PASS=1
fi

HEREDOC_OUT="$(
  cat <<EOF | "$BINARY" exec -it "task/$TASK_ID/step/$PLAN_STEP_ID" -- /bin/bash 2>&1 || true
echo SIM-HEREDOC
exit
EOF
)"
HEREDOC_PASS=0
if echo "$HEREDOC_OUT" | rg -q "SIM-HEREDOC"; then
  HEREDOC_PASS=1
fi

set +e
NON_TTY_OUT="$("$BINARY" exec -it "task/$TASK_ID/step/qa" -- cat 2>&1)"
NON_TTY_CODE=$?
set -e
NON_TTY_REJECT_PASS=0
if [[ "$NON_TTY_CODE" -ne 0 ]] && echo "$NON_TTY_OUT" | rg -q "tty disabled"; then
  NON_TTY_REJECT_PASS=1
fi

PASS=0
if [[ "$PIPE_PASS" -eq 1 && "$HEREDOC_PASS" -eq 1 && "$NON_TTY_REJECT_PASS" -eq 1 ]]; then
  PASS=1
fi

if [[ "${QA_OUTPUT_JSON:-0}" -eq 1 ]]; then
  printf '{"task_id":"%s","plan_step_id":"%s","pipe_pass":%s,"heredoc_pass":%s,"non_tty_reject_pass":%s,"pass":%s}\n' \
    "$TASK_ID" "$PLAN_STEP_ID" \
    "$([[ "$PIPE_PASS" -eq 1 ]] && echo true || echo false)" \
    "$([[ "$HEREDOC_PASS" -eq 1 ]] && echo true || echo false)" \
    "$([[ "$NON_TTY_REJECT_PASS" -eq 1 ]] && echo true || echo false)" \
    "$([[ "$PASS" -eq 1 ]] && echo true || echo false)"
else
  echo "Task ID: $TASK_ID"
  echo "Plan Step ID: $PLAN_STEP_ID"
  echo "Pipe PASS: $PIPE_PASS"
  echo "Here-doc PASS: $HEREDOC_PASS"
  echo "Non-TTY Reject PASS: $NON_TTY_REJECT_PASS"
  [[ "$PASS" -eq 1 ]] && echo "RESULT: PASS" || echo "RESULT: FAIL"
fi

if [[ "$PASS" -ne 1 ]]; then
  exit 4
fi
