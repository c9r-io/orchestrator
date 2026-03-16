# QA 108: Incremental Item Progress (FR-054)

## 概述

验证 item 进度在执行过程中实时递增，而非等待所有 segment 结束后批量写入。

## 前置条件

- orchestrator daemon 运行中
- 已配置包含多个 item 的 workflow（如 full-qa）

## 场景

### 场景 1: Progress 实时递增

1. 创建包含多个 item 的任务（≥5 个 QA 文件）
2. 启动任务执行
3. 每 30 秒检查 `orchestrator task info`
4. **预期**: 第一批 item 完成后，`Progress: X/N` 应 > 0
5. **预期**: Progress 数字应随执行推进而递增

### 场景 2: Step 级进度展示（Table 格式）

1. 任务执行中运行 `orchestrator task info`
2. **预期**: Progress 行下方显示各 step 的完成/运行统计：
   ```
   Progress: 3/10 items
       qa_testing:          5 completed, 2 running
   ```
3. 所有 item 完成后统计数应与 total_items 一致

> **Note**: Step progress counts **CommandRuns**, not items. "completed" means the run's command has exited (exit_code is set), regardless of whether the item has been finalized to a terminal status. During execution, step progress "completed" count can be higher than the `Progress: X/N` item count because:
> - Runs complete before incremental finalize writes the item's terminal status
> - Items with retries have multiple runs (each with an exit_code)
>
> This is expected behavior. Only compare step progress to item progress after the task reaches a terminal state.

### 场景 3: Step 级进度展示（JSON 格式）

1. 任务执行中运行 `orchestrator task info -o json`
2. **预期**: 输出包含 `step_progress` 数组：
   ```json
   "step_progress": [
     {"phase": "qa_testing", "completed": 5, "running": 2}
   ]
   ```

### 场景 4: 失败 item 正确计入

1. 创建含有会导致 agent 失败的 item 的任务
2. 等待任务完成
3. **预期**: 任务最终状态正确反映失败（如 `failed`）
4. **预期**: Progress 数字包含已完成（含失败）的 item

### 场景 5: 批量 finalize 兜底

1. 即使增量 finalize 已写入终态，批量 `finalize_items` 仍应正常执行（幂等）
2. **预期**: 不应出现 duplicate key 错误或状态不一致

## Checklist

| # | Check | Status |
|---|-------|--------|
| 1 | All scenarios verified against implementation | ☑ |

## 关联

- 设计文档: `docs/design_doc/orchestrator/66-incremental-item-progress.md`
- 修改文件: `segment.rs`, `task_detail.rs`, `value.rs`
