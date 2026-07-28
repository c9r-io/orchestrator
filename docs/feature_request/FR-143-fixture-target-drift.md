# FR-143: 变异 fixture 的靶标漂移 — 第三条 meta 断言

## 优先级: P2

## 状态: Proposed

## 背景

FR-129 为门禁建立了两条 meta 断言：**每个 check 都在 `ALL_CHECKS` 注册表中**，**每个注册
check 至少有一条负向 fixture**。它们回答「这道检查存在吗」和「有人试图让它失败过吗」。

**没有一条回答「那次尝试真的施加了变异吗」。**

> **复核**：这两条断言不是一处而是**两处**，且都不在漂移发生的地方。FR-129 把它们建在
> `test-skill-mirror-integrity.sh:517–532`；FR-134（`445fa9ed`）在
> `test-qa-gate-surface.sh:1354–1388` **各自独立地**又建了一遍，那里还有第三条（`9fd60030`
> 加的「每条注册 check 都有描述并被验证模式运行」）。两处都以 `ALL_CHECKS`——一份 **check
> 函数**注册表——为键。
>
> **而本 FR 举证的三道门禁一道都没有注册表。** `test-core-boundary.sh`、
> `test-persistence-dependency.sh`、`test-persistence-extraction.sh` 都是线性的 `Case N`
> 脚本。见需求 4 的复核。

一条负向 fixture 通常写成：找到某个具体文件里的某条具体语句，改掉它，断言门禁报错。
**靶标是枚举出来的。** 当被治理的代码搬家——而搬家正是这些门禁存在的目的——靶标消失，
fixture 不再测任何东西，且**多数情况下不会失败**。

### 九次实测，跨三道门禁、四个 FR

| # | 位置 | 发生了什么 | 结果 |
|---|---|---|---|
| 1 | `test-core-boundary.sh` case 3 | 写死 `pubMod 52 → 53`；Phase A 把它变成 50 | **响亮失败**（良性方向） |
| 2 | `test-core-boundary.sh` case 5（FR-130 Phase A 后） | 从 `core/src/db.rs` 剥 rusqlite；该文件已成 re-export 壳，唯一 token 在 `mod tests` 里而扫描器不数 | 变异没变异任何东西，门禁正确报告"无变化"，**而 case 报告门禁没注意到删除** |
| 3 | `test-core-boundary.sh` case 5（FR-141 B4 后） | 从 `rusqlite.files.keys.min` 取删除目标；core 归零后该读取返回空 | **case 写到了一个目录** |
| 4 | `test-persistence-dependency.sh` case 8 | 中和 scheduler 里的 `SELECT COUNT(*) FROM command_runs`；B3 搬走了它 | 以 `no statement to neutralise` **中止** |
| 5–7 | `test-persistence-dependency.sh` case 12/13/14 | 探测 `crates/daemon/src/server/attention.rs`，硬编码计数 1；B2 把它整个搬出台账 | 三条都变异了一个门禁**首次见到**的文件，于是**经由错误的分支**报告 |
| 8 | `test-persistence-dependency.sh` case 16 | 剥掉一个已不在台账中的 daemon 文件的 category | 无效 |
| 9 | `test-persistence-extraction.sh` case 6 | `git log --grep 'FR-130 A1'` 落空时走 `pass()` | **空 grep 计入通过数** |

> **复核（治理期，基于 `9e250e7f`）**：九次实测全部经 git 历史逐条核对属实，证据分别在
> `75dcf68c`（第 1、2 例）、`e6b5fd70`（第 3–8 例）、`ef6f439f`（第 9 例）三个提交中。
> 「九次里只有第 1 次响亮失败」与三份 diff 一致。**第 9 例的位置原文写作 case 5，实为
> case 6**（基线排序断言；case 5 是错误转换探针），已改正。

九次里只有第 1 次响亮失败。其余八次要么中止、要么空转通过、要么经由错误的分支通过——
**都是绿的**。

第 2 次尤其值得记：它不只是失效，它还**报告了一个不存在的缺陷**（"门禁没注意到删除"），
把审计者的注意力引向门禁而不是引向 fixture。

### 这是 §4.4 shape 2，发生在 fixture 自己身上

`.claude/skills/fr-governance/SKILL.md` §4.4 记录的三类缺陷之一是「枚举式覆盖面只守得住写
它时已知的东西」。这一整轮治理（FR-127 至 FR-142）反复在**被检查的对象**上消灭它——白名单
改为 `git ls-files`、镜像根改为由 git index 推导、job 清单改为解析 workflow、阻塞表改为查
`pragma_foreign_key_list`。

**而检查它们的 fixture 自己仍然是枚举的。**

## 目标

- 让一条 fixture 无法在没有施加变异的情况下报告成功。
- 让一条 fixture 无法在门禁经由**另一条分支**报错时报告成功。

## 非目标

- **不**要求 fixture 停止指名具体文件。指名是可读性的来源，问题不在指名而在**指名之后不
  验证靶标还在**。
- **不**引入通用变异测试框架。本 FR 只加一条 meta 断言与一套约定，规模应与 FR-129 的前两条
  相当。
