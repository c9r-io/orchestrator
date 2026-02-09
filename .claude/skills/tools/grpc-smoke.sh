#!/usr/bin/env bash
# gRPC smoke checks (generic).
#
# This script is intentionally conservative: it only performs checks that can be
# expressed without project-specific RPC knowledge.
#
# Modes:
# - reflection-enabled: expect `grpcurl ... list` to succeed
# - reflection-disabled: expect `grpcurl ... list` to fail with a substring
# - skip (default): do nothing and exit 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GRPCURL="$SCRIPT_DIR/grpcurl-docker.sh"

GRPC_TARGET="${GRPC_TARGET:-}"
GRPC_SMOKE_MODE="${GRPC_SMOKE_MODE:-skip}"
GRPC_REFLECTION_ERROR_SUBSTR="${GRPC_REFLECTION_ERROR_SUBSTR:-server does not support the reflection API}"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

run_cmd_capture() {
  set +e
  local out
  out="$("$@" 2>&1)"
  local code=$?
  set -e
  printf "%s\n" "$code"
  printf "%s\n" "$out"
}

if [ "$GRPC_SMOKE_MODE" = "skip" ]; then
  echo "SKIP: GRPC_SMOKE_MODE=skip"
  exit 0
fi

if [ -z "$GRPC_TARGET" ]; then
  echo "Error: GRPC_TARGET is required (for example: my-grpc:50051)" >&2
  exit 1
fi

echo "[1/1] Reflection check ($GRPC_SMOKE_MODE)"
code=""
out=""
{
  IFS= read -r code
  out="$(cat)"
} < <(run_cmd_capture "$GRPCURL" -plaintext "$GRPC_TARGET" list)

if [ "$GRPC_SMOKE_MODE" = "reflection-enabled" ]; then
  if [ "$code" -ne 0 ]; then
    echo "$out" >&2
    fail "reflection-enabled: expected list to succeed"
  fi
elif [ "$GRPC_SMOKE_MODE" = "reflection-disabled" ]; then
  if [ "$code" -eq 0 ]; then
    fail "reflection-disabled: expected list to fail"
  fi
  if ! echo "$out" | grep -Fq "$GRPC_REFLECTION_ERROR_SUBSTR"; then
    echo "$out" >&2
    fail "reflection-disabled: expected error to contain: $GRPC_REFLECTION_ERROR_SUBSTR"
  fi
else
  fail "unknown GRPC_SMOKE_MODE: $GRPC_SMOKE_MODE"
fi

echo "OK: gRPC smoke checks passed"
