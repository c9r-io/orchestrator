# FR-088: QA Doctor CLI — 可观测性指标暴露

## 优先级

P2

## 状态

Proposed

## 背景

`task_execution_metrics` 表已在数据库中存在并持续写入（由 scheduler 终端路径在每个 task 完成时插入）。然而目前没有 CLI 命令可以查询和展示这些指标，导致 QA-21 Scenario 5 无法验证。

## 需求

实现 `orchestrator qa doctor` 子命令，暴露以下可观测性指标：

1. `observability.task_execution_metrics_total` — `task_execution_metrics` 表总记录数
2. `observability.task_execution_metrics_last_24h` — 最近 24 小时的记录数
3. `observability.task_completion_rate` — 任务完成率（completed / total）

### 输出格式

- `-o json`：结构化 JSON，字段位于 `observability` 键下
- 默认：表格格式，每行显示指标名称和值

### CLI 结构

```
orchestrator qa doctor [-o json|table]
```

需要：
1. 新增 `orchestrator qa` 父子命令
2. 新增 `orchestrator qa doctor` 子命令
3. 查询 `task_execution_metrics` 表
4. 支持 JSON 和表格输出格式

## 验收标准

（源自 QA-21 Scenario 5 复现步骤）

1. `orchestrator qa doctor -o json` 输出包含上述三个指标字段
2. `orchestrator qa doctor` 表格输出包含对应行
3. 当 `task_execution_metrics` 表为空时，指标返回 0 而非错误

## 关联

- QA-21 Scenario 5（blocked，等待本 FR 实现）
- `task_execution_metrics` 表 schema：`core/src/persistence/migration_steps.rs`
- 数据写入路径：scheduler terminal path (`db.rs`)
