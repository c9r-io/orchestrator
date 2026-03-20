# FR-062: Agent Health 状态可观测性

**状态**: Proposed
**优先级**: P2

## 背景

FR-056 实现了 Agent Health Policy 可配置化（disease_threshold、disease_duration_hours、capability_success_threshold），核心调度逻辑已正确工作：

- `increment_consecutive_errors` 和 `mark_agent_diseased` 在 `record_phase_results()` 中被正确调用
- 23 个 health 相关单元测试全部通过
- `is_schedulable()` 正确排除 diseased agent

但 disease 状态存在两个可观测性缺陷：

1. **CLI 不可见**: `orchestrator agent list` 不显示 disease 状态（`AgentStatus` protobuf 缺少 health 字段）
2. **不持久化**: disease 状态仅存于 `InnerState.agent_health`（in-memory `RwLock<HashMap>`），daemon 重启后丢失

### 来源

FR-060 迭代 10 full-QA 回归中发现 QA-110 S2/S5 无法通过 CLI 验证 disease 状态。

## 需求

### Phase 1: CLI 可观测性（最小可行）

1. 在 `AgentStatus` protobuf message 中添加 health 字段：
   - `is_diseased: bool`
   - `diseased_until: string`（ISO 8601，为空表示未 diseased）
   - `consecutive_errors: int32`

2. gRPC `AgentList` handler 从 `InnerState.agent_health` 读取 health 状态并填充

3. `orchestrator agent list` 输出新增 `HEALTH` 列（`healthy` / `diseased (until HH:MM)`）

### Phase 2: 持久化（可选，视需求决定）

4. disease 状态写入 DB（`agent_health_state` 表或 events 表）
5. daemon 启动时从 DB 恢复未过期的 disease 状态
6. consecutive_errors 计数器持久化

## 验收标准

### Phase 1

1. `orchestrator agent list` 在 disease 触发后显示 `diseased (until HH:MM)`
2. `orchestrator agent list` 在 disease 未触发时显示 `healthy`
3. 现有 23 个 health 单元测试继续通过
4. QA-110 S2/S5 可通过 CLI 验证（非仅 unit test）

### Phase 2

5. daemon 重启后，未过期的 disease 状态恢复
6. consecutive_errors 跨 daemon 重启保持

## 实现提示

### Phase 1 涉及文件

| 文件 | 变更 |
|------|------|
| `proto/orchestrator.proto` | `AgentStatus` 添加 health 字段 |
| `crates/daemon/src/server/agent.rs` | `AgentList` handler 读取 `agent_health` |
| `crates/cli/src/agent.rs` | `agent list` 输出新增 HEALTH 列 |
| `core/src/health.rs` | 添加 `query_agent_health_status()` 公开查询接口 |

### Phase 2 涉及文件

| 文件 | 变更 |
|------|------|
| `core/src/db.rs` | 新增 `agent_health_state` 表 schema |
| `core/src/health.rs` | 添加 DB 持久化路径 |
| `crates/daemon/src/main.rs` | 启动时恢复 health 状态 |

## 约束

1. Phase 1 不引入 DB schema 变更
2. 不改变现有 disease 判定逻辑（仅增加可见性）
3. gRPC AgentStatus 变更需向后兼容（新字段为 optional）
