---
self_referential_safe: false
self_referential_safe_scenarios: [S1, S2]
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
1. 记录当前 `event stats` 行数
2. `orchestrator event cleanup --older-than 1`
3. 再次查看 `event stats`

**期望**：
- 仅删除 completed/failed/cancelled task 的超期事件
- running/pending 状态 task 的事件数量不变
- 返回实际删除数量

> **注意**：`--older-than 0` 会被 daemon 视为未指定，自动回退到默认值 30 天。
> 这是 protobuf `uint32` 默认值（0）的安全防护机制，CLI 默认值已设置为 30。
> 若数据库中无超过指定天数的事件，清理结果为 0 属正常行为。

## 场景 4：event cleanup --archive 归档

**步骤**：
1. `orchestrator event cleanup --older-than 1 --archive`
2. 检查 `{data_dir}/archive/events/` 目录

**期望**：
- 对应 task_id 子目录下生成 `{date}.jsonl` 文件
- 每行为有效 JSON 记录
- 被归档的事件已从数据库中删除

## 场景 5：daemon 自动清理

**步骤**：
1. 启动 daemon：`orchestratord --event-retention-days 1 --event-cleanup-interval-secs 10`
2. 等待 > 10 秒

**期望**：
- daemon 日志中出现 `event cleanup: deleted old events` 或无事件时无输出
- 清理过程不影响并发的 task 创建和执行

> **注意**：此场景需要重启 daemon 使自定义参数生效。若当前 daemon 受安全约束保护
> 无法重启，应跳过此场景。可通过单元测试 `cleanup_deletes_only_terminal_old_events`
> 和 `cleanup_respects_batch_limit` 间接验证清理逻辑正确性。

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
| 1 | All scenarios verified | ☑ | S1/S2 executed 2026-03-18. S3-S5 skipped (self-referential unsafe per frontmatter) |
