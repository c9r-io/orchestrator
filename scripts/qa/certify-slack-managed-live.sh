#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=scripts/qa/lib/slack-live-certification-lib.sh
source "$SCRIPT_DIR/lib/slack-live-certification-lib.sh"

POLICY_FILE="$REPO_ROOT/config/qa/slack-live-certification-policy.json"
RECORDED_FIXTURE="$REPO_ROOT/fixtures/slack/certification/recorded-contracts.json"
LATEST_EVIDENCE="$REPO_ROOT/docs/qa/evidence/slack-live-certification-latest.json"
DEFAULT_ENV_FILE="${SLACK_CERT_ENV_FILE:-${FR114_LIVE_ENV_FILE:-$HOME/.config/orchestrator/qa/slack-live.env}}"
WAITING_EXIT=20

usage() {
  cat <<'EOF'
Usage:
  certify-slack-managed-live.sh run --mode shared|dedicated|both [--run-id ID] [--env-file PATH]
  certify-slack-managed-live.sh resume --run-id ID [--env-file PATH]
  certify-slack-managed-live.sh checkpoint --run-id ID --stage STAGE --result pass|blocked|fail [--evidence-code CODE] [--confirm-destructive ID]
  certify-slack-managed-live.sh cleanup --run-id ID [--confirm-destructive ID --mark-external-cleaned]
  certify-slack-managed-live.sh status [--json] [--require-fresh] [--evidence PATH]
  certify-slack-managed-live.sh promote --run-id ID [--evidence PATH]

Exit 20 means the run is safely paused at a human/provider checkpoint.
Live commands are opt-in and never run from ordinary CI.
EOF
}

die() {
  printf 'Slack live certification: %s\n' "$*" >&2
  exit 2
}

set_legacy_shared_aliases() {
  SLACK_LIVE_SHARED_A_DAEMON_DATA="${SLACK_LIVE_SHARED_A_DAEMON_DATA:-${SLACK_LIVE_DAEMON_DATA:-}}"
  SLACK_LIVE_SHARED_A_PROJECT="${SLACK_LIVE_SHARED_A_PROJECT:-${SLACK_LIVE_PROJECT:-}}"
  SLACK_LIVE_SHARED_A_CONNECTION_ID="${SLACK_LIVE_SHARED_A_CONNECTION_ID:-${SLACK_LIVE_CONNECTION_ID:-}}"
  SLACK_LIVE_SHARED_A_CHANNEL_ID="${SLACK_LIVE_SHARED_A_CHANNEL_ID:-${SLACK_LIVE_CHANNEL_ID:-}}"
  SLACK_LIVE_SHARED_A_ACTOR_ID="${SLACK_LIVE_SHARED_A_ACTOR_ID:-${SLACK_LIVE_ACTOR_ID:-}}"
  SLACK_LIVE_SHARED_A_DRIVER_BOT_TOKEN="${SLACK_LIVE_SHARED_A_DRIVER_BOT_TOKEN:-${SLACK_LIVE_DRIVER_BOT_TOKEN:-}}"
}

load_live_environment() {
  local env_file="$1"
  slack_cert_load_env "$env_file" || return 1
  set_legacy_shared_aliases
  SLACK_CERT_TTL_DAYS="${SLACK_CERT_TTL_DAYS:-$(jq -r '.default_ttl_days' "$POLICY_FILE")}"
  ORCHESTRATOR_BIN="${ORCHESTRATOR_BIN:-$REPO_ROOT/target/release/orchestrator}"
  SLACK_LIVE_OFFICIAL_MANIFEST_PATH="${SLACK_LIVE_OFFICIAL_MANIFEST_PATH:-$REPO_ROOT/deploy/slack/official-app-manifest.json}"
  SLACK_LIVE_DEDICATED_MANIFEST_PATH="${SLACK_LIVE_DEDICATED_MANIFEST_PATH:-$REPO_ROOT/deploy/slack/dedicated-app-manifest.json}"
  SLACK_LIVE_TIMEOUT_SECONDS="${SLACK_LIVE_TIMEOUT_SECONDS:-90}"
  SLACK_LIVE_IMPLEMENT_REACTION="${SLACK_LIVE_IMPLEMENT_REACTION:-eyes}"
  SLACK_LIVE_DOCS_REACTION="${SLACK_LIVE_DOCS_REACTION:-white_check_mark}"
  SLACK_LIVE_IMPLEMENT_SKILL_MARKER="${SLACK_LIVE_IMPLEMENT_SKILL_MARKER:-\$ticket-fix}"
  SLACK_LIVE_DOCS_SKILL_MARKER="${SLACK_LIVE_DOCS_SKILL_MARKER:-\$qa-doc-gen}"
}

