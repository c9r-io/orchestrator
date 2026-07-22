# FR-116: Agent Driver 抽象 — 供应商中立的执行后端契约

## 优先级: P1

## 状态: Proposed

## 依赖

- `docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`：Streaming runner 决策记录（本 FR 是其第二阶段，并修订其中一条已失效结论）
- FR-084：Agent 条件命令规则 + Session 复用（`command_rules` 与 `session_id` 持久化）
- FR-044 / FR-091 / FR-093：Sandbox 写入拒绝、Linux filesystem isolation backend、可配置读取路径白名单
- FR-090：`orchestrator run` 轻量化单步执行（直接装配路径同样需要 driver 选择）
- FR-101：统一 Action Audit Envelope（driver 逃生口与权限决策的审计落点）
- FR-096：Attention Inbox（权限审批的唯一路由目的地）

## 计划闭环产物

- `docs/design_doc/orchestrator/127-agent-driver-abstraction.md`
- `docs/qa/orchestrator/164-agent-driver-abstraction.md`
- `docs/guide/agent-driver-model.md`（EN + ZH）
- `fixtures/manifests/bundles/agent-driver-fixture.yaml`
- `scripts/qa/test-agent-driver-abstraction.sh`
- `docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`（修订 seam-fit 结论）
- `docs/architecture.md`、`docs/guide/02-resource-model.md`、`CHANGELOG.md`（更新）

## Background And Product Decision

DD-101 已落地 `RunnerExecutorKind::Streaming` 与 `StreamingAgentRunner`，验证了结构化 tool-calling 契约可行。但第一版把「供应商协议」塞进了字符串拼接，抽象债务已经显性化：

**证据一 — 控制面知道供应商参数拼写。** `crates/orchestrator-runner/src/runner/streaming.rs:70` `build_streaming_command` 在用户 YAML 提供的 `command` 尾部追加 `--output-format stream-json --verbose --mcp-config … --strict-mcp-config --allowedTools … --permission-mode bypassPermissions`。控制面因此耦合到 Claude CLI 的 flag 拼写与语义。

**证据二 — Driver 已经以退化形态存在。** `crates/orchestrator-runner/src/runner/session_adapter.rs` 的 `RunnerSessionAdapter` trait 具备 `provider()` / `supports()` / `prepare_resume_command()`，形状即 driver，但只有一个方法，且靠字符串前缀嗅探 dispatch（`binary == "claude" || binary.ends_with("/claude")`）。

**证据三 — 多供应商已经破了，且被测试固化。** `ClaudeStreamingSessionAdapter::prepare_resume_command("codex exec", …)` 返回错误，测试 `unsupported_provider_has_explicit_new_session_fallback` 将该失败固化为预期行为。

本 FR 把 `AgentSpec.command: String` 升级为 **Driver**：一个供应商中立、能力可声明、可在 `apply` 时校验的执行后端契约。

### 命名决策

采用 **Driver**（讲供应商协议的组件），不采用 "Runtime"——代码库中 `TaskRuntimeContext`、`ConfigRuntimeSnapshot`、`config_runtime`、`RunnerConfig` 已占据该词。`Runner` / `ExecutionProfile` 保持原义，继续指代沙箱与进程层。

供应商与传输拆成正交两维：`driver: codex` + `transport: cli | sdk`，而非并列的 `CodexCliDriver` / `CodexSdkDriver`。否则 `core/src/selection.rs` 的能力匹配需要知道两个名字指同一供应商。

### 接口收缩决策

初始提案包含 `start / stream / send / approve / cancel / resume / collect` 七个动词。评审后收缩为 **`start` + 会话三方法**，理由：

