#!/usr/bin/env bash
set -euo pipefail

qa_info() { echo "[INFO] $*"; }
qa_warn() { echo "[WARN] $*"; }
qa_error() { echo "[ERROR] $*"; }

qa_repo_root() {
  local script_dir
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  cd "$script_dir/../../../.." && pwd
}

qa_binary_path() {
  local root
  root="$(qa_repo_root)"
  echo "$root/core/target/release/agent-orchestrator"
}

qa_require_binary() {
  local bin
  bin="$(qa_binary_path)"
  if [[ ! -x "$bin" ]]; then
    qa_error "Binary not found: $bin"
    qa_error "Build it with: (cd core && cargo build --release)"
    exit 2
  fi
}

qa_parse_common_args() {
  QA_OUTPUT_JSON=0
  QA_WORKSPACE=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --workspace)
        QA_WORKSPACE="$2"
        shift 2
        ;;
      --json)
        QA_OUTPUT_JSON=1
        shift
        ;;
      -h|--help)
        return 1
        ;;
      *)
        qa_error "Unknown argument: $1"
        return 1
        ;;
    esac
  done

  export QA_OUTPUT_JSON QA_WORKSPACE
}

qa_print_usage() {
  cat <<'USAGE'
Usage:
  <script>.sh [--workspace <workspace-id>] [--json]

Options:
  --workspace <workspace-id>  Workspace ID used for task creation.
  --json                      Print machine-readable summary JSON.
USAGE
}

qa_extract_task_id() {
  local create_output="$1"
  echo "$create_output" | grep -oE '[0-9a-f-]{36}' | head -1
}