require_value() {
  local name="$1"
  local value="${!name:-}"
  [[ -n "$value" ]] || slack_cert_fail "missing required live setting: $name"
}

preflight_mode_values() {
  local mode="$1"
  require_value ORCHESTRATOR_BIN || return 1
  require_value SLACK_LIVE_GATEWAY_URL || return 1
  [[ -x "$ORCHESTRATOR_BIN" ]] || slack_cert_fail "orchestrator binary is not executable" || return 1
  [[ "$SLACK_LIVE_GATEWAY_URL" =~ ^https://[^/?#]+$ ]] \
    || slack_cert_fail "gateway URL must be an HTTPS origin without path/query/fragment" \
    || return 1
  if [[ "$mode" == "shared" || "$mode" == "both" ]]; then
    local key
    for key in \
      SLACK_LIVE_SHARED_A_DAEMON_DATA \
      SLACK_LIVE_SHARED_A_PROJECT \
      SLACK_LIVE_SHARED_A_WORKSPACE_ID \
      SLACK_LIVE_SHARED_B_DAEMON_DATA \
      SLACK_LIVE_SHARED_B_PROJECT \
      SLACK_LIVE_SHARED_B_WORKSPACE_ID; do
      require_value "$key" || return 1
    done
  fi
  if [[ "$mode" == "dedicated" || "$mode" == "both" ]]; then
    local key
    for key in \
      SLACK_LIVE_DEDICATED_DAEMON_DATA \
      SLACK_LIVE_DEDICATED_PROJECT \
      SLACK_LIVE_DEDICATED_WORKSPACE_ID; do
      require_value "$key" || return 1
    done
  fi
}

preflight_tools_and_manifests() {
  local command
  for command in bash curl date git jq mktemp rg stat; do
    command -v "$command" >/dev/null 2>&1 \
      || slack_cert_fail "missing required command: $command" \
      || return 1
  done
  [[ -f "$SLACK_LIVE_OFFICIAL_MANIFEST_PATH" ]] \
    || slack_cert_fail "official App manifest not found" \
    || return 1
  [[ -f "$SLACK_LIVE_DEDICATED_MANIFEST_PATH" ]] \
    || slack_cert_fail "dedicated App manifest not found" \
    || return 1
  rg -q 'reactions:read' "$SLACK_LIVE_OFFICIAL_MANIFEST_PATH" \
    || slack_cert_fail "official manifest is missing reactions:read" \
    || return 1
  rg -q 'reactions:read' "$SLACK_LIVE_DEDICATED_MANIFEST_PATH" \
    || slack_cert_fail "dedicated manifest is missing reactions:read" \
    || return 1
  ! rg -q 'chat:write|reactions:write|xox[baprs]-' \
    "$SLACK_LIVE_OFFICIAL_MANIFEST_PATH" "$SLACK_LIVE_DEDICATED_MANIFEST_PATH" \
    || slack_cert_fail "reviewed manifests contain a forbidden write scope or token" \
    || return 1
  curl --silent --show-error --fail --max-time 15 \
    "$SLACK_LIVE_GATEWAY_URL/healthz" >/dev/null \
    || slack_cert_fail "gateway health check failed" \
    || return 1
  curl --silent --show-error --fail --max-time 15 \
    "$SLACK_LIVE_GATEWAY_URL/v1/capabilities" \
    | jq -e '.protocol_version and (.supported_modes | type == "array")' >/dev/null \
    || slack_cert_fail "gateway capability preflight failed" \
    || return 1
}

populate_inventory() {
  local run_id="$1"
  local mode="$2"
  if [[ "$mode" == "shared" || "$mode" == "both" ]]; then
    slack_cert_inventory_add "$run_id" slack_workspace \
      "${SLACK_LIVE_SHARED_A_WORKSPACE_ID:-}" review_workspace_retention true
    slack_cert_inventory_add "$run_id" slack_workspace \
      "${SLACK_LIVE_SHARED_B_WORKSPACE_ID:-}" review_workspace_retention true
    slack_cert_inventory_add "$run_id" source_connection \
      "${SLACK_LIVE_SHARED_A_CONNECTION_ID:-}" disconnect false
    slack_cert_inventory_add "$run_id" source_connection \
      "${SLACK_LIVE_SHARED_B_CONNECTION_ID:-}" disconnect false
    slack_cert_inventory_add "$run_id" slack_channel \
      "${SLACK_LIVE_SHARED_A_CHANNEL_ID:-}" remove_synthetic_messages false
  fi
  if [[ "$mode" == "dedicated" || "$mode" == "both" ]]; then
    slack_cert_inventory_add "$run_id" slack_workspace \
      "${SLACK_LIVE_DEDICATED_WORKSPACE_ID:-}" review_workspace_retention true
    slack_cert_inventory_add "$run_id" source_connection \
      "${SLACK_LIVE_DEDICATED_CONNECTION_ID:-}" disconnect false
    slack_cert_inventory_add "$run_id" slack_channel \
      "${SLACK_LIVE_DEDICATED_CHANNEL_ID:-}" remove_synthetic_messages false
    slack_cert_inventory_add "$run_id" slack_app \
      "${SLACK_LIVE_DEDICATED_APP_ID:-dedicated-app-for-${SLACK_LIVE_DEDICATED_CONNECTION_ID:-unknown}}" \
      delete_slack_app true
  fi
  slack_cert_inventory_add "$run_id" gateway_origin \
    "$SLACK_LIVE_GATEWAY_URL" review_external_domain_retention true
}

run_aggregate() {
  local run_id="$1"
  local mode="$2"
  local log_file
  log_file="$(slack_cert_run_dir "$run_id")/logs/recorded-aggregate.log"
  if [[ "${SLACK_CERT_SKIP_AGGREGATES:-0}" == "1" ]]; then
    printf 'Aggregate skipped by explicit local iteration flag.\n' >"$log_file"
    return 0
  fi
  case "$mode" in
    shared)
      env FR114_ALLOW_DIRTY="${SLACK_CERT_ALLOW_DIRTY:-0}" \
        "$SCRIPT_DIR/test-slack-managed-shared-oauth.sh" >"$log_file" 2>&1
      ;;
    dedicated | both)
      env FR115_ALLOW_DIRTY="${SLACK_CERT_ALLOW_DIRTY:-0}" \
        "$SCRIPT_DIR/test-slack-dedicated-app-provisioning.sh" >"$log_file" 2>&1
      ;;
  esac
}

