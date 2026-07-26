# FR-141: 持久化层不再交出驱动连接 — `AsyncDatabase` 的 API 边界

## 优先级: P1

## 状态: Proposed

## 背景

`crates/orchestrator-persistence/src/async_database.rs` 的两个方法把驱动的连接类型放在
公开 API 上：

```rust
pub fn writer(&self) -> &tokio_rusqlite::Connection   // :60
pub fn reader(&self) -> &tokio_rusqlite::Connection   // :65
```

**持久化层之外有 165 处调用**（core 126、daemon 27、orchestrator-scheduler 12），层内另有
67 处。拿到连接之后，`conn.execute(sql, [])` 不需要在任何地方写出 `rusqlite::` 路径。

这不是新发现。DD-147 在 FR-136 时就点名了它，并把它作为**门禁必须有第二个条件**的全部理由：

> `AsyncDatabase::writer()` 和 `reader()` 返回 `&tokio_rusqlite::Connection`……
> 所以"一个 crate 提到 `rusqlite` 多少次"测的是与问题相邻的东西。

`crates/orchestrator-security/src/secret_store_crypto.rs` **0 处驱动引用、4 条生产 SQL**，
就是这个 API 造成的形态。条件 2（逐文件冻结 SQL 语句数）是为了抓住它的**后果**而存在的。

### 为什么它现在无人认领

FR-130 Phase B 在 2026-07-26 明确拒绝了一个"给 `AsyncDatabase` 加接受 `anyhow::Result`
闭包的方法、删掉六份重复的 `fn other`"的改动，理由是它能在不搬动任何一条 SQL 的前提下把
Phase B 的 83 处收敛掉约 39——**推数字，而不是处置文件**。

**那个拒绝是对的。** 但它把两件事打包成了一件：

| 理由 | 判断 |
|---|---|
| 把 83 收敛掉 39 | 推棘轮，应当拒绝 |
| 持久化层不该把驱动的连接类型放在公开 API 上 | 架构边界，从未被单独判断过 |

按第二个理由，这件事的规模不是 39 处引用，而是 **165 个调用点、跨三个 crate**，验收判据
也完全不同。FR-130 拒绝它之后，它掉出了所有 phase 与所有 FR 的视野。本 FR 是把它接回来，
并把判据写在 API 边界上而不是写在棘轮上。

### 两个台账都数不到它

`crates/daemon` 与 `crates/orchestrator-scheduler` 在 DD-147 中是 `forbidden` 角色，残量
冻结在 22 / 17 处驱动引用。它们同时有 **39 处 `.writer()` / `.reader()` 调用，其中只有 4 行
同时带着 `rusqlite` 词元**——**其余约 35 处两个台账都数不到**。

也就是说：被禁止直接持有驱动的两个 crate，正通过一个不被任何棘轮计数、也不被任何台账冻结的
API，每次都拿到一个真实的驱动连接。条件 2 能抓住它们**用它做了什么**（SQL 语句数），
没有任何东西冻结**它们能拿到它**这件事本身。

## 目标

- 让 `orchestrator-persistence` 的公开 API 不出现驱动类型，调用方拿到的是行为而非连接。
- 让 DD-147 条件 2 的存在理由可以被重新评估——它是为补偿这个泄漏而设的。

## 非目标

- **不**以棘轮收敛为目的，也**不**以棘轮收敛为验收判据。本 FR 落地后 core 的 `rusqlite`
  计数会作为**副作用**下降，该下降**不得**被读作 FR-130 Phase B 的进度。两者的判据不同：
  Phase B 是逐文件处置，本 FR 是 API 边界。台账届时须一次重生成并在评审说明中写清归因。
- **不**单独删除六份重复的 `fn other` 助手。它们随调用点迁移自然消失；若最后仍有残留，
  那是本 FR 未完成，不是可以顺手清理的对象。
- **不**改变任何 SQL 语句、任何 schema、任何 gRPC/CLI 契约。
- **不**与 FR-130 Phase B 并行。本 FR 触及 core 的 126 个调用点，与 Phase B 的逐文件拆分
  冲突面过大，须在 Phase B 闭环之后开始。

## 需求

### 1. 闭包式 API

- `AsyncDatabase` 提供以闭包表达一次数据库交互的方法，调用方不再接触
  `tokio_rusqlite::Connection`。
- `writer()` / `reader()` 从公开 API 移除（或降为 crate 私有）。**这一步是本 FR 的实质**——
  只增加新方法而保留旧方法，等于没有边界。
- `flatten_err(err: tokio_rusqlite::Error) -> anyhow::Error`（`async_database.rs:71`）同属
  公开面上的驱动类型，一并处置。

### 2. 调用点迁移，按 crate 分批

