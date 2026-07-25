#!/usr/bin/env bash

# Shared helpers for the opt-in Slack live certification controller.
# This file is intentionally side-effect free when sourced.

declare -a SLACK_CERT_KNOWN_SECRETS=()

slack_cert_fail() {
  printf 'Slack live certification: %s\n' "$*" >&2
  return 1
}

slack_cert_now() {
  date -u '+%Y-%m-%dT%H:%M:%SZ'
}

slack_cert_epoch() {
  date -u '+%s'
}

slack_cert_add_days() {
  local timestamp="$1"
  local days="$2"
  if date -j -u -v+"${days}"d -f '%Y-%m-%dT%H:%M:%SZ' "$timestamp" '+%Y-%m-%dT%H:%M:%SZ' \
    >/dev/null 2>&1; then
    date -j -u -v+"${days}"d -f '%Y-%m-%dT%H:%M:%SZ' "$timestamp" '+%Y-%m-%dT%H:%M:%SZ'
  else
    date -u -d "$timestamp + ${days} days" '+%Y-%m-%dT%H:%M:%SZ'
  fi
}

slack_cert_sha256_text() {
  if command -v shasum >/dev/null 2>&1; then
    printf '%s' "$1" | shasum -a 256 | awk '{print $1}'
  else
    printf '%s' "$1" | sha256sum | awk '{print $1}'
  fi
}

# Octal permission bits, on both BSD and GNU stat.
#
# The previous form was `stat -f '%Lp' "$1" 2>/dev/null || stat -c '%a' "$1"`,
# which is broken on GNU: there `-f` is --file-system and takes no format, so
# '%Lp' is read as a second operand. GNU then prints the filesystem block for
# "$1" on *stdout*, fails on the missing '%Lp', and the `||` fallback appends
# the real mode to that output. The caller compared the result against "600" and
# never matched. This ran green for its whole life because the job that executes
# it had no ripgrep and exited before reaching any assertion (FR-134).
#
# Reordering alone would fix today's platforms and leave the same shape, so the
# output is validated instead: whatever answers must look like an octal mode.
slack_cert_file_mode() {
  local mode
  for mode in "$(stat -c '%a' "$1" 2>/dev/null)" "$(stat -f '%Lp' "$1" 2>/dev/null)"; do
    if [[ "$mode" =~ ^[0-7]+$ ]]; then
      printf '%s' "$mode"
      return 0
    fi
  done
  echo "cannot read permission bits of $1 with either BSD or GNU stat" >&2
  return 1
}

slack_cert_require_private_file() {
  local path="$1"
  [[ -f "$path" ]] || slack_cert_fail "environment file not found: $path" || return 1
  local permissions
  permissions="$(slack_cert_file_mode "$path")"
  [[ "$permissions" == "600" || "$permissions" == "400" ]] \
    || slack_cert_fail "environment file must have mode 600 or 400: $path" \
    || return 1
}

