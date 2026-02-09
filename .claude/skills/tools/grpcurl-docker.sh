#!/usr/bin/env bash
# Run grpcurl from inside the Docker Compose network (ephemeral container).
#
# Why:
# - Host -> container gRPC ports may be blocked (VPN, firewall rules, sandboxed envs).
# - Running grpcurl from an ephemeral container attached to the same network is reliable.
#
# Usage:
#   .claude/skills/tools/grpcurl-docker.sh [grpcurl args...]
#
# Examples:
#   .claude/skills/tools/grpcurl-docker.sh -plaintext my-grpc:50051 list
#   GRPC_MOUNT_PROTO=core/proto .claude/skills/tools/grpcurl-docker.sh \
#     -import-path /proto -proto my.proto -d '{"hello":"world"}' \
#     my-grpc:50051 my.pkg.Service/MyMethod

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

GRPC_IMAGE="${GRPC_IMAGE:-fullstorydev/grpcurl}"
GRPC_COMPOSE_FILE="${GRPC_COMPOSE_FILE:-}"
GRPC_MOUNT_PROTO="${GRPC_MOUNT_PROTO:-}" # optional, mounted to /proto:ro

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ] || [ $# -eq 0 ]; then
  cat <<EOF
Usage:
  grpcurl-docker.sh [grpcurl args...]

Environment:
  GRPC_NETWORK       Docker network name (auto-detect if unset)
  GRPC_COMPOSE_FILE  Compose file used for auto-detect (defaults to docker/docker-compose.yml if present)
  GRPC_IMAGE         grpcurl image (default: $GRPC_IMAGE)
  GRPC_MOUNT_PROTO   Host proto dir to mount to /proto:ro (optional)

Notes:
  - If GRPC_MOUNT_PROTO is relative, it is resolved from repo root.
  - You usually want (if you mount protos): -import-path /proto -proto <file.proto>
EOF
  exit 0
fi

compose() {
  if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    docker compose "$@"
  else
    docker-compose "$@"
  fi
}

autodetect_compose_file() {
  if [ -n "$GRPC_COMPOSE_FILE" ]; then
    echo "$GRPC_COMPOSE_FILE"
    return 0
  fi

  if [ -f "$PROJECT_ROOT/docker/docker-compose.yml" ]; then
    echo "$PROJECT_ROOT/docker/docker-compose.yml"
    return 0
  fi
  if [ -f "$PROJECT_ROOT/docker-compose.yml" ]; then
    echo "$PROJECT_ROOT/docker-compose.yml"
    return 0
  fi
  if [ -f "$PROJECT_ROOT/compose.yaml" ]; then
    echo "$PROJECT_ROOT/compose.yaml"
    return 0
  fi
  if [ -f "$PROJECT_ROOT/compose.yml" ]; then
    echo "$PROJECT_ROOT/compose.yml"
    return 0
  fi

  echo ""
}

detect_network() {
  if [ -n "${GRPC_NETWORK:-}" ]; then
    echo "$GRPC_NETWORK"
    return 0
  fi

  local compose_file
  compose_file="$(autodetect_compose_file)"
  if [ -n "$compose_file" ] && [ -f "$compose_file" ]; then
    local ids
    ids="$(compose -f "$compose_file" --project-directory "$PROJECT_ROOT" ps -q 2>/dev/null || true)"
    local first_id
    first_id="$(echo "$ids" | head -n 1 | tr -d '\r')"
    if [ -n "$first_id" ]; then
      local n
      n="$(docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{printf "%s\n" $k}}{{end}}' "$first_id" 2>/dev/null | head -n 1 || true)"
      if [ -n "$n" ]; then
        echo "$n"
        return 0
      fi
    fi
  fi

  echo ""
}

NETWORK="$(detect_network)"
if [ -z "$NETWORK" ]; then
  echo "Error: could not auto-detect Docker network. Set GRPC_NETWORK explicitly." >&2
  exit 1
fi

MOUNT_ARGS=()
if [ -n "$GRPC_MOUNT_PROTO" ]; then
  if [[ "$GRPC_MOUNT_PROTO" = /* ]]; then
    if [ ! -d "$GRPC_MOUNT_PROTO" ]; then
      echo "Error: GRPC_MOUNT_PROTO is not a directory: $GRPC_MOUNT_PROTO" >&2
      exit 1
    fi
    MOUNT_ARGS=(-v "$GRPC_MOUNT_PROTO:/proto:ro")
  else
    if [ ! -d "$PROJECT_ROOT/$GRPC_MOUNT_PROTO" ]; then
      echo "Error: GRPC_MOUNT_PROTO is not a directory: $PROJECT_ROOT/$GRPC_MOUNT_PROTO" >&2
      exit 1
    fi
    MOUNT_ARGS=(-v "$PROJECT_ROOT/$GRPC_MOUNT_PROTO:/proto:ro")
  fi
fi

# shellcheck disable=SC2086
exec docker run --rm \
  --network "$NETWORK" \
  "${MOUNT_ARGS[@]}" \
  "$GRPC_IMAGE" \
  "$@"