run_live_smoke() {
  local run_id="$1"
  local mode="$2"
  local prefix log_file driver_token
  case "$mode" in
    shared)
      prefix="SLACK_LIVE_SHARED_A"
      ;;
    dedicated)
      prefix="SLACK_LIVE_DEDICATED"
      ;;
    *)
      slack_cert_fail "smoke mode must be shared or dedicated"
      return 1
      ;;
  esac
  local daemon_var="${prefix}_DAEMON_DATA"
  local project_var="${prefix}_PROJECT"
  local connection_var="${prefix}_CONNECTION_ID"
  local channel_var="${prefix}_CHANNEL_ID"
  local actor_var="${prefix}_ACTOR_ID"
  local token_var="${prefix}_DRIVER_BOT_TOKEN"
  local required
  for required in \
    "$daemon_var" "$project_var" "$connection_var" "$channel_var" "$actor_var" "$token_var"; do
    require_value "$required" || return 1
  done
  driver_token="${!token_var}"
  slack_cert_register_known_secret "$run_id" "$driver_token"
  log_file="$(slack_cert_run_dir "$run_id")/logs/${mode}-badge-smoke.log"
  env -i \
    PATH="$PATH" \
    HOME="$HOME" \
    TMPDIR="${TMPDIR:-/tmp}" \
    ORCHESTRATOR_BIN="$ORCHESTRATOR_BIN" \
    SLACK_LIVE_DAEMON_DATA="${!daemon_var}" \
    SLACK_LIVE_PROJECT="${!project_var}" \
    SLACK_LIVE_CONNECTION_ID="${!connection_var}" \
    SLACK_LIVE_CHANNEL_ID="${!channel_var}" \
    SLACK_LIVE_ACTOR_ID="${!actor_var}" \
    SLACK_LIVE_DRIVER_BOT_TOKEN="$driver_token" \
    SLACK_LIVE_IMPLEMENT_REACTION="$SLACK_LIVE_IMPLEMENT_REACTION" \
    SLACK_LIVE_DOCS_REACTION="$SLACK_LIVE_DOCS_REACTION" \
    SLACK_LIVE_IMPLEMENT_SKILL_MARKER="$SLACK_LIVE_IMPLEMENT_SKILL_MARKER" \
    SLACK_LIVE_DOCS_SKILL_MARKER="$SLACK_LIVE_DOCS_SKILL_MARKER" \
    SLACK_LIVE_TIMEOUT_SECONDS="$SLACK_LIVE_TIMEOUT_SECONDS" \
    "$SCRIPT_DIR/test-slack-managed-live-smoke.sh" >"$log_file" 2>&1
}

