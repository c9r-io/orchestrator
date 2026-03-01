# Orchestrator Foundation Task 05

## Title

低产出步骤检测与长时执行可观测性增强

## Goal

为 `plan` / `implement` 等长时步骤增加“低产出但仍存活”的检测能力，帮助区分正常长时推理与可疑空转。

## Problem

本次自举中，`plan` 多次出现：

- 心跳持续存在
- 进程未退出
- CPU 很低
- 日志长时间几乎不增长

虽然最终完成，但这类状态目前缺少有效解释与告警。

## Scope

- 定义“低产出”判定信号（如 stdout/stderr 增长、心跳时间窗、进程活跃度）
- 在 step heartbeat 或相关观测链路中增加可用指标
- 为长时步骤增加更可解释的状态信号
- 补充测试，验证正常长时任务不会被误判，疑似空转能被识别

## Out Of Scope

- 不做自动 kill / 自动重试策略
- 不更换 agent runner 或模型

## Acceptance Criteria

1. 长时步骤的心跳中能体现有效输出增长或低产出状态。
2. 可以区分“正常长时执行”与“低产出可疑挂起”。
3. 低产出检测不会显著增加误报噪音。

## Suggested Verification

- `cargo test --lib scheduler`
- 使用一个已知长时 `plan` 任务观察心跳输出，确认新增指标可用

