#!/usr/bin/env bash
# QA API testing helper.
# Wraps curl with optional token injection.
#
# Usage:
#   qa-api-test.sh GET  /api/v1/tenants
#   qa-api-test.sh POST /api/v1/users '{"email":"test@example.com","password":"Pass123!"}'
#   qa-api-test.sh PUT  /api/v1/tenants/{id}/password-policy '{"min_length":12}'
#
# Environment:
#   API_BASE_URL   - Base URL (default: http://localhost:8080)
#   API_TOKEN      - Pre-generated token (optional)
#   API_TOKEN_CMD  - Command to print token to stdout (optional)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

API_BASE_URL="${API_BASE_URL:-http://localhost:8080}"

API_TOKEN="${API_TOKEN:-}"
if [ -z "$API_TOKEN" ] && [ -n "${API_TOKEN_CMD:-}" ]; then
  # shellcheck disable=SC2091
  API_TOKEN="$(eval "$API_TOKEN_CMD" 2>/dev/null || true)"
fi

METHOD="${1:?Usage: qa-api-test.sh METHOD PATH [JSON_BODY]}"
PATH_PART="${2:?Usage: qa-api-test.sh METHOD PATH [JSON_BODY]}"
BODY="${3:-}"

AUTH_HEADER=()
if [ -n "$API_TOKEN" ]; then
  AUTH_HEADER=(-H "Authorization: Bearer $API_TOKEN")
fi

if [ -n "$BODY" ]; then
  # Force IPv4 to avoid occasional IPv6 localhost connection issues in sandboxed envs.
  curl -4 -s -X "$METHOD" "${API_BASE_URL}${PATH_PART}" \
    "${AUTH_HEADER[@]}" \
    -H "Content-Type: application/json" \
    -d "$BODY"
else
  # Force IPv4 to avoid occasional IPv6 localhost connection issues in sandboxed envs.
  curl -4 -s -X "$METHOD" "${API_BASE_URL}${PATH_PART}" \
    "${AUTH_HEADER[@]}"
fi