final_inventory_complete() {
  local mode="$1"
  if [[ "$mode" == "shared" || "$mode" == "both" ]]; then
    require_value SLACK_LIVE_SHARED_A_WORKSPACE_ID || return 1
    require_value SLACK_LIVE_SHARED_B_WORKSPACE_ID || return 1
    require_value SLACK_LIVE_SHARED_A_CONNECTION_ID || return 1
    require_value SLACK_LIVE_SHARED_B_CONNECTION_ID || return 1
  fi
  if [[ "$mode" == "dedicated" || "$mode" == "both" ]]; then
    require_value SLACK_LIVE_DEDICATED_WORKSPACE_ID || return 1
    require_value SLACK_LIVE_DEDICATED_CONNECTION_ID || return 1
    require_value SLACK_LIVE_DEDICATED_APP_ID || return 1
  fi
}

manual_stage() {
  case "$1" in
    shared_oauth)
      printf 'Complete or verify shared official-App OAuth for workspace A, then record this checkpoint.\n'
      ;;
    shared_multi_workspace)
      printf 'Complete workspace B OAuth and verify daemon/project isolation, then record this checkpoint.\n'
      ;;
    shared_cursor_recovery)
      printf 'Execute the offline delivery/cursor recovery procedure from the runbook, then record this checkpoint.\n'
      ;;
    shared_revocation_disconnect)
      printf 'Verify revocation fail-closed and reviewed disconnect while retaining evidence, then record this checkpoint.\n'
      ;;
    dedicated_provision_oauth)
      printf 'Complete dedicated validate/create/import-receipt/OAuth using a stdin-only Configuration Token, then record this checkpoint.\n'
      ;;
    dedicated_manifest_receipt)
      printf 'Verify exact-App manifest state and the durable credential import receipt, then record this checkpoint.\n'
      ;;
    dedicated_cursor_recovery)
      printf 'Execute dedicated offline delivery/cursor recovery, then record this checkpoint.\n'
      ;;
    dedicated_reauthorize)
      printf 'Complete reviewed permission upgrade/reauthorization and verify generation advancement, then record this checkpoint.\n'
      ;;
    dedicated_disconnect_delete)
      printf 'Disconnect first. Delete the exact sandbox App only after explicit confirmation, then record this checkpoint.\n'
      ;;
    *)
      return 1
      ;;
  esac
  printf 'Resume command: %s checkpoint --run-id %s --stage %s --result pass --evidence-code <safe-code>\n' \
    "$0" "$2" "$1"
  return 0
}