slack_cert_allowed_env_key() {
  case "$1" in
    ORCHESTRATOR_BIN | \
    SLACK_CERT_TTL_DAYS | \
    SLACK_LIVE_GATEWAY_URL | \
    SLACK_LIVE_OFFICIAL_MANIFEST_PATH | \
    SLACK_LIVE_DEDICATED_MANIFEST_PATH | \
    SLACK_LIVE_TIMEOUT_SECONDS | \
    SLACK_LIVE_IMPLEMENT_REACTION | \
    SLACK_LIVE_DOCS_REACTION | \
    SLACK_LIVE_IMPLEMENT_SKILL_MARKER | \
    SLACK_LIVE_DOCS_SKILL_MARKER | \
    SLACK_LIVE_DAEMON_DATA | \
    SLACK_LIVE_PROJECT | \
    SLACK_LIVE_CONNECTION_ID | \
    SLACK_LIVE_CHANNEL_ID | \
    SLACK_LIVE_ACTOR_ID | \
    SLACK_LIVE_DRIVER_BOT_TOKEN | \
    SLACK_LIVE_SHARED_A_DAEMON_DATA | \
    SLACK_LIVE_SHARED_A_PROJECT | \
    SLACK_LIVE_SHARED_A_WORKSPACE_ID | \
    SLACK_LIVE_SHARED_A_CONNECTION_ID | \
    SLACK_LIVE_SHARED_A_CHANNEL_ID | \
    SLACK_LIVE_SHARED_A_ACTOR_ID | \
    SLACK_LIVE_SHARED_A_DRIVER_BOT_TOKEN | \
    SLACK_LIVE_SHARED_B_DAEMON_DATA | \
    SLACK_LIVE_SHARED_B_PROJECT | \
    SLACK_LIVE_SHARED_B_WORKSPACE_ID | \
    SLACK_LIVE_SHARED_B_CONNECTION_ID | \
    SLACK_LIVE_SHARED_B_CHANNEL_ID | \
    SLACK_LIVE_SHARED_B_ACTOR_ID | \
    SLACK_LIVE_SHARED_B_DRIVER_BOT_TOKEN | \
    SLACK_LIVE_DEDICATED_DAEMON_DATA | \
    SLACK_LIVE_DEDICATED_PROJECT | \
    SLACK_LIVE_DEDICATED_WORKSPACE_ID | \
    SLACK_LIVE_DEDICATED_CONNECTION_ID | \
    SLACK_LIVE_DEDICATED_CHANNEL_ID | \
    SLACK_LIVE_DEDICATED_ACTOR_ID | \
    SLACK_LIVE_DEDICATED_DRIVER_BOT_TOKEN | \
    SLACK_LIVE_DEDICATED_APP_ID)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

slack_cert_trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