- **`approve` 下沉是分层错误。** 审批属控制面，已由 Attention 子系统承载（事件溯源投影、`AttentionActionReservation` 乐观并发、CLI 强制 `--expected-version`、RBAC）。若 driver 可直接 approve，将分叉出第二条不可审计的审批路径。改为：driver 发 `PermissionRequested` 事件 → 控制面路由进 Attention → 决策经 `send` 回注。
- **`collect` 不得是第二数据通路。** 降级为对 `events()` 的默认折叠实现。两条通路会不一致——DD-101 风险章节已预判该 bug 类（256KB stdout 上限截断终局 `result` 事件）。
- **`resume` 重载了两个不同概念。** (a) 供应商会话续接（`--resume <session_id>`，跨 step 保上下文）与 (b) 编排器崩溃恢复（`restart_pending` → `compensate_pending_items`）必须异名。(a) 命名为 `attach_session` 并归 driver；(b) 完全留在 loop engine。
- **胖接口会塌陷。** `GenericShellDriver` 无法实现 `send`；若七方法同 trait，它需对三个方法返回 `Unsupported`，且每个调用点都要处理。改由能力描述符在 `apply` 时拦截。

### 事件封套决策

`events()` 产出**单一** `DriverEvent` 枚举，不提供多种 stream mode。外部教训：LangGraph 的 `stream()` 增生到 7 种 mode（`values` / `updates` / `messages` / `custom` / `checkpoints` / `tasks` / `debug`），最终在 1.1 引入 `version="v2"` 收敛为统一 `StreamPart {type, ns, data}`。单封套的附带收益是 `events` 表摄入退化为平凡映射，DD-101 的「tool I/O 成为一等结构化事件」目标直接达成。

## Goals

- 定义 `AgentDriver` / `DriverSession` trait 与 `DriverEvent` / `DriverInput` 封套，置于 `orchestrator-runner`。
- 提供 `DriverCapabilities` 描述符，并在 `orchestrator apply` 时校验 workflow 与所选 driver 的能力兼容性。
- 首批实现 `claude_cli`（含 streaming）、`codex`（transport: cli）、`shell`（通用一次性）三个 driver。
- 供应商参数三层化：归一化选项 / driver 作用域类型化配置 / 门禁逃生口。控制面不再出现 `--output-format` 之类的字面量。
- 保持沙箱与策略不变量：凡触碰 workspace 的 step 必须经由 `spawn_command_via_shell`。
- `CancelSemantics` 参与安全决策，与既有 `SideEffectClass` fail-closed 语义联动。
- 修订 DD-101 中已失效的 phase pipeline seam-fit 结论，并在重构中消除 stdout 截断风险。
- 修复 MCP config 共享临时路径缺陷。

## Non-goals

- 移除 `ShellRunnerExecutor` 或破坏既有 shell workflow；本 FR additive，shell 保持默认。
- 实现 Option B（Rust 内直连 Messages API 的 agent loop）；仍为显式备选。
- 一次性迁移全部既有 workflow；只做一个 pilot。
- 让 driver 承担审批、RBAC、审计——这些留在控制面。
- 允许 SDK transport 执行 workspace 变更类 step（见 Sandbox Invariant）。
- 改变数据库持久化内核；事件摄入为加法。
- 支持任意供应商 flag 直通而不经门禁。

## Interface Contract

```rust
pub trait AgentDriver: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> DriverCapabilities;
    async fn start(&self, req: DriverStartRequest<'_>) -> Result<Box<dyn DriverSession>>;
}

pub trait DriverSession: Send {
    /// 唯一数据出口——所有观测来源
    fn events(&mut self) -> impl Stream<Item = Result<DriverEvent>>;
    /// 唯一数据入口——权限决策是其 variant
    async fn send(&mut self, input: DriverInput) -> Result<()>;
    async fn cancel(&mut self, mode: CancelMode) -> Result<()>;
    /// 不透明；不得在 proto/DTO 中退化为 String
    fn session_ref(&self) -> Option<&SessionRef>;

    /// 默认实现：折叠 events 至终局，非独立通路
    async fn collect(self) -> Result<RunResult> { /* fold over events */ }
}

pub enum DriverInput {
    UserMessage(String),
    ToolResult { call_id: String, payload: Value },
    PermissionDecision { request_id: String, decision: Decision },
    Interrupt,
}

pub enum DriverEvent {
    Started { session: SessionRef },
    AssistantText(String),
    ToolUse { call_id: String, name: String, args: Value },
    PermissionRequested { request_id: String, scope: PermissionScope },
    Usage { cost_usd: Option<f64>, tokens: TokenCounts },
    Finished { outcome: Outcome },
}

pub struct DriverCapabilities {
    pub multi_turn: bool,
    pub tool_hosting: ToolHosting,        // None | Stdio | Http
    pub session_resume: bool,
    pub cancel: CancelSemantics,          // Guaranteed | Cooperative | None
    pub sandboxable: bool,
    pub cost_reporting: bool,
}
```

