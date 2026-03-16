# FR-056 — Agent Health Policy 可配置化

| 字段 | 值 |
|------|---|
| **优先级** | P1 |
| **状态** | Proposed |
| **前置依赖** | 无（与 ticket `agent-disease-misclassifies-qa-failures` 互补但独立） |
| **触发场景** | full-qa workflow 全量回归测试中，agent disease 机制过早阻断执行 |

---

## 1. 背景

当前 agent health/disease 策略完全硬编码：

| 参数 | 值 | 位置 |
|------|---|------|
| Disease 冷却时长 | 5 小时 | `core/src/health.rs:6` `DISEASE_DURATION_HOURS = 5` |
| Disease 触发阈值 | 连续 2 次失败 | `crates/orchestrator-scheduler/.../record.rs:173` `errors >= 2` |
| 成功率阈值（capability 粒度） | 50% | `core/src/health.rs:31` `success_rate() >= 0.5` |

这些参数对 **代码修改型 workflow**（self-bootstrap）合理 — 连续失败通常意味着代码出了系统性问题，暂停调度可以止损。

但对 **QA-only workflow**（full-qa）、**code review** 等"高预期失败率"场景，这些参数过于激进：
- 134 个 QA 文档中 7 个失败（5%）就耗尽 2 个 agent 的健康额度
- 一旦 diseased，需要等 5 小时或重启 daemon 才能恢复
- 无法按 agent 或 workflow 维度调整

## 2. 需求

### 2.1 Agent 级别 Health Policy 配置

在 Agent YAML 中增加 `health_policy` 字段：

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: qa-tester
spec:
  capabilities: [qa_testing]
  command: claude -p "{prompt}" ...
  health_policy:
    disease_duration_hours: 1       # 默认 5
    disease_threshold: 5            # 连续失败次数，默认 2
    capability_success_threshold: 0.3  # 默认 0.5
```

全部字段可选，缺省时使用全局默认值。

### 2.2 全局默认值可通过 Workspace 覆盖

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: full-qa
spec:
  root_path: "."
  health_policy:
    disease_duration_hours: 0       # 禁用 disease
    disease_threshold: 10
```

优先级：Agent 级别 > Workspace 级别 > 全局硬编码默认值。

### 2.3 `disease_duration_hours: 0` 语义

设为 0 表示 **禁用 disease 机制**，agent 永远不会因为失败被标记为 unhealthy。适用于 QA-only workflow。

## 3. 涉及模块

| 模块 | 改动 |
|------|------|
| `crates/orchestrator-config/src/config/agent.rs` | `AgentConfig` 增加 `health_policy: Option<HealthPolicyConfig>` |
| `crates/orchestrator-config/src/cli_types.rs` | YAML 解析 `HealthPolicy` spec |
| `core/src/health.rs` | `mark_agent_diseased` 和 `is_capability_healthy` 接受策略参数 |
| `crates/orchestrator-scheduler/.../record.rs` | 从 agent config 读取策略参数替代硬编码 |
| `crates/orchestrator-scheduler/.../check/mod.rs` | `check` 命令展示 health policy |

## 4. 与 ticket 的关系

ticket `agent-disease-misclassifies-qa-failures` 修复的是 **disease 触发条件的语义错误**（QA 测试结果 ≠ agent 故障）。即使该 ticket 修复后，仍需要本 FR：

- 某些场景下 agent 确实会频繁失败（API 限流、模型不稳定），需要按 agent 调节容忍度
- QA workflow 可能希望完全禁用 disease（`disease_duration_hours: 0`）而不是依赖语义区分
- 不同成本的 agent（opus vs haiku）对失败的容忍度不同

## 5. 验收标准

1. Agent YAML 可声明 `health_policy`，`orchestrator check` 能正确展示
2. `disease_duration_hours: 0` 可禁用 disease，agent 永远 healthy
3. `disease_threshold` 控制连续失败次数阈值
4. Workspace 级别 `health_policy` 作为 agent 缺省值
5. 未配置 `health_policy` 时行为与当前一致（向后兼容）
6. full-qa workflow 在配置 `disease_duration_hours: 0` 后可完整执行 134 个 item
