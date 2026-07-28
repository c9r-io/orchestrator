# FR-133: 依赖策略门禁 — 重复版本、许可证与来源约束

## 优先级: P3

## 状态: Proposed

## 背景

依赖治理目前只有两条腿：`.github/workflows/security.yml` 的 `cargo audit`（已知漏洞）与 `dependabot.yml`（cargo + github-actions 每周更新，各 10 个 PR 上限）。二者覆盖"有没有已知 CVE"和"版本旧不旧"，但**不覆盖依赖图的形状**。

2026-07-25 实测：工作区共 443 个传递依赖，其中 **37 个 crate 存在多版本共存**：

```
hashbrown ×4    phf / phf_shared / getrandom ×3
indexmap 1.9 / 2.14      base64 0.21 / 0.22      bitflags 1.3 / 2.13
digest 0.10 / 0.11       sha2 / rand / syn / thiserror / toml ×2 …
```

> **复核（治理时现测，`1b5615e2`，cargo 1.96.0 / cargo-deny 0.20.2）**：上面两个数都能复现，
> 但都不是它们自称的东西。
>
> **443 复现得到，且它不是"传递依赖"。** `cargo tree --workspace --prefix none --locked | sort -u`
> 在 aarch64-apple-darwin 上正是 **443** 行——**14 个 workspace 成员全在这 443 之内**，所以宿主
> 图上的传递依赖是 **429**。第二条路径：`cargo metadata` 给出 **667** 个包 / **653** 个非成员包，
> 因为 `Cargo.lock` 覆盖全部目标平台——而**那**才是 `cargo deny` 实际评估的图。
>
> **37 也能由一条具名路径复现，但其中 12 个只有一个版本。** `cargo tree -d --workspace`（宿主）
> 给出 **37 个名字 / 85 条**。可是 `-d` 同样会报告一个**只解析出一个版本**、却因 normal 与
> build/proc-macro 特性集不同而在树里出现两次的 crate。这一类共 12 个：`serde`、`serde_core`、
> `serde_json`、`log`、`prost`、`time`、`typenum`、`semver`、`smallvec`、`deranged`、
> `stable_deref_trait`、`tauri-utils`——每个在 `Cargo.lock` 里都只有一条 `[[package]]`。在同一张
> 宿主图上按 name@version 去重来数，是 **25 个名字 / 30 个多余副本**。
>
> 这是 Phase 2 step 0 点名的**类别混淆**："多版本共存"与"在树里出现两次"是两个构造，而
> `cargo deny check bans` 数的是前者。
>
> **门禁真正会执行的数是 48，不是 37。** `cargo deny check bans` 配 `multiple-versions = "deny"`、
> `--workspace --all-features` → **48 条 `error[duplicate]`**；去掉 `--all-features` 结果相同。
> （`Cargo.lock` 里有 50 个重复名字，`bit-vec` 与 `schemars` 是解析图外的 lock 条目；`rand 0.10.2`
> 同理，所以 lock 说 `rand ×3` 而 cargo-deny 说 2。）按"37 个"去分类会留下 11 个以上未分类，门禁
> 第一次运行就失败——而且是**朝着看起来像完成的方向**失败。
>
> **真正的轴是 GUI：48 个里有 28 个只因为 `orchestrator-gui` 在 workspace 里才存在。**
> `cargo deny check bans --exclude orchestrator-gui` → **20**。这 20 个里又有 10 个是
> `windows-*` / `windows_*` 家族。daemon/CLI 图自己只背 **10 个**：`cpufeatures`、
> `crypto-common`、`getrandom`、`hashbrown`、`phf`、`phf_shared`、`r-efi`、`syn`、`thiserror`、
> `thiserror-impl`。接受理由应当沿这条轴写。

多版本共存本身不总是错误（生态过渡期不可避免），但当前状态是**无人知晓、无人决策**的：没有任何机制区分"已知且接受"与"意外引入"。对一个把 `cargo audit` 纳入 CI 的项目而言，这是安全治理链条上的一处不对称——漏洞被门禁，而依赖图的膨胀不被门禁。

