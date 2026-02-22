#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOL_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$TOOL_ROOT/.." && pwd)"
BINARY="$REPO_ROOT/core/target/release/agent-orchestrator"

if [[ ! -x "$BINARY" ]]; then
  echo "Building orchestrator..."
  cd "$REPO_ROOT/core"
  cargo build --release
  BINARY="$REPO_ROOT/core/target/release/agent-orchestrator"
fi

exec "$BINARY" "$@"