# Parse a deliberately small .env grammar without evaluating shell syntax.
# Values may be unquoted, single quoted, or double quoted. Command substitution,
# backticks, and multiline values are rejected.
slack_cert_load_env() {
  local path="$1"
  slack_cert_require_private_file "$path" || return 1
  local line line_number=0 key value
  while IFS= read -r line || [[ -n "$line" ]]; do
    line_number=$((line_number + 1))
    line="$(slack_cert_trim "$line")"
    [[ -z "$line" || "$line" == \#* ]] && continue
    [[ "$line" =~ ^[A-Z][A-Z0-9_]*= ]] \
      || slack_cert_fail "invalid environment assignment at line $line_number" \
      || return 1
    key="${line%%=*}"
    value="${line#*=}"
    slack_cert_allowed_env_key "$key" \
      || slack_cert_fail "unsupported environment key at line $line_number: $key" \
      || return 1
    if [[ "$value" == \"*\" && "$value" == *\" ]]; then
      value="${value:1:${#value}-2}"
    elif [[ "$value" == \'*\' && "$value" == *\' ]]; then
      value="${value:1:${#value}-2}"
    fi
    [[ "$value" != *'$('* && "$value" != *'`'* && "$value" != *$'\n'* ]] \
      || slack_cert_fail "executable or multiline environment value rejected at line $line_number" \
      || return 1
    printf -v "$key" '%s' "$value"
  done <"$path"
}

slack_cert_mode_valid() {
  [[ "$1" == "shared" || "$1" == "dedicated" || "$1" == "both" ]]
}

slack_cert_state_root() {
  printf '%s' "${SLACK_CERT_STATE_HOME:-${XDG_STATE_HOME:-$HOME/.local/state}/orchestrator/slack-certification}"
}

slack_cert_run_dir() {
  printf '%s/%s' "$(slack_cert_state_root)" "$1"
}

slack_cert_state_file() {
  printf '%s/private-state.json' "$(slack_cert_run_dir "$1")"
}

slack_cert_safe_file() {
  printf '%s/safe-result.json' "$(slack_cert_run_dir "$1")"
}

slack_cert_atomic_jq() {
  local source="$1"
  shift
  local temporary="${source}.tmp.$$"
  jq "$@" "$source" >"$temporary"
  chmod 600 "$temporary"
  mv "$temporary" "$source"
}

slack_cert_stage_names() {
  case "$1" in
    shared)
      printf '%s\n' \
        preflight recorded_fixtures shared_oauth shared_multi_workspace \
        shared_badges shared_cursor_recovery shared_revocation_disconnect \
        privacy_scan cleanup
      ;;
    dedicated)
      printf '%s\n' \
        preflight recorded_fixtures dedicated_provision_oauth dedicated_manifest_receipt \
        dedicated_badges dedicated_cursor_recovery dedicated_reauthorize \
        dedicated_disconnect_delete privacy_scan cleanup
      ;;
    both)
      printf '%s\n' \
        preflight recorded_fixtures shared_oauth shared_multi_workspace \
        shared_badges shared_cursor_recovery shared_revocation_disconnect \
        dedicated_provision_oauth dedicated_manifest_receipt dedicated_badges \
        dedicated_cursor_recovery dedicated_reauthorize dedicated_disconnect_delete \
        privacy_scan cleanup
      ;;
  esac
}

slack_cert_init_state() {
  local run_id="$1"
  local mode="$2"
  local ttl_days="$3"
  local commit="$4"
  slack_cert_mode_valid "$mode" || slack_cert_fail "invalid mode: $mode" || return 1
  [[ "$run_id" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$ ]] \
    || slack_cert_fail "invalid run id" \
    || return 1
  [[ "$ttl_days" =~ ^[0-9]+$ && "$ttl_days" -ge 1 && "$ttl_days" -le 365 ]] \
    || slack_cert_fail "TTL days must be between 1 and 365" \
    || return 1

  local run_dir state created_at expires_at salt stages_json
  run_dir="$(slack_cert_run_dir "$run_id")"
  state="$(slack_cert_state_file "$run_id")"
  [[ ! -e "$run_dir" ]] || slack_cert_fail "run already exists: $run_id" || return 1
  umask 077
  mkdir -p "$run_dir/logs"
  chmod 700 "$run_dir" "$run_dir/logs"
  created_at="$(slack_cert_now)"
  expires_at="$(slack_cert_add_days "$created_at" "$ttl_days")"
  salt="$(slack_cert_sha256_text "$run_id:$created_at:$$:${RANDOM:-0}")"
  stages_json="$(slack_cert_stage_names "$mode" | jq -Rsc 'split("\n")[:-1] | map({name:.,result:"pending",updated_at:null,evidence_code:null})')"
  jq -n \
    --arg run_id "$run_id" \
    --arg mode "$mode" \
    --arg created_at "$created_at" \
    --arg expires_at "$expires_at" \
    --arg commit "$commit" \
    --arg salt "$salt" \
    --argjson ttl_days "$ttl_days" \
    --argjson stages "$stages_json" \
    '{
      schema_version: 1,
      run_id: $run_id,
      mode: $mode,
      status: "in_progress",
      created_at: $created_at,
      updated_at: $created_at,
      expires_at: $expires_at,
      ttl_days: $ttl_days,
      build: {commit: $commit},
      private_salt: $salt,
      stages: $stages,
      inventory: [],
      secret_scan: {result: "pending", scanned_files: 0},
      cleanup: {result: "pending", destructive_confirmation: false}
    }' >"$state"
  chmod 600 "$state"
  slack_cert_emit_safe "$run_id"
}

slack_cert_state_stage_result() {
  local run_id="$1"
  local stage="$2"
  local result="$3"
  local evidence_code="${4:-}"
  case "$result" in
    pass | fail | blocked | waiting | skipped) ;;
    *) slack_cert_fail "invalid stage result: $result" || return 1 ;;
  esac
  [[ "$evidence_code" =~ ^[A-Za-z0-9._:/+-]{0,120}$ ]] \
    || slack_cert_fail "evidence code contains unsafe characters" \
    || return 1
  local state now
  state="$(slack_cert_state_file "$run_id")"
  [[ -f "$state" ]] || slack_cert_fail "unknown run: $run_id" || return 1
  jq -e --arg stage "$stage" 'any(.stages[]; .name == $stage)' "$state" >/dev/null \
    || slack_cert_fail "stage is not part of this run: $stage" \
    || return 1
  now="$(slack_cert_now)"
  slack_cert_atomic_jq "$state" \
    --arg stage "$stage" \
    --arg result "$result" \
    --arg evidence "$evidence_code" \
    --arg now "$now" \
    '(.stages[] | select(.name == $stage)) |=
       (.result = $result | .evidence_code = (if $evidence == "" then null else $evidence end) | .updated_at = $now)
     | .updated_at = $now
     | .status = (
         if any(.stages[]; .result == "fail") then "failed"
         elif any(.stages[]; .result == "blocked") then "blocked"
         elif all(.stages[]; (.result == "pass" or .result == "skipped")) then "passed"
         else "in_progress"
         end
       )'
  slack_cert_emit_safe "$run_id"
}