同时缺失的还有许可证策略：项目已通过 FR-071 建立开源合规基础设施（LICENSE / CHANGELOG / CONTRIBUTING），但传递依赖的许可证兼容性从未被自动校验。

本项在深挖中优先级最低（P3）：它不影响正确性，也没有已知的实际损害；列为 FR 是为了让"未治理"这一事实本身可见，而非暗示紧急。

## 目标

- 引入 `cargo-deny`，为重复版本、许可证、来源建立显式策略。
- 让当前的 37 个重复项从"未知"变为"已审阅并接受（带理由）或已消除"。
- 策略以棘轮方式冻结：接受清单只能缩短，不能因为方便而增长。

## 非目标

- **不**要求消除全部重复版本——生态过渡期的重复（如 `bitflags` 1/2）强行统一可能需要 fork 上游，代价不成比例。
- **不**替换 `cargo audit`；`cargo-deny` 的 advisories 能力与之重叠，需书面决策保留其一还是并存，避免两套重复告警。
- **不**改变 dependabot 的更新节奏或上限。
- **不**在本 FR 内做依赖精简（减少 443 这个总数属独立议题）。

## 需求

### 1. cargo-deny 接入

- 新增 `deny.toml`，覆盖 `bans`（重复版本）、`licenses`、`sources` 三类检查。
- 接入 `security.yml`（或按 FR-127 的分类进入统一的门禁执行面），失败可阻断。

### 2. 重复版本审阅与接受清单

- 对 37 个重复 crate 逐个分类：可统一（升级/统一 workspace 依赖版本）、生态过渡期接受（带理由与预期消解条件）。
- 接受项进入 `deny.toml` 的 `skip` / `skip-tree` 并附注释说明理由与来源依赖。
- 未分类的重复项使门禁失败。

> **复核**：数目是 **48**（见背景处的复核），而**"可统一"这一类是空的**。
>
> 48 个里只有 4 个被任何 workspace 成员直接声明，且四个都轮不到我们统一：
>
> | crate | 我们声明 | 另一版本来自 |
> |---|---|---|
> | `base64` | 4 个 crate 里 `^0.22` | `0.21.7` ← `swift-rs` ← tauri |
> | `sha2` | 7 个 crate 里 `^0.11` | `0.10.9` ← GUI 子树 |
> | `reqwest` | 4 个 crate 里 `^0.12` | `0.13.4` ← GUI 子树 |
> | `rand` | 4 个 crate 里 `^0.8` | `0.9.5` ← `tauri-plugin-notification` |
>
> `rand` 看起来可统一，实际不行：`rand 0.8.7` 同时被
> `cron 0.16 → phf 0.11 → phf_macros → phf_generator 0.11.3` 钉住，所以把我们四处
> `rand = "0.8"` 升上去——9 个调用点、`0.8→0.9` 是破坏性 API 变更——会消掉**零个**重复项。
>
> **因此本 FR 不动任何生产依赖**：每一项都是接受项，不存在"先统一再接受"的分流。
>
> 另：**不使用 `skip-tree`**。一行 `skip-tree = [{ crate = "tauri" }]` 会一次吞掉 48 里的 28
> 个，并且会继续吞掉**尚不存在**的重复项，永远地、无声地。那是 §4.4 shape 2 的反面——不是一张
> 只守住写它那天已知内容的清单，而是一张什么都不守、却报告成功的毯子。48 条显式条目配 48 条
> 理由才是本 FR 的全部意义。

### 3. 许可证策略

- 声明允许的许可证集合与显式拒绝集合；对需要例外的依赖建立带理由的 exception 清单。
- 确认现有 443 个依赖全部落入允许集合或例外清单。

