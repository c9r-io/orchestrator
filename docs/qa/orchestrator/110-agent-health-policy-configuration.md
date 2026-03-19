# QA-110 — Agent Health Policy 可配置化

| 字段 | 值 |
|------|---|
| **关联** | FR-056 / DD-068 |
| **前置条件** | daemon 已启动，至少一个 Agent 和 Workspace 已注册 |

## 场景 1：默认行为向后兼容

**步骤**
1. 使用不包含 `health_policy` 的 Agent YAML 注册 agent
2. 触发 agent 连续 2 次基础设施失败（validation_status == "failed"）
3. 通过 unit test + 代码审查确认 disease 判定路径

**预期**
- 连续 2 次失败后运行时会调用 `mark_agent_diseased`
- 默认 Disease 冷却时长为 5 小时
- 行为与配置化之前完全一致

> **验证说明**: 当前 CLI 只能展示 health policy 配置，不能直接读取内存中的
> `agent_health` disease 状态；因此 S1 的最终验证应以
> `core/src/health.rs` 的 unit tests、`record_phase_results()` 调用链代码审查、
> 以及 `output_validation_failed` 触发路径为准，而不是要求 `orchestrator check`
> 直接显示 diseased 状态。

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
- `increment_consecutive_errors` 不被调用（`disease_duration_hours > 0` 守卫跳过）
- `mark_agent_diseased` 不被调用

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

> **已知限制**: `orchestrator check` 在多 workspace 项目中无法显示 workspace 继承的 health_policy（显示为 "default"）。
> 这是 check 命令的显示限制（`check/mod.rs` 仅在单 workspace 时解析继承），不影响运行时行为。
> 运行时 policy 解析路径（`record.rs:179-199`）正确实现 agent > workspace > default 优先级。
> 验证方式：通过 unit test (`mark_agent_diseased_zero_duration_is_noop`) + 代码审查确认。

## 场景 5：Agent 级别覆盖 Workspace 级别

**步骤**
1. Workspace 设置 `disease_duration_hours: 0`
2. Agent 设置 `disease_duration_hours: 2, disease_threshold: 3`
3. 触发 3 次连续基础设施失败

**预期**
- Agent 使用自身的 health_policy（非 Workspace 的）
- 第 3 次失败后 agent 被标记 diseased，冷却 2 小时

## Checklist

| # | Check | Status |
|---|-------|--------|
| 1 | All scenarios verified against implementation | ☑ |

> **Fixed (2026-03-19)**: Fixtures updated from `exit -1` (bash → 255 positive) to
> `echo 'authentication failed: simulated infra failure' >&2` which triggers the
> `validation_status == "failed"` path in output validation. This correctly sets
> `agent_infra_failed = true` via the fatal provider error detection in `output_validation.rs`.
> 23/23 health unit tests confirm code logic is correct.

## Runtime Integration

Health tracking functions are wired into the production execution path:

- **Caller**: `crates/orchestrator-scheduler/src/scheduler/phase_runner/record.rs` → `record_phase_results()`
- **Trigger**: After every phase execution, if infrastructure failure detected (`exit_code < 0`, sandbox denial, or validation failure)
- **Flow**: `increment_consecutive_errors()` → if threshold met → `mark_agent_diseased()`
- **Reset**: `reset_consecutive_errors()` on successful execution
- **Selection**: `core/src/selection.rs` → `is_capability_healthy()` filters diseased agents

> **Note**: When verifying runtime behavior, search for callers in `crates/orchestrator-scheduler/`, not just `core/src/`. The health functions are defined in `core/` but called from the scheduler crate.

## 单元测试覆盖

| 测试 | 文件 |
|------|------|
| `mark_agent_diseased_zero_duration_is_noop` | `core/src/health.rs` |
| `mark_agent_diseased_custom_duration` | `core/src/health.rs` |
| `is_capability_healthy_custom_threshold` | `core/src/health.rs` |
| `HealthPolicyConfig` serde 序列化 | `crates/orchestrator-config/src/config/agent.rs` |
| Spec ↔ Config 双向转换 | `core/src/resource/agent.rs` |
