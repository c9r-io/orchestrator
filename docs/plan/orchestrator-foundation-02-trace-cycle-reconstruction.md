# Orchestrator Foundation Task 02

## Title

Trace 周期重建与异常检测修复

## Goal

修复 `task trace` 中的 cycle 重建问题，消除 `overlapping_cycles` 误报，并补齐 completed task 的完整时间信息。

## Problem

本次自举任务多次稳定复现：

- `overlapping_cycles` 误报
- 第一轮 `ended_at` 缺失
- `summary.wall_time_secs = null`

这说明 trace 构建层无法可靠解释调度器实际行为。

## Scope

- 修复 cycle `started_at` / `ended_at` 边界重建
- 修复 `overlapping_cycles` anomaly 触发条件
- 为 completed task 正确计算 `wall_time_secs`
- 补齐 trace 相关测试覆盖正常两轮执行、跳过步骤、收尾阶段

## Out Of Scope

- 不重写整套事件模型
- 不改造 CLI 展示样式

## Acceptance Criteria

1. 正常两轮任务不再被误报 `overlapping_cycles`。
2. 每个 completed cycle 都有正确的 `ended_at`。
3. completed task 的 `summary.wall_time_secs` 有值。
4. `task trace --json` 与事件序列一致。

## Suggested Verification

- `cargo test --lib scheduler::trace`
- 对已知两轮任务运行 `./scripts/orchestrator.sh task trace --json <task_id>`，人工核对 cycle 边界

