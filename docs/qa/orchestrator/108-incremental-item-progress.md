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

> **Note (2026-03-19)**: 场景需要专用 daemon 运行环境。当 full-QA 占用 daemon 全部 worker 时，
> 新创建的测试任务会停留在 pending 状态。这是基础设施并发限制，非功能 bug。
> 功能已通过 full-QA 任务本身验证（Progress: 75/139 递增、step_progress 正确）。

## 验证记录 (2026-03-21)

| Scenario | Verification | Task ID | Result |
|----------|--------------|---------|--------|
| S1: Progress 实时递增 | `orchestrator task info 69341fbe` 多次检查，进度从 17→18→19 递增 | 69341fbe | ✅ |
| S2: Step 级进度 Table | `Progress: 18/141 items` 下方显示 `qa_testing: 19 completed` | 69341fbe | ✅ |
| S3: Step 级进度 JSON | JSON 包含 `step_progress: [{'completed': 20, 'phase': 'qa_testing', 'running': 0}]` | 69341fbe | ✅ |
| S4: 失败 item 正确计入 | `Status: failed`, `Progress: 2/130 items`, `Failed: 1` | 757ac3d5 | ✅ |
| S5: 批量 finalize 幂等 | 增量 finalize 已写入终态的事件，未观察到 duplicate key 错误 | 46c76c29 | ✅ |

## 验证记录 (2026-03-29)

| Scenario | Verification | Task ID | Result |
|----------|--------------|---------|--------|
| S1: Progress 实时递增 | Event log timestamps for task `8a605cb6` show incremental `item_finalize_evaluated` per batch (waves of 2 items at T+0ms, +15ms, +30ms, +45ms, +60ms from task start). Echo agent completes too fast (<100ms) to observe mid-execution progress via `orchestrator task info`. Incremental pattern confirmed by event timestamps. | 8a605cb6 | ✅ |
| S2: Step 级进度 Table | `Progress: 10/10 items` 下显示 `qa_testing: 10 completed` — matches expected format. | 8a605cb6 | ✅ |
| S3: Step 级进度 JSON | `step_progress: [{"phase": "qa_testing", "completed": 10, "running": 0}]` — correct JSON structure. | 8a605cb6 | ✅ |
| S4: 失败 item 正确计入 | Task `91f40162` with fail agent: `Status: failed`, `Progress: 10/10 items`, `Failed: 10`. All 10 items counted in progress including failures. | 91f40162 | ✅ |
| S5: 批量 finalize 幂等 | DB event log shows no `UNIQUE constraint` errors or `duplicate key` errors. Incremental `item_finalize_evaluated` events (item status: pending → qa_passed) coexist with batch finalize pass without冲突. Daemon log grep confirms no finalize-related errors. | 8a605cb6, 91f40162 | ✅ |

## 关联

- 设计文档: `docs/design_doc/orchestrator/66-incremental-item-progress.md`
- 修改文件: `segment.rs`, `task_detail.rs`, `value.rs`
