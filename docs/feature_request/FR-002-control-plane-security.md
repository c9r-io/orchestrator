# FR-002 - Daemon 控制面认证、鉴权与传输安全

**Module**: orchestrator  
**Status**: Proposed  
**Priority**: P0  
**Created**: 2026-03-09  
**Last Updated**: 2026-03-09  
**Source**: 深度项目评估报告最高优先级改进建议 #2

## Background

`orchestratord` 当前支持通过 UDS 或 TCP 暴露 gRPC 控制面。控制面接口不仅覆盖任务查询，还覆盖：

- `TaskCreate` / `TaskStart`
- `Apply` / `Delete`
- `StorePut` / `StoreDelete`
- `Shutdown`
- `DbReset`
- `ProjectReset`

当 daemon 通过 `--bind` 对外监听时，当前实现未体现认证、鉴权、TLS/mTLS 或细粒度 ACL 机制。

## Problem Statement

当前控制面默认假设“调用者可信”。这个假设在以下场景不可接受：

- daemon 监听 TCP，位于共享网络环境
- 多用户共用同一开发机或跳板机
- 后续若演进为远程 daemon 或团队共享服务，管理面将直接暴露高权限 RPC

这会带来未授权任务启动、配置篡改、数据擦除、恶意 shutdown 等高危风险。

## Goals

- 为 daemon 控制面提供明确的身份认证机制
- 为高权限 RPC 引入最小权限鉴权模型
- 为 TCP 模式提供加密传输与默认安全配置
- 为 UDS 模式保留轻量本地体验

## Non-goals

- 第一阶段实现完整 IAM 平台
- 第一阶段支持复杂组织级策略引擎
- 替代宿主机层面的文件权限控制

## Scope

- In scope:
  - gRPC 认证拦截器
  - 本地 token / service token / mTLS 方案
  - 按 RPC 分类的权限模型
  - TCP 默认禁用或默认要求认证
  - 安全审计日志
- Out of scope:
  - SSO/OIDC 全接入
  - 多租户账单体系

## Proposed Design

### 1. 传输模式分级

- `UDS`: 维持本地默认模式，依赖 socket 文件权限作为第一层边界
- `TCP insecure`: 明确标记为开发模式，默认关闭
- `TCP secure`: 必须启用 `mTLS` 或 `bearer token + TLS`

### 2. 权限分级

建议将 RPC 划分为三类：

- `read_only`: `Ping`, `TaskList`, `TaskInfo`, `TaskLogs`, `TaskWatch`
- `operator`: `TaskCreate`, `TaskStart`, `TaskPause`, `TaskResume`, `Apply`, `StorePut`
- `admin`: `Shutdown`, `DbReset`, `ProjectReset`, `Delete`

### 3. 认证配置

新增 daemon 配置项：

- `auth.mode`: `disabled` | `token` | `mtls`
- `auth.token_file`
- `tls.cert_file`
- `tls.key_file`
- `tls.ca_file`
- `authz.policy_file`

### 4. 审计事件

新增事件与日志：

- `auth_succeeded`
- `auth_failed`
- `authz_denied`
- `admin_rpc_called`

## Alternatives And Tradeoffs

- **仅依赖 UDS 文件权限**: 适合纯本地单用户，不适合 TCP 模式
- **只做 token，不做 TLS**: 实现快，但明文链路仍有风险
- **一开始就做 RBAC + OIDC**: 过重，不符合当前项目阶段

## Risks And Mitigations

- **引入配置复杂度**: 通过本地默认 UDS 零配置保留开发体验
- **兼容老 CLI**: CLI 自动从环境或配置读取 token / cert
- **误锁死服务**: 提供启动前自检与 `orchestrator check --control-plane`

## Observability

- 记录每次 RPC 的调用主体、认证方式、授权结果
- 为安全拒绝输出结构化错误码，而不是普通 `internal`
- 暴露认证失败计数与高危 RPC 调用计数

## Operations / Release

- 默认文档明确：TCP 模式不应在无认证情况下对外暴露
- 示例配置同时给出 UDS 本地模式和 mTLS 远程模式
- 发布说明列出破坏性安全默认值变更

## Test Plan

- Unit tests:
  - token 校验
  - RPC 到权限级别映射
  - 未授权/越权错误码
- Integration tests:
  - TCP 模式下无 token 访问失败
  - admin token 可调用 `Shutdown` / `DbReset`
  - operator token 不可调用 admin RPC
- E2E:
  - CLI 通过安全配置成功连接 daemon

## Acceptance Criteria

- TCP 模式支持认证并默认要求安全配置
- 至少实现 token 或 mTLS 其中一种可用认证方式
- 高权限 RPC 具有独立授权检查
- 认证失败与鉴权拒绝有结构化审计记录
- 本地 UDS 模式仍可维持低摩擦开发体验