`SessionRef` 必须保持不透明——`session_adapter.rs` 现有文档注释「without exposing provider tokens to API clients」是刻意的安全属性，重构不得削弱。

## Capability Validation

`DriverCapabilities` 的价值在于把运行时失败前移到 `apply` 时失败：

- workflow 使用多轮对话语义（`send`）→ 要求 `multi_turn == true`
- step 声明审批门 → 要求 driver 能发 `PermissionRequested`
- step 使用 orchestrator-owned MCP 工具 → 要求 `tool_hosting != None`
- 跨 step 上下文续接 → 要求 `session_resume == true`
- `SideEffectClass::NonIdempotentExternal` 的 step → **要求 `cancel == Guaranteed`**
- 触碰 workspace 的 step → **要求 `sandboxable == true`**

不兼容组合在 `orchestrator apply` 返回结构化错误并拒绝，不进入运行态。

## Vendor Parameter Tiers

完全隐藏供应商参数是陷阱（部分差异语义承载：模型、thinking budget、权限模式、上下文压缩）；无类型 `extra_args: Vec<String>` 则把泄漏原样带回。采用三层：

- **Tier 1 — 归一化能力选项**：`model`、`max_turns`、`budget_cap`、`permission_mode`（枚举，非字符串）、`allowed_tools`、`cwd`、`env`、`timeout`。以供应商中立词汇出现在 `AgentSpec` / `ExecutionProfile`，每个 driver 负责映射到自家 flag。
- **Tier 2 — driver 作用域类型化配置**：`driver: claude_cli` + 由该 driver 提供 schema、`apply` 时校验的结构体。显式命名空间化，不伪装可移植。
- **Tier 3 — `raw_args` 逃生口**：仅在显式 opt-in 时允许，写入 canonical action audit（复用 FR-101 envelope）。沿用 `self_referential_policy` + `unsafe_mode` 既有门禁模式。

`RunnerConfig` 现有的 `allowed_shells` / `allowed_shell_args` / `env_allowlist` / `redaction_patterns` 需上移为对**所有** driver 生效的策略面，而非仅约束 shell 路径。

## Sandbox Invariant

**这是本 FR 最高优先级的安全约束。**

今日全部执行经由 `crates/orchestrator-runner/src/runner/spawn.rs:90` `spawn_command_via_shell`，该路径串联：`enforce_runner_policy` → `guard_daemon_pid_kill` → `build_command_for_profile`（Seatbelt / Linux namespaces）→ `process_group(0)` → rlimits → `env_clear()` + allowlist。

SDK transport 在 daemon 进程内发起调用会绕过全部上述机制：

| | CLI transport | SDK transport |
|---|---|---|
| 沙箱隔离 | Seatbelt / namespace | 无 |
| 资源上限 | rlimit | 无 |
| cancel | SIGKILL 进程组，保证 | 协作式，可能挂死 |
| 凭据位置 | 子进程 env | daemon 内存 |
| 崩溃隔离 | 子进程死不影响 daemon | 同进程 |

沙箱是本项目相对同类 agent 编排框架最硬的差异化能力。契约规定：

1. 凡触碰 workspace 的 step，driver 必须经由 `spawn_command_via_shell`（`sandboxable == true`）。
2. SDK transport 仅允许用于只读 / 纯推理 step，或运行在编排器自有的 spawned sidecar 中。
3. `cancel != Guaranteed` 的 driver 不得承载 `NonIdempotentExternal` step。

违反项在 `apply` 时拒绝，不依赖运行时自律。

## Phase Pipeline Impact（修订 DD-101）