process_run() {
  local run_id="$1"
  local env_file="$2"
  local state mode stage
  state="$(slack_cert_state_file "$run_id")"
  [[ -f "$state" ]] || die "unknown run: $run_id"
  mode="$(jq -r '.mode' "$state")"
  if ! load_live_environment "$env_file"; then
    slack_cert_state_stage_result "$run_id" preflight blocked missing_live_environment
    return "$WAITING_EXIT"
  fi
  slack_cert_register_known_secret "$run_id" \
    "${SLACK_LIVE_SHARED_A_DRIVER_BOT_TOKEN:-}"
  slack_cert_register_known_secret "$run_id" \
    "${SLACK_LIVE_SHARED_B_DRIVER_BOT_TOKEN:-}"
  slack_cert_register_known_secret "$run_id" \
    "${SLACK_LIVE_DEDICATED_DRIVER_BOT_TOKEN:-}"
  populate_inventory "$run_id" "$mode"

  while stage="$(slack_cert_next_stage "$run_id")" && [[ -n "$stage" ]]; do
    case "$stage" in
      preflight)
        if preflight_mode_values "$mode" && preflight_tools_and_manifests; then
          slack_cert_state_stage_result "$run_id" "$stage" pass live_preflight_ok
        else
          slack_cert_state_stage_result "$run_id" "$stage" blocked live_preflight_blocked
          return "$WAITING_EXIT"
        fi
        ;;
      recorded_fixtures)
        if slack_cert_validate_recorded_fixture "$RECORDED_FIXTURE" \
          && run_aggregate "$run_id" "$mode"; then
          slack_cert_state_stage_result "$run_id" "$stage" pass recorded_contracts_ok
        else
          slack_cert_state_stage_result "$run_id" "$stage" fail recorded_contracts_failed
          return 1
        fi
        ;;
      shared_badges)
        if run_live_smoke "$run_id" shared; then
          slack_cert_state_stage_result "$run_id" "$stage" pass same_message_two_badges
        else
          slack_cert_state_stage_result "$run_id" "$stage" fail shared_badge_smoke_failed
          return 1
        fi
        ;;
      dedicated_badges)
        if run_live_smoke "$run_id" dedicated; then
          slack_cert_state_stage_result "$run_id" "$stage" pass same_message_two_badges
        else
          slack_cert_state_stage_result "$run_id" "$stage" fail dedicated_badge_smoke_failed
          return 1
        fi
        ;;
      privacy_scan)
        if ! final_inventory_complete "$mode"; then
          slack_cert_state_stage_result "$run_id" "$stage" blocked incomplete_cleanup_inventory
          return "$WAITING_EXIT"
        fi
        git -C "$REPO_ROOT" diff --no-ext-diff >"$(slack_cert_run_dir "$run_id")/logs/git-diff.txt"
        if slack_cert_scan_paths "$run_id" \
          "$(slack_cert_run_dir "$run_id")/logs" \
          "$(slack_cert_safe_file "$run_id")" \
          "$REPO_ROOT/gui/test-results" \
          "$REPO_ROOT/gui/playwright-report"; then
          slack_cert_state_stage_result "$run_id" "$stage" pass zero_forbidden_matches
        else
          slack_cert_state_stage_result "$run_id" "$stage" fail secret_scan_failed
          return 1
        fi
        ;;
      cleanup)
        slack_cert_cleanup_mark "$run_id" false
        if jq -e '.inventory | all(.cleanup_result != "pending")' "$state" >/dev/null; then
          slack_cert_state_stage_result "$run_id" "$stage" pass cleanup_complete
        else
          slack_cert_state_stage_result "$run_id" "$stage" waiting destructive_cleanup_confirmation_required
          printf 'Cleanup remains pending for destructive/reused external objects.\n'
          printf 'After reviewed external cleanup: %s cleanup --run-id %s --confirm-destructive %s --mark-external-cleaned\n' \
            "$0" "$run_id" "$run_id"
          return "$WAITING_EXIT"
        fi
        ;;
      *)
        if manual_stage "$stage" "$run_id"; then
          slack_cert_state_stage_result "$run_id" "$stage" waiting human_provider_checkpoint
          return "$WAITING_EXIT"
        fi
        die "unhandled certification stage: $stage"
        ;;
    esac
  done
  printf 'Slack live certification completed: %s\n' "$(slack_cert_safe_file "$run_id")"
}

