---
self_referential_safe: true
---

# QA-77 — Event Table TTL & Archival

## 前提条件

- Daemon 已启动
- 数据库中存在 completed/failed/cancelled 状态的 task 及对应事件

## 场景 1：event stats 显示统计信息

**步骤**：
1. `orchestrator event stats`

**期望**：
- 显示 Total events 行数
- 显示 Earliest / Latest 时间戳
- 显示按 task status 分组的计数

## 场景 2：event cleanup --dry-run 预览

**步骤**：
1. `orchestrator event cleanup --dry-run --older-than 1`

**期望**：
- 输出包含待清理数量
- 实际事件数据未被删除（可通过 `event stats` 验证行数不变）

## 场景 3：event cleanup 实际清理

**步骤**：
1. Code review 确认单元测试存在于 `core/src/event_cleanup.rs`：
   - `cleanup_deletes_only_terminal_old_events`
   - `cleanup_respects_batch_limit`
   - `count_pending_cleanup_returns_correct_count`
2. 运行清理逻辑单元测试（safe: 使用隔离 temp-db）：
   ```bash
   cargo test --lib -p agent-orchestrator -- event_cleanup::tests::cleanup_deletes_only_terminal_old_events
   cargo test --lib -p agent-orchestrator -- event_cleanup::tests::cleanup_respects_batch_limit
   cargo test --lib -p agent-orchestrator -- event_cleanup::tests::count_pending_cleanup_returns_correct_count
   ```

**期望**：
- 仅删除 completed/failed/cancelled task 的超期事件（单元测试验证）
- running/pending 状态 task 的事件数量不变（单元测试验证）
- batch limit 受限清理（单元测试验证）

> **注意**：`--older-than 0` 会被 daemon 视为未指定，自动回退到默认值 30 天。
> 这是 protobuf `uint32` 默认值（0）的安全防护机制，CLI 默认值已设置为 30。

## 场景 4：event cleanup --archive 归档

**步骤**：
1. Code review 确认单元测试存在于 `core/src/event_cleanup.rs`：
   - `archive_events_writes_jsonl_and_deletes`
2. 运行归档逻辑单元测试（safe: 使用隔离 temp-db + temp 目录）：
   ```bash
   cargo test --lib -p agent-orchestrator -- event_cleanup::tests::archive_events_writes_jsonl_and_deletes
   ```

**期望**：
- JSONL 文件正确生成（单元测试验证）
- 被归档的事件已从数据库中删除（单元测试验证）

## 场景 5：daemon 自动清理（单元测试间接验证）

**步骤**：
1. Code review — 确认 daemon 启动时注册了自动清理定时器：
   ```bash
   rg -n "event_cleanup|cleanup_interval|retention_days" crates/daemon/src/server.rs crates/daemon/src/main.rs | head -10
   ```
2. 运行清理逻辑单元测试（与场景 3 相同，覆盖自动清理的核心逻辑）：
   ```bash
   cargo test --lib -p agent-orchestrator -- event_cleanup::tests::cleanup_deletes_only_terminal_old_events event_cleanup::tests::cleanup_respects_batch_limit 2>&1 | tail -5
   ```

**期望**：
- daemon 自动清理的核心逻辑由 `cleanup_deletes_only_terminal_old_events` 和 `cleanup_respects_batch_limit` 单元测试覆盖
- 定时器注册逻辑通过 code review 确认存在

## 单元测试覆盖

| 测试 | 文件 |
|------|------|
| `cleanup_deletes_only_terminal_old_events` | `core/src/event_cleanup.rs` |
| `cleanup_respects_batch_limit` | `core/src/event_cleanup.rs` |
| `count_pending_cleanup_returns_correct_count` | `core/src/event_cleanup.rs` |
| `event_stats_returns_expected_values` | `core/src/event_cleanup.rs` |
| `archive_events_writes_jsonl_and_deletes` | `core/src/event_cleanup.rs` |
| `event_cleanup_subcommand_parses` | `crates/cli/src/cli.rs` |
| `event_stats_subcommand_parses` | `crates/cli/src/cli.rs` |

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S1-S5 PASS (2026-03-28); S1/S2 verified live; S3/S4/S5 verified via unit tests. S5 code review confirms daemon timer registration at main.rs:393-416. |
