#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY="$REPO_ROOT/core/target/release/agent-orchestrator"

if [[ ! -x "$BINARY" ]]; then
  echo "Building orchestrator..."
  cd "$REPO_ROOT/core"
  cargo build --release
  BINARY="$REPO_ROOT/core/target/release/agent-orchestrator"
fi

RESTART_EXIT=75

while true; do
  set +e
  "$BINARY" "$@"
  exit_code=$?
  set -e

  if [[ $exit_code -eq $RESTART_EXIT ]]; then
    echo "[orchestrator] restart requested (exit $RESTART_EXIT) — re-launching"
    # Re-check binary exists (rebuild may have changed path)
    if [[ ! -x "$BINARY" ]]; then
      echo "[orchestrator] binary missing after restart request — rebuilding"
      cd "$REPO_ROOT/core" && cargo build --release
    fi
    continue
  fi

  exit "$exit_code"
done
