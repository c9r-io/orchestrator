#!/usr/bin/env bash
set -euo pipefail

# Find project root: try git root first, fallback to relative path
if git rev-parse --show-toplevel >/dev/null 2>&1; then
  ROOT="$(git rev-parse --show-toplevel)"
else
  ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
fi
cd "$ROOT"

COMPOSE_FILE="${COMPOSE_FILE:-docker/docker-compose.yml}"
LOG_TAIL="${LOG_TAIL:-200}"

say() { printf "%s\n" "$*"; }
die() { printf "ERROR: %s\n" "$*" >&2; exit 1; }

compose() {
  local compose_dir="$(dirname "$COMPOSE_FILE")"
  if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    docker compose -f "$COMPOSE_FILE" --project-directory "$compose_dir" "$@"
  else
    docker-compose -f "$COMPOSE_FILE" "$@"
  fi
}

say "== repo =="
git status --porcelain || true

if [ ! -f "$COMPOSE_FILE" ]; then
  die "compose file not found: $COMPOSE_FILE"
fi

if [ "${RESET_FIRST:-}" = "true" ] && [ -x "./scripts/reset-docker.sh" ]; then
  say "== reset =="
  ./scripts/reset-docker.sh
fi

say "== compose up =="
compose up -d --build

say "== compose ps =="
compose ps

say "== compose logs (tail) =="
compose logs --tail "$LOG_TAIL" || true

# Basic heuristic: surface obvious errors in recent logs (non-fatal, but useful signal).
say "== log scan (ERROR|FATAL|panic) =="
if compose logs --tail "$LOG_TAIL" 2>/dev/null | rg -n "(ERROR|FATAL|panic)" -S; then
  say "WARN: matched error keywords in logs (review above)."
else
  say "OK: no obvious error keywords in last $LOG_TAIL log lines."
fi

say "OK: local compose checks completed."

