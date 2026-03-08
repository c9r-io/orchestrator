# FR-001 - Agent 执行隔离与受限运行时

**Module**: orchestrator  
**Status**: Proposed  
**Priority**: P0  
**Created**: 2026-03-09  
**Last Updated**: 2026-03-09  
**Source**: 深度项目评估报告最高优先级改进建议 #1

## Background

当前 orchestrator 的 agent 执行模型以宿主机 shell 为核心：调度器渲染命令模板后，直接通过 runner 在本机进程空间执行。当前已具备：

- shell allowlist
- shell arg allowlist
- 环境变量 allowlist
- 日志脱敏与敏感词替换

这些措施可以降低误配置风险，但并不能提供真正的执行隔离。agent 进程依然共享宿主机文件系统、网络、进程能力和资源配额，无法满足更高等级的安全边界要求。

## Problem Statement

当前执行模型存在以下结构性问题：

- agent 命令默认运行在宿主机，没有文件系统写入边界、网络出口边界和系统调用边界
- `allowlist` 只约束 shell 和少量环境变量，不能阻止高危命令在允许 shell 内执行
- 一旦 workflow、step template 或 agent prompt 被污染，可能触发越权读写、凭证外泄、横向访问或资源耗尽
- 该执行模型适用于可信本地环境，但不适用于半可信或不可信 agent

## Goals

- 为 agent 执行引入受限运行时，默认提供强于 shell allowlist 的隔离能力
- 将文件系统、网络、进程、资源四类边界纳入统一执行策略
- 保持现有 workflow / agent 配置模型的兼容性
- 支持按 workspace / workflow / step 逐级配置隔离等级

## Non-goals

- 在第一阶段实现完整多租户沙箱平台
- 在第一阶段支持所有操作系统的完全一致隔离语义
- 替换现有 runner 模型中的所有 shell 执行能力

## Scope

- In scope:
  - 新增受限执行模式与 runtime abstraction
  - 文件系统白名单 / 只读挂载 / 工作目录收敛
  - 网络策略开关与域名/IP allowlist
  - CPU / memory / wall clock / process count 资源限制
  - 失败事件、拒绝原因、资源超限原因的结构化审计
- Out of scope:
  - 浏览器级隔离
  - 集群级调度器
  - 完整远程沙箱服务

## Proposed Design

### 1. Runner 执行策略分层

在现有 `RunnerExecutorKind::Shell` 基础上，引入新的执行后端，例如：

- `shell`：兼容模式，保留当前行为
- `sandboxed_process`：本地受限子进程
- `containerized`：基于容器/微虚拟化的隔离执行

### 2. 安全策略模型

新增 `ExecutionSandboxPolicy`，支持以下维度：

- `fs_mode`: `inherit` | `workspace_readonly` | `workspace_rw_scoped`
- `writable_paths`: 显式允许写入路径
- `network_mode`: `inherit` | `deny` | `allowlist`
- `network_allowlist`: 域名/IP/端口范围
- `max_memory_mb`
- `max_cpu_seconds`
- `max_processes`
- `max_open_files`

### 3. 默认策略升级

默认新建配置不再鼓励直接使用宿主机 shell 全权限执行，而是：

- 非自引用 QA / 分析步骤默认 `workspace_readonly + network_deny`
- fix / implement 步骤默认 `workspace_rw_scoped + network_allowlist`
- self-bootstrap workflow 明确声明允许的写路径与构建目录

### 4. 审计与拒绝可观测性

新增事件与日志：

- `sandbox_policy_applied`
- `sandbox_denied`
- `sandbox_resource_exceeded`
- `sandbox_network_blocked`

## Alternatives And Tradeoffs

- **继续增强 shell allowlist**: 实现简单，但无法形成真正安全边界
- **直接强依赖容器**: 隔离强，但本地开发体验变重，兼容性复杂
- **双轨制**: 默认本地受限进程，进阶场景接入容器化执行。该方案更符合当前项目演进阶段

## Risks And Mitigations

- **本地开发体验下降**: 通过 per-workflow override 和 `unsafe` 显式豁免降低影响
- **与现有自举流程冲突**: 为 self-bootstrap 定义受控例外策略与专用 profile
- **平台兼容差异**: 第一阶段明确以 Unix-like 环境为主

## Observability

- 记录每次 step 的 sandbox profile、拒绝原因、资源使用摘要
- `task trace` 中展示被阻止的系统调用/网络访问/路径写入
- `worker_status` 增加受限执行统计

## Operations / Release

- 新增配置必须保持向后兼容，老配置默认映射到兼容策略
- 默认新模板与示例工作流应采用受限执行模式
- 发布说明需明确兼容模式与推荐模式

## Test Plan

- Unit tests:
  - sandbox policy 配置 round-trip
  - 写路径拒绝 / 网络拒绝 / 资源超限检测
  - 日志脱敏与 sandbox 审计共存
- Integration tests:
  - 实际子进程在只读工作区无法写入
  - 禁网模式下访问外网失败并记录事件
  - 限制进程数与超时行为
- E2E:
  - self-bootstrap workflow 在受控白名单下仍可运行

## Acceptance Criteria

- 平台支持至少一种强于宿主 shell 的受限执行模式
- workflow / step 可声明并继承 sandbox 策略
- 文件系统、网络、资源限制至少覆盖三类边界
- 被拒绝执行时系统返回结构化原因并写入事件
- 现有工作流可在兼容模式下无破坏运行
