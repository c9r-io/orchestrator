#!/usr/bin/env bash
# probe-runner-lib.sh — Core library for CLI probe regression runner.
# Source this file from scenario scripts and the main runner.
set -euo pipefail

# ── Configuration ────────────────────────────────────────────────────

PROBE_REPO_ROOT=""
PROBE_BINARY=""
PROBE_PROJECT=""
PROBE_WORKSPACE=""
PROBE_OUTPUT_JSON=0

# Scenario registry: parallel arrays
declare -a _PROBE_GROUPS=()
declare -a _PROBE_NAMES=()
declare -a _PROBE_STATUSES=()
declare -a _PROBE_DURATIONS=()
declare -a _PROBE_ERRORS=()
_PROBE_PASS_COUNT=0
_PROBE_FAIL_COUNT=0

# Per-scenario assertion tracking
_PROBE_CURRENT_ASSERTIONS_OK=1

# ── Init ─────────────────────────────────────────────────────────────

probe_repo_root() {
  local script_dir
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
  cd "$script_dir/../../.." && pwd -P
}

probe_init() {
  PROBE_REPO_ROOT="$(probe_repo_root)"
  PROBE_BINARY="$PROBE_REPO_ROOT/core/target/release/agent-orchestrator"

  if [[ ! -x "$PROBE_BINARY" ]]; then
    probe_error "Binary not found: $PROBE_BINARY"
    probe_error "Build it with: (cd core && cargo build --release)"
    exit 2
  fi
}

probe_ensure_fixtures() {
  "$PROBE_BINARY" init --force >/dev/null 2>&1 || true
  "$PROBE_BINARY" apply -f fixtures/manifests/bundles/cli-probe-fixtures.yaml >/dev/null 2>&1 || {
    probe_error "Failed to apply cli-probe-fixtures.yaml"
    exit 2
  }
}

probe_setup_project() {
  local prefix="${1:-probe-regr}"
  PROBE_PROJECT="${prefix}-$(date +%s)-$RANDOM"
  PROBE_WORKSPACE="${PROBE_PROJECT}-ws"
  export PROBE_PROJECT PROBE_WORKSPACE
}

# ── Logging ──────────────────────────────────────────────────────────

probe_info()  { echo "[INFO]  $*"; }
probe_warn()  { echo "[WARN]  $*"; }
probe_error() { echo "[ERROR] $*"; }

# ── Scenario lifecycle ───────────────────────────────────────────────

probe_begin_scenario() {
  local group="$1"
  local name="$2"
  _PROBE_GROUPS+=("$group")
  _PROBE_NAMES+=("$name")
  _PROBE_CURRENT_ASSERTIONS_OK=1
  probe_info "─── $group / $name ───"
}

