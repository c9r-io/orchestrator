# FR-141: 持久化层不再交出驱动连接 — 连接能力的 API 边界

## 优先级: P1

## 状态: In Progress

## 治理期事实核验（2026-07-27，六处更正）

本节记录 FR 原文与代码实测的差异。计数一律用仓库自己的口径复算——`scripts/lib/rust_source.rb`
的 `rust_files_under` + `strip_test_modules` 加 `scripts/lib/rust_lexer.rb` 的
`mask_literals`，即两个治理台账正在用的那套（生产代码、剥离 `cfg(test)`、词法屏蔽字面量）。
原文的数字未剥离测试代码，且写于 FR-130 Phase B 搬迁之前。

### 更正 1：层外调用点是 54 处，不是 165；而且 crate 清单少了一个

| | 原文 | 实测（生产代码） |
|---|---|---|
| core | 126 | **22** |
| crates/daemon | 27 | **21** |
| crates/orchestrator-scheduler | 12 | **11** |
| 层外合计 | **165** | **54** |
| 层内 | 67 | **145** |

两个原因叠加：FR 写作时 FR-130 Phase B 尚未把约 74 个 core 调用点搬进持久化 crate；且原
计数含 `cfg(test)` 代码。

第三个差异比数字更重要：**`crates/integration-tests/tests/trigger_fire.rs` 有 5 处
`.reader()`，而原文的 crate 清单从头到尾没提过它。** 台账明确给了它 `test-only` 角色
（冻结在 `[dev-dependencies]`），是一个被祝福的合法消费者——`reader()` 一旦下沉，这 5 处
断言当场编译失败。**这正是本 FR 需求 1 自己写下的那句话（"枚举式清单只守得住写它时已知的
东西"）在本 FR 自己身上发作。**

### 更正 2：公开面泄漏的是 87 项，不是 5 项——需求 1 与需求 4 不是同一个范围

按需求 4 要求的方式（解析签名，不 grep 文件）枚举：`pub mod` 下共 **87 个 `pub` 项**的签名
里出现驱动类型。其中：

- **5 项交出或消费连接句柄／驱动错误类型**——`writer`、`reader`、`flatten_err`、
  `db::open_conn`、`sqlite::open_conn`。原文更正后的五项清单，对这一类**完全正确**。
- **另外 82 项**是 `pub fn foo(conn: &Connection, …)`——**索取**连接的函数
  （`task_repository/queries.rs` 20、`session_store.rs` 19、`write_ops.rs` 11、`state.rs` 9、
  `db.rs` 7、`migration.rs` 6、`items.rs` 6…），外加 `pub type TaskRepositoryConn = Connection`。

需求 1 处置前 5 项；需求 4 的断言按其字面（"公开 API 不得出现 rusqlite / tokio_rusqlite
类型"）会在 87 项上全部报红。**两条验收标准不可能被同一个改动同时满足**，而原文从未做出
"哪一个才是边界"的判断。见下方"边界决策"。

### 更正 3：改 `AsyncDatabase` 关不上门——`open_conn(path)` 是第二扇更宽的门

`agent_orchestrator::db::open_conn(&path) -> rusqlite::Connection` 按**路径**新开一个同步
连接，完全绕过 `AsyncDatabase`。层外生产调用点 **27 处**：

| | open_conn（生产） | 分布 |
|---|---|---|
| core | 17 | `persistence/repository/config.rs` 9、`service/bootstrap.rs` 4、`events.rs` 2、`service/system.rs` 1、`service/resource/mod.rs` 1 |
| crates/daemon | 7 | `server/secret.rs` 4、`server/handoff.rs` 2、`server/session.rs` 1 |
| crates/orchestrator-scheduler | 3 | `scheduler/spawn.rs` 2、`scheduler/safety/restart.rs` 1 |

daemon 与 scheduler 都持有 `state.db_path`。**把 `writer()`/`reader()` 拿掉，这 27 处一行
不动，两个 `forbidden` crate 照样每次拿到一个真实的驱动连接。** 原文把 `open_conn` 当作
"初稿漏掉的第四、五项"补进清单，没有看出它对被禁止的两个 crate 才是承重的那一扇。

