# FR-121: Attention Mutation 错误反馈与权威状态对账

## 优先级: P1

## 状态: Proposed

## 背景

Attention Inbox 在 Claim/Snooze/Resolve/Action mutation 失败后会调用 `load()` 恢复 daemon 权威状态。当前 `load()` 开始时同时清空全局 error，因此版本冲突等 mutation 错误可能在 React 批处理过程中从未形成用户可见提示。数据能够恢复，但操作员无法判断动作失败、状态已被他人修改，还是系统已经成功执行。

## 目标

- 将“查询失败”和“mutation 失败”拆分为生命周期明确的反馈通道。
- mutation 失败后仍完成权威重载，同时持续展示可操作的失败原因。
- 避免错误提示、乐观状态、通知 announcement 与真实 daemon 状态互相矛盾。

## 需求

### 1. 错误状态模型

- 区分初始/刷新查询错误、实时流错误和 mutation 错误。
- 权威重载不得隐式清除触发重载的 mutation 错误。
- 错误在用户 dismiss、同类操作成功或明确重试后按规则清除。
- 将 gRPC 稳定状态码映射为可理解文案，不显示内部堆栈或敏感上下文。

### 2. 对账流程

- Claim、Snooze、Resolve 和自定义 Action 共享一致的失败恢复机制。
- 冲突后重新读取最新 item/version，并保持合理选择项。
- 不把失败操作 announce 为成功，不重复提交同一个幂等键。
- 重载自身失败时同时保留 mutation 原因和“无法确认最新状态”的高优先级提示。

### 3. 可访问性

- mutation 失败通过持久 `role="alert"` 或等价 live region 宣告。
- 错误提示提供 dismiss/retry 时，两者均可键盘操作且有明确 accessible name。
- 状态恢复后按钮 enable/disable 与最新 item 状态一致。

### 4. 可观测性

- 记录隐私安全的 mutation result、error category 和 reconciliation result 指标。
- 不记录 requested decision、Slack message body、token 或原始 provider 错误正文。

## 验收标准

- [ ] 版本冲突后用户能看到持续错误提示，同时列表已经恢复为 daemon 最新状态
- [ ] Claim、Snooze、Resolve、自定义 Action 使用同一错误/对账契约
- [ ] 对账失败时明确提示状态尚未确认，不展示虚假的成功状态
- [ ] 后续成功或用户 dismiss 按规范清除旧错误
- [ ] 屏幕阅读器能够获知失败，焦点与当前选择项保持稳定
- [ ] Vitest 覆盖 mutation 失败+重载成功、双重失败、后续成功和 dismiss
- [ ] 隔离 daemon 测试覆盖两个客户端针对同一 version 的竞争

## QA 计划

- Vitest：四类 mutation 的错误生命周期和权威对账。
- Rust/daemon：稳定 gRPC 状态码以及并发 version fence。
- Playwright：用户点击 Claim 遭遇冲突后，错误、最新 assignee/version 和可操作按钮同步呈现。
- 复用 `scripts/qa/test-attention-inbox.sh` 的并发 claim fixture 扩展垂直场景。

## 风险与缓解

- **错误长期滞留造成误解**：错误关联 item/action/version，并提供明确清除规则。
- **重载风暴**：一次失败只触发一次有界对账，不自动无限重试。
- **敏感信息泄漏**：只呈现 allowlisted category 和安全摘要。

## 依赖与参考

- `docs/design_doc/orchestrator/106-attention-inbox.md`
- `docs/qa/orchestrator/143-attention-inbox.md`
- `docs/design_doc/orchestrator/111-control-plane-action-audit-envelope.md`

