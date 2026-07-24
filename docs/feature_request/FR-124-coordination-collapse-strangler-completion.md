# FR-124: 协调坍缩 Strangler 迁移收尾与遗留路径退役治理

## 优先级: P1

## 状态: Proposed

## 背景

DD-101（流式 Agent Runner 架构转向）与 DD-130（编排器自有协调 MCP 工具）已把"协调智能从声明层坍缩进 Agent 会话 + 类型化工具契约"的架构验证完毕，并在一个 pilot 工作流上证明了行为对等（38→21 有效行、15→0 手写协调行）。DD-130 同时正确地把"是否引入 LangGraph 式 typed state + reducer"标记为 **closed, not deferred**——因为坍缩后没有中央协调状态存活，为不存在的东西建类型层属于过度设计。

但**架构验证完成 ≠ strangler 迁移完成**。当前实测证据显示，旧的整套协调机器仍是生产主干，新工具路径只是一个孤立 pilot：

| 度量 | 现状 |
|---|---|
| 生产工作流（`docs/workflow/`）迁移比例 | **1 / 14**（仅 `streaming-mark-done-convergence.yaml`） |
| 仍依赖 CEL / captures / pipeline-var 的生产工作流 | 5 |
| 旗舰 `self-bootstrap.yaml` | 仍为 **443 行**（DD-101 立的靶子，未减一行） |
| 遗留协调代码触点 | **~225**（captures/json_path 169 + `PipelineVariables` 47 + cel-interpreter 9） |

这构成本项目当前**唯一的结构性技术负债**：两套完整协调架构长期并存，新路径仅 1 个消费者。真正的风险不是任一套失效，而是系统**永久停在"两套都要维护、两种心智模型、两条测试路径"的中间态**；同时 DD-130 的 "closed" 措辞容易被误读为"迁移已完成"，从而**掩盖批量迁移工作根本尚未开展**这一事实。

本 FR 不重开任何已关闭的架构决策，而是把 DD-101/130 已论证的方向**推到收尾**：完成剩余生产工作流迁移、建立遗留路径冻结门禁、并定义遗留协调机器的分级退役标准。

## 目标

- 完成剩余生产工作流从 CEL/captures/pipeline-var 协调到 MCP 工具路径的迁移，每个迁移都附带与 pilot 同级的行为对等证据。
- 为遗留协调通道建立**冻结门禁**：新工作流默认工具路径，新增 pipeline-var/captures 协调用法被守卫拦截，防止负债回潮。
- 明确区分"**协调**"（应迁出、最终退役）与"**确定性治理闸门 + 安全信号**"（DD-130 认定应保留）：CEL 作为治理门禁、以及 4 个残余安全/意图通道（`goal`、`last_sandbox_denied`、`sandbox_denied_count`、`last_sandbox_denial_reason`）不在退役范围内。
- 为遗留协调机器（CEL 协调用法、`PipelineVariables`、captures/json_path、`builtin:` 派发）定义分级退役标准与可审计的触点基线下降曲线。

## 非目标

- **不**引入 typed pipeline vars / LangGraph 式 typed state + reducer——此项已由 DD-130 判定为 closed, not deferred，本 FR 尊重该结论，不得以"迁移收尾"为名重开。
- **不**移除 CEL 作为确定性治理闸门（prehook 布尔门禁）的能力；仅移除其承担的**协调**职责。
- **不**在对等证据齐备前破坏任何现存工作流；`ShellRunnerExecutor` 与遗留路径在退役标准满足前保持可用。
- **不**要求 fixtures/ 下的历史演示 manifest 全部迁移；范围以生产工作流为准，fixtures 按需保留以验证遗留路径回归。

## 需求

### 1. 迁移清单与对等台账

- 枚举全部生产工作流（`docs/workflow/`、`config/`），逐个分类为：`tool-migratable`（可迁移）/ `governance-only`（仅确定性治理闸门，合理保留 CEL）/ `hybrid`（协调迁出、治理闸门保留）。
- 建立机器可读的迁移台账，记录每个工作流的分类、迁移状态、对等证据链接与残余通道。
- 台账须显式列出每个仍存的 pipeline-var/captures 触点及其分类（协调 / 安全信号 / 用户意图），复用 DD-130 残余通道分类模型。

### 2. 分批迁移与逐工作流对等证明

