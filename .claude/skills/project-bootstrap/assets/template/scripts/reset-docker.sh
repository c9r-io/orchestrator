#!/usr/bin/env bash
# Project Docker Environment Reset
#
# Resets the local Docker environment to a clean state: stops containers,
# removes images/volumes, rebuilds, starts services, and waits for health.
#
# Usage:
#   ./scripts/reset-docker.sh          # Normal reset (preserves BuildKit mount caches)
#   ./scripts/reset-docker.sh --purge  # Full purge (also clears BuildKit caches)
#
# Env:
#   PROJECT_NAME (default: {{project_name}})
#   COMPOSE_FILE (default: <project_root>/docker/docker-compose.yml)
#   ENABLE_PROJECT_EXTRAS=true  # enable project-specific optional steps

set -e

PROJECT_NAME="${PROJECT_NAME:-{{project_name}}}"

# Parse arguments
PURGE=false
for arg in "$@"; do
  case $arg in
    --purge) PURGE=true ;;
  esac
done

# Resolve project root and compose file location.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
COMPOSE_FILE="${COMPOSE_FILE:-$PROJECT_ROOT/docker/docker-compose.yml}"

compose() {
  if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    docker compose -f "$COMPOSE_FILE" "$@"
  else
    docker-compose -f "$COMPOSE_FILE" "$@"
  fi
}

if [ ! -f "$COMPOSE_FILE" ]; then
  echo "Error: compose file not found: $COMPOSE_FILE" >&2
  echo "Hint: set COMPOSE_FILE, or ensure docker/docker-compose.yml exists." >&2
  exit 1
fi

# Banner
echo "${PROJECT_NAME} Docker Environment Reset"
echo "======================================="
if [ "$PURGE" = true ]; then
  echo "Mode: FULL PURGE (clearing all caches)"
else
  echo "Mode: Normal (preserving BuildKit mount caches for fast rebuilds)"
fi
echo ""

# Step 1: Stop and remove containers and volumes
printf "[1/6] Stopping and removing containers and volumes...\n"
compose down -v --remove-orphans 2>/dev/null || true

# Step 2: Remove project images (force rebuild)
printf "[2/6] Removing project images...\n"
docker rmi ${PROJECT_NAME}-core ${PROJECT_NAME}-portal 2>/dev/null || true

# Step 3: Prune Docker builder cache (only in --purge mode)
if [ "$PURGE" = true ]; then
  printf "[3/6] Pruning ALL builder cache (--purge)...\n"
  docker builder prune -af 2>/dev/null | tail -1 || true
else
  printf "[3/6] Skipping builder cache prune (mount caches preserved for fast rebuilds)\n"
fi

# Step 4: Build all images (parallel)
printf "[4/6] Building all images (parallel)...\n"
compose build --no-cache --parallel

# Step 5: Start services and wait for health checks
printf "[5/6] Starting services...\n"
compose up -d

if [ "${ENABLE_PROJECT_EXTRAS:-}" = "true" ]; then
  echo "[project extras] Add optional steps here if needed."
fi

echo "  Waiting for services to become healthy..."
TIMEOUT=120
ELAPSED=0
while [ $ELAPSED -lt $TIMEOUT ]; do
  NOT_READY=$(compose ps --format "{{.Status}}" 2>/dev/null | grep -ciE "starting|unhealthy" || true)
  if [ "$NOT_READY" -eq 0 ] 2>/dev/null; then
    echo "  All services healthy! (${ELAPSED}s)"
    break
  fi
  sleep 5
  ELAPSED=$((ELAPSED + 5))
  echo "  Still waiting... ($NOT_READY services not ready, ${ELAPSED}s elapsed)"
done

if [ $ELAPSED -ge $TIMEOUT ]; then
  echo "  WARNING: Timed out after ${TIMEOUT}s, some services may not be healthy"
fi

# Step 6: Verify
printf "[6/6] Verifying...\n"
compose ps --format "table {{.Name}}\t{{.Status}}" 2>/dev/null || compose ps

echo ""
echo "URLs:"
echo "  Core:   http://localhost:{{core_port}}/health"
echo "  Portal: http://localhost:{{portal_port}}"
echo ""
echo "Reset complete!"
