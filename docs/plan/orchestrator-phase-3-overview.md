# Orchestrator Phase 3 Overview

## Title

从“高信任自举底座”到“可运营、自解释、自回归”的第三阶段细化任务

## Goal

在 Phase 2 已完成的基础上，把 orchestrator 从“核心行为可信”继续推进到“长期运行更容易观测、回归、审计和恢复”的状态。

## Context

Phase 2 已经完成并收口：

- scope-aware observability
- reusable CLI probe fixtures
- self-referential probe safety
- active config self-healing

这些工作解决了执行语义、控制面稳定性、正式 probe 工装，以及持久化配置历史漂移的核心问题。下一阶段不再优先补“功能缺口”，而是收拢长期运维中的剩余摩擦：

1. CLI 回归目前仍主要依赖人工按文档执行，缺少统一的可重复回归入口。
2. 运行期异常（尤其 low-output）已经能被观察到，但还缺少更明确的升级信号与事后聚合。
3. active config 自愈已能在单进程启动时生效，但诊断信息仍偏瞬时，缺少更强的历史可审计性。
4. 历史事件与新观测语义之间仍存在“只能降级显示”的兼容层，缺少更系统的旧数据治理策略。

## Phase 3 Task Set

第三阶段拆为 4 个独立任务：

1. `orchestrator-phase-3-01-cli-regression-runner.md`
   - 把现有 CLI probe fixtures 和 QA 文档场景收敛为统一的可重复回归入口。

2. `orchestrator-phase-3-02-runtime-anomaly-escalation.md`
   - 在 low-output / transient read / long-running 等运行期信号之上，建立更一致的升级表达与聚合输出。

3. `orchestrator-phase-3-03-config-heal-auditability.md`
   - 让 active config 自愈从“当前进程 notice”提升到更可追溯、可审计、可解释的状态。

4. `orchestrator-phase-3-04-legacy-observability-backfill.md`
   - 为历史事件与旧任务提供更系统的观测兼容与治理策略，减少长期 `unknown` 降级。

## Success Criteria

1. Phase 3 的目标从“能不能用”切换到“长期运维是否省心、可解释、可回归”。
2. 新任务以运维治理、回归执行、历史兼容为主，不重新打开已经完成的基础能力问题。
3. 完成后，orchestrator 的日常验证与故障排查应更少依赖人工经验和一次性操作。
