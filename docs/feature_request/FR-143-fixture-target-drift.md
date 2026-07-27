# FR-143: 变异 fixture 的靶标漂移 — 第三条 meta 断言

## 优先级: P2

## 状态: Proposed

## 背景

FR-129 为门禁建立了两条 meta 断言：**每个 check 都在 `ALL_CHECKS` 注册表中**，**每个注册
check 至少有一条负向 fixture**。它们回答「这道检查存在吗」和「有人试图让它失败过吗」。

**没有一条回答「那次尝试真的施加了变异吗」。**

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
| 9 | `test-persistence-extraction.sh` case 5 | `git log --grep 'FR-130 A1'` 落空时走 `pass()` | **空 grep 计入通过数** |

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

### 2. 断言必须匹配门禁的**具体诊断**，而非仅匹配非零退出

- 第 5–7 例是这条的直接理由：三条 fixture 都得到了非零退出，但来自
  `+ file … is not in the ledger`（新增分支）而不是它们声称测试的
  `~ file … 1 → 0`（变更分支）。**只断言"失败了"无法区分这两者。**
- 既有 fixture 大多已经 `grep -q` 具体诊断串；须把它变成**要求**而非习惯。

### 3. 期望值必须从台账派生，不得在断言里重述

- 第 5–7 例硬编码了计数 1；第 1 例硬编码了 `52 → 53`。台账里已经有这些数字。
- FR-141 的重定向提交已经这样做了（"the expected count now comes from the ledger instead
  of being restated in the assertion"）——把它推广为规则。

### 4. 第三条 meta 断言

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

- [ ] 第三条 meta 断言存在，与 FR-129 的两条并列注册
- [ ] 负向 fixture：一条靶标已失效的 fixture → meta 断言失败并点名
- [ ] 全仓既有 fixture 已复核：靶标存在性有断言、诊断有匹配、期望值从台账派生；
      不满足的逐条修正或书面记录理由
- [ ] `test-core-boundary.sh`、`test-persistence-dependency.sh`、
      `test-persistence-extraction.sh` 三道门禁的 fixture 全绿且断言数不减少
- [ ] SKILL.md §4.4 已补记
- [ ] 设计记录列出九次实测
- [ ] 全部既有门禁与 CI job 状态不因本 FR 变化

## QA 计划

- **本 FR 最容易犯的错是让 meta 断言自己变成枚举。** 它必须发现 fixture，而不是持有一份
  fixture 清单——发现方式应与 FR-129 的既有两条一致（从注册表与函数名推导，不写清单）。
- **区分性对照是主证据**：meta 断言必须在「fixture 靶标有效」时通过、在「靶标失效」时失败。
  只做后者会让"任何 fixture 改动都报错"与"检测靶标漂移"有同样的绿记录。
- **不要求跑真实门禁两遍**。若代价过高，可接受更轻的形态（例如要求 fixture 显式声明并断言
  其前提），但**必须书面说明为何更轻的形态足够**，且该说明本身要有一条能证伪它的 fixture。
- **不需要新的 CI job**。若新增门禁，往 `governance` job 的 `OUTCOMES` 加行——FR-137 已闭环,
  忘记加会让 `check_continue_on_error_aggregated` 失败，不再靠记性。

## 附注：一个相邻的空白

实施本 FR 时会反复运行 Ruby 门禁。**macOS 出厂的是 Ruby 2.6**，而 `filter_map` 等
2.7+ 方法在本地开发机（多为 3.x）上不会暴露——FR-136 的 `stale_residual_errors` 就是这样
在提交前一刻被一次本地运行拦下的，而"本地跑一次"不是机制。

shell 侧有 FR-135 建立的 bash 3.2 门禁；**Ruby 侧没有对应物**，而仓库有 30 余个 Ruby 门禁
脚本。这不属于本 FR 的范围，记在这里是因为它是同一类空白，且下一次它不会被人眼拦下。
