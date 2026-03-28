# QA-110b — Agent Health Policy 高级场景

Split from doc 110: capability threshold and check output.

| 字段 | 值 |
|------|---|
| **关联** | FR-056 / DD-068 |
| **前置条件** | daemon 已启动，至少一个 Agent 和 Workspace 已注册 |

## 场景 1：capability_success_threshold 自定义阈值

**步骤**
1. Agent 设置 `capability_success_threshold: 0.3`
2. Agent 被标记 diseased
3. Agent 的 `qa` capability 成功率为 35%

**预期**
- 35% ≥ 30% 阈值 → agent 仍可被选中执行 `qa` 任务
- 默认阈值 50% 下同场景 agent 会被排除

**验证方法**

以单元测试作为权威验证（FR-086 Option 2 闭环）：

1. `is_capability_healthy_custom_threshold`（`core/src/health.rs`）— 直接验证 diseased agent 在自定义阈值下的健康判定
2. `test_diseased_agent_with_passing_capability_threshold_is_selected`（`core/src/selection.rs`）— 集成级验证：diseased agent（35% 成功率 + 0.3 阈值）在 `select_agent_advanced()` 中不被过滤

```bash
cargo test -p agent-orchestrator -- is_capability_healthy_custom_threshold
cargo test -p agent-orchestrator -- test_diseased_agent_with_passing_capability_threshold_is_selected
```

## 场景 2：orchestrator check 展示 health policy

**步骤**
1. 注册多个 Agent，部分包含自定义 health_policy
2. 运行 `orchestrator check`

**预期**
- 默认策略的 agent 显示 `health policy = default (duration=5h, threshold=2, cap_success=0.5)`
- 自定义策略的 agent 显示具体配置值
- `disease_duration_hours: 0` 的 agent 显示 `disease DISABLED`

## Checklist

| # | Check | Status |
|---|-------|--------|
| 1 | All scenarios verified against implementation | ☑ |

> **Note (2026-03-19)**: 场景 2 中 `default-agent-fail` 的 `health_policy` 未出现在 DB spec_json 中
> 属于正常行为。系统在 spec→config→spec 转换时，值与全局默认相同的字段会被
> `skip_serializing_if = "is_default"` 优化省略。`orchestrator check` 仍正确显示
> `default (duration=5h, threshold=2, cap_success=0.5)`，因为运行时从 HealthPolicyConfig::default()
> 填充。仅当值与默认不同时（如 `custom-agent` 和 `nodisease-agent`）才会持久化。
