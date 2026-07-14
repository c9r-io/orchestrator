# FR-105: Session RuntimePolicy Authority And Deterministic Control Gates

## 优先级: P0

## 状态: Proposed

## 依赖: FR-102

## 计划闭环产物

- `docs/design_doc/orchestrator/115-session-runtime-policy-authority.md`
- `docs/qa/orchestrator/152-session-runtime-policy-authority.md`
- `scripts/qa/test-agent-session-control-plane.sh`（更新）

## Background

FR-102 将 agent session 提升为可观察、可重入且受单 writer lease、fencing、RBAC、审计与 feature flag 保护的控制面资源。其设计约定 `_system` RuntimePolicy 中的 `session_read_enabled` 与 `session_control_enabled` 是全局 rollout/rollback 开关。

当前 daemon 的 session handler 却通过无 project 参数的 `OrchestratorConfigExt::runtime_policy()` 读取这两个开关。该方法从 `ResourceStore` 的全部 RuntimePolicy 中取第一个投影；当 `_system` 和普通 project 同时存在 RuntimePolicy 时，结果依赖 `HashMap` 迭代顺序。仓库审计已经复现：即使 `_system.session_control_enabled=false`，writer attach 仍可能成功。

这不是单纯的测试不稳定。它使禁用 session mutation 的应急回滚开关失去确定性，直接影响终端输入、writer attach、detach 与 close 的控制面边界。

## Goals

- 为不带 project 的全局 RuntimePolicy 读取建立唯一、确定性的 `_system` 权威语义。
- 使 session read/control feature gate 与 DD-108、DD-112 及 QA-149 的全局开关契约一致。
- 确保 apply 返回成功前，新的 `_system` 策略已经原子地影响后续 session 请求。
- 在多个 project RuntimePolicy、不同插入顺序和 daemon restart 下保持相同判定。
- 保持现有 session RBAC、lease、fencing、idempotency、audit、redaction 与 public response 契约不变。

## Non-goals

- 将 session feature flags 改为 per-project 授权模型。
- 重构全部 RuntimePolicy 字段或删除 `runtime_policy_for_project()`。
- 改变 runner、workspace、source、Attention 或 process-metrics 的既有 project policy 语义。
- 修改 session protobuf、CLI 命令、Tauri command 或数据库 schema。
- 放宽 read-only/operator/admin 的现有角色边界。

## Scope

### In scope

- 明确定义并实现 `_system` RuntimePolicy 的全局读取入口；不得通过跨 project 的无序扫描选择 singleton。
- 将 session read/control gate 切换到该显式全局入口。
- 审计 `runtime_policy()` 的 daemon 调用点，确认每个调用点明确选择 global 或 project-scoped 语义；非 session 行为若无需变化，应由回归测试锁定。
- 增加 `_system`/project 策略冲突、相反插入顺序、热加载、restart 和缺失 `_system` 策略的测试。
- 更新 isolated session QA，使其从当前 HEAD 重建所需二进制并稳定验证禁用开关。

### Out of scope

- 新增 RuntimePolicy kind、配置文件格式或 down migration。
- 引入数据库级 feature flag 表。
- 为 session RPC 增加 project 参数。
- 重新设计 Session Inspector UI。

## Interfaces And Data Changes

该 FR 不增加公开 RPC、CLI、Tauri 或持久化接口。内部配置接口应表达两种不同语义：

1. **Global policy**：只读取 `_system` RuntimePolicy，缺失时使用安全默认值。
2. **Project policy**：读取指定 project，按既有规则回退 `_system`，再回退默认值。

不允许继续用“任取一个 RuntimePolicy”的方式表达 global policy。若保留 `runtime_policy()` 名称，其实现必须等价于显式 `_system` 查询，并由插入顺序无关测试约束；也可以增加命名更清晰的 global accessor，并迁移 session gate。

## Key Design

- `_system` 是 session read/control rollout 与 emergency rollback 的唯一权威来源。
- 普通 project 的 RuntimePolicy 不能覆盖或污染 global session gate。
- `session_control_enabled=false` 必须在任何 writer-side I/O、lease reservation、process signal 或 domain mutation之前 fail closed。
- `session_read_enabled=false` 必须阻止 list/get/read/reader attach，同时不得隐式关闭已有 agent process。
- config apply 使用现有原子 runtime snapshot；apply 成功响应之后发出的请求必须观察到新策略。
- 缺失、不可解析或不可运行的 global policy 必须使用现有安全默认/错误语义，不得因为读取失败而放开 mutation。

## Tradeoffs

- 全局 `_system` 语义牺牲了按 project 单独启用 session control 的灵活性，但与已批准设计、回滚手册和当前 RPC 形状一致。
- 仅修补 session handler 可以缩小 diff，但无法消除无 project singleton API 的歧义；本 FR 要求至少把该 API 的 global 语义固定并测试。
- 为每个 session 请求读取原子 snapshot 有少量开销，但避免缓存 feature gate 造成热加载漂移。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| 修正 global accessor 意外改变其他 RuntimePolicy 消费者 | 审计所有调用点；对明确 project-scoped 的消费者改用 `runtime_policy_for_project()`；增加定向回归 |
| 默认值变化导致现有 session 突然不可写 | 保持 `session_read_enabled=true`、`session_control_enabled=false` 的既有安全默认，并在 release notes 中说明 |
| apply 与请求并发时观察到旧策略 | 继续通过原子 runtime snapshot 更新，并用 apply-response 后立即请求的集成测试验证 |
| 测试继续受 stale debug binary 影响 | QA 脚本在 daemon 场景前显式构建或校验当前 HEAD 二进制，不接受仅检查文件存在 |
| project 策略被错误地当成 global fallback | 双策略、反向插入顺序和 restart fixture 必须产生完全相同结果 |

## Observability And Operations

- Policy denial 继续返回 `permission_denied` 和现有 request ID，不记录 terminal input 或 transcript 内容。
- 不增加高基数 metric；可继续使用 action audit 的 action/result/request ID 关联拒绝。
- Rollout：先在 `_system` 保持 `session_control_enabled=false`，部署新 binary，验证 read-only session inspection，再显式启用 mutation。
- Rollback：将 `_system.session_control_enabled=false`；该动作必须在无需 daemon restart 的情况下确定性生效。
- 无 schema 变化，不删除现有 session、attachment 或 action-audit 数据。

## Testing And Acceptance

实现后更新 QA-149 的可执行覆盖，并生成 QA-152 记录该 follow-up 的独立验收证据。

Acceptance criteria:

- [ ] `_system.session_control_enabled=false` 时，即使目标 task project 的 RuntimePolicy 为 true，writer attach、heartbeat、send-input、writer detach 与 close 均 fail closed。
- [ ] `_system.session_read_enabled=false` 时，session list/get/read 与 reader attach 均被拒绝；恢复为 true 后无需 restart 即可读取。
- [ ] `_system` 为 true、普通 project 为 false 时，session global gate 的结果仍由 `_system` 决定。
- [ ] 交换 RuntimePolicy 的创建/插入顺序不会改变任何 session read/control 判定。
- [ ] Apply `_system` policy 成功返回后立即发出的请求观察到新值；daemon restart 后结果一致。
- [ ] 缺失或无效 global policy 不会 fail open。
- [ ] 现有 single-writer、monotonic fencing、exactly-once input、PID identity、restart reconciliation、RBAC、audit 和 redaction 测试继续通过。
- [ ] `scripts/qa/test-agent-session-control-plane.sh` 使用当前 HEAD 构建产物并报告 5 passed、0 failed。
- [ ] `cargo test --workspace` 与严格 Clippy 通过。