- **不**重写既有 fixture 的语义。已被 FR-141 重定向的五条已经修好，本 FR 是防止下一次。
- **不**扩展到 Rust 单元测试。本 FR 的对象是 `scripts/qa/test-*.sh` 里的门禁 fixture。

## 需求

### 1. 靶标存在性必须被断言，且缺失即失败

- 每条施加变异的 fixture，在变异之前必须断言其靶标存在；靶标缺失时必须 `fail`，
  **不得 `abort`、不得跳过、不得 `pass`**。
- 已有先例可直接沿用：`test-persistence-extraction.sh` 的 case 8 在变异前断言
  fixture 文件确实不可解析，case 15 断言构建脚本此前不含驱动 token——**两条都是"如果我的
  前提已经不成立，那是失败而不是跳过"**。约定存在，只是没有被强制。

> **复核**：这是四条需求里**唯一有大量待修存量**的一条，实测 **21 处**，分布在 7 道门禁：
> `test-agent-driver-production-parity.sh` 4、`test-persistence-dependency.sh` 8、
> `test-persistence-extraction.sh` 3、`test-doc-lifecycle.sh` 2、
> `test-governance-ledger-tooling.sh` 2、`test-core-boundary.sh` 1、
> `test-qa-gate-surface.sh` 1。每一处都是 fixture 的 `ruby -e` 体内的 `abort`/`raise`，
> 前提不成立时杀掉整轮而不是记一次 fail。
>
> 另有一类需求原文没有单列、但同属需求 1 的语义：**原地改写一个临时树文件而不证明改写落
> 地**，实测 **27 处，分布在 8 道门禁**——包括发明了落地证明（`inject`）的
> `test-qa-gate-surface.sh` 自己，以及上周才写的 `test-jq-status-observed.sh`。第 2 例
> 正是这一类。两类合并去重后共 **9 道门禁**。
>
> 这个门禁数我第一次写成了 11：那是从一张按门禁列出的表里数行数得来的，而表里有两行的计数
> 是 0。**用第二条路径（`grep -lE` 直接数文件，与词法器无关）重算才落到 9。** 站点数 21 与
> 27 两条路径一致。原样记下，因为这正是本 FR 的主题在治理它的人手上又发生了一次。

### 2. 断言必须匹配门禁的**具体诊断**，而非仅匹配非零退出

- 第 5–7 例是这条的直接理由：三条 fixture 都得到了非零退出，但来自
  `+ file … is not in the ledger`（新增分支）而不是它们声称测试的
  `~ file … 1 → 0`（变更分支）。**只断言"失败了"无法区分这两者。**
- 既有 fixture 大多已经 `grep -q` 具体诊断串；须把它变成**要求**而非习惯。

> **复核**：「大多」实为**全部**。在 27 道 ci-required shell 门禁上实测，以「非零退出码作为
> 唯一条件」报 `pass` 的断言有 **0 处**——每一条都已配了诊断匹配或第二个观测。所以本条不是
> 待修存量，而是**一条尚不存在的回归护栏**。这与 §4.4 的原话是同一句话的机器形式：
> 代理可以是附加条件，不可以是唯一条件。

### 3. 期望值必须从台账派生，不得在断言里重述

- 第 5–7 例硬编码了计数 1；第 1 例硬编码了 `52 → 53`。台账里已经有这些数字。
- FR-141 的重定向提交已经这样做了（"the expected count now comes from the ledger instead
  of being restated in the assertion"）——把它推广为规则。

> **复核**：与需求 2 同状态。实测 ci-required shell 门禁的期望诊断串中，**0 处**仍写着字面
> 的 `N -> M`——第 1 例与第 5–7 例都已被 `75dcf68c` 与 `e6b5fd70` 清掉。同样是护栏而非修复。

### 4. 第三条 meta 断言

> **复核（本 FR 最重要的一处偏差）**：「在既有 meta 断言旁并列注册」这个实现方式，对本 FR
> 自己的证据是错的。既有 meta 断言以 `ALL_CHECKS`（一份 check 函数注册表）为键，而
> **举证的三道门禁一道都没有注册表**。照字面并列注册，新断言会落在两道**从未漂移过**的门禁
> 里，而对三道**真的漂移了**的门禁一处都盖不到。
>
> 因此第三条 meta 断言必须是**一道扫描 fixture 脚本本身的门禁**，其覆盖面从
> `qa-gate-surface.json` 发现（`jq-status-observed.rb` 的先例），而不是两份注册表里的第四个
> 条目。这本身就是 §4.4 shape 2 又一次发生在写它的人手上。

- 在既有 meta 断言旁新增一条：**每条负向 fixture 必须证明它施加了变异**。
  最直接的实现是要求每条 fixture 在同一棵树上同时报告两件事——未变异时该 check 通过，
  变异后该 check 失败**且诊断匹配**——即把 FR-136 case 4 / FR-137 fixture 22b 那种
  「区分性对照」从个别 case 的良好习惯提升为普遍要求。
- 该 meta 断言本身须有一条负向 fixture：**造一条靶标已失效的 fixture，meta 断言必须报错。**
  否则本 FR 就在重复它要修的错误。

