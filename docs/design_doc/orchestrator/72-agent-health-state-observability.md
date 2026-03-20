# DD-072: Agent Health 状态可观测性

## 概述

为 `orchestrator agent list` 和 `task info` 添加 agent health 状态显示，使 disease 状态（由 FR-056 Health Policy 触发）可通过 CLI 观测。

## 设计决策

### Phase 1 实现范围

仅添加 CLI/gRPC 可见性，不引入 DB 持久化。Disease 状态仍为内存态（`InnerState.agent_health`），daemon 重启后丢失。

### 新增 protobuf 字段

在 `AgentStatus` message 中添加 3 个向后兼容字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `is_healthy` | `bool` | 综合健康状态（false = diseased 且未过期） |
| `diseased_until` | `optional string` | ISO 8601 时间戳，仅在 diseased 且未过期时有值 |
| `consecutive_errors` | `int32` | 当前连续错误计数 |

### 查询接口

新增 `agent_health_summary()` 函数（`core/src/health.rs`），封装 health map 读取 + 过期判断 + RFC 3339 格式化，避免 daemon/integration-tests 直接依赖 `chrono`。

### CLI 输出

`agent list` 表格新增 `HEALTH` 列：
- `healthy` — 默认状态
- `diseased(HH:MM)` — 显示恢复时间（仅时:分）

### 关键代码路径

| 文件 | 变更 |
|------|------|
| `proto/orchestrator.proto` | `AgentStatus` 添加 3 个字段 |
| `core/src/health.rs` | 新增 `agent_health_summary()` |
| `crates/daemon/src/server/agent.rs` | `agent_list` 读取 `agent_health` map |
| `crates/daemon/src/server/task.rs` | `task_info` agent states 读取 health |
| `crates/cli/src/commands/agent.rs` | 表格/JSON/YAML 输出新增 health 字段 |
| `crates/cli/src/output/value.rs` | JSON value 新增 health 字段 |
| `crates/cli/src/output/task_detail.rs` | task info agent 表格新增 HEALTH 列 |
| `crates/integration-tests/src/lib.rs` | 测试 harness 填充新字段 |
