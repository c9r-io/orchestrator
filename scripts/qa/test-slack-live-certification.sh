#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=scripts/qa/lib/slack-live-certification-lib.sh
source "$SCRIPT_DIR/lib/slack-live-certification-lib.sh"

QA_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/slack-certification-unit.XXXXXX")"
export SLACK_CERT_STATE_HOME="$QA_ROOT/state"
PASS=0

cleanup() {
  find "$QA_ROOT" -depth -delete 2>/dev/null || true
}
trap cleanup EXIT INT TERM

pass() {
  printf '  PASS: %s\n' "$1"
  PASS=$((PASS + 1))
}

fail() {
  printf '  FAIL: %s\n' "$1" >&2
  exit 1
}

expect_failure() {
  if "$@" >/dev/null 2>&1; then
    return 1
  fi
}

for command in bash date git jq mktemp rg stat; do
  command -v "$command" >/dev/null 2>&1 || fail "missing command: $command"
done

VALID_ENV="$QA_ROOT/valid.env"
cat >"$VALID_ENV" <<'EOF'
ORCHESTRATOR_BIN=/tmp/orchestrator
SLACK_CERT_TTL_DAYS=14
SLACK_LIVE_IMPLEMENT_SKILL_MARKER='$ticket-fix'
SLACK_LIVE_SHARED_A_DRIVER_BOT_TOKEN='literal-token-value'
EOF
chmod 600 "$VALID_ENV"
slack_cert_load_env "$VALID_ENV"
[[ "$SLACK_CERT_TTL_DAYS" == "14" ]] || fail "valid env TTL was not parsed"
[[ "$SLACK_LIVE_IMPLEMENT_SKILL_MARKER" == '$ticket-fix' ]] \
  || fail "quoted env value was evaluated or changed"
pass "mode-0600 allowlisted env is parsed as inert data"

BAD_KEY_ENV="$QA_ROOT/bad-key.env"
printf 'UNREVIEWED_SECRET=value\n' >"$BAD_KEY_ENV"
chmod 600 "$BAD_KEY_ENV"
expect_failure slack_cert_load_env "$BAD_KEY_ENV" \
  || fail "unsupported env key was accepted"

EXEC_ENV="$QA_ROOT/executable.env"
printf 'SLACK_LIVE_PROJECT=$(touch /tmp/should-not-exist)\n' >"$EXEC_ENV"
chmod 600 "$EXEC_ENV"
expect_failure slack_cert_load_env "$EXEC_ENV" \
  || fail "command substitution was accepted"
[[ ! -e /tmp/should-not-exist ]] || fail "env parser executed shell syntax"

PUBLIC_ENV="$QA_ROOT/public.env"
printf 'SLACK_LIVE_PROJECT=test\n' >"$PUBLIC_ENV"
chmod 644 "$PUBLIC_ENV"
expect_failure slack_cert_load_env "$PUBLIC_ENV" \
  || fail "world-readable live env was accepted"
pass "unknown keys, shell syntax, and unsafe env permissions fail closed"

slack_cert_init_state state-run both 30 deadbeef
STATE_FILE="$(slack_cert_state_file state-run)"
SAFE_FILE="$(slack_cert_safe_file state-run)"
[[ "$(slack_cert_next_stage state-run)" == "preflight" ]] \
  || fail "initial checkpoint is not preflight"
slack_cert_state_stage_result state-run preflight waiting human_provider_checkpoint
[[ "$(slack_cert_next_stage state-run)" == "preflight" ]] \
  || fail "waiting checkpoint did not remain resumable"
slack_cert_state_stage_result state-run preflight pass live_preflight_ok
[[ "$(slack_cert_next_stage state-run)" == "recorded_fixtures" ]] \
  || fail "completed checkpoint did not advance"
pass "stable checkpoint state resumes and advances idempotently"

slack_cert_inventory_add state-run source_connection raw-private-connection disconnect false
slack_cert_inventory_add state-run slack_app raw-private-app delete_slack_app true
rg -q 'raw-private-connection|raw-private-app' "$STATE_FILE" \
  || fail "private inventory did not retain cleanup identity"
if rg -q 'raw-private-connection|raw-private-app|private_salt|private_id' "$SAFE_FILE"; then
  fail "safe evidence leaked private inventory identity or salt"
fi
jq -e '.inventory | length == 2
  and all(.[]; (.identity_digest | length == 64))' "$SAFE_FILE" >/dev/null \
  || fail "safe inventory digest projection is invalid"
pass "private cleanup inventory projects only anonymous safe identities"

