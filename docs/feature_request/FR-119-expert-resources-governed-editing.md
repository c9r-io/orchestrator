# FR-119: Expert Resources 可达列表与受治理编辑闭环

## 优先级: P1

## 状态: Proposed

## 背景

FR-067 之后，GUI 已具备 `resource_get`、`resource_describe` 与 `resource_apply` 的 Tauri 命令，但 `ExpertResources` 当前只把集合结果作为一段 YAML 展示，没有可选择的资源行。组件内部的 describe、edit、copy、apply 路径因此没有真实 UI 入口，属于结构性不可达代码；继续增加 mock 测试无法证明用户可以完成资源编辑。

该能力必须保持现有架构边界：daemon/core 仍是资源读取、验证、应用与 RBAC 的权威，GUI 不应通过解析任意 YAML 猜测资源身份或绕过服务端审计。

## 目标

- 提供可键盘操作的资源集合视图，并能进入单资源详情。
- 让 Operator/Admin 在明确确认后编辑和应用资源，Read-only 仅可查看与复制。
- 对验证失败、版本冲突和后端不可用提供可恢复、可审计的反馈。
- 删除或接通所有与 describe/edit/apply 相关的不可达 UI 分支。

## 非目标

- 不在 GUI 内复制 manifest validator 或资源业务规则。
- 不引入绕过 gRPC/daemon 的本地文件直接写入。
- 不在本 FR 中重新设计全部 Expert Console 信息架构。

## 需求

### 1. 结构化资源集合

- daemon/proto/Tauri 边界提供稳定的资源摘要：`kind`、`name`、`project_id`、可选版本/来源。
- GUI 使用结构化摘要渲染列表，不从 YAML 文本推断资源名称。
- 集合为空、加载中、加载失败分别具有明确状态。

### 2. 可达详情与编辑流程

- 鼠标点击、Enter/Space 均可从列表进入 `resource_describe`。
- 详情页支持返回列表、复制和权限允许时的编辑入口。
- Apply 前展示资源身份、影响范围及确认步骤。
- Apply 成功后重新读取 daemon 权威状态；失败时保留编辑内容供修复。

### 3. 权限、并发与审计

- Read-only 不渲染可执行的 Apply 控件。
- Operator/Admin 权限由现有控制面 RBAC 再次校验，前端隐藏不作为安全边界。
- 对过期版本或并发修改返回稳定冲突语义，不静默覆盖。
- Apply 沿用统一 Action Audit Envelope，不记录 SecretStore 明文。

### 4. UI 与可访问性

- 使用现有设计 token、按钮变体和可见 focus ring。
- 列表、详情、确认对话框具备正确的 accessible name 和焦点返回。
- Liquid Glass 不可用时保持可读的实体背景 fallback。

## 验收标准

- [ ] 用户可从五类资源集合中的资源行进入详情，不存在只能通过内部函数到达的路径
- [ ] Read-only 可查看/复制但不能 Apply；Operator/Admin 可进入受确认的编辑流程
- [ ] GUI 不解析 YAML 来推断资源身份，资源摘要来自 daemon 权威 DTO
- [ ] Apply 成功后展示重新读取的权威内容；验证失败和版本冲突不丢失用户草稿
- [ ] Apply 产生可关联的 action audit，敏感字段不进入日志或 UI 错误
- [ ] Vitest 覆盖列表、详情、RBAC、成功、验证失败、冲突与恢复
- [ ] Playwright 覆盖键盘进入详情、编辑确认和焦点返回

## QA 计划

- Rust 单元测试：资源摘要投影、版本冲突、RBAC 和错误映射。
- Vitest：结构化列表、详情、草稿保留、Apply 后重载和错误恢复。
- Playwright：Read-only 与 Operator 两条真实 UI 路径。
- 隔离 daemon QA：应用临时资源、读取审计证据并验证无敏感值泄漏。

## 风险与缓解

- **资源 DTO 与 YAML 漂移**：DTO 只承载导航元数据，完整 manifest 仍由 describe 返回。
- **并发覆盖**：使用版本/摘要 fence，在 daemon 端 fail closed。
- **前端权限误判**：所有 mutation 在 daemon 再授权。
- **大集合性能**：支持分页或虚拟化扩展，不要求一次传输无限集合。

## 依赖与参考

- `docs/design_doc/orchestrator/77-gui-cli-rpc-parity.md`
- `docs/qa/orchestrator/119-gui-cli-rpc-parity.md`
- `docs/design_doc/orchestrator/110-process-console-information-architecture.md`
- `docs/design-system.md`
