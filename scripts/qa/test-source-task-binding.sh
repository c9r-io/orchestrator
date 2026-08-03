#!/usr/bin/env bash

set -euo pipefail

# FR-158: record this run in config/governance/manual-gate-freshness.json.
# Sourced before the gate's own trap so gate_runlog_arm can compose with it.
. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
GRPC_BIND="${GRPC_BIND:-127.0.0.1:19220}"
WEBHOOK_BIND="${WEBHOOK_BIND:-127.0.0.1:19221}"
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
  echo "binaries not found; build orchestratord and orchestrator-cli first" >&2
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
gate_runlog_arm "scripts/qa/test-source-task-binding.sh"

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
  DAEMON_PID="$(<"$QA_ROOT/daemon.pid")"
  for _ in {1..60}; do
    "$ORCH" task list -o json >/dev/null 2>&1 && return 0
    sleep 0.25
  done
  echo "isolated daemon failed to start" >&2
  sed -n '1,240p' "$QA_ROOT/daemon.log" >&2
  return 1
}

stop_daemon() {
  kill "$DAEMON_PID" 2>/dev/null || true
  wait "$DAEMON_PID" 2>/dev/null || true
  DAEMON_PID=""
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

PROJECT="qa-source-binding"
FIXTURE="$REPO_ROOT/fixtures/manifests/bundles/source-task-binding-fixture.yaml"
start_daemon
wait_for_active_key
"$ORCH" apply --project "$PROJECT" -f "$FIXTURE" >/dev/null

"$ORCH" get sourcetaskbinding slack-code-analysis --project "$PROJECT" -o json > "$QA_ROOT/get.json"
"$ORCH" describe sourcetaskbinding/slack-code-analysis --project "$PROJECT" -o yaml > "$QA_ROOT/describe.yaml"
"$ORCH" manifest export -o yaml > "$QA_ROOT/export.yaml"
if jq -e '
    .kind == "SourceTaskBinding" and
    .metadata.name == "slack-code-analysis" and
    .spec.triggerRef == "slack-main" and
    .spec.match.reaction == "agent-analyze" and
    .spec.match.channels == ["C_QA_ALLOWED"] and
    .spec.templateRef == "analyze-from-slack" and
    .spec.allowedActorRoles == ["operator", "admin"]
  ' "$QA_ROOT/get.json" >/dev/null &&
  grep -q 'kind: SourceTaskBinding' "$QA_ROOT/describe.yaml" &&
  grep -q 'name: slack-code-analysis' "$QA_ROOT/export.yaml"; then
  pass "SourceTaskBinding apply, get, describe, and export round-trip"
else
  fail "SourceTaskBinding resource lifecycle projection differs"
fi

simulate() {
  "$ORCH" source binding simulate \
    --project "$PROJECT" \
    --provider slack \
    --installation T_QA_BINDING \
    --reaction agent-analyze \
    --channel C_QA_ALLOWED \
    --actor U_OPERATOR \
    -o json "$@"
}

simulate > "$QA_ROOT/matched.json"
REVISION="$(jq -r '.binding_revision' "$QA_ROOT/matched.json")"
if jq -e '
    .status == "matched" and
    .reason == "binding_matched" and
    .trigger_name == "slack-main" and
    .resolved_role == "operator" and
    .binding_id == "slack-code-analysis" and
    .template_ref == "analyze-from-slack" and
    (.binding_revision | test("^[0-9a-f]{64}$")) and
    (.candidates | length == 1) and
    .candidates[0].reason == "matched"
  ' "$QA_ROOT/matched.json" >/dev/null; then
  pass "exact reaction, channel, installation, and trusted role select one template"
else
  fail "exact deterministic match result differs"
fi

"$ORCH" source binding simulate --project "$PROJECT" --provider slack \
  --installation T_QA_BINDING --reaction wrong --channel C_QA_ALLOWED \
  --actor U_OPERATOR -o json > "$QA_ROOT/wrong-reaction.json"
"$ORCH" source binding simulate --project "$PROJECT" --provider slack \
  --installation T_QA_BINDING --reaction agent-analyze --channel C_OTHER \
  --actor U_OPERATOR -o json > "$QA_ROOT/wrong-channel.json"
"$ORCH" source binding simulate --project "$PROJECT" --provider slack \
  --installation T_QA_BINDING --reaction agent-analyze --channel C_QA_ALLOWED \
  --actor U_READER -o json > "$QA_ROOT/wrong-role.json"
"$ORCH" source binding simulate --project "$PROJECT" --provider slack \
  --installation T_OTHER --reaction agent-analyze --channel C_QA_ALLOWED \
  --actor U_OPERATOR -o json > "$QA_ROOT/wrong-installation.json"
"$ORCH" source binding simulate --project "$PROJECT" --provider slack \
  --installation T_QA_BINDING --reaction agent-analyze --target-kind file \
  --channel C_QA_ALLOWED --actor U_OPERATOR -o json > "$QA_ROOT/wrong-target.json"
"$ORCH" source binding simulate --project "$PROJECT" --provider slack \
  --installation T_QA_BINDING --reaction agent-analyze --channel C_QA_ALLOWED \
  --actor U_UNKNOWN -o json > "$QA_ROOT/unknown-actor.json"
if [[ "$(jq -r '.reason' "$QA_ROOT/wrong-reaction.json")" == "reaction_mismatch" ]] &&
  [[ "$(jq -r '.reason' "$QA_ROOT/wrong-channel.json")" == "channel_not_allowed" ]] &&
  [[ "$(jq -r '.reason' "$QA_ROOT/wrong-role.json")" == "actor_role_not_allowed" ]] &&
  [[ "$(jq -r '.reason' "$QA_ROOT/wrong-installation.json")" == "trigger_not_found" ]] &&
  [[ "$(jq -r '.reason' "$QA_ROOT/wrong-target.json")" == "target_kind_mismatch" ]] &&
  [[ "$(jq -r '.reason' "$QA_ROOT/unknown-actor.json")" == "actor_unknown" ]]; then
  pass "no-match matrix returns stable safe reason codes"
else
  fail "no-match reason matrix differs"
fi

OVERLAP="$QA_ROOT/overlap.yaml"
cat > "$OVERLAP" <<'EOF'
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: overlapping-analysis
spec:
  triggerRef: slack-main
  match:
    eventKind: reaction_added
    reaction: agent-analyze
    targetKind: message
    allChannels: true
  templateRef: analyze-from-slack
  allowedActorRoles: [operator]
EOF
if ! "$ORCH" apply --project "$PROJECT" -f "$OVERLAP" > "$QA_ROOT/overlap.log" 2>&1 &&
  grep -q 'overlaps enabled binding' "$QA_ROOT/overlap.log" &&
  [[ "$(simulate | jq -r '.binding_id')" == "slack-code-analysis" ]]; then
  pass "overlapping enabled rules fail atomically without replacing active config"
else
  fail "overlap rejection or active-config rollback differs"
fi

"$ORCH" source binding suspend slack-code-analysis --project "$PROJECT" > "$QA_ROOT/suspend.yaml"
simulate > "$QA_ROOT/suspended.json"
"$ORCH" source binding resume slack-code-analysis --project "$PROJECT" > "$QA_ROOT/resume.yaml"
simulate > "$QA_ROOT/resumed.json"
RESUMED_REVISION="$(jq -r '.binding_revision' "$QA_ROOT/resumed.json")"
stop_daemon
start_daemon
simulate > "$QA_ROOT/restarted.json"
RESTARTED_REVISION="$(jq -r '.binding_revision' "$QA_ROOT/restarted.json")"
"$ORCH" audit list --project "$PROJECT" -o json > "$QA_ROOT/audit.json"
if [[ "$(jq -r '.reason' "$QA_ROOT/suspended.json")" == "binding_suspended" ]] &&
  [[ "$REVISION" == "$RESUMED_REVISION" && "$RESUMED_REVISION" == "$RESTARTED_REVISION" ]] &&
  jq -e '
    any(.[]; .action == "source.binding.suspend" and .status == "succeeded") and
    any(.[]; .action == "source.binding.resume" and .status == "succeeded") and
    any(.[]; .action == "source.binding.apply" and .status == "succeeded")
  ' "$QA_ROOT/audit.json" >/dev/null &&
  ! grep -Eq 'slack\.com/archives|message_url|message_body' "$QA_ROOT/audit.json"; then
  pass "suspend/resume hot reload, restart revision, and canonical audit are safe"
else
  fail "binding mutation, restart, or audit contract differs"
fi

if ! "$ORCH" delete sourcetasktemplate/analyze-from-slack --project "$PROJECT" --force \
    > "$QA_ROOT/delete-blocked.log" 2>&1 &&
  grep -q 'referenced by SourceTaskBinding' "$QA_ROOT/delete-blocked.log" &&
  "$ORCH" delete sourcetasktemplate/analyze-from-slack --project "$PROJECT" --force \
    --force-references >/dev/null &&
  ! "$ORCH" get sourcetaskbinding slack-code-analysis --project "$PROJECT" -o json \
    >/dev/null 2>&1; then
  "$ORCH" apply --project "$PROJECT" -f "$FIXTURE" >/dev/null
  if ! "$ORCH" delete trigger/slack-main --project "$PROJECT" --force \
      > "$QA_ROOT/delete-trigger-blocked.log" 2>&1 &&
    grep -q 'referenced by SourceTaskBinding' "$QA_ROOT/delete-trigger-blocked.log" &&
    "$ORCH" delete trigger/slack-main --project "$PROJECT" --force --force-references \
      >/dev/null &&
    ! "$ORCH" get sourcetaskbinding slack-code-analysis --project "$PROJECT" -o json \
      >/dev/null 2>&1; then
    "$ORCH" apply --project "$PROJECT" -f "$FIXTURE" >/dev/null
    "$ORCH" delete sourcetaskbinding/slack-code-analysis --project "$PROJECT" --force \
      >/dev/null
    "$ORCH" audit list --project "$PROJECT" --action source.binding.delete -o json \
      > "$QA_ROOT/delete-audit.json"
    if ! "$ORCH" get sourcetaskbinding slack-code-analysis --project "$PROJECT" -o json \
        >/dev/null 2>&1 &&
      jq -e 'any(.[]; .action == "source.binding.delete" and .status == "succeeded")' \
        "$QA_ROOT/delete-audit.json" >/dev/null; then
      pass "template, trigger, and direct binding deletion are reference-safe and audited"
    else
      fail "direct SourceTaskBinding deletion or audit differs"
    fi
  else
    fail "Trigger reference-safe deletion differs"
  fi
else
  fail "SourceTaskTemplate reference-safe deletion differs"
fi

echo ""
echo "Source task binding QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
