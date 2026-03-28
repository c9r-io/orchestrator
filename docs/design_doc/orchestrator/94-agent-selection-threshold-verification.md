# DD-094: Agent Selection Threshold Verification

## 背景

FR-086 提出增加 `orchestrator agent simulate-selection` CLI 命令，以验证 diseased agent 在自定义 `capability_success_threshold` 下是否仍可被选中。经审计发现核心逻辑已完整实现且可通过纯函数单元测试确定性验证，无需引入 CLI 命令的额外复杂度（gRPC 表面积、daemon 依赖）。

## 决策

采用 FR-086 验收标准 Option 2：以单元测试 + 代码检查作为充分验证手段。

## 验证覆盖

| 测试 | 位置 | 验证内容 |
|------|------|----------|
| `is_capability_healthy_custom_threshold` | `core/src/health.rs` | diseased agent 在自定义阈值下的 `is_capability_healthy()` 判定 |
| `test_diseased_agent_with_passing_capability_threshold_is_selected` | `core/src/selection.rs` | 集成级：diseased agent（35% 成功率 + 0.3 阈值）在 `select_agent_advanced()` 中不被过滤，可被选中 |

## 逻辑链

1. `AgentConfig.health_policy.capability_success_threshold` 配置自定义阈值（默认 0.5）
2. `select_agent_advanced()` 调用 `is_capability_healthy(health_map, id, capability, cfg.health_policy.capability_success_threshold)` 过滤候选
3. `is_capability_healthy()` 对 diseased agent 比较 `cap_health.success_rate() >= threshold`
4. 单元测试直接验证：30% rate >= 0.3 threshold → true；30% rate >= 0.5 threshold → false
5. 集成测试验证：diseased agent（35% rate, 0.3 threshold）出现在 `select_agent_advanced()` 候选集中

## 关联

- QA 文档：`docs/qa/orchestrator/110b-agent-health-policy-advanced.md`（S1 已更新验证方法）
- 原始 FR：FR-086（已闭环删除）
