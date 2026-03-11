# FR-004 - DAG / 动态编排主路径化与可观测化

**Module**: orchestrator  
**Status**: In Progress  
**Priority**: P1  
**Created**: 2026-03-09  
**Last Updated**: 2026-03-11  
**Source**: 深度项目评估报告最高优先级改进建议 #4

## Background

项目已经具备较强的动态编排设计资产，包括：

- 动态 step pool
- prehook 扩展决策
- DAG 数据结构
- adaptive planner

但当前主执行路径仍然以 cycle + scope segment 为中心。DAG 与动态编排能力更像“已建模块”和“设计方向”，尚未成为普遍可用、可观测、可调试的主路径执行模型。

## Problem Statement

当前动态编排能力存在以下问题：

- 用户容易将现有能力理解为“完整 DAG 调度器”
- DAG/动态 step 与主循环执行模型之间的衔接不够清晰
- trigger 与 prehook 存在能力分裂，部分逻辑仍依赖简化条件匹配
- 缺乏针对动态计划的 trace、snapshot、失败定位视图

这会导致功能感知强于实际落地程度，影响平台可解释性与可维护性。

## Goals

- 让 DAG / 动态编排成为明确、可使用、可追踪的执行能力
- 统一 prehook、dynamic step trigger、adaptive plan 的决策语义
- 为动态计划提供一等公民级观测与调试接口
- 保持现有 segment 模型的兼容性

## Non-goals

- 一次性废弃现有 segment-based loop engine
- 第一阶段实现跨 task 的全局 DAG 调度

## Scope

- In scope:
  - DAG execution plan 与现有 execution plan 的桥接
  - 动态 step 注入的主路径执行支持
  - adaptive planner 输出的运行态可观测性
  - trace / info / debug 输出增强
- Out of scope:
  - 分布式图执行
  - 图形化 UI

## Proposed Design

## Implementation Status

### Implemented on 2026-03-11

- 新增显式 workflow execution mode：
  - `static_segment`
  - `dynamic_dag`
- `dynamic_dag` 已进入主执行路径，主循环会在 segment engine 与 graph engine 间按 mode 分派
- 静态 workflow steps 可物化为 runtime execution graph，并作为 deterministic DAG fallback 使用
- adaptive planner 输出可转换为统一 graph model 并进入 graph executor
- `DynamicStepConfig.trigger` 与 DAG edge condition 已统一到 CEL 求值路径，不再依赖简化字符串匹配
- 新增 graph-aware 事件：
  - `dynamic_plan_generated`
  - `dynamic_plan_validated`
  - `dynamic_plan_materialized`
  - `dynamic_node_ready`
  - `dynamic_node_started`
  - `dynamic_node_finished`
  - `dynamic_node_skipped`
  - `dynamic_edge_evaluated`
  - `dynamic_edge_taken`
  - `dynamic_steps_injected`
- `task trace` 已支持 `graph_runs` 视角，用于展示动态图执行事件
- QA 文档已补充：
  - `docs/qa/orchestrator/59-dynamic-dag-mainline-execution.md`
  - 同步更新 `docs/qa/orchestrator/13-dynamic-orchestration.md`
  - 同步更新 `docs/qa/orchestrator/32-task-trace.md`

### Remaining Gaps Before Closure

- `task_info` 尚未返回当前 effective execution graph
- graph snapshot 还未独立持久化到 task 级数据结构，仅通过 events/trace 间接可见
- 尚未持久化以下调试材料：
  - 原始 planner 输出
  - 规范化 DAG JSON
  - 节点执行顺序快照
  - 条件命中原因
- `persist_graph_snapshots` 配置已存在，但尚未形成真正的落库/回放能力
- `config debug` 尚未增加 DAG mode 生效配置与 graph materialization 详情输出
- 当前 graph engine 为 deterministic sequential v1，尚未覆盖更完整的 branch / transform / replay 调试面

### 1. 执行模型分层

定义两类明确模式：

- `static_segment_mode`: 当前默认模式
- `dynamic_dag_mode`: 显式启用的 DAG 模式

二者共享 task lifecycle、DB、event、runner，但拥有不同的 plan materialization 阶段。

### 2. 决策语义统一

统一以下入口：

- `StepPrehookConfig`
- `DynamicStepConfig.trigger`
- `AdaptivePlanner` 输出计划中的条件边

目标是尽量收敛到同一套 CEL 语义和上下文变量，而不是继续保留字符串匹配过渡实现。

### 3. 动态计划可观测性

新增：

- `task_trace` 中展示 dynamic plan snapshot
- `task_info` 可返回当前 effective execution graph
- event:
  - `dynamic_plan_generated`
  - `dynamic_plan_validated`
  - `dynamic_node_started`
  - `dynamic_node_finished`
  - `dynamic_edge_taken`

### 4. 调试能力

为每个 task 保留：

- 原始 planner 输出
- 规范化 DAG JSON
- 节点执行顺序
- 条件命中原因

说明：

- `task_trace` 与 `dynamic_*` events 已部分实现
- `task_info` effective graph 与独立调试快照持久化尚未完成

## Alternatives And Tradeoffs

- **继续以文档说明“未来支持 DAG”**: 风险最低，但平台表述和实现长期错位
- **直接替换主调度器**: 风险过高，容易引入回归
- **双模式共存并逐步迁移**: 最适合当前架构

## Risks And Mitigations

- **执行语义复杂度上升**: 通过显式 mode 和更强 trace 降低理解成本
- **动态计划不稳定**: 引入 validation、fallback 和 deterministic replay
- **调试成本上升**: 将 plan snapshot 和 edge decision 持久化

## Observability

- `task trace` 支持静态计划与动态计划双视角
- event 中包含 node id、edge condition、decision source
- `config debug` 可输出 DAG mode 有效配置

## Operations / Release

- 新能力应以 feature flag 或显式 workflow mode 推出
- 文档明确“当前已 GA”与“实验态”边界
- 自举 workflow 在第一阶段不强依赖 DAG mode

## Test Plan

- Unit tests:
  - DAG validation、topological order、conditional edge
  - CEL 条件与上下文统一
- Integration tests:
  - dynamic plan 从 planner 输出到运行
  - fallback plan 与 fail-closed 行为
- E2E:
  - 含 branch / dynamic add / conditional edge 的 workflow 全链路

## Acceptance Criteria

- 系统存在明确、可启用的 DAG / 动态编排执行模式
- 动态 step 与 planner plan 能进入主执行路径
- 动态计划具备持久化、trace 和事件可观测性
- trigger / prehook 条件语义明显收敛

## Current Evaluation

- `明确、可启用的执行模式`: 已完成
- `动态 step 与 planner plan 进入主执行路径`: 已完成
- `trigger / prehook 条件语义收敛`: 已完成
- `动态计划具备持久化、trace 和事件可观测性`: 部分完成

未完成项集中在“持久化与调试快照”以及 `task_info` graph 输出，因此本 FR 暂不关闭。
