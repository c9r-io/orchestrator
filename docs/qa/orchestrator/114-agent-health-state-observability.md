---
self_referential_safe: true
---

# QA-114: Agent Health 状态可观测性

**关联**: FR-062 / DD-072
**Scope**: 验证 `orchestrator agent list` 和 `task info` 正确显示 agent disease 状态

## 场景 1: 健康 agent 显示 `healthy`

**步骤**:
1. **Code review** — 确认 `agent_health_summary` 对无 health 记录的 agent 返回 `(true, None, 0)`：
   ```bash
   rg -n "fn agent_health_summary" core/src/health.rs
   ```
2. **Unit test** — 验证 `is_agent_healthy` 对无记录 agent 返回 true：
   ```bash
   cargo test -p agent-orchestrator -- health::tests::healthy_agent_without_entry
   ```

**预期**:
- 无 disease 记录的 agent `is_healthy = true`
- `diseased_until = None`, `consecutive_errors = 0`

## 场景 2: Diseased agent 显示 `diseased(HH:MM)`

**步骤**:
1. **Unit test** — 验证 `mark_agent_diseased` 设置正确的 `diseased_until`：
   ```bash
   cargo test -p agent-orchestrator -- health::tests::mark_agent_diseased_custom_duration
   ```
2. **Code review** — 确认 CLI 表格输出格式：
   ```bash
   rg -n "diseased" crates/cli/src/commands/agent.rs
   ```

**预期**:
- `is_healthy = false` 当 `diseased_until > now()`
- CLI 显示 `diseased(HH:MM)` 格式

## 场景 3: Protobuf AgentStatus 包含 health 字段

**步骤**:
1. **Code review** — 确认 proto 定义包含新字段：
   ```bash
   rg -n "is_healthy|diseased_until|consecutive_errors" proto/orchestrator.proto
   ```

**预期**:
- `is_healthy` (field 7), `diseased_until` (field 8), `consecutive_errors` (field 9)
- 字段为 optional/default，向后兼容

## 场景 4: gRPC handler 读取 agent_health map

**步骤**:
1. **Code review** — 确认 `agent_list` 和 `task_info` handler 读取 `agent_health`：
   ```bash
   rg -n "agent_health" crates/daemon/src/server/agent.rs crates/daemon/src/server/task.rs
   ```

**预期**:
- 两个 handler 均通过 `agent_health_summary()` 填充 health 字段

## 场景 5: 全部 health 单元测试通过

**步骤**:
1. 运行 health 相关测试：
   ```bash
   cargo test -p agent-orchestrator -- health::tests
   ```
2. 运行 workspace 回归：
   ```bash
   cargo test --workspace --lib
   ```

**预期**:
- 23+ health 测试全部通过
- 409+ workspace 测试全部通过

---

## Checklist

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1 | 健康 agent 显示 healthy | ☐ | |
| 2 | Diseased agent 显示 diseased(HH:MM) | ☐ | |
| 3 | Protobuf AgentStatus 包含 health 字段 | ☐ | |
| 4 | gRPC handler 读取 agent_health map | ☐ | |
| 5 | 全部 health 单元测试通过 | ☐ | |
