#!/usr/bin/env bash
# watchdog.sh — Independent health monitor for the orchestrator binary.
# Polls periodically, checks binary health, and restores from .stable on repeated failure.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

BINARY_PATH="${BINARY_PATH:-$PROJECT_ROOT/core/target/release/agent-orchestrator}"
STABLE_PATH="${STABLE_PATH:-$PROJECT_ROOT/.stable}"
POLL_INTERVAL="${WATCHDOG_POLL_INTERVAL:-60}"
MAX_FAILURES="${WATCHDOG_MAX_FAILURES:-3}"
HEALTH_TIMEOUT="${WATCHDOG_HEALTH_TIMEOUT:-10}"

consecutive_failures=0

cleanup() {
    echo "[watchdog] shutting down gracefully"
    exit 0
}

trap cleanup SIGTERM SIGINT

health_check() {
    if [ ! -f "$BINARY_PATH" ]; then
        echo "[watchdog] binary not found at $BINARY_PATH"
        return 1
    fi

    if timeout "$HEALTH_TIMEOUT" "$BINARY_PATH" --help >/dev/null 2>&1; then
        return 0
    else
        echo "[watchdog] binary --help failed or timed out"
        return 1
    fi
}

restore_stable() {
    if [ ! -f "$STABLE_PATH" ]; then
        echo "[watchdog] ERROR: no .stable binary found at $STABLE_PATH — cannot restore"
        return 1
    fi

    echo "[watchdog] restoring .stable binary to $BINARY_PATH"
    cp "$STABLE_PATH" "$BINARY_PATH"
    chmod +x "$BINARY_PATH"
    echo "[watchdog] binary restored successfully"
}

echo "[watchdog] started — polling every ${POLL_INTERVAL}s, max failures: $MAX_FAILURES"
echo "[watchdog] binary: $BINARY_PATH"
echo "[watchdog] stable: $STABLE_PATH"

while true; do
    if health_check; then
        if [ "$consecutive_failures" -gt 0 ]; then
            echo "[watchdog] binary recovered after $consecutive_failures failure(s)"
        fi
        consecutive_failures=0
    else
        consecutive_failures=$((consecutive_failures + 1))
        echo "[watchdog] health check failed ($consecutive_failures/$MAX_FAILURES)"

        if [ "$consecutive_failures" -ge "$MAX_FAILURES" ]; then
            echo "[watchdog] $MAX_FAILURES consecutive failures — triggering restore"
            if restore_stable; then
                consecutive_failures=0
            else
                echo "[watchdog] restore failed — will keep retrying"
            fi
        fi
    fi

    sleep "$POLL_INTERVAL"
done
