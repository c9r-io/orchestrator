# Orchestrator Phase 3 Task 01

## Title

建立统一的 CLI 回归执行入口，收敛当前“按 QA 文档手工敲命令”的验证流程

## Goal

让现有的 CLI probe fixtures 和关键 QA 场景拥有一个统一、可重复、可脚本化的执行入口，降低人工按文档逐条执行的成本。

## Problem

虽然 Phase 2 已经把 CLI probe fixtures 固定下来了，但当前回归仍主要依赖：

- 人工阅读 QA 文档
- 手工选择 workspace / workflow
- 手工运行 `task create` / `task watch` / `task trace`

这带来几个问题：

- 回归速度慢
- 执行者容易漏步骤
- 结果难以标准化记录

## Scope

- 为现有 probe fixtures 提供统一的脚本化回归入口
- 支持按场景分组执行（task create / runtime control / trace / low-output）
- 让 QA 文档优先引用统一入口，而不是直接展开所有命令细节

## Out Of Scope

- 不引入新的 CI 平台
- 不重写为浏览器或 UI 自动化
- 不替代已有单元测试

## Acceptance Criteria

1. 至少存在一个官方、固定的 CLI regression runner 入口。
2. 现有关键 QA 场景可按“场景名”或“场景组”执行，而不是只能手工逐条跑命令。
3. QA 文档可以缩减为“调用哪个 runner + 如何验结果”，而不是重复大量手工步骤。

## Suggested Verification

- 运行统一回归入口，覆盖：
  - task create target resolution
  - runtime control
  - trace
  - low-output
- 验证输出能明确区分通过/失败场景
