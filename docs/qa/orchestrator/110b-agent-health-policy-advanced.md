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
