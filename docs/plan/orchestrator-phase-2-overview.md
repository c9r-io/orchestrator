# Orchestrator Phase 2 Overview

## Title

从“基础能力补齐”到“高信任自举底座”的第二阶段细化任务

## Goal

在 Foundation 01-05 完成后，继续提升 orchestrator 的可解释性、可验证性与自举工程可用性，使其从“基础能力齐全”进入“更高信任、低摩擦”的下一阶段。

## Context

第一阶段已经完成：

- Step 语义一致性
- Trace 周期重建
- `task create` 与 QA markdown 解耦
- 运行期控制面稳定性
- 低产出检测

这些修复解决了主要的执行与观测断裂点，但仍有一些更高层次的问题尚未收口：

1. 任务级 / 条目级步骤在观测层仍存在 item 归属歧义。
2. 运行真实 CLI QA 仍依赖临时 workflow / 临时 probe 配置，缺少正式、可复用的验证工装。
3. self-referential workspace 下做内部 probe / 自检类工作流时，安全约束和体验仍偏硬。
4. 历史脏配置不会自动自愈，仍需要人工 re-apply 修正持久化 config 漂移。

## Phase 2 Task Set

第二阶段拆为 4 个独立任务：

1. `orchestrator-phase-2-01-scope-aware-observability.md`
   - 修正 task-scoped / item-scoped 步骤在控制面与 trace 中的 item 归属表达。

2. `orchestrator-phase-2-02-reusable-cli-qa-fixtures.md`
   - 建立正式的、可重复执行的 CLI probe fixtures，减少临时调试式 QA。

3. `orchestrator-phase-2-03-self-referential-probe-safety.md`
   - 优化 self-referential workspace 下内部 probe / 诊断工作流的安全与使用体验。

4. `orchestrator-phase-2-04-config-self-healing.md`
   - 减少持久化 active config 与源 manifest 漂移后的人工修复成本。

## Success Criteria

1. Phase 2 任务不再聚焦“基础功能是否能用”，而是提升“长期可信”和“低摩擦运维”。
2. 所有 Phase 2 任务都能独立实施和验收，不相互阻塞。
3. 完成后，真实 CLI 验证应更接近“固定工装 + 固定场景”，而不是临时拼接探针。