slack_cert_next_stage() {
  jq -r '.stages[] | select(.result == "pending" or .result == "waiting" or .result == "blocked") | .name' \
    "$(slack_cert_state_file "$1")" | head -n 1
}

slack_cert_inventory_add() {
  local run_id="$1"
  local object_type="$2"
  local raw_id="$3"
  local action="$4"
  local destructive="$5"
  [[ -n "$raw_id" ]] || return 0
  [[ "$object_type" =~ ^[a-z_]+$ && "$action" =~ ^[a-z_]+$ ]] \
    || slack_cert_fail "invalid inventory type or action" \
    || return 1
  [[ "$destructive" == "true" || "$destructive" == "false" ]] \
    || slack_cert_fail "invalid inventory destructive flag" \
    || return 1
  local state digest now
  state="$(slack_cert_state_file "$run_id")"
  digest="$(slack_cert_sha256_text "$(jq -r '.private_salt' "$state"):$raw_id")"
  now="$(slack_cert_now)"
  slack_cert_atomic_jq "$state" \
    --arg object_type "$object_type" \
    --arg raw_id "$raw_id" \
    --arg digest "$digest" \
    --arg action "$action" \
    --argjson destructive "$destructive" \
    --arg now "$now" \
    'if any(.inventory[]; .object_type == $object_type and .identity_digest == $digest) then .
     else .inventory += [{
       object_type: $object_type,
       private_id: $raw_id,
       identity_digest: $digest,
       cleanup_action: $action,
       destructive: $destructive,
       cleanup_result: "pending",
       updated_at: $now
     }] end
     | .updated_at = $now'
  slack_cert_emit_safe "$run_id"
}

slack_cert_cleanup_mark() {
  local run_id="$1"
  local include_destructive="$2"
  local state now
  state="$(slack_cert_state_file "$run_id")"
  now="$(slack_cert_now)"
  if [[ "$include_destructive" == "true" ]]; then
    slack_cert_atomic_jq "$state" \
      --arg now "$now" \
      '.inventory |= map(.cleanup_result = "cleaned" | .updated_at = $now)
       | .cleanup = {result:"pass",destructive_confirmation:true}
       | .updated_at = $now'
  else
    slack_cert_atomic_jq "$state" \
      --arg now "$now" \
      '.inventory |= map(
         if .destructive then .
         else .cleanup_result = "cleaned" | .updated_at = $now
         end
       )
       | .cleanup = {
           result: (if any(.inventory[]; .cleanup_result == "pending") then "pending" else "pass" end),
           destructive_confirmation: false
         }
       | .updated_at = $now'
  fi
  slack_cert_emit_safe "$run_id"
}

