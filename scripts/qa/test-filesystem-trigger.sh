#!/usr/bin/env bash
#
# QA test: Filesystem Trigger (FR-085 / QA-132)
# Validates filesystem trigger config types, validation, and code structure.
# Daemon-level integration scenarios (file create → task) require manual testing.

set -euo pipefail

# Declared so the surface gate can check them against the job that runs this,
# and so the environment-parity gate knows this one pays for a workspace build.
# Without a preamble a gate declares nothing and neither check can see it.
for required in cargo rg; do
  command -v "$required" >/dev/null 2>&1 || {
    echo "missing required command: $required" >&2
    exit 1
  }
done

PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); echo "  PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL: $1"; }

LOG_DIR="$(mktemp -d "${TMPDIR:-/tmp}/fr085-filesystem-trigger.XXXXXX")"
cleanup() { rm -rf "$LOG_DIR"; }
trap cleanup EXIT

# Runs a cargo command, keeping its output. Discarding it with >/dev/null 2>&1
# is why a real CI failure here read `FAIL: cargo test --workspace` and nothing
# else, and had to be reproduced locally and cross-compared against a sibling
# job before anyone could say what broke. On failure the tail of the log goes to
# stderr, so the CI log carries the compiler's diagnosis.
run_cargo() {
  local label="$1"
  shift
  local log="$LOG_DIR/$(echo "$label" | tr -c 'A-Za-z0-9' '-').log"
  if "$@" > "$log" 2>&1; then
    pass "$label"
  else
    fail "$label"
    echo "    --- last 40 lines of $* ---" >&2
    tail -40 "$log" >&2
    echo "    --- end ---" >&2
  fi
}

echo "=== QA 132: Filesystem Trigger ==="
echo ""

# ── Scenario 1: Compilation and tests ─────────────────────────────────────────
# orchestrator-gui stays excluded here even though the sibling test and clippy
# jobs stopped excluding it (FR-076 requirement 1): those jobs install the
# webkit2gtk/gtk packages and build gui/dist before cargo runs, and the job
# that runs this gate does not. The unexcluded form is therefore still a
# superset whose extra member cannot build where this gate runs; GUI coverage
# is owned by the siblings.
#
# FR085_SKIP_WORKSPACE=1 defers these two commands to the sibling test and
# clippy jobs of the same workflow, which run a strict superset of them (same
# -D warnings, no GUI exclusion) — the FR-126 FAST-mode shape, where the
# certifying aggregate is the workflow. The flag is set only by ci.yml's
# governance job; a local run still pays for the full pair, and the
# behavioural fixture in test-qa-gate-surface.sh still drives this path to
# prove a cargo failure's diagnosis reaches the log.
echo "--- Scenario 1: Compilation and tests ---"
if [[ "${FR085_SKIP_WORKSPACE:-0}" != "1" ]]; then
  run_cargo "cargo test --workspace" \
    cargo test --workspace --exclude orchestrator-gui
  run_cargo "cargo clippy clean" \
    cargo clippy --workspace --exclude orchestrator-gui --all-targets -- -D warnings
else
  pass "workspace test and clippy explicitly deferred to the sibling jobs that run a superset of them"
fi

# ── Scenario 6: serde roundtrip ──────────────────────────────────────────────
echo ""
echo "--- Scenario 6: serde roundtrip ---"
run_cargo "trigger_yaml_roundtrip_filesystem" \
  cargo test -p agent-orchestrator -- trigger_yaml_roundtrip_filesystem

# ── Scenario 7: Unit tests for filesystem validation ─────────────────────────
echo ""
echo "--- Scenario 7: Filesystem validation unit tests ---"
for test_name in \
  trigger_validate_accepts_filesystem_source \
  trigger_validate_filesystem_requires_paths \
  trigger_validate_filesystem_requires_block \
  trigger_validate_filesystem_rejects_invalid_events; do
  run_cargo "$test_name" cargo test -p agent-orchestrator -- "$test_name"
done

# ── Scenario 8: Config types exist ───────────────────────────────────────────
echo ""
echo "--- Scenario 8: Config types exist ---"
if rg -q "pub struct TriggerFilesystemSpec" crates/orchestrator-config/src/cli_types.rs; then
  pass "TriggerFilesystemSpec defined"
else
  fail "TriggerFilesystemSpec missing"
fi

if rg -q "pub struct TriggerFilesystemConfig" crates/orchestrator-config/src/config/trigger.rs; then
  pass "TriggerFilesystemConfig defined"
else
  fail "TriggerFilesystemConfig missing"
fi

# ── Scenario 9: FsWatcher module structure ───────────────────────────────────
echo ""
echo "--- Scenario 9: FsWatcher module structure ---"
if rg -q "fn reload_watches" crates/daemon/src/fs_watcher.rs; then
  pass "reload_watches function exists"
else
  fail "reload_watches function missing"
fi

if rg -q "no active filesystem triggers, releasing watcher" crates/daemon/src/fs_watcher.rs; then
  pass "lazy watcher release logic exists"
else
  fail "lazy watcher release logic missing"
fi

# ── Scenario 10: Trigger engine notifies fs_watcher ──────────────────────────
echo ""
echo "--- Scenario 10: Trigger engine notifies fs_watcher ---"
if rg -q "fs_watcher_reload_tx" core/src/trigger_engine.rs; then
  pass "notify_trigger_reload sends to fs_watcher"
else
  fail "fs_watcher notification missing from trigger engine"
fi

# ── Scenario 11: Path safety checks ─────────────────────────────────────────
echo ""
echo "--- Scenario 11: Path safety checks ---"
if rg -q "outside root_path" crates/daemon/src/fs_watcher.rs; then
  pass "root_path fence check"
else
  fail "root_path fence check missing"
fi

if rg -q "skipping .git path" crates/daemon/src/fs_watcher.rs; then
  pass ".git exclusion"
else
  fail ".git exclusion missing"
fi

# ── Scenario 12: Event payload format ────────────────────────────────────────
echo ""
echo "--- Scenario 12: Event payload format ---"
for field in '"path"' '"filename"' '"dir"' '"event_type"' '"timestamp"'; do
  if rg -q "$field" crates/daemon/src/fs_watcher.rs; then
    pass "payload field $field present"
  else
    fail "payload field $field missing"
  fi
done

# ── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [[ $FAIL -gt 0 ]]; then
  exit 1
fi
exit 0
