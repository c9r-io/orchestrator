#!/usr/bin/env bash
# probe-low-output.sh — Validate low-output vs active-output detection consistency.
# Sourced by run-cli-probes.sh (probe-runner-lib.sh already loaded).

probe_setup_project "probe-lo"

# ── Sub-scenario: detect low output in watch + trace ─────────────────

_lo_run_detect_low_output() {
  local start_ts
  start_ts=$(date +%s)

  probe_begin_scenario "low-output" "detect-low-output"

  local create_output
  create_output="$(probe_create_task "probe_low_output" "cli_probe_ws" "--no-start" 2>&1)" || true
  local task_id
  task_id="$(probe_extract_task_id "$create_output")"

  if [[ -z "$task_id" ]]; then
    probe_error "  ✗ failed to extract task id"
    _PROBE_CURRENT_ASSERTIONS_OK=0
    probe_record_duration "0"
    probe_end_scenario
    return
  fi

  probe_info "  task_id=$task_id (probe_low_output — ~125s sleep)"
  "$PROBE_BINARY" task start "$task_id" >/dev/null 2>&1 &
  local runner_pid=$!

  # Capture watch output once we expect heartbeats to have fired.
  # Low output triggers after >=90s elapsed + 3 stagnant heartbeats (30s each).
  local watch_output=""
  local attempt=0
  while [[ $attempt -lt 5 ]]; do
    sleep 25
    attempt=$((attempt + 1))
    # Capture a single watch frame snapshot via task info + events query
    watch_output="$("$PROBE_BINARY" task info "$task_id" 2>&1 || true)"
    if echo "$watch_output" | grep -qE "completed|failed"; then
      break
    fi
  done

  probe_wait_task_done "$task_id" 200 || true
  wait "$runner_pid" 2>/dev/null || true

  # Verify trace detects low_output
  local trace_json
  trace_json="$("$PROBE_BINARY" task trace "$task_id" --json 2>/dev/null || echo "{}")"
  probe_assert_trace_anomaly "$trace_json" "low_output" "trace detects low_output"

  # Verify trace terminal output uses unified naming
  local trace_terminal
  trace_terminal="$("$PROBE_BINARY" task trace "$task_id" 2>/dev/null || echo "")"
  probe_assert_output_contains "$trace_terminal" "low_output" "trace terminal shows canonical rule name"

  local end_ts
  end_ts=$(date +%s)
  probe_record_duration "$(( end_ts - start_ts ))"
  probe_end_scenario
}

# ── Sub-scenario: active output should NOT trigger low-output ────────

_lo_run_active_output_clean() {
  local start_ts
  start_ts=$(date +%s)

  probe_begin_scenario "low-output" "active-output-clean"

  local create_output
  create_output="$(probe_create_task "probe_active_output" "cli_probe_ws" "--no-start" 2>&1)" || true
  local task_id
  task_id="$(probe_extract_task_id "$create_output")"

  if [[ -z "$task_id" ]]; then
    probe_error "  ✗ failed to extract task id"
    _PROBE_CURRENT_ASSERTIONS_OK=0
    probe_record_duration "0"
    probe_end_scenario
    return
  fi

  probe_info "  task_id=$task_id (probe_active_output — continuous output)"
  "$PROBE_BINARY" task start "$task_id" >/dev/null 2>&1 &
  local runner_pid=$!

  probe_wait_task_done "$task_id" 200 || true
  wait "$runner_pid" 2>/dev/null || true

  local trace_json
  trace_json="$("$PROBE_BINARY" task trace "$task_id" --json 2>/dev/null || echo "{}")"

  # Active output task should NOT have low_output anomaly
  local low_output_count
  low_output_count="$(echo "$trace_json" | jq '[.anomalies[] | select(.rule == "low_output")] | length' 2>/dev/null || echo "0")"

  if [[ "$low_output_count" -eq 0 ]]; then
    probe_info "  ✓ active-output task has no low_output anomaly"
  else
    probe_error "  ✗ active-output task incorrectly flagged with low_output anomaly"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  fi

  local trace_terminal
  trace_terminal="$("$PROBE_BINARY" task trace "$task_id" 2>/dev/null || echo "")"
  probe_assert_output_not_contains "$trace_terminal" "low_output" \
    "trace terminal does not show low_output for active output"

  local end_ts
  end_ts=$(date +%s)
  probe_record_duration "$(( end_ts - start_ts ))"
  probe_end_scenario
}

_lo_run_detect_low_output
_lo_run_active_output_clean
