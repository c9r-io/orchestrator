#!/usr/bin/env bash

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "FR-001 sandbox matrix requires macOS sandbox-exec" >&2
  exit 1
fi

PROJECT="${QA_PROJECT:-qa-fr001-sandbox}"
DB_PATH="${ORCHESTRATOR_DB:-data/agent_orchestrator.db}"
BUNDLE="fixtures/manifests/bundles/sandbox-execution-profiles.yaml"

run_task() {
  local workflow="$1"
  local name="$2"
  local goal="$3"
  local event_type="$4"
  local reason_code="$5"
  local resource_kind="${6:-}"

  local task_id
  local task_create_output
  task_create_output=$(
    orchestrator task create \
      --project "${PROJECT}" \
      --workflow "${workflow}" \
      --name "${name}" \
      --goal "${goal}" \
      --no-start
  )
  task_id=$(
    printf '%s\n' "${task_create_output}" | grep -oE '[0-9a-f-]{36}' | tail -1
  )
  orchestrator task start "${task_id}" || true

  for _ in {1..30}; do
    local task_info_output
    task_info_output=$(orchestrator task info "${task_id}")
    case "${task_info_output}" in
      *"Status: completed"*|*"Status: failed"*) break ;;
    esac
    sleep 1
  done

  local payload
  payload=$(
    sqlite3 "${DB_PATH}" \
      "SELECT payload_json FROM events WHERE task_id='${task_id}' AND event_type='${event_type}' ORDER BY created_at DESC LIMIT 1;"
  )

  if [[ -z "${payload}" ]]; then
    echo "[FAIL] ${workflow}: missing ${event_type}" >&2
    exit 1
  fi
  if [[ "${payload}" != *"\"reason_code\":\"${reason_code}\""* ]]; then
    echo "[FAIL] ${workflow}: expected reason_code=${reason_code}" >&2
    echo "${payload}" >&2
    exit 1
  fi
  if [[ -n "${resource_kind}" && "${payload}" != *"\"resource_kind\":\"${resource_kind}\""* ]]; then
    echo "[FAIL] ${workflow}: expected resource_kind=${resource_kind}" >&2
    echo "${payload}" >&2
    exit 1
  fi

  echo "[PASS] ${workflow}: ${reason_code}"
}

echo "Resetting FR-001 sandbox QA project: ${PROJECT}"
orchestrator delete "project/${PROJECT}" --force 2>/dev/null || true
rm -rf "workspace/${PROJECT}"
orchestrator apply --project "${PROJECT}" -f "${BUNDLE}"

run_task "sandbox-open-files-limit" "sandbox fd limit" "sandbox fd limit" "sandbox_resource_exceeded" "open_files_limit_exceeded" "open_files"
run_task "sandbox-cpu-limit" "sandbox cpu limit" "sandbox cpu limit" "sandbox_resource_exceeded" "cpu_limit_exceeded" "cpu"
run_task "sandbox-memory-limit" "sandbox memory limit" "sandbox memory limit" "sandbox_resource_exceeded" "memory_limit_exceeded" "memory"
run_task "sandbox-process-limit" "sandbox process limit" "sandbox process limit" "sandbox_resource_exceeded" "processes_limit_exceeded" "processes"
run_task "sandbox-network-deny" "sandbox network deny" "sandbox network deny" "sandbox_network_blocked" "network_blocked"
run_task "sandbox-network-allowlist" "sandbox allowlist unsupported" "sandbox allowlist unsupported" "sandbox_network_blocked" "unsupported_backend_feature"

echo "FR-001 sandbox matrix passed"
