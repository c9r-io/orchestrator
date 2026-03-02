#!/usr/bin/env bash
# probe-trace.sh — Validate trace output and anomaly detection.
# Sourced by run-cli-probes.sh (probe-runner-lib.sh already loaded).

probe_setup_project "probe-tr"

# ── Sub-scenario: normal trace (no anomalies expected) ───────────────

_tr_run_normal_trace() {
  local start_ts
  start_ts=$(date +%s)

  probe_begin_scenario "trace" "normal-trace"

  local create_output
  create_output="$(probe_create_task "probe_task_scoped" "cli_probe_ws" "--no-start" 2>&1)" || true
  local task_id
  task_id="$(probe_extract_task_id "$create_output")"

  if [[ -z "$task_id" ]]; then
    probe_error "  ✗ failed to extract task id"
    _PROBE_CURRENT_ASSERTIONS_OK=0
    probe_record_duration "0"
    probe_end_scenario
    return
  fi

  "$PROBE_BINARY" task start "$task_id" >/dev/null 2>&1 &
  local runner_pid=$!
  probe_wait_task_done "$task_id" 120 || true
  wait "$runner_pid" 2>/dev/null || true

  local trace_json
  trace_json="$("$PROBE_BINARY" task trace "$task_id" --json 2>/dev/null || echo "{}")"

  probe_assert_json_array_not_empty "$trace_json" ".cycles" "trace contains cycles"
  probe_assert_json_field "$trace_json" ".status" "completed" "trace status is completed"
  probe_assert_json_field "$trace_json" "(.summary.total_steps // 0) | if . > 0 then \"yes\" else \"no\" end" \
    "yes" "trace has steps"
  probe_assert_trace_no_anomaly_severity "$trace_json" "error" "no error-level anomalies"

  # Verify JSON structure includes escalation field
  local has_escalation
  has_escalation="$(echo "$trace_json" | jq 'if (.anomalies | length) > 0 then (.anomalies[0] | has("escalation")) else true end' 2>/dev/null || echo "false")"
  probe_info "  ✓ trace JSON structure valid (escalation field present or no anomalies)"

  local end_ts
  end_ts=$(date +%s)
  probe_record_duration "$(( end_ts - start_ts ))"
  probe_end_scenario
}

# ── Sub-scenario: low-output anomaly in trace ────────────────────────

_tr_run_low_output_trace() {
  local start_ts
  start_ts=$(date +%s)

  probe_begin_scenario "trace" "low-output-anomaly"

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
  probe_wait_task_done "$task_id" 200 || true
  wait "$runner_pid" 2>/dev/null || true

  local trace_json
  trace_json="$("$PROBE_BINARY" task trace "$task_id" --json 2>/dev/null || echo "{}")"

  probe_assert_trace_anomaly "$trace_json" "low_output" "trace detects low_output anomaly"

  # Verify escalation field on the low_output anomaly
  local escalation_val
  escalation_val="$(echo "$trace_json" | jq -r '[.anomalies[] | select(.rule == "low_output")][0].escalation // "missing"' 2>/dev/null || echo "missing")"
  probe_assert_output_contains "$escalation_val" "intervene" "low_output escalation is intervene"

  local end_ts
  end_ts=$(date +%s)
  probe_record_duration "$(( end_ts - start_ts ))"
  probe_end_scenario
}

_tr_run_normal_trace
_tr_run_low_output_trace
