# FR-139: 持久化收口门禁的扫描面与断言有效性

## 优先级: P2

## 状态: Proposed

## 背景

FR-136 建立的 `scripts/qa/persistence-dependency.rb` 以两条互不替代的条件执行收口决策：谁可以
**声明**驱动（由 `[workspace] members` 发现、逐 section 解析 manifest），谁可以**使用**驱动
（按文件冻结 SQL 语句数与驱动引用数）。第二条的必要性由一个真实反例支撑——
`crates/orchestrator-security/src/secret_store_crypto.rs` 有 4 条生产 SQL、0 处 `rusqlite`
引用，只查 manifest 的门禁会报它干净。该设计经变异复核成立：`none` 角色的 crate 新增带驱动
的生产文件、`cli` 在 `[dependencies]` 声明驱动、`integration-tests` 把驱动从 dev 挪到生产，
三者均被精确拦截并给出可执行的诊断。

但闭环后审计发现三处问题：一条断言无论输入如何都不会失败、一个真实 SQL 动词不在计数集内、
以及扫描面小于 scope 散文所声称的范围。

**本 FR 排在 FR-130 之前，理由不是"它阻塞 Phase A"**——Phase A 的成功判据是 core 的
`rusqlite` token 收敛，由 `core-boundary.rb` 产出，与本文的任何一条都不相干。理由是**冻结
时机**：Phase A 会把约 115 处引用、23 个文件从 core（该门禁排除）搬进新 crate（该门禁扫描），
持久化台账因此要重生成并由人评审 `role` 与 23 条 `category`。若此时动词集仍缺 `PRAGMA`，
那批数字从诞生起就是错的，修好之后还要再评审一次。**先修尺子，只评审一次。**

core 现有 4 条 `PRAGMA` 与 1 条 `VACUUM` 就在 Phase A 的搬迁范围内
（`async_database.rs:100,110`、`db.rs:451`、`persistence/migration_steps.rs:8`、
`db_maintenance.rs:32`），它们会随 Phase A 进入新 crate 的台账条目。

### 缺陷 A：分类总和断言是自我比较，任何输入都无法使其失败

`classification_errors` 有两个分支。第一个断言每个被扫到的文件都有评审过的 `category`，有效。
第二个：

```ruby
classified = snapshot["references"].values.sum { |entry| entry["rusqlite"] }
unless classified == snapshot["totals"]["rusqlite"]
```

而 `snapshot["totals"]["rusqlite"]` 的定义是：

```ruby
"rusqlite" => references.values.sum { |entry| entry["rusqlite"] }
```

`references` 与 `snapshot["references"]` 是同一个 hash，两侧是**同一个归约**，中间没有任何
改写。该分支不可达。

实证（`git archive HEAD` 副本，只停掉 `unclassified` 分支与引用冻结，让总和分支单独发言，
再放入一个无分类、含 1 处驱动引用的文件）：

```
Persistence dependency: PASS
  56 driver reference(s) and 113 SQL statement(s) across 17 file(s) outside core
```

它把该文件计入了总数，然后与自己相等。

**覆盖面实际没有漏**——`reference_errors` 的精确相等会报出"文件不在台账中"。问题在于
DD-147 的 Known Limits 把这条不可达断言当作现行保证陈述：

> The gate asserts that every scanned file *has* one **and that the categorised references
> sum to the scan**, so a file cannot arrive unclassified

前半句成立，后半句是装饰。设计记录声称了一项代码不提供的执行力。

### 缺陷 B：`PRAGMA` 不在 SQL 动词集内

`SQL_STATEMENT` 的动词集为 `SELECT|INSERT|UPDATE|DELETE|CREATE TABLE|CREATE INDEX|DROP|ALTER|REPLACE INTO`。
大写且锚定于字符串起始引号——这个窄口径是**对的**，其注释记录的教训（大小写不敏感会把英文
散文里的 update/create/delete 读成 SQL，首版因此在 daemon 数出 26 条而实为 19）成立，我在
审计中复算确认：放宽到大小写不敏感会在 `crates/cli/src/commands/guide.rs` 多出 20 条帮助
文案；`VACUUM` 在 `daemon/src/server/system.rs:140` 与 `integration-tests/src/lib.rs:1600`
命中的都是日志字符串。

缺的只是一个真动词。仅补 `PRAGMA`（同样大写、同样锚定）后：

