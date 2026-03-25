# FR-079: 数据生命周期治理 — 日志清理、DB 瘦身与自动化回收

## 优先级: P1

## 状态: Proposed

## 背景

Orchestrator 在 `~/.orchestratord/` 下持久化运行时数据（SQLite DB、stdout/stderr 日志、event 归档）。当前仅有 event TTL 自动清理机制，其余数据会无限增长，长期运行后可能占用数 GB 至数十 GB 用户空间。

已有机制：
- `--event-retention-days 30` + 每小时自动清理 sweep
- `--event-archive-enabled` 归档到 JSONL
- `orchestrator event cleanup` 手动触发
- `orchestrator task delete --all --status completed` 手动清理已完成任务

缺失的能力：
1. 日志文件无 TTL/轮转，随 task 执行无限增长
2. SQLite 删除记录后不回收磁盘空间（需 VACUUM）
3. 已终结的 task（completed/failed/cancelled）没有自动清理策略
4. 没有全局磁盘空间告警或水位线保护
5. 用户没有 `orchestrator db vacuum` 或 `orchestrator db size` 等自查手段

## 需求

### 1. 日志文件 TTL 清理

- Daemon 后台定期扫描 `~/.orchestratord/logs/` 下的 stdout/stderr 日志文件
- 清理已终结 task（completed/failed/cancelled）且创建超过 N 天的日志文件
- 可配置：`--log-retention-days <DAYS>`（默认 30，0=禁用）
- CLI 手动触发：`orchestrator db cleanup --logs`

### 2. SQLite VACUUM

- 新增 `orchestrator db vacuum` 命令，执行 `VACUUM` 回收磁盘空间
- Daemon 后台可选自动 VACUUM：`--auto-vacuum-interval-hours <HOURS>`（默认 0=禁用）
- 执行前显示当前 DB 文件大小，执行后显示回收量

### 3. 已终结 Task 自动清理

- Daemon 后台定期清理已终结 task 及其关联数据（items、runs、events、logs）
- 可配置：`--task-retention-days <DAYS>`（默认 0=禁用，建议 90）
- 清理范围：task + task_items + command_runs + events + 对应的日志文件
- 级联删除，确保无孤立数据

### 4. 磁盘空间可观测性

- `orchestrator db status` 增强：显示 DB 文件大小、日志目录大小、event 归档大小
- 可选：当数据目录超过阈值（如 1GB）时，在 `orchestrator daemon status` 中发出警告

## 验收标准

- [ ] `orchestrator db vacuum` 执行 VACUUM 并显示回收量
- [ ] `orchestrator db status` 显示 DB 大小、日志大小、归档大小
- [ ] `orchestrator db cleanup --logs` 清理过期日志文件
- [ ] `--log-retention-days` daemon flag 启用自动日志清理
- [ ] `--task-retention-days` daemon flag 启用自动 task 清理
- [ ] 级联删除：task 删除时清理关联的 items、runs、events、日志文件
- [ ] 长期运行场景下数据目录大小可控（不无限增长）

## 风险

- VACUUM 会临时占用与 DB 等量的磁盘空间（SQLite 创建临时副本）
- 自动清理已终结 task 可能影响用户事后审计需求 — 默认禁用，需用户显式启用
- 日志文件删除需确认对应 task 确实已终结，避免删除正在运行的 task 的日志