验收标准第 1 条"持久化层之外零处驱动连接获取"因此**无法由需求 1–2 达成**。

### 更正 4：第三扇门——`orchestrator-security` 的公开 API 逼迫调用方持有连接

`crates/orchestrator-security` 有 **9 个 `pub fn (conn: &Connection, …)`**
（`secret_key_lifecycle.rs` 7、`secret_key_audit.rs` 2）。`daemon/src/server/secret.rs` 那
4 处 `open_conn` 存在的**唯一原因**就是要造一个 `&Connection` 递给它们（`begin_rotation`、
`complete_rotation`、`resume_rotation`、`bootstrap_key`、`revoke_key`、
`query_key_audit_events`…）。

DD-147 把 security 判为 `exempt`，理由是"它在 core 之下，自开连接"。**但它的 API 形状把
驱动推给了上面的调用方**：豁免的那个 crate 正是被禁止的那个 crate 必须持有驱动的原因。
原文需求 5 说 security"不受本 FR 影响"——恰恰相反，它是第 1 条验收标准的直接阻塞项。

### 更正 5：`fn other` 是 4 份，不是 6 份，其中 2 份不会"随迁移消失"

`core/src/attention.rs`、`core/src/process_metrics.rs`（层外，会消失）；
`crates/orchestrator-persistence/src/{source_automation_routes,trigger_state}.rs`
（**层内**，是拥有驱动的那个 crate 的内部助手，本 FR 不会也不该让它们消失）。
原验收标准"六份重复的 `fn other` 已随迁移消失"两处皆误。

### 更正 6：需求 4 的括号注记已过期

"（在 FR-137 闭环之前，没有任何东西替你守这一步）"——FR-137 已闭环（DD-149），
`check_continue_on_error_aggregated` 已在 `ALL_CHECKS` 中注册。忘记往 `OUTCOMES` 加行现在
**会让门禁失败**，不再需要靠记性。

### 更正 7（附带）：本 FR 是 DD-147 冻结残量的偿付方，而原文没说

DD-147 记 daemon 22 / scheduler 17 驱动引用为 `residualDeclaration: true`，并写明
"The flag comes off when the residual reaches zero, **and the declaration itself then starts
failing**"。它把这笔账记在 FR-130 Phase B 名下，而 Phase B 已闭环且没做。本 FR 是唯一能
清零它的改动，因此闭环时必须一并：

- 从 `crates/daemon/Cargo.toml` 与 `crates/orchestrator-scheduler/Cargo.toml` 删除
  `rusqlite` / `tokio-rusqlite`（否则条件 1 转为失败）；
- `persistence-dependency-ledger.json` 移动约 39 处驱动引用与约 42 条 SQL，两个
  `residualDeclaration` 翻转，角色由 `forbidden` 改为 `none`。

原文的非目标"不改变任何 SQL 语句"与此相容（语句逐字搬家，不改写），但原文对这笔台账变动
只字未提，而它是本 FR 最大的一次 ledger diff。

---

## 实施进度（2026-07-27）

**已完成并各自可回退**（提交见 `git log --grep FR-141`）：

| 批次 | 内容 | 结果 |
|---|---|---|
| 门禁 | `scripts/qa/persistence-api-boundary.rb` + 11 条 fixture，注册进 `qa-gate-surface.json` 与 `governance` job | 三类事实各自冻结在 `config/governance/persistence-api-boundary-ledger.json` |
| B1 | `orchestrator-security` 的 11 个索取连接的 `pub fn` → `SecretStoreSession` 不透明句柄 | 索取项 79 → 68；daemon 的 4 处 `open_conn` 消失；补上轮换可恢复性断言（变异实测会失败） |
| B2 | daemon 的 22 处驱动引用 / 19 条 SQL 全部迁入持久化层 | 层外获取 76 → 52 |
| B3 | scheduler 的 17 处 / 16 条全部迁入，含 DD-147 点名的 `task_state.rs`；其自身 `pub fn create_dynamic_task_items(&Connection)` 一并消失 | 层外获取 52 → 38 |
| B4a | `attention.rs`(14/34)、`process_metrics.rs`(6/27) 整体下沉，另六个小调用点 | core 驱动引用 8 → 2；层外获取 38 → 9 |
| B4b | `persistence/repository/config.rs` 的 16 条语句下沉为 `config_store`，事务由 `ConfigTx` 不透明句柄表达 | **core 驱动引用 = 0；层外连接获取 = 0** |
| 附带 | 两个兄弟门禁中五条写死文件名的 fixture 已重定向并改为从台账取计数 | 本 FR 把它们的目标搬空了，它们此前是 abort 而非失败 |