checkpoint_is_manual() {
  case "$1" in
    shared_oauth | shared_multi_workspace | shared_cursor_recovery | \
    shared_revocation_disconnect | dedicated_provision_oauth | \
    dedicated_manifest_receipt | dedicated_cursor_recovery | \
    dedicated_reauthorize | dedicated_disconnect_delete)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

command_status() {
  local evidence="$1"
  local json_output="$2"
  local require_fresh="$3"
  [[ -f "$evidence" ]] || die "latest evidence not found: $evidence"
  jq -e '.schema_version == 1 and (.certifications | type == "array")' "$evidence" >/dev/null \
    || die "latest evidence index is invalid"
  local now output
  now="$(slack_cert_now)"
  output="$(jq --arg now "$now" '
    {
      schema_version,
      checked_at: $now,
      certifications: [
        .certifications[] |
        . + {
          freshness: (if .expires_at >= $now then "fresh" else "stale" end),
          interpretation: (
            if .expires_at >= $now then "live_certification_current"
            else "recertification_required_not_product_regression"
            end
          )
        }
      ]
    }
  ' "$evidence")"
  if [[ "$json_output" == "true" ]]; then
    printf '%s\n' "$output"
  else
    printf '%-10s %-8s %-7s %-21s %-21s %s\n' MODE RESULT FRESH CERTIFIED_AT EXPIRES_AT RUN_ID
    jq -r '.certifications[] |
      [.mode,.result,.freshness,.certified_at,.expires_at,.run_id] | @tsv' <<<"$output" \
      | while IFS=$'\t' read -r mode result freshness certified expires run_id; do
          printf '%-10s %-8s %-7s %-21s %-21s %s\n' \
            "$mode" "$result" "$freshness" "$certified" "$expires" "$run_id"
        done
  fi
  if [[ "$require_fresh" == "true" ]]; then
    jq -e '.certifications | length >= 2
      and any(.[]; .mode == "shared" and .result == "pass" and .freshness == "fresh")
      and any(.[]; .mode == "dedicated" and .result == "pass" and .freshness == "fresh")' \
      <<<"$output" >/dev/null
  fi
}

command_promote() {
  local run_id="$1"
  local evidence="$2"
  local safe temporary
  safe="$(slack_cert_safe_file "$run_id")"
  [[ -f "$safe" ]] || die "unknown run: $run_id"
  slack_cert_validate_safe_evidence "$safe" || die "safe evidence is invalid"
  jq -e '.status == "passed" and .secret_scan.result == "pass" and .cleanup.result == "pass"' \
    "$safe" >/dev/null || die "only passed, scanned, cleaned evidence may be promoted"
  mkdir -p "$(dirname "$evidence")"
  temporary="${evidence}.tmp.$$"
  if [[ ! -f "$evidence" ]]; then
    printf '{"schema_version":1,"certifications":[]}\n' >"$evidence"
  fi
  jq --slurpfile run "$safe" '
    . as $index
    | $run[0] as $r
    | (if $r.mode == "both" then ["shared","dedicated"] else [$r.mode] end) as $modes
    | reduce $modes[] as $mode ($index;
        .certifications = (
          [.certifications[] | select(.mode != $mode)] +
          [{
            mode: $mode,
            result: "pass",
            run_id: $r.run_id,
            certified_at: $r.updated_at,
            expires_at: $r.expires_at,
            build: $r.build,
            stages: [
              $r.stages[] |
              select(
                .name == "preflight"
                or .name == "recorded_fixtures"
                or .name == "privacy_scan"
                or .name == "cleanup"
                or (.name | startswith($mode + "_"))
              )
            ],
            cleanup: $r.cleanup,
            secret_scan: $r.secret_scan,
            source: "reviewed_live_run"
          }]
        )
      )
  ' "$evidence" >"$temporary"
  mv "$temporary" "$evidence"
  printf 'Promoted reviewed safe evidence to %s\n' "$evidence"
}

