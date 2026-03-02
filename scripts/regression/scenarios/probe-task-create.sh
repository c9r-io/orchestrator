#!/usr/bin/env bash
# probe-task-create.sh — Validate task create target resolution.
# Sourced by run-cli-probes.sh (probe-runner-lib.sh already loaded).

# ── Sub-scenario: task-scoped workflow ───────────────────────────────

probe_setup_project "probe-tc"

_tc_run_task_scoped() {
  local start_ts
  start_ts=$(date +%s)

  probe_begin_scenario "task-create" "task-scoped"

  local create_output
  create_output="$(probe_create_task "probe_task_scoped" "cli_probe_ws" "--no-start" 2>&1)" || true
  local task_id
  task_id="$(probe_extract_task_id "$create_output")"

  probe_assert_output_matches "$create_output" "[0-9a-f-]{36}" "create returns a UUID"

  if [[ -n "$task_id" ]]; then
    "$PROBE_BINARY" task start "$task_id" >/dev/null 2>&1 &
    local runner_pid=$!
    probe_wait_task_done "$task_id" 120 || true
    wait "$runner_pid" 2>/dev/null || true

    local status
    status="$(probe_task_status "$task_id")"
    probe_assert_output_contains "$status" "completed" "task-scoped completes successfully"

    local trace_json
    trace_json="$("$PROBE_BINARY" task trace "$task_id" --json 2>/dev/null || echo "{}")"
    probe_assert_json_array_not_empty "$trace_json" ".cycles" "trace has cycles"
  else
    probe_error "  ✗ failed to extract task id from create output"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  fi

  local end_ts
  end_ts=$(date +%s)
  probe_record_duration "$(( end_ts - start_ts ))"
  probe_end_scenario
}

# ── Sub-scenario: item-scoped workflow ───────────────────────────────

_tc_run_item_scoped() {
  local start_ts
  start_ts=$(date +%s)

  probe_begin_scenario "task-create" "item-scoped"

  local create_output
  create_output="$(probe_create_task "probe_item_scoped" "cli_probe_ws" "--no-start" 2>&1)" || true
  local task_id
  task_id="$(probe_extract_task_id "$create_output")"

  probe_assert_output_matches "$create_output" "[0-9a-f-]{36}" "create returns a UUID"

  if [[ -n "$task_id" ]]; then
    "$PROBE_BINARY" task start "$task_id" >/dev/null 2>&1 &
    local runner_pid=$!
    probe_wait_task_done "$task_id" 120 || true
    wait "$runner_pid" 2>/dev/null || true

    local status
    status="$(probe_task_status "$task_id")"
    probe_assert_output_contains "$status" "completed" "item-scoped completes successfully"
  else
    probe_error "  ✗ failed to extract task id"
    _PROBE_CURRENT_ASSERTIONS_OK=0
  fi

  local end_ts
  end_ts=$(date +%s)
  probe_record_duration "$(( end_ts - start_ts ))"
  probe_end_scenario
}

# ── Sub-scenario: empty workspace ────────────────────────────────────

_tc_run_empty_workspace() {
  local start_ts
  start_ts=$(date +%s)

  probe_begin_scenario "task-create" "empty-workspace"

  local create_output
  local create_exit=0
  create_output="$(probe_create_task "probe_task_scoped" "cli_probe_empty_ws" "--no-start" 2>&1)" || create_exit=$?

  # With an empty workspace, task creation may succeed but produce zero items,
  # or it may fail outright. Either behavior is acceptable for regression.
  probe_assert_output_matches "$create_output" "[0-9a-f-]{36}|no.*target|0 item" \
    "empty workspace returns task-id or empty-target indication"

  local end_ts
  end_ts=$(date +%s)
  probe_record_duration "$(( end_ts - start_ts ))"
  probe_end_scenario
}

# ── Execute ──────────────────────────────────────────────────────────

_tc_run_task_scoped
_tc_run_item_scoped
_tc_run_empty_workspace
