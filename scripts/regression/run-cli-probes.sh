#!/usr/bin/env bash
# run-cli-probes.sh — Unified CLI probe regression runner.
#
# Usage:
#   ./scripts/regression/run-cli-probes.sh                     # run all
#   ./scripts/regression/run-cli-probes.sh --group trace       # run one group
#   ./scripts/regression/run-cli-probes.sh --list              # list scenarios
#   ./scripts/regression/run-cli-probes.sh --json              # JSON output
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/probe-runner-lib.sh
source "$SCRIPT_DIR/lib/probe-runner-lib.sh"

GROUP_FILTER=""
LIST_ONLY=0
JSON_OUTPUT=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --group)
      GROUP_FILTER="$2"
      shift 2
      ;;
    --list)
      LIST_ONLY=1
      shift
      ;;
    --json)
      JSON_OUTPUT=1
      shift
      ;;
    -h|--help)
      cat <<'USAGE'
Usage: run-cli-probes.sh [OPTIONS]

Options:
  --group <name>   Only run scenarios in the given group
                   (task-create, runtime-control, trace, low-output)
  --list           List available scenario groups and exit
  --json           Output results as JSON
  -h, --help       Show this help
USAGE
      exit 0
      ;;
    *)
      echo "[ERROR] Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

AVAILABLE_SCENARIOS=(
  "task-create:probe-task-create.sh"
  "runtime-control:probe-runtime-control.sh"
  "trace:probe-trace.sh"
  "low-output:probe-low-output.sh"
)

if [[ $LIST_ONLY -eq 1 ]]; then
  echo "Available scenario groups:"
  for entry in "${AVAILABLE_SCENARIOS[@]}"; do
    local_group="${entry%%:*}"
    local_script="${entry##*:}"
    echo "  $local_group  ($local_script)"
  done
  exit 0
fi

probe_init
cd "$PROBE_REPO_ROOT"
probe_ensure_fixtures

probe_info "═══════════════════════════════════════════════"
probe_info "  CLI Probe Regression Runner"
probe_info "═══════════════════════════════════════════════"
probe_info ""

for entry in "${AVAILABLE_SCENARIOS[@]}"; do
  group="${entry%%:*}"
  script="${entry##*:}"

  if [[ -n "$GROUP_FILTER" && "$group" != "$GROUP_FILTER" ]]; then
    continue
  fi

  scenario_script="$SCRIPT_DIR/scenarios/$script"
  if [[ ! -f "$scenario_script" ]]; then
    probe_warn "Scenario script not found: $scenario_script (skipping)"
    continue
  fi

  probe_info "Running group: $group"
  # shellcheck source=/dev/null
  source "$scenario_script"
done

if [[ $JSON_OUTPUT -eq 1 ]]; then
  probe_summary_json
else
  probe_summary
fi

probe_exit_code
