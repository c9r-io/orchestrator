# Design Doc 80: evo_apply_winner 可观测性增强

## 来源

FR-070 — 首次 self-evolution 实测中发现的四项可观测性缺口。

## 问题

1. `item_select` 失败（如 `score` 变量未 capture）时，仅 `warn!` 日志，无事件记录，监控者无法在事件流中发现选择失败。
2. `evo_apply_winner` 合并 worktree 时无日志说明 diff 规模（变更文件数、增删行数）。
3. Capture 提取失败（JSON path 无法 resolve）时，变量静默变为空字符串，`step_finished` 事件不包含任何缺失信息。
4. `item_selected` 事件不包含各候选的评分，难以判断选择依据。

## 设计决策

### R1: `item_select_failed` 事件

在 `segment.rs` 的 `Err(e)` 分支（原仅有 `warn!`），新增 `item_select_failed` 事件：

```json
{
  "error": "item_select: no items have parseable metric_var 'score'",
  "metric_var": "score",
  "item_count": 2,
  "item_vars": {
    "item-a": ["build_exit_code", "error_count"],
    "item-b": ["build_exit_code", "error_count"]
  }
}
```

`item_vars` 列出每个候选实际拥有的 pipeline var key，便于排查哪些变量被成功 capture。

错误分支仍调用 `apply_winner_if_needed`（回退逻辑），不改变控制流。

### R2: worktree 合并 diff 统计

`isolation.rs` 的 `apply_winner_if_needed` 在 `git merge --ff-only` 成功后，运行 `git diff --numstat HEAD~1..HEAD` 提取变更统计，写入 `info!` 日志和 `item_isolation_winner_applied` 事件的 payload：

```json
{
  "strategy": "git_worktree",
  "branch": "orchestrator-item/task-1/item-winner",
  "files_changed": 3,
  "insertions": 145,
  "deletions": 28
}
```

新增 `parse_numstat()` 辅助函数解析 `--numstat` 输出（每行 `<added>\t<removed>\t<file>`）。

### R3: Capture 失败追踪

`apply_captures()` 返回值从 `()` 改为 `Vec<String>`（缺失变量名列表）。调用方在构建 `step_finished` 事件时附加 `captures_missing` 字段（仅非空时添加，向后兼容）：

```json
{
  "step": "evo_benchmark",
  "captures_missing": ["score"]
}
```

### R4: `item_selected` 事件增强

成功路径的 `item_selected` 事件新增 `selection_succeeded: true` 和 `scores` 字段：

```json
{
  "winner": "item-b",
  "eliminated": ["item-a"],
  "selection_succeeded": true,
  "scores": {"item-a": 72.0, "item-b": 85.0}
}
```

`scores` 从各候选的 pipeline_vars 中提取 `metric_var` 对应值。无 metric_var 或无法解析的候选不出现在 `scores` 中。

## 约束

- 不改变现有 pipeline 控制流
- 所有新增字段为 append-only，不修改已有字段
- 事件通过现有 `insert_event` 基础设施持久化

## 变更文件

| 文件 | 变更 |
|------|------|
| `crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs` | R1: `item_select_failed` 事件；R4: `item_selected` 增强 |
| `crates/orchestrator-scheduler/src/scheduler/loop_engine/isolation.rs` | R2: diff 统计日志 + 事件增强 + `parse_numstat` 辅助 |
| `crates/orchestrator-scheduler/src/scheduler/item_executor/accumulator.rs` | R3: `apply_captures` 返回缺失变量列表 |
| `crates/orchestrator-scheduler/src/scheduler/item_executor/apply.rs` | R3: `captures_missing` 注入 `step_finished` payload |