**核心目标已达成**：`config/governance/persistence-api-boundary-ledger.json` 的
`totals.acquisitions` 为 **0**，`core-boundary-ledger.json` 的 `rusqlite.total` 为 **0**，
DD-147 冻结的 daemon(22/19) 与 scheduler(17/16) 两笔残量均已清零。
`schema-snapshot.sql` 全程逐字节未变；2726 个测试全绿；strict clippy 干净。

**剩余（B5 与闭环产物）**：

1. **可见性下沉**：6 个 `yields` 项（`writer`/`reader`/`flatten_err`/两个 `open_conn`/
   `TaskRepositoryConn`，以及门禁发现的 `struct Migration` 的 `pub up: fn(&Connection)` 字段）
   与 67 个 `demands` 项降为 `pub(crate)`。**阻塞面已实测**：层外仍有约 150 处 *测试* 代码
   直接使用它们（`core/src/task_repository/tests/*` 81 处、scheduler 与 daemon 的
   `cfg(test)` 模块、`crates/integration-tests/tests/trigger_fire.rs` 5 处）。这些测试测的是
   已经搬走的语句，应随之迁入持久化 crate——与 B4a 对 `attention`/`process_metrics` 测试
   所做的一致。这是一个独立批次的工作量，不是收尾。
2. `crates/daemon` 与 `crates/orchestrator-scheduler` 的 `Cargo.toml` 删除
   `rusqlite`/`tokio-rusqlite`（生产残量已为 0，仅 `cfg(test)` 仍在用，须先随第 1 项迁走）；
   两个 `residualDeclaration` 翻转。
3. DD-151、QA-189、CHANGELOG、`docs/feature_request/README.md` 闭环注记、
   `doc-lifecycle-index.json` 重生成、删除本文件、§4.6 认证运行。

在第 1 项完成之前，验收标准第 1 条只在**行为**上为真（层外拿不到连接，因为没有任何生产
代码再去拿），在**类型**上尚未为真（它们仍是 `pub`）。这个区别必须写进 DD-151：一道断言
"没有人这么做"的门禁，和一道断言"没有人能这么做"的编译器，不是同一件事。


## 背景

`crates/orchestrator-persistence/src/async_database.rs` 的两个方法把驱动的连接类型放在
公开 API 上：

```rust
pub fn writer(&self) -> &tokio_rusqlite::Connection   // :60
pub fn reader(&self) -> &tokio_rusqlite::Connection   // :65
```

**持久化层之外有 54 处调用**（core 22、daemon 21、scheduler 11），层内另有 145 处；
`crates/integration-tests` 另有 5 处测试断言（`test-only` 角色，合法但会随之失效）。
拿到连接之后，`conn.execute(sql, [])` 不需要在任何地方写出 `rusqlite::` 路径。

这不是新发现。DD-147 在 FR-136 时就点名了它，并把它作为**门禁必须有第二个条件**的全部理由：

> `AsyncDatabase::writer()` 和 `reader()` 返回 `&tokio_rusqlite::Connection`……
> 所以"一个 crate 提到 `rusqlite` 多少次"测的是与问题相邻的东西。

`crates/orchestrator-security/src/secret_store_crypto.rs` **0 处驱动引用、4 条生产 SQL**，
就是这个 API 造成的形态。条件 2（逐文件冻结 SQL 语句数）是为了抓住它的**后果**而存在的。

### 为什么它现在无人认领

