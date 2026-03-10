# FR-001 - Step 执行隔离与按需 Sandbox

**Module**: orchestrator  
**Status**: Partially Implemented  
**Priority**: P0  
**Created**: 2026-03-09  
**Last Updated**: 2026-03-10  
**Source**: 深度项目评估报告最高优先级改进建议 #1

## Background

当前 orchestrator 的 agent 执行模型以宿主机 shell 为核心：调度器渲染命令模板后，直接通过 runner 在本机进程空间执行。当前已具备：

- shell allowlist
- shell arg allowlist
- 环境变量 allowlist
- 日志脱敏与敏感词替换

这些措施可以降低误配置风险，但并不能提供真正的执行边界。更重要的是，隔离需求并不由 `agent` 身份决定，而由 `step` 语义决定：

- `implement` / `ticket_fix` 会修改代码，应优先放入 sandbox
- `qa_testing` 需要最大宿主机测试能力，应允许直接在宿主机执行

因此将 sandbox 绑定到 agent 会导致能力耦合和误用，无法优雅表达同一 agent 在不同 step 下的不同执行边界。

## Implementation Status

截至 2026-03-10，本需求已完成主路径落地：

- 已支持 project-scoped `ExecutionProfile` 资源
- 已支持 `WorkflowStep.execution_profile`
- 已支持 agent step 的 `host` / `sandbox` 路由
- 已支持 `execution_profile_applied` 与 `sandbox_denied` 事件
- 已补充 QA 文档并完成主场景验证

当前仍有未完成项：

- `max_memory_mb` / `max_cpu_seconds` / `max_processes` / `max_open_files` 已接入 Unix child `setrlimit` enforcement，但除 `max_open_files` 外仍缺少稳定、端到端 QA 覆盖
- `sandbox_resource_exceeded` 与 `sandbox_network_blocked` 已实现并完成主场景 QA，但 `network_target` 仍属于 best-effort 元数据
- sandbox 网络 allowlist 仍未形成完整、可验证的后端能力；当前行为是结构化拒绝，不再静默降级

因此本 FR 不应视为“完全关闭”，而应视为“核心能力已交付，剩余增强项待补完”。

## Problem Statement

当前执行模型存在以下结构性问题：

- agent 命令默认运行在宿主机，没有文件系统写入边界、网络出口边界和资源边界
- `allowlist` 只约束 shell 和少量环境变量，不能表达 “这个 step 可以写代码，但那个 step 只能读”
- 若把 sandbox 绑到 agent，会迫使同一 agent 在所有能力上共享一个隔离等级，丢失 step 语义
- workflow 作者无法显式声明哪些步骤需要保护宿主机，哪些步骤需要最大系统可见性

## Goals

- 将执行隔离决策下沉到 `workflow step`
- 新增可复用的 `ExecutionProfile` 资源，由 step 显式引用
- 保持 `Agent` 继续只负责 capability / template，不承载运行隔离策略
- 默认兼容：旧工作流不配置时仍按宿主机模式运行
- 第一阶段支持至少一种本地 sandbox 实现，并为不可用平台返回结构化错误

## Non-goals

- 第一阶段不把 sandbox 绑定到 agent
- 第一阶段不把 builtin / command step 全部纳入同一体系
- 第一阶段不实现完整远程沙箱服务或容器调度平台
- 第一阶段不在工作流层引入自动默认映射或隐式继承链

## Scope

- In scope:
  - `ExecutionProfile` project-scoped 资源
  - `WorkflowStep.execution_profile` 字段
  - agent step 的 host / sandbox 选择
  - 文件系统写边界、资源限制、事件审计
  - 新示例工作流：`implement` / `ticket_fix` 使用 sandbox，`qa_testing` 使用 host
- Out of scope:
  - builtin step 隔离
  - command step 隔离
  - 跨 project profile 继承
  - 浏览器级或集群级隔离

## Proposed Design

### 1. Step 选择执行 Profile

在 `WorkflowStep` 上新增可选字段：

```yaml
- id: implement
  required_capability: implement
  execution_profile: sandbox_write

- id: qa_testing
  required_capability: qa_testing
  execution_profile: host
```

规则：

- 未声明 `execution_profile` 时，使用隐式 `host`
- `execution_profile` 仅允许用于 agent step
- profile 必须在同一 project 内存在

### 2. 新增 ExecutionProfile 资源

