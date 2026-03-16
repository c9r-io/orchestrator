# QA-110 — Agent Health Policy 可配置化

| 字段 | 值 |
|------|---|
| **关联** | FR-056 / DD-068 |
| **前置条件** | daemon 已启动，至少一个 Agent 和 Workspace 已注册 |

## 场景 1：默认行为向后兼容

**步骤**
1. 使用不包含 `health_policy` 的 Agent YAML 注册 agent
2. 触发 agent 连续 2 次基础设施失败（exit_code < 0）
3. 检查 agent 健康状态

**预期**
- 连续 2 次失败后 agent 被标记为 diseased
- Disease 冷却时长为 5 小时
- 行为与配置化之前完全一致

## 场景 2：Agent YAML 声明 health_policy

**步骤**
1. 注册包含 `health_policy` 的 Agent YAML：
   ```yaml
   spec:
     health_policy:
       disease_duration_hours: 1
       disease_threshold: 5
       capability_success_threshold: 0.3
   ```
2. 运行 `orchestrator check`
3. 触发 4 次连续基础设施失败

**预期**
- `orchestrator check` 输出显示 `health policy = custom (duration=1h, threshold=5, cap_success=0.3)`
- 4 次失败后 agent 仍然 healthy（阈值为 5）
- 第 5 次失败后 agent 被标记 diseased，冷却 1 小时

## 场景 3：disease_duration_hours: 0 禁用 disease

**步骤**
1. 注册 Agent YAML：
   ```yaml
   spec:
     health_policy:
       disease_duration_hours: 0
   ```
2. 触发 10 次连续基础设施失败

**预期**
- Agent 始终保持 healthy
- 不触发 `increment_consecutive_errors`
- 不触发 `mark_agent_diseased`

## 场景 4：Workspace 级别 health_policy 作为 agent 缺省值

**步骤**
1. 注册 Workspace YAML：
   ```yaml
   spec:
     health_policy:
       disease_duration_hours: 0
       disease_threshold: 10
   ```
2. 注册 Agent YAML（不包含 `health_policy`）
3. 触发多次基础设施失败

**预期**
- Agent 使用 Workspace 的 health_policy（disease 已禁用）
- Agent 始终保持 healthy

## 场景 5：Agent 级别覆盖 Workspace 级别

**步骤**
1. Workspace 设置 `disease_duration_hours: 0`
2. Agent 设置 `disease_duration_hours: 2, disease_threshold: 3`
3. 触发 3 次连续基础设施失败

**预期**
- Agent 使用自身的 health_policy（非 Workspace 的）
- 第 3 次失败后 agent 被标记 diseased，冷却 2 小时

## 场景 6：capability_success_threshold 自定义阈值

**步骤**
1. Agent 设置 `capability_success_threshold: 0.3`
2. Agent 被标记 diseased
3. Agent 的 `qa` capability 成功率为 35%

**预期**
- 35% ≥ 30% 阈值 → agent 仍可被选中执行 `qa` 任务
- 默认阈值 50% 下同场景 agent 会被排除

## 场景 7：orchestrator check 展示 health policy

**步骤**
1. 注册多个 Agent，部分包含自定义 health_policy
2. 运行 `orchestrator check`

**预期**
- 默认策略的 agent 显示 `health policy = default (duration=5h, threshold=2, cap_success=0.5)`
- 自定义策略的 agent 显示具体配置值
- `disease_duration_hours: 0` 的 agent 显示 `disease DISABLED`

## 单元测试覆盖

| 测试 | 文件 |
|------|------|
| `mark_agent_diseased_zero_duration_is_noop` | `core/src/health.rs` |
| `mark_agent_diseased_custom_duration` | `core/src/health.rs` |
| `is_capability_healthy_custom_threshold` | `core/src/health.rs` |
| `HealthPolicyConfig` serde 序列化 | `crates/orchestrator-config/src/config/agent.rs` |
| Spec ↔ Config 双向转换 | `core/src/resource/agent.rs` |