main() {
  [[ "${-}" != *x* ]] || die "shell xtrace must be disabled for live certification"
  [[ $# -ge 1 ]] || {
    usage
    exit 2
  }
  local command="$1"
  shift
  local mode="" run_id="" env_file="$DEFAULT_ENV_FILE" stage="" result=""
  local evidence_code="" destructive_confirmation="" mark_external_cleaned="false"
  local json_output="false" require_fresh="false" evidence="$LATEST_EVIDENCE"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --mode) mode="${2:-}"; shift 2 ;;
      --run-id) run_id="${2:-}"; shift 2 ;;
      --env-file) env_file="${2:-}"; shift 2 ;;
      --stage) stage="${2:-}"; shift 2 ;;
      --result) result="${2:-}"; shift 2 ;;
      --evidence-code) evidence_code="${2:-}"; shift 2 ;;
      --confirm-destructive) destructive_confirmation="${2:-}"; shift 2 ;;
      --mark-external-cleaned) mark_external_cleaned="true"; shift ;;
      --json) json_output="true"; shift ;;
      --require-fresh) require_fresh="true"; shift ;;
      --evidence) evidence="${2:-}"; shift 2 ;;
      -h | --help) usage; exit 0 ;;
      *) die "unknown argument: $1" ;;
    esac
  done

  case "$command" in
    run)
      slack_cert_mode_valid "$mode" || die "--mode must be shared, dedicated, or both"
      run_id="${run_id:-slack-cert-$(date -u +%Y%m%dT%H%M%SZ)}"
      local ttl_days
      ttl_days="$(jq -r '.default_ttl_days' "$POLICY_FILE")"
      if [[ -f "$env_file" ]] && load_live_environment "$env_file"; then
        ttl_days="$SLACK_CERT_TTL_DAYS"
      fi
      slack_cert_init_state "$run_id" "$mode" "$ttl_days" "$(git -C "$REPO_ROOT" rev-parse HEAD)"
      trap 'slack_cert_emit_safe "'"$run_id"'"' EXIT INT TERM
      process_run "$run_id" "$env_file"
      ;;
    resume)
      [[ -n "$run_id" ]] || die "--run-id is required"
      trap 'slack_cert_emit_safe "'"$run_id"'"' EXIT INT TERM
      process_run "$run_id" "$env_file"
      ;;
    checkpoint)
      [[ -n "$run_id" && -n "$stage" && -n "$result" ]] \
        || die "--run-id, --stage, and --result are required"
      checkpoint_is_manual "$stage" || die "stage cannot be manually attested: $stage"
      local current
      current="$(slack_cert_next_stage "$run_id")"
      [[ "$current" == "$stage" ]] || die "checkpoint is not current: expected $current"
      if [[ "$stage" == "dedicated_disconnect_delete" && "$result" == "pass" ]]; then
        [[ "$destructive_confirmation" == "$run_id" ]] \
          || die "dedicated App deletion evidence requires --confirm-destructive $run_id"
      fi
      slack_cert_state_stage_result "$run_id" "$stage" "$result" "$evidence_code"
      printf 'Checkpoint recorded. Continue with: %s resume --run-id %s --env-file %s\n' \
        "$0" "$run_id" "$env_file"
      ;;
    cleanup)
      [[ -n "$run_id" ]] || die "--run-id is required"
      if [[ "$mark_external_cleaned" == "true" ]]; then
        [[ "$destructive_confirmation" == "$run_id" ]] \
          || die "external destructive cleanup requires --confirm-destructive $run_id"
        slack_cert_cleanup_mark "$run_id" true
        if [[ "$(slack_cert_next_stage "$run_id")" == "cleanup" ]]; then
          slack_cert_state_stage_result "$run_id" cleanup pass cleanup_complete
        fi
      else
        slack_cert_cleanup_mark "$run_id" false
      fi
      jq '{run_id,status,inventory,cleanup}' "$(slack_cert_safe_file "$run_id")"
      ;;
    status)
      command_status "$evidence" "$json_output" "$require_fresh"
      ;;
    promote)
      [[ -n "$run_id" ]] || die "--run-id is required"
      command_promote "$run_id" "$evidence"
      ;;
    *)
      usage
      exit 2
      ;;
  esac
}

main "$@"
