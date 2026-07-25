# FR-125: 遗留协调机器分级退役（Deprecate → Remove）

## 优先级: P2

## 状态: Proposed

## 背景

FR-124 完成了协调坍缩的 strangler 迁移收尾：11 个生产 Workflow 精确分类、7 个非 governance-only 已迁移并具独立 legacy/tool 对等证据、冻结棘轮生效、`config/governance/coordination-collapse-ledger.json` 建立三级退役标准（freeze → deprecate → remove）。当前进度停在**第一级 freeze**：遗留协调代码全部仍在源码中，仅被冻结、封顶、拒绝新消费者。

退役基线（ledger `sourceBaseline`，非 test Rust 源 + Cargo 清单）：

| 通道 | 触点 | 关闭门禁实测 |
|---|---:|---:|
| captures / json_path | 143 | 143 |
| PipelineVariables | 47 | 46 |
| cel-interpreter | 9 | 9 |

FR-124 的退役标准已写明 deprecate/remove 的前置条件，但**未执行**。本 FR 执行 freeze 之后的两级，把"冻结、封顶"推进到"实际移除零消费者的遗留协调代码"，同时严格保护 DD-130 认定应永久保留的机制。

本 FR 不重新论证任何架构决策；它是 FR-124 三级退役标准的执行阶段，受 ledger 既有前置条件门控。

## 目标

- 对每个遗留协调通道建立机器可读的**生产消费者清单**，据此逐通道推进 deprecate → remove。
- 优先移除已确认零生产消费者的通道，从 **captures/json_path** 开始（现测零生产 Workflow 消费者）。
- 为 `PipelineVariables` 通用存储的移除**排除阻塞依赖**：先把 4 个保留通道（`goal` + 3 个 sandbox 安全信号）迁移到一个窄的专用载体，再退役通用 `HashMap<String,String>` 协调存储。
- 每次移除都附兼容窗口、legacy fixture 回归与回滚证据，并令退役后棘轮基线单调下降。

## 非目标

- **不**移除 CEL 作为确定性治理闸门的能力（`is_last_cycle` 等 prehook 门禁），也不移除 `cel-interpreter` 依赖本身——governance-only 与 hybrid 工作流的治理闸门继续使用它。仅退役其承担的**协调**用法。
- **不**移除 builtin 派发机制——`self_test`、`self_restart`、`loop_guard` 是生存/治理机制而非协调，永久保留。
- **不**引入 typed pipeline vars / LangGraph 式 typed state + reducer——DD-130 判定 closed, not deferred。为 4 个保留通道建立窄载体是 DD-130 明确认可的 "small safety-scoped change"，与被否决的通用 typed-state 层是两回事，不得借本 FR 扩大为后者。
- **不**在本 FR 内移除 `ShellRunnerExecutor`——其退役卡在**另一条独立迁移轴**（agent 由 legacy shell command 模板迁移到 typed driver），当前仍有生产工作流/agent 使用 shell 命令模板。本 FR 仅澄清该前置条件并将其显式移交后续 FR，避免与协调退役混淆（见需求 4）。

## 需求

### 1. 生产消费者清单

- 为 captures/json_path、`PipelineVariables`、cel-interpreter 协调用法各建立机器可读清单，区分"生产 Workflow 消费者"与"仅内部代码机器"。
- 清单纳入 ledger 或其旁生成物，随退役进度更新；每个通道的"零生产消费者"结论必须可复现（脚本化扫描）。
- 显式登记 4 个保留通道及其当前载体，标注为"退役范围外，但阻塞 `PipelineVariables` 通用存储移除"。

### 2. captures / json_path 退役（首个，风险最低）

- 前置：现测零生产 Workflow 消费者，据 ledger `deprecate` 条件先标记弃用并保留兼容窗口。
- 弃用期后移除 captures/json_path 的抽取与消费代码路径；legacy fixture 保留以证明回归。
- 移除后棘轮基线的 `capturesOrJsonPath` 从 143 显著下降，并在关闭门禁中体现。

### 3. PipelineVariables 通用存储退役（依赖窄载体先行）

