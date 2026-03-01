# Orchestrator Foundation Task 04

## Title

运行期控制面稳定性提升

## Goal

提升 `task info`、`task logs`、`task watch` 在任务运行中的稳定性，减少必须直接读取 SQLite 与 `data/logs/` 的情况。

## Problem

本次监控中，运行中的任务多次出现：

- `task info` 返回异常或无输出
- `task logs` 不稳定

这使 CLI 控制面在最需要观测的时候变得不可靠。

## Scope

- 复现运行中查询失败 / 空输出场景
- 修复 `task info` / `task logs` 运行期查询路径中的稳定性问题
- 审查可能的锁竞争、状态读取时序、异常吞掉问题
- 为运行中任务的查询行为补充测试

## Out Of Scope

- 不重做整个 CLI UI
- 不引入新的外部监控系统

## Acceptance Criteria

1. 运行中任务的 `task info` 能稳定返回状态。
2. 运行中任务的 `task logs` 能稳定返回最新日志片段。
3. 常见运行中查询场景不再随机返回空结果或异常退出。

## Suggested Verification

- `cargo test --lib scheduler::query`
- 启动一个长时任务，在其运行中重复执行 `task info` / `task logs` / `task watch` 验证稳定性

