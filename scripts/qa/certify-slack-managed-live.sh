#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ENV_FILE="${FR114_LIVE_ENV_FILE:-$HOME/.config/orchestrator/qa/fr114.env}"

[[ -f "$ENV_FILE" ]] || {
  echo "FR-114 live environment not found: $ENV_FILE" >&2
  echo "Copy config/qa/slack-live.env.example outside the repository and chmod 600." >&2
  exit 2
}

permissions="$(stat -f '%Lp' "$ENV_FILE" 2>/dev/null || stat -c '%a' "$ENV_FILE")"
[[ "$permissions" == "600" || "$permissions" == "400" ]] || {
  echo "FR-114 live environment must have mode 600 or 400" >&2
  exit 2
}

set -a
# shellcheck disable=SC1090
source "$ENV_FILE"
set +a

cd "$REPO_ROOT"
FR114_ALLOW_DIRTY="${FR114_ALLOW_DIRTY:-0}" ./scripts/qa/test-slack-managed-shared-oauth.sh

if [[ -n "${SLACK_LIVE_OFFICIAL_MANIFEST_PATH:-}" ]]; then
  [[ -f "$SLACK_LIVE_OFFICIAL_MANIFEST_PATH" ]] || {
    echo "reviewed Slack manifest not found" >&2
    exit 2
  }
  rg -q 'reactions:read' "$SLACK_LIVE_OFFICIAL_MANIFEST_PATH"
  ! rg -q 'chat:write|reactions:write|xox[baprs]-' "$SLACK_LIVE_OFFICIAL_MANIFEST_PATH"
fi

./scripts/qa/test-slack-managed-live-smoke.sh
