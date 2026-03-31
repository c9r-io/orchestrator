---
self_referential_safe: false
self_referential_safe_scenarios: [S1, S2, S3, S4, S8]
---

# QA 127: Data Lifecycle Governance

## FR Reference

FR-079

## Prerequisites

- CLI binary must be built from current source: `cargo build --release --bin orchestrator`
- Features added in commit `e917583` (2026-03-25); binaries built before this date lack `db vacuum` and `db cleanup` subcommands.

## Verification Scenarios

### Scenario 1: db status shows sizes

**Steps:**
1. `orchestrator db status`

**Expected:** Output includes DB Size, Logs Size, Archive Size in human-readable format (B/KB/MB/GB).

### Scenario 2: db status JSON includes size fields

**Steps:**
1. `orchestrator db status -o json`

**Expected:** JSON includes `db_size_bytes`, `logs_size_bytes`, `archive_size_bytes` fields.

### Scenario 3: db vacuum

**Steps:**
1. `orchestrator db vacuum`

**Expected:** Output shows size before, size after, and freed space.

### Scenario 4: db cleanup

**Steps:**
1. `orchestrator db cleanup --older-than 30`

**Expected:** Output shows number of files deleted and bytes freed.

### Scenario 5: daemon auto log cleanup

**Steps:**
1. Start daemon with `--log-retention-days 30` (default)
2. Wait for cleanup sweep (default 3600s, or set `--event-cleanup-interval-secs 10` for testing)

**Expected:** Log files for terminal tasks older than 30 days are deleted.

### Scenario 6: daemon auto task cleanup

**Steps:**
1. Start daemon with `--task-retention-days 1`
2. Create and complete a task
3. Wait for cleanup sweep

**Expected:** Task and associated data (items, runs, events, logs) are deleted after retention period.

### Scenario 7: log-retention-days=0 disables cleanup

**Steps:**
1. Start daemon with `--log-retention-days 0`

**Expected:** No log files are automatically deleted.

### Scenario 8: compilation and tests

**Steps:**
1. `cargo test --workspace`

**Expected:** All tests pass.

## Checklist

- [x] S1: db status shows sizes — **PASS** (DB Size, Logs Size, Archive Size in human-readable format)
- [x] S2: db status JSON includes size fields — **PASS** (db_size_bytes, logs_size_bytes, archive_size_bytes present)
- [x] S3: db vacuum — **PASS** (shows size before, size after, freed space)
- [x] S4: db cleanup — **PASS** (shows files deleted and bytes freed)
- [ ] S5: daemon auto log cleanup — requires isolated daemon with custom `--log-retention-days` flag
- [ ] S6: daemon auto task cleanup — requires isolated daemon with custom `--task-retention-days` flag
- [ ] S7: log-retention-days=0 disables cleanup — requires isolated daemon with custom flags
- [x] S8: compilation and tests — **FAIL** (doctest: pre-existing rlib path issue)