slack_cert_cleanup_mark state-run false
jq -e '
  any(.inventory[]; .destructive == false and .cleanup_result == "cleaned")
  and any(.inventory[]; .destructive == true and .cleanup_result == "pending")
  and .cleanup.destructive_confirmation == false
' "$SAFE_FILE" >/dev/null || fail "non-destructive cleanup did not preserve destructive pending state"
slack_cert_cleanup_mark state-run true
jq -e '.cleanup.result == "pass"
  and .cleanup.destructive_confirmation == true
  and all(.inventory[]; .cleanup_result == "cleaned")' "$SAFE_FILE" >/dev/null \
  || fail "explicit destructive cleanup confirmation was not recorded"
pass "cleanup is rerunnable and destructive objects require a distinct confirmation"

mkdir -p "$(slack_cert_run_dir state-run)/logs"
SECRET_VALUE="known-certification-secret"
slack_cert_register_known_secret state-run "$SECRET_VALUE"
[[ ! -e "$(slack_cert_run_dir state-run)/.known-secrets" ]] \
  || fail "known secret was persisted as a scan artifact"
printf 'safe log\n' >"$(slack_cert_run_dir state-run)/logs/safe.log"
slack_cert_scan_paths state-run "$(slack_cert_run_dir state-run)/logs" \
  || fail "safe log failed leakage scan"
printf 'leak=%s\n' "$SECRET_VALUE" >"$(slack_cert_run_dir state-run)/logs/leak.log"
expect_failure slack_cert_scan_paths state-run "$(slack_cert_run_dir state-run)/logs" \
  || fail "known secret leakage was not detected"
rm "$(slack_cert_run_dir state-run)/logs/leak.log"
printf 'Authorization: Bearer bearer-example-leak-value\n' \
  >"$(slack_cert_run_dir state-run)/logs/pattern.log"
expect_failure slack_cert_scan_paths state-run "$(slack_cert_run_dir state-run)/logs" \
  || fail "generic Slack token leakage was not detected"
rm "$(slack_cert_run_dir state-run)/logs/pattern.log"
slack_cert_scan_paths state-run "$(slack_cert_run_dir state-run)/logs" \
  || fail "clean scan did not recover after leaked file removal"
pass "known values and generic Slack credential patterns are scanned in memory without retention"

slack_cert_validate_recorded_fixture \
  "$REPO_ROOT/fixtures/slack/certification/recorded-contracts.json" \
  || fail "committed recorded fixture is invalid"
jq 'del(.cases[] | select(.kind == "manifest_diff"))' \
  "$REPO_ROOT/fixtures/slack/certification/recorded-contracts.json" \
  >"$QA_ROOT/incomplete-recording.json"
expect_failure slack_cert_validate_recorded_fixture "$QA_ROOT/incomplete-recording.json" \
  || fail "incomplete provider recording was accepted"
pass "recorded OAuth, Events API, manifest, and receipt contracts are complete"

FRESH_CREATED="$(slack_cert_now)"
FRESH_EXPIRES="$(slack_cert_add_days "$FRESH_CREATED" 1)"
STALE_EXPIRES="2000-01-01T00:00:00Z"
[[ "$(slack_cert_freshness "$FRESH_EXPIRES")" == "fresh" ]] \
  || fail "future evidence was not fresh"
[[ "$(slack_cert_freshness "$STALE_EXPIRES")" == "stale" ]] \
  || fail "expired evidence was not stale"
pass "evidence freshness is derived from expiry without changing test result"

slack_cert_validate_safe_evidence "$SAFE_FILE" \
  || fail "safe evidence schema validation failed"
if jq -e '.. | strings | select(test("known-certification-secret|raw-private"))' \
  "$SAFE_FILE" >/dev/null; then
  fail "safe evidence contains private test values"
fi
pass "safe evidence schema excludes private state and registered secrets"

while IFS= read -r pending_stage; do
  [[ -n "$pending_stage" ]] || continue
  slack_cert_state_stage_result state-run "$pending_stage" pass unit_verified
done < <(jq -r '.stages[] | select(.result != "pass") | .name' "$STATE_FILE")
PROMOTED="$QA_ROOT/promoted-latest.json"
SLACK_CERT_STATE_HOME="$SLACK_CERT_STATE_HOME" \
  "$SCRIPT_DIR/certify-slack-managed-live.sh" promote \
    --run-id state-run \
    --evidence "$PROMOTED" >/dev/null
jq -e '
  [.certifications[].mode] == ["shared","dedicated"]
  and all(.certifications[]; .result == "pass"
    and .source == "reviewed_live_run"
    and .secret_scan.result == "pass"
    and .cleanup.result == "pass")