slack_cert_scan_paths() {
  local run_id="$1"
  shift
  local state matches_file scanned=0 path secret
  state="$(slack_cert_state_file "$run_id")"
  matches_file="$(slack_cert_run_dir "$run_id")/.secret-matches"
  : >"$matches_file"
  chmod 600 "$matches_file"
  for path in "$@"; do
    [[ -e "$path" ]] || continue
    scanned=$((scanned + 1))
    rg -n -I \
      'xox[baprs]-[A-Za-z0-9-]+|xoxe\.[A-Za-z0-9._-]+|Authorization:[[:space:]]*Bearer|client_secret[=:][[:space:]]*"?[A-Za-z0-9_-]{12,}|signing_secret[=:][[:space:]]*"?[A-Za-z0-9_-]{12,}|oauth/v2/authorize.*(code|state)=' \
      "$path" >>"$matches_file" 2>/dev/null || true
    for secret in "${SLACK_CERT_KNOWN_SECRETS[@]:-}"; do
      [[ -n "$secret" ]] || continue
      rg -n -I -F "$secret" "$path" >>"$matches_file" 2>/dev/null || true
    done
  done
  local now result
  now="$(slack_cert_now)"
  if [[ -s "$matches_file" ]]; then
    result="fail"
  else
    result="pass"
  fi
  slack_cert_atomic_jq "$state" \
    --arg result "$result" \
    --argjson scanned "$scanned" \
    --arg now "$now" \
    '.secret_scan = {result:$result,scanned_files:$scanned}
     | .updated_at = $now'
  rm -f "$matches_file"
  slack_cert_emit_safe "$run_id"
  [[ "$result" == "pass" ]]
}

slack_cert_register_known_secret() {
  local _run_id="$1"
  local value="$2"
  [[ -n "$value" ]] || return 0
  local existing
  for existing in "${SLACK_CERT_KNOWN_SECRETS[@]:-}"; do
    [[ "$existing" == "$value" ]] && return 0
  done
  SLACK_CERT_KNOWN_SECRETS+=("$value")
}

slack_cert_emit_safe() {
  local run_id="$1"
  local state safe temporary
  state="$(slack_cert_state_file "$run_id")"
  safe="$(slack_cert_safe_file "$run_id")"
  temporary="${safe}.tmp.$$"
  jq '{
    schema_version,
    run_id,
    mode,
    status,
    created_at,
    updated_at,
    expires_at,
    ttl_days,
    build,
    stages,
    inventory: [.inventory[] | {
      object_type,
      identity_digest,
      cleanup_action,
      destructive,
      cleanup_result,
      updated_at
    }],
    secret_scan,
    cleanup
  }' "$state" >"$temporary"
  chmod 600 "$temporary"
  mv "$temporary" "$safe"
}

slack_cert_validate_safe_evidence() {
  local file="$1"
  jq -e '
    .schema_version == 1
    and (.run_id | type == "string" and length > 0)
    and (.mode == "shared" or .mode == "dedicated" or .mode == "both")
    and (.status == "passed" or .status == "failed" or .status == "blocked" or .status == "in_progress")
    and (.created_at | type == "string")
    and (.expires_at | type == "string")
    and (.stages | type == "array" and length > 0)
    and (.inventory | type == "array")
    and (all(.inventory[]; has("private_id") | not))
    and (has("private_salt") | not)
  ' "$file" >/dev/null
}

slack_cert_freshness() {
  local expires_at="$1"
  local expires_epoch
  if expires_epoch="$(date -u -j -f '%Y-%m-%dT%H:%M:%SZ' "$expires_at" '+%s' 2>/dev/null)"; then
    :
  else
    expires_epoch="$(date -u -d "$expires_at" '+%s')"
  fi
  if (( expires_epoch >= $(slack_cert_epoch) )); then
    printf 'fresh'
  else
    printf 'stale'
  fi
}

slack_cert_validate_recorded_fixture() {
  local file="$1"
  jq -e '
    .schema_version == 1
    and .provider == "slack"
    and ([.cases[].kind] | sort ==
      ["events_api_delivery","gateway_import_receipt","manifest_diff","oauth_callback"])
    and all(.cases[]; .sanitized == true and (.payload | type == "object"))
  ' "$file" >/dev/null
}
