#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY="$REPO_ROOT/target/release/orchestrator"

if [[ ! -x "$BINARY" ]]; then
  echo "Building orchestrator CLI..."
  cd "$REPO_ROOT"
  cargo build --release -p orchestrator-cli
  BINARY="$REPO_ROOT/target/release/orchestrator"
fi

exec "$BINARY" "$@"