```yaml
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: sandbox_write
spec:
  mode: sandbox
  fs_mode: workspace_rw_scoped
  writable_paths:
    - src
    - docs
    - Cargo.toml
  network_mode: deny
  max_processes: 32
  max_open_files: 256
```

建议字段：

- `mode`: `host` | `sandbox`
- `fs_mode`: `inherit` | `workspace_readonly` | `workspace_rw_scoped`
- `writable_paths`
- `network_mode`: `inherit` | `deny` | `allowlist`
- `network_allowlist`
- `max_memory_mb`
- `max_cpu_seconds`
- `max_processes`
- `max_open_files`

### 3. 执行层解耦

- `RuntimePolicy.runner` 保留为全局 runner 基线配置
- scheduler 在 step dispatch 时解析 `execution_profile`
- runner 基于 `EffectiveExecutionContext` 选择：
  - `host` -> 现有 shell runner
  - `sandbox` -> 本地 sandbox runner

这样 `Agent`、`Workflow`、`Runner` 三层职责分离：

- Agent: 我会做什么
- Workflow Step: 这一步需要什么能力，以及是否进入 sandbox
- ExecutionProfile: 这一步如何被隔离

### 4. 默认推荐

- `implement` / `ticket_fix`: `sandbox`
- `qa_testing`: `host`
- `plan` / `qa_doc_gen` / `align_tests`: 根据具体 workspace 决定，默认仍建议显式配置

## Alternatives And Tradeoffs

- **继续按 agent 配置 sandbox**: 配置看似简单，但能力和隔离耦合，无法表达同一 agent 在不同 step 下的不同边界
- **workflow 级统一 sandbox**: 会把 `qa_testing` 这类需要宿主机能力的步骤一起关进沙箱，损失测试能力
- **step 引用命名 profile**: 最解耦，可复用，可逐步演进。缺点是多一个资源类型，但长期最清晰

## Risks And Mitigations

- **用户忘记给危险步骤配 sandbox**: 通过文档、模板和 QA 场景强化推荐做法
- **平台隔离能力不一致**: 第一阶段以 Unix-like 本地实现为主，其他平台返回结构化错误，不静默降级
- **profile 过度复杂**: 第一阶段只支持少量稳定字段，不引入 workflow-level inheritance

## Observability

新增事件：

- `execution_profile_applied`
- `sandbox_denied`
- `sandbox_resource_exceeded`
- `sandbox_network_blocked`

当前状态：

- 已实现：`execution_profile_applied`、`sandbox_denied`、`sandbox_resource_exceeded`
- 已实现：`sandbox_network_blocked`

`task trace` 和 step 事件应展示：

- profile 名称
- 执行模式 `host` / `sandbox`
- 被拒绝的路径/网络/资源摘要

## Operations / Release

- 旧工作流无配置时保持宿主机执行，不破坏兼容
- 新模板和示例工作流应显式使用 `ExecutionProfile`
- 发布说明应强调：
  - sandbox 现在是 step 级选择
  - host 是隐式默认
  - 推荐将 `implement` / `ticket_fix` 放入 sandbox

## Test Plan

- Unit tests:
  - `ExecutionProfile` 配置 round-trip
  - `WorkflowStep.execution_profile` round-trip
  - agent step profile 引用校验
  - 非 agent step 使用 profile 时拒绝
- Integration tests:
  - sandbox step 在只读工作区无法写入
  - `workspace_rw_scoped` 仅允许写白名单路径
  - 同一 workflow 中 `implement` 用 sandbox、`qa_testing` 用 host
- Compatibility:
  - 老工作流不配置 profile 时行为不变

## Acceptance Criteria

- 平台支持按 step 选择 `host` 或 `sandbox`
- `Agent` 资源不需要知道 sandbox 策略
- `ExecutionProfile` 可在 project 内复用
- `implement` / `ticket_fix` / `qa_testing` 可在同一 workflow 中使用不同执行模式
- 旧工作流在未配置 profile 时可无破坏运行

## Remaining Gaps

- 为 `max_memory_mb` / `max_cpu_seconds` / `max_processes` 补齐稳定、端到端 QA 覆盖，当前仅 `max_open_files` 已完成主场景验证
- 为真正可验证的 `network_mode=allowlist` 补齐后端能力；当前 macOS 路径仍以 `unsupported_backend_feature` 结构化拒绝
- 在以上尾项完成后，再将 FR 状态从 `Partially Implemented` 推进到 `Implemented`
