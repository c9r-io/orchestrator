# 38 — Event Table TTL & Archival

## 概述

为 `events` 表实现基于 TTL 的自动清理与可选 JSONL 归档机制，防止长期运行场景下事件表无限膨胀。

## 设计决策

### 清理策略

- **仅清理终结状态任务的事件**：`completed`、`failed`、`cancelled` 三种状态的 task 的事件在超过保留期限后被清理。`running`、`pending`、`paused` 状态的 task 事件始终保留。
- **分批删除**：每次最多删除 1000 条记录（`LIMIT 1000`），避免长时间持有 SQLite 写锁影响正常写入。
- **基于 `created_at` 时间戳**：利用已有的 `idx_events_task_created_at` 复合索引高效筛选过期事件。

### 配置层级

配置放在 daemon CLI 参数（而非 WorkspaceConfig），因为事件清理是全局守护进程级别的关注点：

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--event-retention-days` | 30 | 事件保留天数，0 表示禁用自动清理 |
| `--event-cleanup-interval-secs` | 3600 | 清理扫描间隔（秒） |
| `--event-archive-enabled` | false | 是否在清理前归档 |
| `--event-archive-dir` | `{data_dir}/archive/events` | 归档目录 |

### 归档格式

- 归档文件路径：`{archive_dir}/{task_id}/{date}.jsonl`
- 每行一条完整事件 JSON 记录
- 归档默认关闭，按需开启

### gRPC API

新增两个 RPC：

- `EventCleanup(EventCleanupRequest) → EventCleanupResponse`：支持 `dry_run` 和 `archive` 选项
- `EventStats(EventStatsRequest) → EventStatsResponse`：返回事件表统计信息

### CLI

- `orchestrator event cleanup --older-than 30 [--dry-run] [--archive]`
- `orchestrator event stats`

## 核心模块

| 文件 | 职责 |
|------|------|
| `core/src/event_cleanup.rs` | 清理/归档/统计核心逻辑 |
| `crates/daemon/src/main.rs` | 后台清理任务 spawn |
| `crates/daemon/src/server/system.rs` | gRPC handler |
| `crates/cli/src/commands/event.rs` | CLI dispatch |
| `proto/orchestrator.proto` | EventCleanup/EventStats 消息定义 |

## 风险缓解

- 误删保护：严格限制仅清理终结任务的事件 + `--dry-run` 预览
- 写锁竞争：分批删除 + 可配置间隔
- 归档膨胀：默认关闭，可配合 logrotate 管理