```
gate today = 112   with PRAGMA added = 114   delta = 2
  +1  crates/orchestrator-security/src/lib.rs      conn.execute_batch("PRAGMA foreign_keys = ON;")
  +1  crates/slack-gateway/src/store.rs            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; ..."
```

零误报。其中 `orchestrator-security/src/lib.rs:104` 尤其切题：那是 `exempt` crate 借连接
执行 SQL，正是条件 2 存在的形态，而它的台账条目现记 `sql: 1`，实为 2。

同一形状的**潜伏**逃逸：字面量以转义换行开头（`"\n            SELECT ..."`）时，`"\s*` 无法
跨过源码中的 `\` `n` 两个字符。当前树上此形态 **0** 例，故不构成少计，但它是零成本的绕过。

### 缺陷 C：扫描面小于 scope 散文所声称的范围

`member_references` 只读 `<member>/src`：

```ruby
files = rust_files_under(repo_root, roots.map { |member| repo_root.join(member, "src") })
```

而 SCOPE 散文写的是"its non-test Rust source"。两处后果：

1. **`build.rs` 从不被读。** 工作区有 5 个 member 构建脚本——`cli`、`daemon`、`gui`、
   `orchestrator-scheduler`、`proto`，其中 `daemon` 与 `orchestrator-scheduler` 正是两个
   `forbidden` crate。同时条件 1 的第 130 行把 `[build-dependencies]` 归入**生产**声明：

   ```ruby
   when "dependencies", "build-dependencies" then found["dependencies"] << match[1]
   ```

   于是门禁治理一类它永远看不见的驱动用法。五个构建脚本当前都无驱动无 SQL，属潜伏。

2. **`src/` 下文件名匹配 `test*.rs` 的文件被整体排除**（`rust_source.rb:56`）。
   `crates/orchestrator-runner/src/test_env.rs` 在 `lib.rs:23` 是 `pub(crate) mod test_env;`
   ——无条件编译进生产，条件 2 看不见它（当前无驱动无 SQL）。该排除对 core 是正确的
   （命中的都是 `task_repository/tests/*` 之类真测试），但它按**文件名**而非按 `cfg(test)`
   判定，因此以文件名伪装的生产模块可以绕过。

此外，第 316 行的 scope 检查是：

```ruby
if expected["scope"] != SCOPE
```

它比对台账里的字符串副本与代码里的常量——**散文对散文**。它能发现台账没跟着常量更新，
但无法发现常量本身描述的不是扫描所做的事，而这正是本缺陷的形态。

## 目标

- 让每一条断言要么有真实对照物，要么不再被设计记录当作保证陈述。
- 让扫描面与 scope 散文一致，或让散文如实收窄。
- 在 FR-130 Phase A 重生成持久化台账**之前**修好动词集，使那批数字只需评审一次。

## 非目标

- **不**放宽 SQL 匹配到大小写不敏感或更宽的动词集。窄口径是经实测的正确选择，本 FR 只补
  `PRAGMA` 这一个有真实命中的动词。`VACUUM`、`BEGIN`、`COMMIT`、`WITH` 在当前树上的命中
  全为散文或无命中，加入即引入误报。
- **不**改变收口决策本身、`role` 划分或任何 crate 的豁免理由。DD-147 的决策不在本 FR 范围内。
- **不**要求 `exempt` 与 `separate-database` 的残量归零。那是决策，不是遗漏。
- **不**验证 `category` 的正确性。DD-147 已声明其正确性依赖评审，本 FR 不改变该结论——
  只纠正它引用的那条不可达断言。

## 需求

### 1. 分类断言：给它对照物，或删除并改写记录

- 总和分支须要么获得一个**独立于扫描输出**的对照物（例如：台账中已评审的 `references` 条目
  集合与本次扫描结果的差集为空——但这与 `reference_errors` 重复，实现方应判断是否值得保留），
  要么直接删除。
- 无论选哪条，**DD-147 的 Known Limits 必须同步改写**，不得继续把它列为现行保证。
- 若保留某种形式的总和检查，须附一条使其失败的负向 fixture。**一条无法失败的断言不得进入
  仓库**——这正是本缺陷本身。

### 2. SQL 动词集补 `PRAGMA`

- 仅新增 `PRAGMA`，保持大写与起始引号锚定。
- 处理转义换行开头的字面量（`"\n   SELECT`）。当前 0 例，故这是**先于发作**的处置，不是修复
  已发生的少计。
- 台账 `references` 随之重生成：`crates/orchestrator-security/src/lib.rs` 与
  `crates/slack-gateway/src/store.rs` 各 +1，总数 112 → 114。**除这两处外不得有任何变化**
  ——这是"只修了缺陷、没换口径"的双向判据，与 FR-134 需求 9 用 `200/37` 不变来验证词法器
  修复是同一条方法。

### 3. 扫描面与 scope 散文一致

- `build.rs` 与 `<member>` 下 `src/` 之外的其他生产 Rust 源须纳入扫描；或者，若判定不值得，
  须**收窄 SCOPE 散文**并在 DD-147 记录，同时说明为何 `[build-dependencies]` 仍按生产声明
  处理（两者不一致是当前状态，任何一个方向都可以，不一致不可以）。
- `test*.rs` 的按文件名排除须记录在 DD-147 的 Known Limits 中，并说明以文件名伪装生产模块
  可绕过。`crates/orchestrator-runner/src/test_env.rs` 是当前唯一的活例，须点名。
- **scope 检查须对行为负责**：至少断言扫描实际读取的根与散文描述一致（例如由同一常量派生
  出扫描根，使二者不能各自漂移），或明确降级为"台账副本与常量一致"并在记录中说明它不校验
  常量的真实性。

## 验收标准

- [ ] 负向 fixture：`PRAGMA` 语句被计入；仅补 `PRAGMA` 后台账总数 112 → 114，且**仅**
      `orchestrator-security/src/lib.rs` 与 `slack-gateway/src/store.rs` 各 +1
- [ ] 负向 fixture：以转义换行开头的 SQL 字面量被计入
- [ ] 负向 fixture：`VACUUM complete: {}` 一类日志字符串**不**被计入（防止修复走向放宽）
- [ ] 需求 1 的处置已落地；若保留总和检查，存在一条使其失败的 fixture；若删除，DD-147 已改写
- [ ] 负向 fixture：`forbidden` crate 的 `build.rs` 使用驱动 → 检查失败（或 SCOPE 已收窄且
      DD-147 记录了该缺口与 `[build-dependencies]` 归类的不一致处置）
- [ ] `crates/orchestrator-runner/src/test_env.rs` 的形态已在 DD-147 记录
- [ ] fixture 遵循既有隔离约定——每条只打中目标断言，其余在同一棵树上仍通过
- [ ] `test-persistence-dependency.sh` 全绿（当前 12 例），既有 12 例的语义未被削弱
- [ ] `core-boundary.rb` 仍为 `200 / 37` 与 `52 / 924 / 143`，`coordination-governance` 的
      `53 / 30 / 9 / 0` 不变——本 FR 不碰共享扫描器的口径
- [ ] 全部既有门禁与 CI job 状态不因本 FR 变化

## QA 计划

- **双向判据是主证据**：需求 2 的正确性不由"门禁通过"证明，而由"恰好 +2、且恰好是这两个
  文件"证明。任何其他数字都说明改的是口径而非缺陷。
- **误报方向必须有 fixture**。本缺陷的诱人修法是放宽匹配，而放宽会把 `guide.rs` 的 20 条
  帮助文案读成 SQL。日志字符串不得被计入这一条与 `PRAGMA` 必须被计入同等重要。
- **不可失败的断言不得留下**：需求 1 的验收方式是"存在一条使它失败的输入"。找不到这样的
  输入，就是它应当被删除的证明。
- **隔离断言**：沿用 FR-127 建立的约定。
- **不需要 CI 实证**：本 FR 不改变任何 job 的运行结果，证据完全在 fixture 与台账 diff 内。

## 与 FR-130 的关系

Phase A 会使持久化台账新增约 23 个文件条目并由人评审。本 FR 须在那次重生成**之前**闭环，
使那批 `sql` 计数一次成型。Phase A 之后再修，等于把同一批条目评审两次。

Phase B 才是这些计数真正承载判断的地方——其逐文件处置结论是"SQL 迁出 / 领域逻辑留下 /
保留并记录理由"，证据即 per-file `sql` 计数。一个被判定"SQL 已迁出"却还留着 `PRAGMA` 的
文件，在缺陷 B 未修时会被读作干净。
