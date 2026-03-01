# Orchestrator Phase 2 Task 02

## Title

建立可复用的 CLI probe fixtures，减少临时运行配置与一次性探针

## Goal

把当前依赖临时 apply 的 CLI 验证方式，升级为仓库内正式维护的最小 probe fixtures，使运行期 / trace / 低产出 / 任务创建等回归场景可重复、可审计。

## Problem

目前真实 CLI 验证虽然已经可做，但仍有明显工程摩擦：

- 为验证低产出，需要临时追加 `low-output-probe` / `active-output-probe`
- 为验证 target resolution，需要临时拼最小 config
- 某些 CLI smoke 依赖手工挑选 workflow，容易受当前 active config 污染

这会导致：

- QA 可重复性差
- 历史验证路径难以复盘
- 临时配置容易污染本地环境，增加清理成本

## Scope

- 在仓库中补正式维护的最小 CLI probe manifest bundle
- 为关键控制面 / 低产出 / task create 行为提供固定测试工装
- 更新 QA 文档，使其引用固定 fixture，而不是临时拼配置

## Out Of Scope

- 不引入新的集成测试框架
- 不把这些 CLI QA 全部自动化为 CI E2E
- 不扩展为浏览器/UI 测试

## Acceptance Criteria

1. 低产出检测、运行期控制面、task create target resolution 都能基于固定 fixtures 执行。
2. QA 文档不再依赖“临时 apply 一个自定义 workflow”这类一次性步骤。
3. 运行真实 CLI 回归时，环境污染和人工清理步骤明显减少。

## Suggested Verification

- 新 fixture bundle 能通过：
  - `./scripts/orchestrator.sh apply -f <fixture-bundle>`
- 相关 QA 文档能直接引用该 bundle 跑通：
  - `docs/qa/orchestrator/02-cli-task-lifecycle.md`
  - `docs/qa/orchestrator/32-task-trace.md`