FR-130 Phase B 在 2026-07-26 明确拒绝了一个"给 `AsyncDatabase` 加接受 `anyhow::Result`
闭包的方法、删掉重复的 `fn other`"的改动，理由是它能在不搬动任何一条 SQL 的前提下把
Phase B 的 83 处收敛掉约 39——**推数字，而不是处置文件**。

**那个拒绝是对的。** 但它把两件事打包成了一件：

| 理由 | 判断 |
|---|---|
| 把 83 收敛掉 39 | 推棘轮，应当拒绝 |
| 持久化层不该把驱动的连接类型放在公开 API 上 | 架构边界，从未被单独判断过 |

按第二个理由，验收判据完全不同：它问的是**层外还能不能拿到一个驱动连接**，而不是层外
提到驱动多少次。FR-130 拒绝它之后，它掉出了所有 phase 与所有 FR 的视野。本 FR 是把它接
回来，并把判据写在能力边界上而不是写在棘轮上。

### 两个台账都数不到它

`crates/daemon` 与 `crates/orchestrator-scheduler` 在 DD-147 中是 `forbidden` 角色，残量
冻结在 22 / 17 处驱动引用。它们同时有 **32 处 `.writer()` / `.reader()` 调用，其中只有 4 行
同时带着 `rusqlite` 词元**——**其余 28 处两个台账都数不到**；另有 10 处 `open_conn(path)`
连一个 `AsyncDatabase` 都不经过。

也就是说：被禁止直接持有驱动的两个 crate，正通过一组不被任何棘轮计数、也不被任何台账冻结的
API，每次都拿到一个真实的驱动连接。条件 2 能抓住它们**用它做了什么**（SQL 语句数），
没有任何东西冻结**它们能拿到它**这件事本身。

## 目标

- 让 `orchestrator-persistence` 的公开 API 不交出连接能力，调用方拿到的是行为而非连接。
- 让 DD-147 条件 2 的存在理由可以被重新评估——它是为补偿这个泄漏而设的。

## 边界决策：三扇门全关（2026-07-27 裁决）

三扇门是串联的，依赖方向由下往上：

**门 3**（`orchestrator-security` 的 `&Connection` API）→ **门 2**（`open_conn(path)`）
→ **门 1**（`writer()` / `reader()`）

门 3 不动，daemon 就必须能 `open_conn`；门 2 不关，门 1 关了等于没关。**已定：三扇门全关。**

被否决的替代：

- **只关门 1（原文的字面范围）**——新门禁会在治理规程 §4.4 的反问上失败：
  "这条断言还会在什么坏状态上通过？" 答案是**"今天这个状态"**（27 处 `open_conn` 一行未动，
  daemon 与 scheduler 照旧每次拿到真实驱动连接，只是换了个函数名拿）。一道认证了自己
  观察不到的强制的门禁，比没有门禁更坏。
- **门 1+2，门 3 立后继 FR**——daemon 仍须为 security 造一条连接，`open_conn` 因而无法真正
  降为 `pub(crate)`，验收标准第 1 条对 `secret.rs` 那条路径仍为假。

由此本 FR 的范围**明确大于原文**：它同时是 DD-147 冻结残量的偿付方，并改动一个被 DD-147
判为 `exempt` 的 crate 的公开 API。两处扩张都须在设计文档中作为对本 FR 非目标的书面背离
留档（DD-145 为 FR-134 的非目标做过同样的事，沿用该先例与写法）。

## 非目标

- **不**以棘轮收敛为目的，也**不**以棘轮收敛为验收判据。本 FR 落地后 core 的 `rusqlite`
  计数会作为**副作用**下降，该下降**不得**被读作 FR-130 Phase B 的进度。两者的判据不同：
  Phase B 是逐文件处置，本 FR 是能力边界。台账届时须一次重生成并在评审说明中写清归因。
- **不**单独删除重复的 `fn other` 助手。层外那两份随调用点迁移自然消失；层内那两份是持久化
  crate 自己的内部助手，不在范围内。
- **不**改变任何 SQL 语句、任何 schema、任何 gRPC/CLI 契约。语句逐字搬家，不改写。
- ~~**不**与 FR-130 Phase B 并行~~——Phase B 已于 2026-07-27 闭环，前置条件已满足。

## 需求