### 5. 记录

- `.claude/skills/fr-governance/SKILL.md` §4.4 补一条：枚举式覆盖面这条规则**同样适用于
  fixture 自己**，并给出「靶标缺失即失败」「断言诊断而非退出码」「期望值从台账派生」三条
  具体做法。
- 设计记录须列出上表九次实测，因为**这条规则的说服力全在于它已经发生过九次**。

## 验收标准

> **复核**：验收标准按上述四处复核改写如下。原第 1 条（「与 FR-129 的两条并列注册」）与第
> 4 条（只点名三道门禁）都以未经核对的范围为前提，照原样验收会让本 FR 在它自己举证的缺陷
> 上留下八道门禁。

- [ ] 第三条 meta 断言存在，形态为**一道扫描 fixture 脚本的门禁**，覆盖面从
      `qa-gate-surface.json` 发现而非枚举；已注册为 `ci-required` 并接入 `governance` job
      （含 `id:`、`continue-on-error:` 与 `OUTCOMES` 行）
- [ ] 该门禁定义的**每一条规则**都有一条能触发它的 fixture，且每条都配一条**不该触发**的
      对照——只做前者会让「任何 fixture 改动都报错」与「检测靶标漂移」有同样的绿记录
- [ ] 负向 fixture：复现第 2 例的形态（原地替换匹配不到任何东西），断言门禁报告
      **变异未落地**，而非像 `9ca1ea75` 那样报告被测门禁漏掉了一次删除
- [ ] 21 处 `abort`/`raise` 前提全部改为 `fail`；27 处原地改写全部经由落地证明
- [ ] 受影响的 9 道门禁全绿且**通过数不减少**（基线在各自转换提交前现测，不引用旧提交
      信息里的数字）
- [ ] 需求 2 与需求 3 的护栏存在且可被触发（当前存量为 0，故必须由 fixture 证明它会响）
- [ ] SKILL.md §4.4 已补记
- [ ] 设计记录列出九次实测，并记下三处复核偏差
- [ ] 全部既有门禁与 CI job 状态不因本 FR 变化

## QA 计划

- **本 FR 最容易犯的错是让 meta 断言自己变成枚举。** 它必须发现 fixture，而不是持有一份
  fixture 清单——发现方式应与 FR-129 的既有两条一致（从注册表与函数名推导，不写清单）。
- **区分性对照是主证据**：meta 断言必须在「fixture 靶标有效」时通过、在「靶标失效」时失败。
  只做后者会让"任何 fixture 改动都报错"与"检测靶标漂移"有同样的绿记录。
- **不要求跑真实门禁两遍**。若代价过高，可接受更轻的形态（例如要求 fixture 显式声明并断言
  其前提），但**必须书面说明为何更轻的形态足够**，且该说明本身要有一条能证伪它的 fixture。

> **复核（本条已作决定）**：采用更轻的形态。理由不是代价而是九次实测本身——fixture 报告而
> 未证明的四种方式各由一条规则封住：没变异→落地证明；前提消失→前提即失败；经由错误分支→
> 诊断匹配 + 期望值不得重述。
>
> **残留一并写明**：**变异之前门禁就已经在失败**的情形，四条规则全都满足。诊断匹配收窄了它
> ——一个无关的既有失败产生不出点名刚被变异对象的诊断——但没有封死。这正是
> `test-core-boundary.sh` case 9 与 `test-persistence-dependency.sh` case 10 各自带一次
> before-run 的原因，也是需求 2 的护栏写成「诊断匹配**或** before-run」而不是只写前者的原因。
> 能证伪这条说明的 fixture 即针对此形。
>
> 代价数字一并记下，但它不是论据：预算余量 **330s / 2700s（12%）**，而
> `test-persistence-extraction.sh` 一道就 200s（其 case 在 `git archive` 副本上跑
> `cargo check`），全面 before-run 仅它一道就会吃光全部余量。
- **不需要新的 CI job**。若新增门禁，往 `governance` job 的 `OUTCOMES` 加行——FR-137 已闭环,
  忘记加会让 `check_continue_on_error_aggregated` 失败，不再靠记性。

## 附注：一个相邻的空白

实施本 FR 时会反复运行 Ruby 门禁。**macOS 出厂的是 Ruby 2.6**，而 `filter_map` 等
2.7+ 方法在本地开发机（多为 3.x）上不会暴露——FR-136 的 `stale_residual_errors` 就是这样
在提交前一刻被一次本地运行拦下的，而"本地跑一次"不是机制。

shell 侧有 FR-135 建立的 bash 3.2 门禁；**Ruby 侧没有对应物**，而仓库有 30 余个 Ruby 门禁
脚本。这不属于本 FR 的范围，记在这里是因为它是同一类空白，且下一次它不会被人眼拦下。

> **复核**：「30 余个」实为 **16 个**被追踪的 `.rb` 文件，其中 15 个在 `scripts/` 下
> （`git ls-files '*.rb'`）。约 2 倍高估。空白本身属实，范围判断不变——但一个没人复核过的
> 数字出现在一份主题正是「没人复核过的靶标」的文档里，值得原样记下而不是悄悄改掉。