DD-101 "Key Design" 第 1 点的 seam-fit finding 断言 *"no refactor of the phase pipeline's control flow is required"*，理由是 phase pipeline 把 child 视为不透明对象、`.wait()` 后读文件。

**引入 `events()` 后该结论失效。** `crates/orchestrator-scheduler/src/scheduler/phase_runner/` 的五阶段需改为：

```
setup → spawn → wait  → validate → record        (现状)
setup → start → consume → fold   → record        (本 FR)
```

`validate` 折叠进流消费。这是 `phase_runner/mod.rs` 的真实重构，非加法改动，实施估算须相应调整。

附带收益：不再存在「重读被截断文件」步骤，DD-101 风险清单中的 256KB stdout 上限截断终局 `result` 事件的问题在此重构中一并消除。

## Known Defect To Fix

`crates/orchestrator-runner/src/runner/streaming.rs` `write_mcp_config` 写入固定共享路径 `$TMPDIR/orch-streaming-mcp.json`，其正确性论证依赖「内容仅取决于 binary 路径，故并发 step 写入相同内容」。

DD-101 首选的 HTTP MCP 方案要求 per-run token（其风险章节明载 "require a per-run token passed to `claude` via `--mcp-config`"）。该前提一旦成立，共享路径即刻成为跨任务串号与 token 泄漏点。本 FR 须将其改为 per-run 目录下的文件，随 run artifacts 统一生命周期管理。

## Risks And Mitigations

- 风险：driver 抽象增加一层间接，调试链路变长。
  - 缓解：`DriverEvent` 全量落 `events` 表；`tracing` 在 start / 每 turn / tool dispatch / 协议解码失败四点埋点。
- 风险：stream-json 事件 schema 跨 CLI 版本半稳定。
  - 缓解：driver 即适配器；pin 并记录 CLI 版本；录制协议 fixture 做 conformance 测试；版本门控。
- 风险：能力校验过严导致既有 workflow 在升级后 apply 失败。
  - 缓解：shell driver 保持默认且能力集与现状等价；校验仅对显式声明 driver 的 agent 生效；提供 `--dry-run` 预检。
- 风险：Tier 3 逃生口被滥用为常规路径。
  - 缓解：需 `unsafe_mode` opt-in、写审计、在 `orchestrator get` 输出中显式标记。
- 风险：phase pipeline 重构影响面大于预期，波及 `command_runs` 记录语义。
  - 缓解：`setup` / `record` 阶段的 `NewCommandRun` 契约保持不变；仅替换中段；既有 shell 路径回归测试全绿为准入条件。
- 风险：per-run MCP config 增加文件句柄与清理负担。
  - 缓解：纳入 run artifacts 目录，复用既有 artifacts 清理策略。

## Acceptance Criteria

- `cargo build` / `cargo test --workspace` 通过；既有 shell workflow 行为不变，回归测试全绿。
- `AgentDriver` / `DriverSession` / `DriverEvent` / `DriverInput` / `DriverCapabilities` 落地于 `orchestrator-runner`，`claude_cli`、`codex`、`shell` 三 driver 实现完成。
- 控制面代码与 YAML 清单中不再出现任何供应商 flag 字面量；`build_streaming_command` 式字符串拼接被移除。
- 能力不兼容的 workflow 在 `orchestrator apply` 阶段被拒绝并返回结构化错误码，附至少 4 个 QA 场景覆盖（multi_turn、tool_hosting、cancel × SideEffectClass、sandboxable）。
- SDK transport 承载 workspace 变更 step 的尝试被拒绝，且有 QA 场景验证。
- `session_ref` 在 gRPC / DTO / 日志 / 审计中均不泄漏原始 provider token，安全文档补充对应场景。
- pilot workflow 在 driver 抽象下与 shell 版本行为等价，YAML 行数对比记录在案。
- `DriverEvent` 全量进入 `events` 表，tool I/O 为一等结构化事件。
- MCP config 改为 per-run 路径，并发场景 QA 验证无串号。
- DD-101 seam-fit 结论完成修订。
