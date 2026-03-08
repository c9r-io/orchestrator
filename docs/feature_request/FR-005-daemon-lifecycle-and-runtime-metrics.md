# FR-005 - Daemon 生命周期治理与运行态指标补完

**Module**: orchestrator  
**Status**: Proposed  
**Priority**: P1  
**Created**: 2026-03-09  
**Last Updated**: 2026-03-09  
**Source**: 深度项目评估报告最高优先级改进建议 #5

## Background

当前 daemon 已具备：

- gRPC server
- embedded workers
- UDS / TCP 双模式
- 后台 claim pending task 并执行
- 基本 PID / socket 生命周期处理

但与“生产可运维 daemon”相比，仍有明显缺口，例如优雅停机、活跃 worker 统计、真实 uptime、远程 shutdown 生效路径等。

## Problem Statement

当前 daemon 生命周期治理不完整，导致：

- `Shutdown` RPC 语义存在但未真正驱动关闭流程
- `uptime_secs` 始终为 `0`
- `active_workers` 未统计
- 收到终止信号后，对运行中任务和子进程的排空策略不完整
- 观测面无法准确反映 daemon 当前工作状态

这会影响平台稳定性，也会削弱用户对“daemon mode”的信任。

## Goals

- 完成 daemon 的优雅关闭闭环
- 为 worker / task / daemon 提供最小但可信的运行态指标
- 保证停机时不会遗留混乱状态或孤儿进程
- 保持现有 worker loop 架构不被大改

## Non-goals

- 第一阶段接入完整 Prometheus 指标系统
- 第一阶段实现复杂自动扩缩容

## Scope

- In scope:
  - graceful shutdown
  - uptime tracking
  - active worker / running task / queue depth 指标
  - stop signal 与 runtime state 收敛
  - orphan child process 清理
- Out of scope:
  - 分布式 worker 协调
  - 集群 leader election

## Proposed Design

### 1. Daemon Runtime State

引入 daemon 级 runtime state，记录：

- `started_at`
- `active_workers`
- `idle_workers`
- `running_tasks`
- `shutdown_requested`

### 2. 优雅停机流程

停机顺序建议为：

1. 标记 `shutdown_requested`
2. 停止接收新 RPC 或拒绝新的 task start
3. 通知 worker 停止 claim 新任务
4. 对运行中任务执行 pause / stop / drain
5. 关闭 gRPC listener
6. 清理 socket / pid / 临时信号文件

### 3. RPC 与信号统一

将以下入口统一为同一条关闭路径：

- `SIGINT`
- `SIGTERM`
- `Shutdown` RPC
- self-restart drain path

### 4. 运行态接口增强

增强 `Ping` / `WorkerStatus` 返回：

- `uptime_secs`
- `active_workers`
- `idle_workers`
- `running_tasks`
- `pending_tasks`
- `shutdown_requested`

## Alternatives And Tradeoffs

- **保持现状**: 实现成本最低，但 daemon 更像“能跑起来的后台进程”而非可靠服务
- **完整指标系统先行**: 价值高，但若停机语义没补完，指标也不可信
- **先补正确性，再补外露指标**: 最合理

## Risks And Mitigations

- **停机逻辑引入竞态**: 通过统一状态机和原子状态标志降低风险
- **任务中断导致状态不一致**: 停机阶段应先写 DB 状态，再清理子进程
- **self-restart 与 graceful shutdown 相互干扰**: restart path 明确为单独状态

## Observability

- event:
  - `daemon_shutdown_requested`
  - `daemon_shutdown_completed`
  - `worker_state_changed`
  - `task_drain_started`
  - `task_drain_completed`
- `worker_status` 返回真实活动指标
- `ping` 返回真实 uptime 与构建信息

## Operations / Release

- CLI 增加 daemon health / status 可读输出
- 发布说明列出 shutdown 行为变化
- runbook 更新 daemon 停机、重启与异常恢复流程

## Test Plan

- Unit tests:
  - runtime state 计数
  - shutdown state machine
- Integration tests:
  - RPC `Shutdown` 能真正关闭 daemon
  - 正在执行任务时停机可正确排空
  - `Ping` 返回非零 uptime
- E2E:
  - daemon 启动、执行任务、停机、重启的完整回归

## Acceptance Criteria

- `Shutdown` RPC 能触发真实优雅停机
- `Ping` 与 `WorkerStatus` 返回真实运行态数据
- 停机过程中不会继续 claim 新任务
- 运行中任务与子进程有明确 drain/stop 行为
- daemon 关闭后 pid/socket/信号文件被正确清理