### 1. 不交出连接能力

- `AsyncDatabase` 提供以闭包或具名方法表达一次数据库交互的 API，调用方不再接触
  `tokio_rusqlite::Connection`。
- `writer()` / `reader()` / `flatten_err` 从公开 API 移除（或降为 crate 私有）。
  **这一步是本 FR 的实质**——只增加新方法而保留旧方法，等于没有边界。
- `db::open_conn` 与 `sqlite::open_conn`（各返回 `rusqlite::Connection`，两个模块都是
  `pub mod`）一并降为 crate 私有。**它们不是清单末尾的第四、五项，而是门 2 本身**：
  27 处层外生产调用点全部经由它们，不经过 `AsyncDatabase`。
- `pub type TaskRepositoryConn = Connection` 同属公开面上的驱动类型，一并处置。
- **处置对象由需求 4 的解析器给出，不由这里的清单给出。** 上面各项是当前已知值，不是范围
  定义；**枚举式清单只守得住写它时已知的东西**，所以实施顺序是先建需求 4 的断言、由它列出
  全集，再按全集处置。

### 2. `orchestrator-security` 不再要求调用方持有连接（门 3）

- 9 个 `pub fn (conn: &Connection, …)` 改为经由一个自持连接的不透明句柄
  （工作名 `SecretStoreSession`）表达。
- 必须**保留调用方控制的事务范围**：`begin_rotation` → `re_encrypt_all_secrets` →
  `complete_rotation` 今天靠调用方持有的同一条连接保证原子性，句柄化不得把它拆成三条独立
  连接。这是 DD-147 `transaction-boundary` 类别（今天 0 个文件）的第一个成员。
- security 仍是 `exempt`：它在 core 之下，自开连接是它的既定形态。改的是它**要求别人持有
  连接**这一点，不是它自己持有连接这一点。

### 3. 调用点迁移，按 crate 分批

- 54 处 `writer/reader` + 27 处 `open_conn` 分批迁移，顺序由依赖方向决定：先 security（门 3），
  再下游消费者（daemon、scheduler），再 core，最后一批才删旧 API。
- 每批一次提交、一次可回退证据，与 FR-130 Phase A/B 同一标准。不接受一次性大提交。
- **逐个 commit 具名回退，不用 range**——FR-130 Phase A 的第一次证明用了 range，把中间一个
  无关提交也回退了，回退了 45 个路径而非 44。
- `crates/integration-tests` 的 5 处 `.reader()` 断言改走公开读 API，或由本 FR 明确给出
  test-only 出口并说明理由。

### 4. 行为等价证明

- `config/governance/schema-snapshot.sql` 逐批不变。
- **Phase C 钉住的错误 category 保证必须存活。** `phase_c_preserves_the_external_dependency_category`
  以真实未迁移库产出的真实错误断言驱动错误归类为 `ExternalDependency`，而该 category 在
  gRPC/CLI 契约上。闭包 API 会重写错误传播路径，这是最容易在无编译错误的情况下改掉它的
  改动形态——FR-130 B1 已经踩过一次（`classify_resource_error` 把
  `no such table: resources` 读成 `NotFound`）。
- **密钥轮换的原子性必须有断言。** 门 3 改动的是一条今天靠"调用方持有同一条连接"隐式保证
  原子性的路径，而这个保证没有任何测试钉住它。须补一条：轮换中途失败后，密钥表不得停在
  半完成状态。
- `cargo test --workspace`、strict Clippy、既有集成测试在不修改断言的前提下全绿。

### 5. 公开面断言

- 新增检查：`orchestrator-persistence` 的公开 API 不得交出驱动连接能力。**由解析签名得出，
  不是 grep 文件**——一个 `pub fn` 返回类型里的驱动类型与一条文档注释里提到它，是两件事。
- 三类事实各自单独报告，互不替代：
  1. **交出能力**：任何公开项的**返回位置**出现驱动连接类型；
  2. **索取能力**：任何公开项的**参数位置**出现驱动类型；
  3. **层外持有**：层外任何 crate 的生产源码里出现连接获取调用——**由 `[workspace] members`
     发现根，不写死 crate 名**。
