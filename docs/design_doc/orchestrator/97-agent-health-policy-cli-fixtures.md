# DD-097: Agent Health Policy CLI 测试夹具

| 字段 | 值 |
|------|---|
| **关联** | FR-087, FR-056, DD-068, QA-110b S2 |
| **状态** | Final |

## 背景

FR-056 实现了 Agent Health Policy 可配置化，但 QA-110b S2（`orchestrator check` 展示自定义 health policy）被标记为"已知限制"，认为 CLI 无法注册带有自定义 health_policy 的测试夹具。

## 分析结论

经审计，fixture manifest 通过 `orchestrator apply` 的完整数据路径已正确保留 health_policy：

1. **YAML 解析** — `parse_manifests_from_yaml()` 将 `AgentSpec.health_policy: Option<HealthPolicySpec>` 正确反序列化
2. **gRPC 传输** — `ApplyRequest.content` 携带完整 YAML 原文，无字段过滤
3. **资源存储** — `apply_to_store()` 将 AgentSpec 序列化为 JSON 存入 resource_store
4. **配置调和** — `reconcile_single_resource()` 调用 `agent_spec_to_config()`，显式映射 health_policy 各字段
5. **Check 展示** — `check/mod.rs` 通过 `is_default()` 判断显示 default/custom/disabled

**结论**：无需新增代码。Fixture manifest + `orchestrator apply --project` + `orchestrator check --project` 三步即可验证全部场景。

## 设计决策

| 决策 | 理由 |
|------|------|
| 不新增 CLI 子命令 | `orchestrator apply -f <fixture>` 已满足需求，新增命令引入不必要的 API 表面 |
| 使用 project 隔离 | `--project` 参数确保 QA 夹具不污染用户数据 |
| 自动化 QA 脚本 | `scripts/qa/test-health-policy-check.sh` 提供可重复验证 |

## 验证

- 单元测试：`cargo test --workspace` 全部通过
- QA 脚本：`scripts/qa/test-health-policy-check.sh` 3/3 场景通过
  - S2-a: custom thresholds `(duration=1h, threshold=5, cap_success=0.3)`
  - S2-b: disease DISABLED
  - S2-c: default policy baseline

## 受影响文件

| 文件 | 变更 |
|------|------|
| `scripts/qa/test-health-policy-check.sh` | 新增 — 自动化 QA 脚本 |
| `docs/qa/orchestrator/110b-agent-health-policy-advanced.md` | 更新 — 移除已知限制，补充验证步骤 |