' "$PROMOTED" >/dev/null || fail "reviewed combined evidence did not promote safely"
pass "only passed, scanned, cleaned evidence promotes into per-mode latest status"

LATEST="$QA_ROOT/latest.json"
jq -n \
  --arg certified "$FRESH_CREATED" \
  --arg fresh "$FRESH_EXPIRES" \
  --arg stale "$STALE_EXPIRES" \
  '{
    schema_version: 1,
    certifications: [
      {mode:"shared",result:"pass",run_id:"shared-run",certified_at:$certified,expires_at:$fresh},
      {mode:"dedicated",result:"pass",run_id:"dedicated-run",certified_at:$certified,expires_at:$stale}
    ]
  }' >"$LATEST"
STATUS_JSON="$("$SCRIPT_DIR/certify-slack-managed-live.sh" status --json --evidence "$LATEST")"
jq -e '
  any(.certifications[]; .mode == "shared" and .freshness == "fresh")
  and any(.certifications[]; .mode == "dedicated" and .freshness == "stale"
    and .interpretation == "recertification_required_not_product_regression")
' <<<"$STATUS_JSON" >/dev/null || fail "status projection did not distinguish fresh and stale"
if "$SCRIPT_DIR/certify-slack-managed-live.sh" status --require-fresh \
  --evidence "$LATEST" >/dev/null 2>&1; then
  fail "require-fresh accepted an expired mode"
fi
pass "status and release checks expose stale evidence without calling it regression"

FAKE_BIN="$QA_ROOT/fake-bin"
mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/curl" <<'EOF'
#!/usr/bin/env bash
printf '{"protocol_version":1,"supported_modes":["shared","dedicated"]}\n'
EOF
chmod 700 "$FAKE_BIN/curl"
START_ENV="$QA_ROOT/start-from-zero.env"
cat >"$START_ENV" <<'EOF'
ORCHESTRATOR_BIN=/usr/bin/true
SLACK_LIVE_GATEWAY_URL=https://gateway.example.test
SLACK_LIVE_SHARED_A_DAEMON_DATA=/tmp/shared-a
SLACK_LIVE_SHARED_A_PROJECT=shared-a
SLACK_LIVE_SHARED_A_WORKSPACE_ID=workspace-a
SLACK_LIVE_SHARED_B_DAEMON_DATA=/tmp/shared-b
SLACK_LIVE_SHARED_B_PROJECT=shared-b
SLACK_LIVE_SHARED_B_WORKSPACE_ID=workspace-b
EOF
chmod 600 "$START_ENV"
ZERO_RUN="from-zero-run"
set +e
PATH="$FAKE_BIN:$PATH" \
SLACK_CERT_STATE_HOME="$QA_ROOT/from-zero-state" \
SLACK_CERT_SKIP_AGGREGATES=1 \
  "$SCRIPT_DIR/certify-slack-managed-live.sh" run \
    --mode shared \
    --run-id "$ZERO_RUN" \
    --env-file "$START_ENV" >/dev/null 2>&1
zero_exit=$?
set -e
[[ "$zero_exit" == "20" ]] || fail "from-zero run did not pause at OAuth"
jq -e '
  any(.stages[]; .name == "preflight" and .result == "pass")
  and any(.stages[]; .name == "recorded_fixtures" and .result == "pass")
  and any(.stages[]; .name == "shared_oauth" and .result == "waiting")
  and ([.inventory[] | select(.object_type == "slack_workspace")] | length == 2)
' "$QA_ROOT/from-zero-state/$ZERO_RUN/safe-result.json" >/dev/null \
  || fail "from-zero run required post-OAuth IDs or missed workspace inventory"
pass "from-zero preflight reaches OAuth before requiring connection and App IDs"

MISSING_RUN="missing-env-run"
set +e
SLACK_CERT_STATE_HOME="$QA_ROOT/missing-state" \
  "$SCRIPT_DIR/certify-slack-managed-live.sh" run \
    --mode shared \
    --run-id "$MISSING_RUN" \
    --env-file "$QA_ROOT/does-not-exist.env" >/dev/null 2>&1
missing_exit=$?
set -e
[[ "$missing_exit" == "20" ]] || fail "missing-secret run did not pause with exit 20"
jq -e '.status == "blocked"
  and any(.stages[]; .name == "preflight" and .result == "blocked")
  and (has("private_salt") | not)' \
  "$QA_ROOT/missing-state/$MISSING_RUN/safe-result.json" >/dev/null \
  || fail "missing-secret run did not emit safe resumable evidence"
pass "missing live secrets pause safely and do not make ordinary tests fail"

printf 'Slack continuous certification QA: %d passed, 0 failed\n' "$PASS"