probe_end_scenario() {
  local status
  local idx=$(( ${#_PROBE_GROUPS[@]} - 1 ))

  if [[ $_PROBE_CURRENT_ASSERTIONS_OK -eq 1 ]]; then
    status="PASS"
    _PROBE_PASS_COUNT=$(( _PROBE_PASS_COUNT + 1 ))
  else
    status="FAIL"
    _PROBE_FAIL_COUNT=$(( _PROBE_FAIL_COUNT + 1 ))
  fi
  _PROBE_STATUSES+=("$status")
}

probe_record_duration() {
  _PROBE_DURATIONS+=("$1")
}

probe_record_error() {
  _PROBE_ERRORS+=("$1")
}

# ── Assertions ───────────────────────────────────────────────────────

probe_assert_output_contains() {
  local output="$1"
  local pattern="$2"
  local label="${3:-output should contain '$pattern'}"

  if echo "$output" | grep -qF "$pattern"; then
    probe_info "  ✓ $label"
  else
    probe_error "  ✗ $label"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  fi
}

probe_assert_output_matches() {
  local output="$1"
  local regex="$2"
  local label="${3:-output should match '$regex'}"

  if echo "$output" | grep -qE "$regex"; then
    probe_info "  ✓ $label"
  else
    probe_error "  ✗ $label"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  fi
}

probe_assert_output_not_contains() {
  local output="$1"
  local pattern="$2"
  local label="${3:-output should NOT contain '$pattern'}"

  if echo "$output" | grep -qF "$pattern"; then
    probe_error "  ✗ $label"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  else
    probe_info "  ✓ $label"
  fi
}

probe_assert_exit_code() {
  local actual="$1"
  local expected="$2"
  local label="${3:-exit code should be $expected}"

  if [[ "$actual" -eq "$expected" ]]; then
    probe_info "  ✓ $label"
  else
    probe_error "  ✗ $label (got $actual)"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  fi
}

probe_assert_json_field() {
  local json_output="$1"
  local jq_expr="$2"
  local expected="$3"
  local label="${4:-JSON field $jq_expr should be $expected}"

  local actual
  actual="$(echo "$json_output" | jq -r "$jq_expr" 2>/dev/null || echo "__jq_error__")"

  if [[ "$actual" == "$expected" ]]; then
    probe_info "  ✓ $label"
  else
    probe_error "  ✗ $label (got: $actual)"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  fi
}

probe_assert_json_array_not_empty() {
  local json_output="$1"
  local jq_expr="$2"
  local label="${3:-JSON array $jq_expr should not be empty}"

  local count
  count="$(echo "$json_output" | jq "$jq_expr | length" 2>/dev/null || echo "0")"

  if [[ "$count" -gt 0 ]]; then
    probe_info "  ✓ $label (count=$count)"
  else
    probe_error "  ✗ $label (empty)"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  fi
}

probe_assert_trace_anomaly() {
  local trace_json="$1"
  local rule="$2"
  local label="${3:-trace should contain anomaly rule '$rule'}"

  local found
  found="$(echo "$trace_json" | jq --arg r "$rule" '[.anomalies[] | select(.rule == $r)] | length' 2>/dev/null || echo "0")"

  if [[ "$found" -gt 0 ]]; then
    probe_info "  ✓ $label"
  else
    probe_error "  ✗ $label"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  fi
}

probe_assert_trace_no_anomaly_severity() {
  local trace_json="$1"
  local severity="$2"
  local label="${3:-trace should have no $severity anomalies}"

  local found
  found="$(echo "$trace_json" | jq --arg s "$severity" '[.anomalies[] | select(.severity == $s)] | length' 2>/dev/null || echo "0")"

  if [[ "$found" -eq 0 ]]; then
    probe_info "  ✓ $label"
  else
    probe_error "  ✗ $label (found $found)"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  fi
}

# ── Task helpers ─────────────────────────────────────────────────────

probe_extract_task_id() {
  local create_output="$1"
  echo "$create_output" | grep -oE '[0-9a-f-]{36}' | head -1
}

probe_create_task() {
  local workflow="$1"
  local workspace="${2:-cli_probe_ws}"
  local extra_args="${3:-}"

  local task_name="probe-$(date +%s)-$RANDOM"
  local create_args=(
    task create
    --name "$task_name"
    --goal "Probe regression test"
    --project "$PROBE_PROJECT"
    --workspace "$workspace"
    --workflow "$workflow"
  )
  if [[ -n "$extra_args" ]]; then
    read -ra extra <<< "$extra_args"
    create_args+=("${extra[@]}")
  fi

  "$PROBE_BINARY" "${create_args[@]}" 2>&1
}

probe_wait_task_done() {
  local task_id="$1"
  local timeout="${2:-300}"
  local elapsed=0

  while [[ $elapsed -lt $timeout ]]; do
    local info_output
    info_output="$("$PROBE_BINARY" task info "$task_id" 2>&1 || true)"
    if echo "$info_output" | grep -qiE 'status:[[:space:]]*(completed|failed)'; then
      return 0
    fi
    sleep 3
    elapsed=$((elapsed + 3))
  done
  probe_warn "Task $task_id did not finish within ${timeout}s"
  return 1
}

probe_task_status() {
  local task_id="$1"
  local info
  info="$("$PROBE_BINARY" task info "$task_id" 2>&1 || true)"
  echo "$info" | sed -n 's/.*[Ss]tatus:[[:space:]]*\([^ ]*\).*/\1/p' | head -1
}

# ── Summary ──────────────────────────────────────────────────────────

probe_summary() {
  echo ""
  echo "═══════════════════════════════════════════════"
  echo "  CLI Probe Regression Results"
  echo "═══════════════════════════════════════════════"

  local i
  for i in "${!_PROBE_GROUPS[@]}"; do
    local group="${_PROBE_GROUPS[$i]}"
    local name="${_PROBE_NAMES[$i]}"
    local status="${_PROBE_STATUSES[$i]}"
    local dur="${_PROBE_DURATIONS[$i]:-?}"

    if [[ "$status" == "PASS" ]]; then
      printf "  \x1b[32m[PASS]\x1b[0m  %-20s / %-25s (%ss)\n" "$group" "$name" "$dur"
    else
      printf "  \x1b[31m[FAIL]\x1b[0m  %-20s / %-25s (%ss)\n" "$group" "$name" "$dur"
    fi
  done

  echo "═══════════════════════════════════════════════"
  echo "  $_PROBE_PASS_COUNT passed, $_PROBE_FAIL_COUNT failed"
  echo "═══════════════════════════════════════════════"
}

probe_summary_json() {
  local results="["
  local i
  for i in "${!_PROBE_GROUPS[@]}"; do
    [[ $i -gt 0 ]] && results+=","
    results+="$(printf '{"group":"%s","name":"%s","status":"%s","duration_secs":"%s"}' \
      "${_PROBE_GROUPS[$i]}" "${_PROBE_NAMES[$i]}" "${_PROBE_STATUSES[$i]}" "${_PROBE_DURATIONS[$i]:-0}")"
  done
  results+="]"

  printf '{"passed":%d,"failed":%d,"scenarios":%s}\n' \
    "$_PROBE_PASS_COUNT" "$_PROBE_FAIL_COUNT" "$results"
}

probe_exit_code() {
  if [[ $_PROBE_FAIL_COUNT -gt 0 ]]; then
    return 1
  fi
  return 0
}