- 该检查进 `ALL_CHECKS` 或等价注册表，从而受 FR-129 的两条 meta 断言约束。
- 按 FR-127 的分类进入 CI 强制执行面，并往 `governance` job 的 `OUTCOMES` 加行——此步现由
  FR-137 的 `check_continue_on_error_aggregated` 强制，忘了会失败。

### 6. 重新评估 DD-147 的条件 2

- 泄漏关闭后，条件 2（逐文件冻结 SQL 语句数）是否仍需保持当前形态，是一次**决策**，
  不是自动结论。写下判断与理由。
- 若判断为可收窄，须同时说明 `orchestrator-security`（`exempt`，自开连接）与
  `slack-gateway`（`separate-database`）不受本 FR 影响，因此条件 2 对它们仍然必要。

## 验收标准

- [ ] `writer()` / `reader()` / `flatten_err` / 两处 `open_conn` / `TaskRepositoryConn`
      不再是公开 API；持久化层之外零处驱动连接获取（三扇门全关）
- [ ] `orchestrator-security` 的 9 个 `pub fn (conn: &Connection, …)` 不再要求调用方持有连接，
      且轮换原子性有断言
- [ ] 54 处 `writer/reader` + 27 处 `open_conn` 全部迁移，按 crate 分批，每批独立可回退
- [ ] 每批之后 `schema-snapshot.sql` 未变
- [ ] `phase_c_preserves_the_external_dependency_category` 未被修改且通过；负向变异
      （改回按消息分类）仍以 `left: NotFound, right: ExternalDependency` 失败
- [ ] 公开面断言存在，由解析签名得出，负向 fixture 至少覆盖：返回类型里放回驱动类型 → 失败；
      仅文档注释里提到 → 通过；仅 SQL 字面量里出现 → 通过；`use … as` 改名后放进签名 → 失败；
      签名跨行 → 失败
- [ ] 新 check 已注册并受 meta 断言约束；`governance` job 的 `OUTCOMES` 已加行
- [ ] `crates/daemon` 与 `crates/orchestrator-scheduler` 的 `Cargo.toml` 已删除
      `rusqlite` / `tokio-rusqlite`，两个 `residualDeclaration` 已翻转
- [ ] 层外两份 `fn other` 已随迁移消失；层内两份保留并记录理由
- [ ] 条件 2 的去留已书面决策
- [ ] FR-130 Phase B 移交的"三个保留文件的 SQL 护栏从未被审计"已在搬迁时逐条完成并留档
- [ ] `cargo test --workspace`、strict Clippy、全部既有 CI job 状态不因本 FR 变化
- [ ] 台账重生成的评审说明写明：core 计数的下降归因于本 FR 的 API 迁移，**不是** Phase B 进度

## QA 计划

- **行为证据先于结构证据**。"驱动类型不在公开 API 上"是结构性的，它本身不证明数据仍被
  正确读写。每批之后须有一次真实写入-读回穿过被改动的路径，形态沿用
  `crates/orchestrator-persistence/tests/round_trip.rs` 及其未迁移库的反向半边。
- **错误 category 与轮换原子性是本 FR 最大的两个静默风险**，因为它们都不产生编译错误而都在
  对外契约上。每批之后跑 Phase C 那条测试，并至少一次以变异确认它仍会失败。
- **公开面断言必须解析签名**。grep `rusqlite` 会被文档注释满足，那正是 FR-134 反复消灭的
  "文本存在性当作事实"。正反 fixture 缺一不可。
- **分批可回退**：每批提取 commit 各自可机械回退，逐个具名，不用 range。
- **不需要新的 CI job**。

## 与 FR-130 的关系

独立议题，**不得并入 FR-130 Phase B**。Phase B 的完成判据是 18 个文件各有一条书面处置
（迁出 / 拆分 / 保留并记录理由），不是引用计数归零。Phase B 已于 2026-07-27 闭环，其移交给
本 FR 的三件事中，触发历史上限已由 FR-142 处置，余下两件——`persistence/repository/config.rs`
的残余，以及三个保留文件的 SQL 护栏审计——在本 FR 的 B4 批次内交付。
