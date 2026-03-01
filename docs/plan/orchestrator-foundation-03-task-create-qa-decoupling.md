# Orchestrator Foundation Task 03

## Title

Task Create 与 QA Markdown 前置依赖解耦

## Goal

让纯重构 / 纯内核任务在未启用 QA 阶段时可以正常创建和运行，不再被 QA markdown 硬性前置条件阻塞。

## Problem

本次任务中，即使显式禁用了 `qa_testing` / `ticket_fix`，`task create` 仍因为“没有 QA markdown 文件”而失败，说明任务创建逻辑与 QA 文档存在不必要的硬耦合。

## Scope

- 梳理 `task create` 当前对 QA 目标的前置校验逻辑
- 将 QA markdown 要求改为由 workflow 实际启用的阶段决定
- 确保纯 refactor 任务可用显式 target 创建
- 增加相应的单元测试 / 集成测试

## Out Of Scope

- 不调整 QA 文档本身的格式
- 不改动 `qa-testing` 技能逻辑

## Acceptance Criteria

1. 当 workflow 未启用 `qa_testing` / `ticket_fix` 时，`task create` 不强制要求 QA markdown。
2. 当 workflow 启用相关 QA 阶段时，原有校验仍生效。
3. 显式 target 的纯重构任务可被正常创建并启动。

## Suggested Verification

- `cargo test --lib task_ops`
- 使用一个禁用 QA 阶段的 workflow 执行 `task create`，确认成功
- 使用启用 QA 阶段的 workflow 执行 `task create`，确认原有约束仍生效

