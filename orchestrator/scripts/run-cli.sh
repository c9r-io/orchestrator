#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOL_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY="$TOOL_ROOT/src-tauri/target/release/agent-orchestrator"

if [[ ! -x "$BINARY" ]]; then
  echo "Building orchestrator..."
  cd "$TOOL_ROOT/src-tauri"
  cargo build --release
  BINARY="$TOOL_ROOT/src-tauri/target/release/agent-orchestrator"
fi

exec "$BINARY" task start --latest "$@"
