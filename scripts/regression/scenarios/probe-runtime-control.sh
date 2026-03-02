#!/usr/bin/env bash
# probe-runtime-control.sh — Validate task pause/resume during runtime.
# Sourced by run-cli-probes.sh (probe-runner-lib.sh already loaded).

probe_setup_project "probe-rc"

_rc_run_pause_resume() {
  local start_ts
  start_ts=$(date +%s)

  probe_begin_scenario "runtime-control" "pause-resume"

  local create_output
  create_output="$(probe_create_task "probe_runtime_control" "cli_probe_ws" "--no-start" 2>&1)" || true
  local task_id
  task_id="$(probe_extract_task_id "$create_output")"

  if [[ -z "$task_id" ]]; then
    probe_error "  ✗ failed to extract task id"
    _PROBE_CURRENT_ASSERTIONS_OK=0
    probe_record_duration "0"
    probe_end_scenario
    return
  fi

  probe_info "  task_id=$task_id"

  "$PROBE_BINARY" task start "$task_id" >/dev/null 2>&1 &
  local runner_pid=$!
  sleep 5

  # Pause
  local pause_output
  pause_output="$("$PROBE_BINARY" task pause "$task_id" 2>&1)" || true
  local status_after_pause
  status_after_pause="$(probe_task_status "$task_id")"
  probe_assert_output_contains "$status_after_pause" "paused" "status is paused after pause"

  sleep 2

  # Resume
  "$PROBE_BINARY" task resume "$task_id" >/dev/null 2>&1 &
  local resume_pid=$!
  sleep 3

  local status_after_resume
  status_after_resume="$(probe_task_status "$task_id")"
  probe_assert_output_contains "$status_after_resume" "running" "status is running after resume"

  # Wait for completion
  probe_wait_task_done "$task_id" 180 || true
  wait "$runner_pid" 2>/dev/null || true
  wait "$resume_pid" 2>/dev/null || true

  local final_status
  final_status="$(probe_task_status "$task_id")"
  probe_assert_output_contains "$final_status" "completed" "task completes after resume"

  local end_ts
  end_ts=$(date +%s)
  probe_record_duration "$(( end_ts - start_ts ))"
  probe_end_scenario
}

_rc_run_pause_resume
