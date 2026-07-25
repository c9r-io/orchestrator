# FR-136: 持久化依赖收口决策 — 新 crate 是收口点还是又一个共享依赖

## 优先级: P1

## 状态: Proposed

## 背景

FR-130 需求 1 冻结 core 边界时，暴露了一条它自己"从未纳入预算"的第二轴：**core 不是持久化的收口点**。

DD-142 记录的 `rusqliteDependentCrates` 有 6 个 crate 直接声明 `rusqlite`。逐个核实其实际用量（本 FR 撰写时实测）：

| Crate | `rusqlite` 出现 | 文件 | 性质 |
|---|---|---|---|
| `core`（`agent-orchestrator`） | 200 | 37 | FR-130 提取的对象 |
| `orchestrator-scheduler` | 37 | 13 | 生产依赖，含 `task_state.rs`(9)、`service/task.rs`(4)、`spawn.rs`(3) |
| `daemon` | 22 | 5 | 生产依赖 |
| `slack-gateway` | 9 | 1 | 生产依赖 |
| `orchestrator-security` | 7 | 4 | 生产依赖 |
| `integration-tests` | 0（src） | — | **dev-dependency**，仅 `tests/trigger_fire.rs` 使用 |

即：除 core 外还有 **4 个生产 crate、23 个文件、75 处引用**直接持有 SQLite 驱动。DD-142 的"6 个 crate"口径包含了一个 dev-dependency，本 FR 按生产依赖重新计数。

FR-130 正文已经指出这条轴的关键性质：

> 在 core 里定义 port trait 并不能阻止这一点——那些 crate 会转而直接依赖新 crate，与目标相反。

这句话是对的，而且它意味着**提取本身无法回答这个问题**。如果先做提取再考虑收口，最可能的结局是：`orchestrator-persistence` 被 5 个 crate 共同依赖，`rusqlite` 的传播面一个字节没减少，只是多了一层目录结构。那是把 god crate 换成 god dependency。

因此这是一个必须**先于实施做出的架构决策**，而不是提取过程中的细节。

## 目标

- 明确 `orchestrator-persistence` 在依赖图中的角色：唯一收口点，或受控的共享底座。
- 为 4 个非 core 生产 crate 的 75 处引用各自定出归属：迁往新 crate、改走抽象接口、或书面保留。
- 产出一条**可被门禁固定**的依赖规则，使决策不依赖后续实施者的记忆。

## 非目标

- **不**写实现代码。本 FR 的产物是设计记录与依赖规则，不含 crate 提取、文件移动或接口改写。
- **不**替 FR-130 决定持久化模块的切分方式。本 FR 只回答"谁可以依赖 SQLite 驱动"，不回答"哪些文件属于持久化层"。
- **不**引入 ORM、更换数据库或改变 schema。
- **不**处理 `integration-tests` 的 dev-dependency——测试代码直接使用驱动做断言是合理的，但需在决策中显式豁免而非默认忽略。

## 需求

### 1. 逐 crate 的引用性质分类

- 对 4 个非 core 生产 crate 的 23 个文件、75 处引用，逐处分类为：
  - **纯数据访问**——可整体迁往新 crate；
  - **类型穿透**——只因签名里出现 `rusqlite::Connection`/`Error` 等类型而引用，可由抽象类型替代；
  - **事务边界控制**——调用方需要显式控制事务范围，是最难消除的一类，需单独设计；
  - **测试断言**——直接查库验证状态，可豁免但须显式记录。
- 分类结果机器可读，作为后续实施的工作项清单。

### 2. 收口形态决策

在下列形态中作出选择并记录理由：

- **A. 严格收口**——只有 `orchestrator-persistence` 依赖 `rusqlite`，其余 crate 通过 repository trait 访问。传播面最小，代价是事务边界控制需要显式的接口设计。
- **B. 受控共享**——允许指定 crate 直接依赖，但依赖清单被门禁冻结，新增需评审。摩擦低，代价是"收口"只是名义上的。
- **C. 分层收口**——core + persistence 为一层，上层 crate（daemon/scheduler/slack-gateway/security）严格禁止直接依赖。折中，需明确分层线在哪。

决策必须回答：`orchestrator-scheduler` 的 `task_state.rs`(9 处) 这类**在调度逻辑内直接持有连接**的代码归哪一侧。它是本决策的判定性用例——如果连它都能留在原地，那么选的实际上是形态 B。

### 3. 依赖规则门禁

- 决策落地为可执行断言：哪些 crate 允许在 `Cargo.toml` 中声明 `rusqlite`，哪些不允许。
- 与 FR-133 的 `cargo-deny` 能力重叠时，明确二者分工，避免两处表达同一规则。
- 规则采用**发现式覆盖**（扫描全部 member manifest）而非枚举允许清单之外无检查，遵循 FR-134 需求 4/12/13 确立的原则。
- 按 FR-127 的分类进入 CI 强制执行面。

### 4. 事务边界的接口草案

- 若选择形态 A 或 C，需给出跨 crate 事务控制的接口草案（如 `with_transaction(|tx| ...)` 闭包式、或工作单元对象），并至少用一个真实调用点验证其可行性。
- 草案不要求实现，但必须证明 `task_state.rs` 与 `daemon` 中现有的事务用法能在该接口下表达。无法表达的用法需显式列出。

## 验收标准

- [ ] 4 个非 core 生产 crate 的 23 个文件、75 处引用全部完成四类分类，结果机器可读
- [ ] 收口形态 A/B/C 已选定，理由书面记录，并明确回答 `orchestrator-scheduler/src/scheduler/task_state.rs` 的归属
- [ ] 依赖规则门禁存在并进入 CI；负向 fixture 证明"在不被允许的 crate 中新增 `rusqlite` 依赖"会失败
- [ ] 门禁的覆盖面由扫描全部 member manifest 得出，而非仅检查枚举出的允许清单
- [ ] `integration-tests` 的 dev-dependency 已显式豁免并记录理由
- [ ] 选定 A 或 C 时，事务边界接口草案存在，且已证明能表达 `task_state.rs` 与 daemon 的现有用法；不能表达的用法逐项列出
- [ ] 与 FR-133 的职责划分已书面记录
- [ ] 本 FR 未引入任何生产代码变更（`git diff` 仅含文档、治理配置与门禁脚本）

## QA 计划

- **分类完整性**：断言分类清单覆盖的引用总数等于扫描所得的 75 处；任一遗漏使检查失败。这与 FR-130 的逐文件台账同一模式——散文式的"已审阅"不构成证据。
- **门禁负向 fixture**：向一个被禁止的 crate 的 `Cargo.toml` 添加 `rusqlite` 依赖 → 门禁失败；移除后通过。再向**允许**的 crate 添加 → 不得失败（防止规则写成一刀切）。
- **发现式覆盖验证**：新增一个 member crate 并在其中声明 `rusqlite`，门禁必须发现它，而不是因为它不在枚举清单里而放行。
- **接口草案可行性**：以 `task_state.rs` 的现有事务用法为样本，逐个写出在草案接口下的等价表达。写不出来的即为该形态的真实代价，须记录而非回避。
- **无代码变更验证**：闭环时确认 `git diff` 不含 `core/`、`crates/*/src/` 下的改动。本 FR 的价值在于决策先行；一旦掺入实现，就失去了"决策可被推翻"的性质。
