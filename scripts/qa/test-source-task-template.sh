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
GRPC_BIND="${GRPC_BIND:-127.0.0.1:19218}"
WEBHOOK_BIND="${WEBHOOK_BIND:-127.0.0.1:19219}"
PASS=0
FAIL=0
DAEMON_PID=""

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in jq mktemp sqlite3; do
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
gate_runlog_arm "scripts/qa/test-source-task-template.sh"

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/runtime"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/docs/qa/orchestrator" "$QA_ROOT/docs/ticket"

start_daemon() {
  (
    cd "$QA_ROOT"
    "$ORCHD" --foreground --bind "$GRPC_BIND" --webhook-bind "$WEBHOOK_BIND" --workers 1 \
      --uds-max-role admin > daemon.log 2>&1 &
    echo $! > daemon.pid
  )
  DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"
  gate_daemon_wait_ready "$ORCH" && return 0
  echo "isolated daemon failed to start" >&2
  sed -n '1,240p' "$QA_ROOT/daemon.log" >&2
  return 1
}

stop_daemon() {
  local rc=0
  gate_daemon_stop "$DAEMON_PID" "$ORCHESTRATORD_DATA_DIR/daemon.pid" || rc=$?
  DAEMON_PID=""
  return "$rc"
}

wait_for_active_key() {
  for _ in {1..60}; do
    if "$ORCH" secret key status -o json 2>/dev/null | jq -e '.active_key != null' >/dev/null; then
      return 0
    fi
    sleep 0.25
  done
  echo "isolated daemon did not publish an active encryption key" >&2
  return 1
}

start_daemon
wait_for_active_key
PROJECT="qa-source-template"
FIXTURE="$REPO_ROOT/fixtures/manifests/bundles/source-task-template-fixture.yaml"
"$ORCH" apply --project "$PROJECT" -f "$FIXTURE" >/dev/null

"$ORCH" get sourcetasktemplate slack-docs --project "$PROJECT" -o json > "$QA_ROOT/get.json"
"$ORCH" describe sourcetasktemplate/slack-docs --project "$PROJECT" -o yaml > "$QA_ROOT/describe.yaml"
"$ORCH" manifest export -o yaml > "$QA_ROOT/export.yaml"
if jq -e '
    .kind == "SourceTaskTemplate" and
    .metadata.name == "slack-docs" and
    .spec.skill.invocation == "$docs" and
    .spec.action.workflow == "source-template-fixture"
  ' "$QA_ROOT/get.json" >/dev/null &&
  grep -q 'kind: SourceTaskTemplate' "$QA_ROOT/describe.yaml" &&
  grep -q 'name: slack-docs' "$QA_ROOT/export.yaml"; then
  pass "SourceTaskTemplate supports apply, get, describe, and export"
else
  fail "SourceTaskTemplate resource lifecycle projection differs"
fi

DB="$QA_ROOT/runtime/agent_orchestrator.db"
TASKS_BEFORE="$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE project_id='$PROJECT';")"
EVENTS_BEFORE="$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_events WHERE project_id='$PROJECT';")"
BINDINGS_BEFORE="$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_bindings WHERE project_id='$PROJECT';")"
preview() {
  "$ORCH" source template preview slack-docs \
    --project "$PROJECT" \
    --provider slack \
    --installation qa-installation \
    --message-url 'https://qa.slack.com/archives/C123/p1234567890000100?thread_ts={source_reaction}' \
    -o json
}
preview > "$QA_ROOT/preview-v1.json"
HASH_V1="$(jq -r '.content_hash' "$QA_ROOT/preview-v1.json")"
if jq -e '
    .skill.name == "docs" and
    .skill.invocation == "$docs" and
    .skill.args == ["--concise"] and
    (.goal | startswith("{source} $docs: review https://qa.slack.com/archives/")) and
    (.goal | contains("{source_reaction}")) and
    .action.workflow == "source-template-fixture" and
    .action.workspace == "source-template-fixture" and
    .action.start == true and
    .action.initial_vars.provenance == "[REDACTED]-value" and
    (.content_hash | test("^[0-9a-f]{64}$")) and
    .revision == .content_hash and
    (.warnings | index("sample_url_not_verified_against_installation") != null)
  ' "$QA_ROOT/preview-v1.json" >/dev/null &&
  ! grep -q 'qa-sensitive-value' "$QA_ROOT/preview-v1.json"; then
  pass "preview renders once, preserves inert source tokens, hashes, warns, and redacts"
else
  fail "preview rendering, hash, warning, or redaction contract differs"
fi

TASKS_AFTER="$(sqlite3 "$DB" "SELECT COUNT(*) FROM tasks WHERE project_id='$PROJECT';")"
EVENTS_AFTER="$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_events WHERE project_id='$PROJECT';")"
BINDINGS_AFTER="$(sqlite3 "$DB" "SELECT COUNT(*) FROM source_bindings WHERE project_id='$PROJECT';")"
if [[ "$TASKS_BEFORE:$EVENTS_BEFORE:$BINDINGS_BEFORE" == "$TASKS_AFTER:$EVENTS_AFTER:$BINDINGS_AFTER" ]]; then
  pass "preview is read-only for task, source event, and source binding persistence"
else
  fail "preview mutated task or source persistence"
fi