> **复核**：受检对象是 **653 个非成员包**（全平台解析图），不是 443（那是宿主图且含 14 个
> workspace 成员）。全部 653 个**都声明了 license**，零缺失，共 **34 种不同表达式**。
>
> 一份 14 条的 allow 清单可以放行其中除**一个**以外的全部：`target-lexicon 0.12.16` 的
> **`Apache-2.0 WITH LLVM-exception`**——它没有提供 `OR MIT`（另外 5 个带 LLVM-exception 的
> crate 都提供了）。它需要一条 exception。
>
> 需要**记录决策**而非现场发现的几类：MPL-2.0 ×5（`cssparser`、`cssparser-macros`、
> `dtoa-short`、`option-ext`、`selectors`；弱 copyleft、文件级，我们未修改）、
> CDLA-Permissive-2.0 ×2（`webpki-roots`，数据许可证）、CC0-1.0（`notify`）、
> Unicode-3.0 ×18（ICU）。
>
> 另：**需求 1 的第三类 `sources` 当前存量为 0**。`Cargo.lock` 里每一条 `source` 都是
> crates.io，`unknown-registry` / `unknown-git` 都设为 `deny` 时 `cargo deny check sources`
> 今天就退出 0。它是护栏而非修复——与 FR-143 的需求 2、3 同样的处境，也同样的后果：
> **它的 fixture 是它能工作的唯一证据**。

### 4. 接受清单棘轮

- 记录接受清单初始长度作为基线，门禁断言其单调不增。注意 FR-124/125 的 `sourceBaseline` 棘轮**已不再是单调口径**：FR-128 将其收紧为精确相等，因为单调规则下"下降静默通过"，而下降恰恰是台账唯一需要记录的事件。若本需求确实要单调不增，须写明为何此处与 `sourceBaseline` 取不同口径。
- 新引入的重复版本必须先消解或显式审阅，不能靠追加清单静默通过。

> **复核**：这个问题有比它给出的两个选项都好的答案，而且不需要新口径。
>
> DD-153 已经定下规则：**从树推导出来的量用精确相等，只有被测量出来的量才用阈值。** 接受清单
> 的长度两者都不是——它是一个已提交的文件，所以把基线数字放进**第二个**已提交文件，等于把同一
> 个事实抄两份、在同一个提交里一起改，而 code review 本来就看得见那个 diff。那样的棘轮是仪式。
>
> 真正有牙齿的是工具自己的诊断，已实测：
>
> - `cargo-deny check --deny unmatched-skip bans` →
>   `error[unmatched-skip]: skipped crate 'serde = =9.9.9' was not encountered` → `bans FAILED`
> - `[licenses] unused-allowed-license = "deny"` → `unmatched license exception` → `licenses FAILED`
>
> 于是：一个被接受的重复项在上游**被解决**之后，构建会一直失败到它的条目被删掉为止。清单不能
> 无声增长（新重复项直接报错），不能留着死条目（未匹配报错），也不需要第二本台账去漂移。
> **与 `sourceBaseline` 同口径——精确而非单调**，只是机制换成了工具自带的诊断。

### 5. advisories 职责划分

- 书面决策 `cargo audit` 与 `cargo-deny advisories` 的关系（保留其一或并存），并使 CI 配置与该决策一致，避免同一问题产生两处告警。

