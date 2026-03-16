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
