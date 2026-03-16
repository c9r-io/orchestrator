# DD-068 — Agent Health Policy 可配置化

| 字段 | 值 |
|------|---|
| **关联 FR** | FR-056 |
| **状态** | 已实现 |

## 1. 概述

将原本硬编码的 agent disease 参数（冷却时长、触发阈值、capability 成功率阈值）改为可通过 Agent/Workspace YAML 配置的 `health_policy` 字段，支持按场景调优或完全禁用 disease 机制。

## 2. 数据模型

### 2.1 HealthPolicyConfig（运行时）

```rust
// crates/orchestrator-config/src/config/agent.rs
pub struct HealthPolicyConfig {
    pub disease_duration_hours: u64,        // 默认 5
    pub disease_threshold: u32,             // 默认 2
    pub capability_success_threshold: f64,  // 默认 0.5
}
```

- 挂载于 `AgentConfig.health_policy` 和 `WorkspaceConfig.health_policy`
- 全部字段有 `#[serde(default)]`，未配置时使用全局默认值
- `is_default()` 方法用于序列化时省略默认值

### 2.2 HealthPolicySpec（YAML）

```rust
// crates/orchestrator-config/src/cli_types.rs
pub struct HealthPolicySpec {
    pub disease_duration_hours: Option<u64>,
    pub disease_threshold: Option<u32>,
    pub capability_success_threshold: Option<f64>,
}
```

- 挂载于 `AgentSpec.health_policy` 和 `WorkspaceSpec.health_policy`（均为 `Option`）
- `deny_unknown_fields` 防止拼写错误

## 3. 配置优先级

```
Agent 级别 > Workspace 级别 > 全局硬编码默认值
```

- Spec → Config 转换时，缺省字段填充全局默认值
- 在 disease 触发路径（`record.rs`），如果 agent 的 health_policy 为默认值，则回退到 workspace 的 health_policy

## 4. 关键行为变更

| 场景 | 变更前 | 变更后 |
|------|--------|--------|
| Disease 冷却 | 固定 5 小时 | `disease_duration_hours` 可配置 |
| Disease 触发 | 连续 2 次失败 | `disease_threshold` 可配置 |
| Capability 过滤 | 成功率 ≥ 50% | `capability_success_threshold` 可配置 |
| 禁用 Disease | 不支持 | `disease_duration_hours: 0` |

### 4.1 `disease_duration_hours: 0` 语义

- `mark_agent_diseased()` 直接返回，不修改健康状态
- `record.rs` 跳过 `increment_consecutive_errors` 和 disease 检查
- Agent 永远保持 healthy

## 5. 涉及文件

| 文件 | 改动 |
|------|------|
| `crates/orchestrator-config/src/config/agent.rs` | 新增 `HealthPolicyConfig` 结构体和 `AgentConfig.health_policy` 字段 |
| `crates/orchestrator-config/src/config/safety.rs` | `WorkspaceConfig.health_policy` 字段 |
| `crates/orchestrator-config/src/cli_types.rs` | 新增 `HealthPolicySpec`，`AgentSpec` 和 `WorkspaceSpec` 字段 |
| `core/src/health.rs` | `mark_agent_diseased` 接受 `&HealthPolicyConfig`；`is_capability_healthy` 接受 `success_threshold` 参数；移除 `DISEASE_DURATION_HOURS` 常量 |
| `core/src/selection.rs` | 传递 `cfg.health_policy.capability_success_threshold` |
| `core/src/resource/agent.rs` | Spec ↔ Config 双向转换 |
| `core/src/resource/workspace.rs` | Spec ↔ Config 双向转换 |
| `crates/orchestrator-scheduler/.../record.rs` | 从配置读取策略参数，agent > workspace > 全局默认 |
| `crates/orchestrator-scheduler/.../check/mod.rs` | `orchestrator check` 展示每个 agent 的 health policy |

## 6. 向后兼容

所有默认值与变更前的硬编码值一致：
- `disease_duration_hours: 5`
- `disease_threshold: 2`
- `capability_success_threshold: 0.5`

未配置 `health_policy` 的 YAML 行为与变更前完全相同。