- 按风险与价值排序推进，建议顺序：`qa-loop` → `plan-execute` → `full-qa` → `self-bootstrap`（旗舰、风险最高，置于最后）。
- 每个迁移产出 legacy 与 tool 双版本的对等证据：相同终态（`completed`/`qa_passed` 等）、协调行数下降量、事件完整性（`coordination_tool_*` 与 `driver_tool_*` 双证据）。
- `self-bootstrap` 迁移须在 4 层生存机制（binary snapshot / self_test gate / self-referential enforcement / watchdog）保护下进行，并证明迁移后自举 2-Cycle 策略行为不变。

### 3. 遗留路径冻结门禁

- 新工作流默认工具路径；遗留 CEL/captures/pipeline-var 协调用法进入 "frozen, governance-only" 状态：不再接受新的协调特性。
- 提供守卫（lint 或 apply 期校验），对**新增**的 pipeline-var 协调用法或 captures/json_path 协调用法告警/拒绝，安全信号与治理闸门用法在允许清单内。
- 守卫须区分协调用法与被保留用法，避免误伤 4 个残余安全/意图通道与确定性治理闸门。

### 4. 分级退役标准

- 为每个遗留协调通道（cel-interpreter 协调、`PipelineVariables`、captures/json_path、`builtin:` 派发）定义三级退役：冻结 → 弃用 → 移除；每级的前置条件（"零生产消费者" + "回归证据齐备"）书面化。
- 建立遗留触点基线（当前 ~225）并跟踪其单调下降；退役某通道的代码前，必须先证明该通道在生产工作流中零消费。
- 明确 `ShellRunnerExecutor` 与遗留验证路径的退役从属关系：其移除以所有生产工作流具备工具路径对等证据为前置，与 DD-130 "Do not remove CEL compatibility until all production workflows have independent parity evidence" 一致。

## 验收标准

- [ ] 全部生产工作流在迁移台账中被明确分类（tool-migratable / governance-only / hybrid），无未分类项
- [ ] 除显式 governance-only 外的生产工作流均完成迁移并附逐工作流对等证据
- [ ] `self-bootstrap.yaml` 协调行显著下降，且迁移后自举 2-Cycle 行为与生存机制回归通过
- [ ] 冻结门禁生效：新增 pipeline-var/captures 协调用法被守卫拦截，保留用法与治理闸门不误伤
- [ ] 遗留协调触点基线（~225）已建立并可追踪下降；每次退役附零消费者证明
- [ ] 四个残余安全/意图通道与 CEL 治理闸门被显式标注为"保留、非退役范围"
- [ ] 台账、门禁与退役标准纳入 `cargo test --workspace` / Clippy / QA 脚本的分层验证
- [ ] 明确不触碰 typed-state 决策，文档交叉引用 DD-130 的 closed 结论

## QA 计划

- 逐工作流对等测试：legacy 与 tool 双版本同输入，断言终态、协调行数与事件证据一致（复用 `scripts/qa/test-coordination-collapse.sh` 模式）。
- 冻结门禁 fixture 测试：新增协调用法被拒、保留用法通过、安全信号通过。
- `self-bootstrap` 回归：迁移后在隔离 daemon 上跑通 2-Cycle，验证 self_test gate 与 watchdog 未被削弱。
- 退役标准 dry-run：对拟退役通道执行"零生产消费者"扫描，输出可审计报告。
- 触点基线快照测试：CI 记录遗留触点计数，回退（新增协调触点）失败。

## 风险与缓解

- **迁移 churn 过大**：分批推进，`self-bootstrap` 置于最后；遗留路径在对等证据齐备前保持可用，附加即回滚。
- **误删确定性治理能力**：严格区分协调与治理闸门；CEL 布尔门禁与 4 个残余通道显式排除在退役范围外。
- **旗舰工作流迁移打断自举**：在 4 层生存机制下操作，binary snapshot + self_test gate 兜底；迁移前后各跑一次 2-Cycle 基线对比。
- **迁移半途停滞、负债回潮**：冻结门禁作为棘轮，阻止新协调用法进入；触点基线单调下降门禁使停滞可见。
- **误读为重开 typed-state**：非目标段与验收标准显式声明尊重 DD-130 closed 结论，仅做迁移收尾与退役。

## 依赖与参考

- `docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`
- `docs/design_doc/orchestrator/130-coordination-collapse-mcp-tools.md`
- `docs/design_doc/orchestrator/127-agent-driver-abstraction.md`
- `docs/guide/coordination-tools.md`
- `fixtures/manifests/bundles/coordination-collapse-pilot.yaml`
- `scripts/qa/test-coordination-collapse.sh`
- `docs/feature_request/FR-122-boundary-layer-coverage-governance.md`（遗留协调代码退役将改变覆盖基线，需与其非回退门禁协同）