UPDATE_MANIFEST="$QA_ROOT/update.yaml"
cat > "$UPDATE_MANIFEST" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: SourceTaskTemplate
metadata:
  name: slack-docs
spec:
  skill:
    name: docs
    invocation: "$docs-v2"
    args: ["--concise"]
  action:
    workflow: source-template-fixture
    workspace: source-template-fixture
    start: true
    initial_vars:
      provenance: qa-sensitive-value
  goalTemplate: "{skill_invocation}: review {source_message_url}"
  allowedVariables: [skill_invocation, source_message_url]
EOF
"$ORCH" apply --project "$PROJECT" -f "$UPDATE_MANIFEST" >/dev/null
preview > "$QA_ROOT/preview-v2.json"
HASH_V2="$(jq -r '.content_hash' "$QA_ROOT/preview-v2.json")"
stop_daemon
start_daemon
preview > "$QA_ROOT/preview-restart.json"
HASH_RESTART="$(jq -r '.content_hash' "$QA_ROOT/preview-restart.json")"
if [[ "$HASH_V1" != "$HASH_V2" && "$HASH_V2" == "$HASH_RESTART" ]] &&
  [[ "$(jq -r '.skill.invocation' "$QA_ROOT/preview-restart.json")" == '$docs-v2' ]]; then
  pass "hot apply changes the revision and daemon restart preserves it deterministically"
else
  fail "hot reload or restart revision stability differs"
fi

INVALID_VARIABLE="$QA_ROOT/invalid-variable.yaml"
cat > "$INVALID_VARIABLE" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: SourceTaskTemplate
metadata:
  name: invalid-variable
spec:
  skill: {name: docs, invocation: "$docs"}
  action: {workflow: source-template-fixture, workspace: source-template-fixture}
  goalTemplate: "review {source_message_url} {source_body}"
  allowedVariables: [source_message_url, source_body]
EOF
INVALID_REFERENCE="$QA_ROOT/invalid-reference.yaml"
cat > "$INVALID_REFERENCE" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: SourceTaskTemplate
metadata:
  name: invalid-reference
spec:
  skill: {name: docs, invocation: "$docs"}
  action: {workflow: missing-workflow, workspace: source-template-fixture}
  goalTemplate: "review {source_message_url}"
  allowedVariables: [source_message_url]
EOF
if ! "$ORCH" apply --project "$PROJECT" -f "$INVALID_VARIABLE" >"$QA_ROOT/invalid-variable.log" 2>&1 &&
  ! "$ORCH" apply --project "$PROJECT" -f "$INVALID_REFERENCE" >"$QA_ROOT/invalid-reference.log" 2>&1 &&
  [[ "$(preview | jq -r '.content_hash')" == "$HASH_V2" ]]; then
  pass "invalid variables and missing references fail closed without replacing active config"
else
  fail "invalid template validation or active-config rollback differs"
fi

BINDING_MANIFEST="$QA_ROOT/binding.yaml"
cat > "$BINDING_MANIFEST" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: slack-template-reference-secret
spec:
  data:
    signing: qa-template-reference-signing-secret
---
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: slack-template-reference
spec:
  event:
    source: webhook
    webhook:
      provider: slack
      installationId: qa-template-reference
      actorRoles: {qa-operator: operator}
      secret:
        fromRef: slack-template-reference-secret
  action:
    workflow: source-template-fixture
    workspace: source-template-fixture
---
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: slack-docs-binding
spec:
  triggerRef: slack-template-reference
  match:
    eventKind: reaction_added
    reaction: agent-docs
    targetKind: message
    channels: [C_TEMPLATE_QA]
  templateRef: slack-docs
  allowedActorRoles: [operator]
  suspend: false
EOF
"$ORCH" apply --project "$PROJECT" -f "$BINDING_MANIFEST" >/dev/null
if ! "$ORCH" delete sourcetasktemplate/slack-docs --project "$PROJECT" --force >"$QA_ROOT/delete-blocked.log" 2>&1 &&
  grep -q 'referenced by SourceTaskBinding' "$QA_ROOT/delete-blocked.log" &&
  "$ORCH" delete sourcetasktemplate/slack-docs --project "$PROJECT" --force --force-references >/dev/null &&
  ! "$ORCH" get sourcetasktemplate slack-docs --project "$PROJECT" -o json >/dev/null 2>&1 &&
  ! "$ORCH" get sourcetaskbinding slack-docs-binding --project "$PROJECT" -o json >/dev/null 2>&1; then
  "$ORCH" audit list --project "$PROJECT" --action delete_references -o json > "$QA_ROOT/audit.json"
  if jq -e '
      length == 1 and
      .[0].resolved_role == "admin" and
      .[0].target_type == "source_task_template" and
      .[0].target_id == "sourcetasktemplate/slack-docs" and
      .[0].action == "delete_references" and
      .[0].status == "succeeded"
    ' "$QA_ROOT/audit.json" >/dev/null &&
    ! grep -Eq 'qa-sensitive|slack\.com/archives' "$QA_ROOT/audit.json"; then
    pass "referenced delete is blocked; Admin force cleanup is atomic and audited"
  else
    fail "force-reference cleanup audit contract differs"
  fi
else
  fail "reference-aware delete contract differs"
fi

echo ""
echo "Source task template QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
