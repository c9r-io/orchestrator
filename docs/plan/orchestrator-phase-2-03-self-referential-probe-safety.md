# Orchestrator Phase 2 Task 03

## Title

优化 self-referential workspace 下 probe / 诊断工作流的安全与体验

## Goal

降低在 self-referential workspace 中执行内部 probe、诊断、长时观察类 workflow 的摩擦，同时保留当前安全边界。

## Problem

当前在 self-referential workspace 中运行内部 probe 存在几个体验问题：

- workflow 没有显式安全策略就会被直接拦住
- 临时 probe workflow 容易因为没有 `self_test` / `auto_rollback` 触发额外 warning
- 为了做低产出验证，不得不借用现有 phase（例如 `build`），从而触发现有输出校验，导致任务状态变成 `failed`

这些都不是核心执行错误，但会让“做诊断”本身变得笨重。

## Scope

- 为 self-referential workspace 下的 probe / 诊断类 workflow 定义更清晰的安全约束与推荐形态
- 降低内部诊断 workflow 的无关 warning 噪音
- 避免为了探针复用现有 phase 验证逻辑而引入伪失败

## Out Of Scope

- 不放宽 self-referential 的核心安全要求
- 不禁用 checkpoint 保护
- 不取消 self-test / rollback 相关能力

## Acceptance Criteria

1. self-referential workspace 下存在一个清晰、正式支持的 probe workflow 形态。
2. 诊断类 workflow 不再需要借用业务 phase 才能运行。
3. 做内部观测验证时，warning/失败更聚焦真实问题，而不是探针副作用。

## Suggested Verification

- 使用一个 self-referential workspace 中的 probe workflow：
  - 能成功创建并执行
  - 不因无关输出校验而变成 `failed`
- 对低产出 / 运行期控制面 / trace 的内部验证可直接复用该 probe 形态
