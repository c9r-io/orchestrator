#!/usr/bin/env bash
#
# QA test: Filesystem Trigger (FR-085 / QA-132)
# Validates filesystem trigger config types, validation, and code structure.
# Daemon-level integration scenarios (file create → task) require manual testing.

set -euo pipefail

PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); echo "  PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL: $1"; }

echo "=== QA 132: Filesystem Trigger ==="
echo ""

# ── Scenario 1: Compilation and tests ─────────────────────────────────────────
echo "--- Scenario 1: Compilation and tests ---"
if cargo test --workspace >/dev/null 2>&1; then
  pass "cargo test --workspace"
else
  fail "cargo test --workspace"
fi

if cargo clippy --workspace --all-targets -- -D warnings >/dev/null 2>&1; then
  pass "cargo clippy clean"
else
  fail "cargo clippy"
fi

# ── Scenario 6: serde roundtrip ──────────────────────────────────────────────
echo ""
echo "--- Scenario 6: serde roundtrip ---"
if cargo test -p agent-orchestrator -- trigger_yaml_roundtrip_filesystem >/dev/null 2>&1; then
  pass "trigger_yaml_roundtrip_filesystem"
else
  fail "trigger_yaml_roundtrip_filesystem"
fi

# ── Scenario 7: Unit tests for filesystem validation ─────────────────────────
echo ""
echo "--- Scenario 7: Filesystem validation unit tests ---"
for test_name in \
  trigger_validate_accepts_filesystem_source \
  trigger_validate_filesystem_requires_paths \
  trigger_validate_filesystem_requires_block \
  trigger_validate_filesystem_rejects_invalid_events; do
  if cargo test -p agent-orchestrator -- "$test_name" >/dev/null 2>&1; then
    pass "$test_name"
  else
    fail "$test_name"
  fi
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
