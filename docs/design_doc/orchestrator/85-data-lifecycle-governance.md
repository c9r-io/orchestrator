# Design Doc 85: Data Lifecycle Governance

## FR Reference

FR-079: 数据生命周期治理 — 日志清理、DB 瘦身与自动化回收

## Design Decisions

### Log File TTL (Default Enabled)

Log files for terminated tasks are automatically cleaned up after 30 days (`--log-retention-days 30`, default). Piggybacks on the existing event cleanup sweep timer (`--event-cleanup-interval-secs 3600`) — no additional timer.

Logic: query terminal tasks older than N days → walk `{logs_dir}/{task_id}/` → delete files and empty directories.

### Task Auto-Cleanup (Default Disabled)

Terminated tasks and all associated data (items, runs, events, log files) are auto-deleted after N days. Default 0 (disabled) to avoid surprising users who need post-mortem access. Enable with `--task-retention-days 90`.

Reuses existing `delete_task_and_collect_log_paths()` cascade delete.

### DB VACUUM

New `orchestrator db vacuum` command executes SQLite `VACUUM` to reclaim disk space. Reports size before/after and bytes freed.

Note: VACUUM temporarily requires ~2x DB size in free disk space.

### DB Status Enhanced

`orchestrator db status` now shows data directory sizes:
- DB file size (including WAL + SHM)
- Logs directory size
- Archive directory size

### Manual Log Cleanup

`orchestrator db cleanup --older-than 30` for on-demand log cleanup without waiting for the background sweep.

## Files Created

| File | Purpose |
|------|---------|
| `core/src/log_cleanup.rs` | Log file TTL cleanup |
| `core/src/task_cleanup.rs` | Task auto-cleanup with cascade delete |
| `core/src/db_maintenance.rs` | VACUUM and size reporting |

## Files Modified

| File | Change |
|------|--------|
| `crates/proto/orchestrator.proto` | Added DbVacuum, DbLogCleanup RPCs; extended DbStatusResponse with size fields |
| `crates/daemon/src/main.rs` | Added `--log-retention-days`, `--task-retention-days`; data lifecycle sweep |
| `crates/daemon/src/server/system.rs` | Added vacuum + log cleanup handlers |
| `crates/daemon/src/server/mod.rs` | Wired new RPCs |
| `crates/cli/src/cli.rs` | Added Vacuum + Cleanup to DbCommands |
| `crates/cli/src/commands/db.rs` | Added handlers + size display in status |
| `core/src/service/system.rs` | Enhanced db_status with size info |
| `core/src/lib.rs` | Registered new modules |