> **复核：这一条的前提是反的——两个工具谁也不包含谁。**
>
> | | `cargo audit` | `cargo deny check advisories`（`version = 2`） |
> |---|---|---|
> | 退出码 | **0** | **1** |
> | unmaintained | 17 条，作为**警告** | 17 条，作为**错误** |
> | unsound | **1 条** —— RUSTSEC-2024-0429 | **完全不报告** |
>
> 两者读的是同一个 RustSec 数据库（cargo-deny 现场拉取了自己那份到
> `~/.cargo/advisory-dbs/`）。所以"能力与之重叠…保留其一"不成立：**用 `cargo deny` 换掉
> `cargo audit` 会丢掉一条活的 unsoundness 发现**；而原样引入 `cargo deny check advisories`
> 会让 17 个上游已归档的传递依赖在第一天就把构建变红。
>
> RUSTSEC-2024-0429 是 `glib 0.18.5`：`VariantStrIter::impl_get` 把 `&p` 传给一个会通过该指针
> 回写的 C 函数，新版 rustc 在优化下直接忽略这些写入——UB，实际表现为
> `CStr::from_ptr(NULL)`。影响 `>=0.15.0,<0.20.0`，`>=0.20.0` 已修，只经由
> `orchestrator-gui → tauri 2.11 → gtk 0.18 → glib 0.18.5` 到达。我们修不了，得等 Tauri 动。
>
> **决策：一个问题一个工具，不重叠。** `cargo-deny` 管图的形状（`bans` / `licenses` /
> `sources`），CI 调用里精确列这三项，永不出现 `advisories` 或 `all`；`cargo audit` 管公告库，
> 并且收紧为 `cargo audit --deny unsound`，RUSTSEC-2024-0429 进入已提交的 `.cargo/audit.toml`
> ignore，带理由和退役条件。17 条 unmaintained 保持警告——它们是 Tauri 2 还在用 gtk-rs 0.18
> 期间我们搬不动的上游归档 crate，把它们也 deny 会得到一份 18 条的 ignore 文件，那份文件就变成
> 了真正的策略。
>
> 净效果：今天那个"18 条警告、退出 0"里藏着的真实 unsoundness，变成一条有日期、被审阅过的
> 接受；而**下一条** unsound 公告会让构建变红，而不是加入一堆 18 条里。

## 验收标准

- [ ] `deny.toml` 存在，覆盖 bans / licenses / sources 三类检查，且**不含 `skip-tree`**
- [ ] `cargo deny check` 在 CI 中执行且可阻断构建（无 `continue-on-error`）
- [ ] **48** 个重复 crate 全部已分类并带理由接受，无未分类项（"可统一"一类实测为空）
- [ ] 全部 **653** 个非成员依赖的许可证落入允许集合或带理由的例外清单
- [ ] 棘轮由工具自身承担：CI 调用带 `--deny unmatched-skip`，`deny.toml` 设
      `unused-allowed-license = "deny"`；负向 fixture 证明追加一条匹配不到的 skip 会失败，
      删掉一条真实 skip 也会失败
- [ ] `cargo audit` 与 `cargo deny advisories` 的职责划分已书面决策并落实：cargo-deny 的检查
      清单精确为 `bans licenses sources`，`cargo audit --deny unsound` 阻断，
      `.cargo/audit.toml` 的每条 ignore 带理由
- [ ] **存在一道门禁断言上述配置仍然生效**——即调用行上的旗标没有被悄悄改掉。工具证明策略成立，
      没有任何东西证明策略仍然绑定
- [ ] 五条护栏（sources、`skip-is-live`、棘轮、职责划分、`skip-tree` 缺席）当前存量均为 0，
      故每条都必须由能触发它的 fixture 证明会响，并各配一个不该触发的对照
- [ ] `cargo test --workspace`、strict Clippy、既有 CI job 全部通过

## QA 计划

- **棘轮负向 fixture**：向 `deny.toml` 的 skip 清单追加一项 → 门禁失败；移除后恢复。
- **新增重复检出**：引入一个会带来新重复版本的依赖 → `cargo deny check bans` 失败；回退后通过。
- **许可证负向验证**：临时把某个已用许可证移出允许集合 → 门禁失败，证明许可证检查确实在评估真实依赖而非空转。
- **告警不重复**：确认同一漏洞不会同时由 `cargo audit` 与 `cargo deny` 报出两次（或已按决策仅保留一处）。

> **复核**：前三条保留，并各补一个"不该触发"的对照——一条在正确输入上也会响的规则，早在它抓到
> 任何东西之前就会被关掉。第四条按上面的职责划分改写为可解析的断言：cargo-deny 的调用行精确列
> `bans licenses sources`，永不出现 `advisories` 或 `all`——这是可以从 `security.yml` 解析出来
> 的事实，而不是一次人工确认。
>
> 另需一条 FR 原文没有的空扫描护栏：一个**没有任何 job** 的 `security.yml` 必须 FAIL 而不是干净
> 通过。§4.4 shape 5，就在上一个 FR。