- 先为 4 个保留通道（`goal`、`last_sandbox_denied`、`sandbox_denied_count`、`last_sandbox_denial_reason`）建立专用窄载体（例如显式 typed 字段），与通用 `vars: HashMap<String,String>` 解耦。
- 迁移这 4 个通道到窄载体后，证明通用协调存储零生产消费者，再移除 `HashMap` 协调存储与其 4KB spill-to-disk 机制。
- 窄载体的语义、持久化（当前 `pipeline_vars_json`）与 CEL prehook 变量注入（`core/src/prehook/context.rs`）保持行为不变，附回归证据。

### 4. cel-interpreter 协调用法清零 与 ShellRunnerExecutor 前置澄清

- 证明 cel-interpreter 在生产工作流中零**协调**用法（治理闸门用法保留、不计入退役）。
- 澄清 ledger 中 `shellRunnerExecutor.removeAfter` 的口径：协调路径的 tool-path 对等 ≠ agent 进程 spawn 路径的 driver 化；显式记录仍使用 legacy shell command 模板的生产工作流/agent 数量，并将 `ShellRunnerExecutor` 移除移交独立的 driver 迁移 FR，本 FR 只保持其 `frozen` 状态与前置条件文档化。

## 验收标准

- [ ] 每个遗留协调通道具备可复现的生产消费者清单，"零消费者"结论可脚本化验证
- [ ] captures/json_path 完成 deprecate → remove，棘轮基线 `capturesOrJsonPath` 从 143 显著下降且门禁反映
- [ ] 4 个保留通道迁移到窄载体，行为（持久化 + CEL 变量注入）回归通过，且**未**引入通用 typed-state 层
- [ ] `PipelineVariables` 通用协调存储在窄载体就绪且零消费者后移除，或明确记录其阻塞项与下一步
- [ ] cel-interpreter 协调用法证明清零；治理闸门用法与 `cel-interpreter` 依赖保留
- [ ] `ShellRunnerExecutor` 退役前置条件澄清并移交独立 FR，本 FR 内保持 `frozen`
- [ ] 每次移除附兼容窗口、legacy fixture 回归与回滚证据；退役后棘轮基线单调下降
- [ ] `cargo test --workspace`、strict Clippy、边界层覆盖率治理与协调 strangler 关闭门禁全部通过

## QA 计划

- 消费者清单脚本 fixture 测试：注入一个协调消费者应使"零消费者"断言失败，移除后恢复。
- captures/json_path 移除回归：legacy fixture 证明移除前后行为、移除后代码路径不可达。
- 窄载体行为对等测试：4 个保留通道在窄载体下的持久化与 CEL 注入与迁移前逐一比对。
- 棘轮单调性测试：退役某通道后基线下降；任何回升（新增触点）导致门禁失败。
- 全量回归：复用 `scripts/qa/test-coordination-strangler.sh` 证明 7 条工作流对等与生存机制不受退役影响。

## 风险与缓解

- **误删治理/生存机制**：非目标显式排除 CEL 治理闸门、builtin 派发与 4 个保留通道；每次移除前跑关闭门禁确认对等与生存回归。
- **窄载体扩张为 typed-state**：验收标准与非目标显式禁止；窄载体仅覆盖 4 个既有保留通道，不建通用 reducer 存储。
- **captures 移除影响隐藏消费者**：先弃用 + 兼容窗口，legacy fixture 保留回归证据，可回滚。
- **ShellRunnerExecutor 误混入**：显式移交独立 driver 迁移 FR，本 FR 不触其移除，仅文档化前置。
- **退役半途停滞**：棘轮单调下降门禁使停滞可见；每级 deprecate/remove 附 ledger 前置条件校验。

## 依赖与参考

- `docs/design_doc/orchestrator/136-coordination-strangler-completion.md`
- `docs/design_doc/orchestrator/130-coordination-collapse-mcp-tools.md`
- `config/governance/coordination-collapse-ledger.json`（`retirement.deprecate` / `retirement.remove` / `shellRunnerExecutor`）
- `docs/qa/orchestrator/174-coordination-strangler-completion.md`
- `scripts/qa/test-coordination-strangler.sh`
- `crates/orchestrator-config/src/config/pipeline.rs`、`core/src/prehook/context.rs`、`crates/orchestrator-runner/src/runner/spawn.rs`