- 165 个调用点分批迁移：**core（126）、daemon（27）、orchestrator-scheduler（12）**。
- 每批一次提交、一次可回退证据，与 FR-130 Phase A/B 同一标准。不接受一次性大提交。
- 分批顺序由依赖方向决定：先迁下游消费者（daemon、scheduler），再迁 core，使每一批之间
  旧 API 都还在，最后一批才把它删掉。

### 3. 行为等价证明

- `config/governance/schema-snapshot.sql` 逐批不变。
- **Phase C 钉住的错误 category 保证必须存活。** `phase_c_preserves_the_external_dependency_category`
  以真实未迁移库产出的真实错误断言驱动错误归类为 `ExternalDependency`，而该 category 在
  gRPC/CLI 契约上。闭包 API 会重写错误传播路径，这是最容易在无编译错误的情况下改掉它的
  改动形态——FR-130 B1 已经踩过一次（`classify_resource_error` 把
  `no such table: resources` 读成 `NotFound`）。
- `cargo test --workspace`、strict Clippy、既有集成测试在不修改断言的前提下全绿。

### 4. 公开面断言

- 新增检查：`orchestrator-persistence` 的公开 API 不得出现 `rusqlite` / `tokio_rusqlite`
  类型。**由解析签名得出，不是 grep 文件**——一个 `pub fn` 返回类型里的驱动类型与一条
  文档注释里提到它，是两件事。
- 该检查进 `ALL_CHECKS` 或等价注册表，从而受 FR-129 的两条 meta 断言约束。
- 按 FR-127 的分类进入 CI 强制执行面，并记得往 `governance` job 的 `OUTCOMES` 加行
  （在 FR-137 闭环之前，没有任何东西替你守这一步）。

### 5. 重新评估 DD-147 的条件 2

- 泄漏关闭后，条件 2（逐文件冻结 SQL 语句数）是否仍需保持当前形态，是一次**决策**，
  不是自动结论。写下判断与理由。
- 若判断为可收窄，须同时说明 `orchestrator-security`（`exempt`，自开连接）与
  `slack-gateway`（`separate-database`）不受本 FR 影响，因此条件 2 对它们仍然必要。

## 验收标准

- [ ] `writer()` / `reader()` / `flatten_err` 不再是公开 API；持久化层之外零处驱动连接获取
- [ ] 165 个调用点全部迁移，按 crate 分批，每批独立可回退
- [ ] 每批之后 `schema-snapshot.sql` 未变
- [ ] `phase_c_preserves_the_external_dependency_category` 未被修改且通过；负向变异
      （改回按消息分类）仍以 `left: NotFound, right: ExternalDependency` 失败
- [ ] 公开面断言存在，由解析签名得出，有负向 fixture（在 `pub fn` 签名里放回驱动类型 → 失败；
      仅在文档注释里提到 → 通过）
- [ ] 新 check 已注册并受 meta 断言约束；`governance` job 的 `OUTCOMES` 已加行
- [ ] 六份重复的 `fn other` 已随迁移消失；若有残留，逐个记录原因
- [ ] 条件 2 的去留已书面决策
- [ ] `cargo test --workspace`、strict Clippy、全部既有 CI job 状态不因本 FR 变化
- [ ] 台账重生成的评审说明写明：core 计数的下降归因于本 FR 的 API 迁移，**不是** Phase B 进度

## QA 计划

- **行为证据先于结构证据**。"驱动类型不在公开 API 上"是结构性的，它本身不证明数据仍被
  正确读写。每批之后须有一次真实写入-读回穿过被改动的路径，形态沿用
  `crates/orchestrator-persistence/tests/round_trip.rs` 及其未迁移库的反向半边。
- **错误 category 是本 FR 最大的静默风险**，因为它不产生编译错误而它在对外契约上。
  每批之后跑 Phase C 那条测试，并至少一次以变异确认它仍会失败。
- **公开面断言必须解析签名**。grep `rusqlite` 会被文档注释满足，那正是 FR-134 反复消灭的
  "文本存在性当作事实"。正反两条 fixture 缺一不可。
- **分批可回退**：每批提取 commit 各自可机械回退，按 FR-130 Phase A 建立的做法——
  **逐个 commit 具名回退，不用 range**（FR-130 Phase A 的第一次证明用了 range，把中间一个
  无关提交也回退了，回退了 45 个路径而非 44）。
- **不需要新的 CI job**。

## 与 FR-130 的关系

独立议题，**不得并入 FR-130 Phase B**。Phase B 的完成判据是 18 个文件各有一条书面处置
（迁出 / 拆分 / 保留并记录理由），不是引用计数归零；`attention.rs` 与 `process_metrics.rs`
今天的诚实处置就是"保留并记录理由"，因为**只有本 FR 的改动能清零它们**。把本 FR 并进
Phase B，等于让 Phase B 的判据重新变回那个数字。

排在 Phase B 之后开始。
