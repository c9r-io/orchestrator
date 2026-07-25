# FR-134: 门禁执行事实校验 — 消除 FR-127 中"文本存在性即执行"的代理

## 优先级: P1

## 状态: Proposed

## 背景

FR-127 已闭环，其核心交付是真实的：`ci-required` 门禁从 3 个扩到 12 个，台账 `config/governance/qa-gate-surface.json` 与磁盘双向完整（45 = 45），5 个孤儿脚本全部处置，并在接线过程中发现 `test-legacy-coordination-decommission.sh` **自 FR-126 起已持续失败一个完整 FR 周期**——它断言恰好 4 个 legacy command-only Agent，而 FR-126 早已将其清零，无人知晓是因为没有任何 workflow 执行它。

2026-07-25 的闭环后严格审计对 `scripts/qa/test-qa-gate-surface.sh` 施加变异测试，发现 4 个可复现的漏洞。其中 3 个共享同一根因：**用文本存在性代替执行事实**——而这正是 FR-127 存在的理由所要消灭的错误类别。

复现记录（均在仓库副本上执行，工作树未被触碰）：

| # | 注入的缺陷 | 门禁结果 | 位置 |
|---|---|---|---|
| 1 | 把 `run: ./scripts/qa/test-filesystem-trigger.sh` 换成注释 `# disabled: ./scripts/qa/test-filesystem-trigger.sh was flaky` | **5/5 PASS** | `test-qa-gate-surface.sh:133` |
| 2 | 把 parity 门禁的 `export PATH="$QA_ROOT/bin:$PATH"` 注释掉 | **provider isolation PASS** | 同上 `:201` |
| 3 | 向 fixture-pinned bundle 追加一个未钉死的 `provider: claude` agent，外加一个无关的 `binary: fake-decoy` | **PASS**（providers=2 / pins=2） | 同上 `:166-169` |
| 4 | 在 `README.md` 植入一条针对某个 manual-runbook 门禁的强制执行声明（措辞见需求 4） | **PASS** | 同上 `:241` |

三点加重情节：

- **缺陷 1 命中的是承重检查**。DD-139 称 `check_wiring_truth` 是"no gate may claim CI enforcement it does not have 的持久形式"。"有人把门禁注释掉并留下解释"是这类退化最常见的形态，而它完全通不过检测。既有 fixture 4 测的是"job 名指错"，绕开了这个更现实的变异。
- **缺陷 2 的既有 fixture 选错了变异**。fixture 5 测的是**删除** `export PATH`（能正确失败），但**注释掉**才是重构中更可能发生的事。该 fixture 提供的信心恰好覆盖不到最可能的失效路径。
- **缺陷 4 的正确写法就在同一仓库内**。`test-agent-driver-documentation-alignment.sh:40-45` 已采用 `git ls-files '*.md'` 全集减去带理由豁免的策略，且 `.claude/skills/qa-doc-gen/SKILL.md` Step 5 明文写着"A whitelist of known files is not an acceptable scope"。新门禁退回了旧模式，扫描面漏掉 **83 个被追踪的 Markdown**，含 `README.md`、`CHANGELOG.md`、`AGENTS.md`、`SKILLS.md`、`CONTRIBUTING.md` 与全部 crate README。

关于 provider 兜底的作用域：`exit 97` stub 只安装在 `governance` job（`ci.yml:135`）。`coordination-strangler` job（`ci.yml:91-110`）**没有 stub**，而它执行的 `test-coordination-strangler.sh` 正是以 `fixture-pinned` 为唯一屏障的门禁——缺陷 3 在该 job 上没有第二道防线。本地执行则完全无保护：开发者机器上装有真实 `claude` 时，缺陷 2 会静默走真实 CLI。

（客观边界：GitHub 托管 runner 当前不预装 `claude`/`codex`，因此 CI 内实际消耗 token 目前不可能发生。风险面是本地执行、自托管 runner，以及未来 runner 镜像变化。这是"当前无损害"，不是"机制成立"。）

## 首次真实 CI 运行推翻的前提

FR-127 闭环后首次把 `main` 推送到 GitHub 并观察真实 workflow（run `30152482382`，commit `cf958e3c`），结果为 **11 成功 / 5 失败**。两个失败暴露了本地验证无法看到的问题，且都不在 FR-127 已披露的限制之内。

### 发现 A：`ci-required` 门禁的系统依赖与其所在 job 不一致

```
Coordination strangler governance and parity → missing required command: rg
Slack certification recorded contracts ×2    → FAIL: missing command: rg
```

`coordination-strangler` job 与 `slack-certification-recorded` job 均未安装 ripgrep（前者装的是 `jq ruby sqlite3 protobuf-compiler`）。脚本在起始的 `command -v` 前置检查处即退出，**一条断言都未执行**。

推论极为重要：FR-127 立论为"46 个门禁只有 3 个在 CI"，实测是**那 3 个里至少 2 个是死的**——它们被 job 引用、被 workflow 调度、在日志中出现，但从未验证过任何东西。新建的 `governance` job 恰好安装了 ripgrep，说明依赖需求是已知的，只是没有回头核对既有 job。

**这构成 FR-127 门禁的第五个缺口，且与前四个不同源**：`check_wiring_truth` 只断言"脚本被其声明的 job 引用"，从不断言"被引用的门禁实际可执行"。`test-coordination-strangler.sh` 完全满足 wiring 检查，同时在 CI 中持续失败。**"接线了"不等于"在守"**——这是 FR-127 所要终结的那句话的下一层。

### 发现 B：`ci-required` 门禁的 workspace 范围与 sibling job 不一致

`governance` job 因 `test-filesystem-trigger.sh` 失败：

```bash
# 脚本（test-filesystem-trigger.sh:20,26）
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# sibling job（ci.yml:66,87）
cargo test   --workspace --exclude orchestrator-gui
cargo clippy --workspace --exclude orchestrator-gui --all-targets -- -D warnings
```

核查确认 `.github/workflows/` 中**没有任何 job 安装 Tauri/webkit 依赖**，`orchestrator-gui` 从未在 Linux 上构建过，因此脚本的无 exclude 版本在 ubuntu 上必然失败。本地 `cargo check -p orchestrator-gui --all-targets` 在 macOS 上 exit 0，确认是平台依赖缺失而非代码缺陷。

DD-139 将这条记为"accepted cost：与 sibling `test`/`clippy` job 重复执行"。它不是重复，**是超集**，而超集多出的那一部分正是 sibling job 刻意排除的。该判断在本地无法证伪，因为 macOS 上 Tauri 依赖由系统框架提供。

次生问题：脚本以 `>/dev/null 2>&1` 吞掉 cargo 输出，CI 日志中只有 `FAIL: cargo test --workspace` 一行，根因无法从日志判定，需本地复现与交叉比对才能定位。诊断信息的丢失使门禁失败的修复成本远高于必要。

## FR-128 闭环后审计并入的两项

FR-128（台账再生工具）已闭环，其交付质量经独立复核确认：四项棘轮基线以独立实现重算得 `53 / 30 / 9 / 0`，与台账逐项吻合；`cfg(test)` 扫描口径的修正方向正确（挤掉被误计入的测试代码，而非放松）；检查本身还从 `count > baseline`（单调）收紧为 `count != baseline`（精确相等）。以下两项缺陷不足以单开 FR，且与本 FR 的需求 4 同源，故并入实施批次。

### 缺陷 X：`strip_test_modules` 以文本计数括号，字符串字面量会使棘轮静默失效

对 `scripts/qa/coordination-governance.rb` 的真实实现验证：

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn t() { assert_eq!(fmt("{"), "{"); }
}

pub fn legacy() {
    let c = captures();      // 应被计数
}
```

结果 `raw=1 → after strip=0`。`depth` 逐行统计 `{`/`}` 而不做词法分析，字符串字面量中的花括号使深度永不归零，该 `cfg(test)` 块的范围一路吃到文件末尾，其后的生产代码全部从扫描中消失。

全仓扫描确认已存在 **3 处**永不闭合的 `cfg(test)` 块：

| 文件 | 起始行 | 失衡来源 |
|---|---|---|
| `core/src/error.rs` | 283 / 750 | `format!("{err}")`、`"{{bad"` |
| `core/src/source_task_template.rs` | 363 / 539 | `"{source_message_url"` |
| `crates/orchestrator-scheduler/src/scheduler/coordination_tools.rs` | 634 / 1118 | `.body("{")` |

三处均为尾部测试模块，其后无生产代码，被吃区域内 legacy 模式命中数为 0，**因此当前基线正确，缺陷是潜伏而非已发作**。

潜伏性与新的精确相等检查叠加后构成真实风险：一旦此类模块之后新增生产代码，或某个中部 `cfg(test)` 模块出现字符串花括号，该区域的 legacy 用量将永久不可见——计数不会移动，`count != baseline` 不会触发，门禁保持绿色。棘轮静默失效开放。

FR-128 的 QA 测试 7 覆盖了"中部位置 + 非 `tests` 命名"，但括号是平衡的；**在本仓库已出现三次的失效形态未被测到**。

### 缺陷 Y：语义变更留下 6 处被证伪的陈述

棘轮语义已由单调改为精确相等，下列陈述未随之更新：

```
docs/design_doc/orchestrator/136-coordination-strangler-completion.md:136   "a monotonic source baseline"
docs/design_doc/orchestrator/136-coordination-strangler-completion.md:169   "monotonic source counters"
docs/design_doc/orchestrator/138-agent-driver-execution-migration.md:260    "source baselines remain monotonic"
docs/architecture.md:140                                                   "monotonic legacy-coordination ratchet"
docs/design_doc/README.md:106                                              "monotonic legacy ratchet"
docs/design_doc/orchestrator/130-coordination-collapse-mcp-tools.md:116     "exact production inventory and monotonic ratchet"
```

前三条经上下文确认无歧义指向 `sourceBaseline`。`docs/qa/orchestrator/178-governance-ledger-regeneration.md:17` 自身写的是"the ratchets **were** monotonic"——语义变更是被知晓的，只是未横扫其余文档面。这正是 `fr-governance` Phase 5"本次变更是否证伪了仓库别处的既有陈述"未执行到位。

本 FR 的需求 4（stale-claim 扫描改为全集减豁免）落地后，这一类漂移本应被自动捕获；缺陷 Y 因此既是待修项，也是需求 4 的现成验收样本。

## FR-129 闭环后审计并入的一项

FR-129（Skill 单一来源与镜像完整性）已闭环，是这一批治理 FR 中实现质量最高的一个：6 个 check、19 条负向 fixture 断言，其中 fixture 4a（"一个是目录的 `SKILL.md` 通过全部结构性检查，只有读取失败"）是本 FR 缺陷的最小形态并被隔离到读取检查；两条 meta 断言（每个 check 都在注册表中、每个注册 check 都至少有一条负向 fixture 证明）为其余治理门禁树立了标准。三次针对性攻击（错指目标的符号链接、陈旧 `notSkills` 条目、未声明根中的真实副本）均被正确拦截。

### 缺陷 Z：未声明的镜像根完全不可见

`config/governance/skill-mirrors.json` 以 `mirrorRoots` 枚举需要检查的镜像根。覆盖率、形状、读取三项检查只在已声明的根上执行，没有任何机制发现新出现的根。

复现——建立一个只含符号链接的 `.windsurf/skills/`，其中一条故意错名错指，且只覆盖 29 个 skill 中的 1 个：

```
.windsurf/skills/fr-governance       -> ../../.claude/skills/fr-governance
.windsurf/skills/BROKEN-wrong-target -> ../../.claude/skills/qa-doc-gen
```

结果：**6 个 check 全部通过**。

这与本 FR 需求 4 的缺陷 4 同源——**枚举式覆盖面只能守住已知的那些**——但后果更具体：这正是催生 FR-129 的失效模式本身。FR-129 原文遗漏了 `.cursor/skills`，而它当时握有 29 个 skill 中的 16 个且从未被检查。门禁现在保证两个已声明根完美，对第三个根依然沉默。本仓库的历史已证明根会增减：FR-129 的 CHANGELOG 记录了 `SKILLS.md` 曾声明一个磁盘上并不存在的 `.gemini/skills/` 根。

部分缓解：若未声明根中放的是**副本**，`check_no_content_copies` 会捕获（已验证）。仅全符号链接的未声明根不可见——而符号链接恰是本仓库文档化的约定形状，因此这是更可能出现的形态。

`DD-141` 的 Known Limits 诚实列举了三项限制（不解析 frontmatter、`notSkills` 是逃生舱、大小写敏感性），但未包含此项。

## FR-130 部分闭环后审计并入的两项

FR-130 的需求 1（边界冻结）与需求 3（迁移语义等价）已闭环，需求 2（crate 提取）与需求 4 明确标为未开始，FR 保留在原地并改为 `In Progress`——这是本批 FR 中第一个没有把真正的重活一并硬闭环的。台账数字经独立复核全部属实（`52 / 924 / 143` 与 `rusqlite` 的 `200 / 37`、6 个依赖 crate、46 表 + 92 索引），`cargo test -p agent-orchestrator schema_snapshot` 实跑 4 passed，断点续跑确认遍历**全部 74 个中断点**而非抽样。DD-142 还主动纠正了 FR 原文四处事实错误，其中一处是我原文的 14 文件清单与"core 不再直接依赖 rusqlite"这一验收标准根本不相容。

### 缺陷 W：括号缺陷被搬进共享扫描器，波及第二个台账

FR-130 把 `strip_test_modules` 抽取到 `scripts/lib/rust_source.rb` 供两个治理台账共用（并以 Case 9 断言两个门禁都无私有副本——做法本身是对的）。但**缺陷 X 所述的文本式括号计数被原样搬入**，`scripts/lib/rust_source.rb:68` 仍是 `depth += lines[cursor].count("{") - lines[cursor].count("}")`。

对新台账的直接验证：

```rust
#[cfg(test)]
mod tests { fn t() { assert_eq!(fmt("{"), "{"); } }

pub fn new_persistence_path(conn: &rusqlite::Connection) { ... }
```

```
rusqlite refs — raw=1  visible to ledger=0
```

在 FR-130 的场景里后果比 FR-128 严重：

- 本 FR 的全部目的是把 core 的 `rusqlite` 引用从 200 推向 0，而提取要重度编辑的正是那 37 个文件；
- `core-boundary-ledger.json` 同样采用精确相等，被藏起来的引用不会让计数移动，门禁保持绿；
- core 中已有 2 个文件带永不闭合的 `cfg(test)` 块——`core/src/error.rs`（第 283 行起）与 `core/src/source_task_template.rs`（第 363 行起）；
- 其中 `error.rs` 恰是 FR-130 正文点名的阻塞项（第 154 行 `impl From<rusqlite::Error> for OrchestratorError`），必然会被改动。

当前无实际损害：`error.rs` 的生产引用位于被吃区域之前，区域内第 428 行的引用本就是测试代码。但"收敛到 0"这一结论的可信度取决于先修掉此缺陷。

时序值得记录：本 FR 的需求 9 已于 `96ba7a7d` 提交，早于该工作两个提交，QA-180 也引用了本 FR 的另一条结论。抽取的恰是被标记为有缺陷的函数，既未修复也未列入 DD-142 的 Known Limits（该节诚实列了另外三条）。

### 缺陷 V：门禁分类的非递归 glob 盲点

`scripts/qa/test-qa-gate-surface.sh:44` 以 `ls scripts/qa/*.sh scripts/qa/*.rb` 枚举待分类脚本，**不递归**。放置一个 `scripts/qa/lib/hidden-gate.sh` 后，5 个 check 全部通过，该文件完全不可见。

DD-142 发现了这一点并主动绕开——把共享库放在 `scripts/lib/` 而非 `scripts/qa/lib/`，原话是"Exploiting that blind spot to avoid classification would be a governance evasion inside the governance tooling"。判断正确，但盲点本身仍在。

这是同一模式在本 FR 中的第三个实例：需求 4（stale-claim 白名单）、需求 12（镜像根枚举）、以及此处的非递归 glob——**枚举式覆盖面只守得住已知的形状**。

## 第二次真实 CI 运行：一个被自己验证的机制锁死的门禁

Run `30156418299`（commit `c9044183`）的 `governance` job 失败在一个**新**位置：

```
Governance ledger regeneration tooling
  PASS: --emit-inventory reproduces the reviewed inventory byte for byte
  PASS: --write refuses under CI (exit 2) and leaves the ledger untouched
  ##[error]Process completed with exit code 2
```

`scripts/qa/test-governance-ledger-tooling.sh:101` 直接调用 `--write`：

```bash
ruby "$DIR/$GATE" --emit-inventory --emit-baseline --write >/dev/null 2>&1
```

而 `--write` 的设计正是在 `CI` 存在时退出 2。脚本带 `set -euo pipefail`，于是整个门禁死在这一行。本地一行复现：

```
CI=1 bash scripts/qa/test-governance-ledger-tooling.sh   → EXIT=2（停在第 2 个用例后）
     bash scripts/qa/test-governance-ledger-tooling.sh   → EXIT=0（8/8 全绿）
```

**该门禁自 FR-128 将其接入 CI 起从未在 CI 中成功过一次。** 它的第 2 个用例专门验证"`--write` 在 CI 下拒绝执行"、验证通过，随后第 3 个用例又去调用 `--write`，被自己刚刚验证过的机制拦下。

这个形态比需求 8 现有表述更具体：不是"门禁碰巧红着没人看"，而是**门禁的正向用例与其自身的安全机制互斥**，且这种互斥只在门禁实际运行的环境里显现。FR-128 的闭环认证是本地跑的 8/8，结构上看不到它。

副作用一并记录：该步骤排在 `Filesystem trigger contracts` 之前，因此它的失败**遮住了缺陷 B（workspace 范围超集）**。缺陷 B 仍然存在，只是当前不可见——这也说明 job 内串行步骤的首个失败会掩盖其后所有诊断。

## 目标

- 把 `test-qa-gate-surface.sh` 中三处以文本存在性为代理的判定，替换为对执行事实的判定。
- 把 stale-claim 扫描的覆盖面从白名单改为全集减豁免，与同仓库既有门禁的策略统一。
- 把 provider stub 兜底推广到所有执行 provider 相关门禁的 job，使兜底不再依赖单个 job 的配置。
- 把台账的断言从"门禁被声明为 ci-required"推进到"门禁在真实 CI 中确实可执行且当前为绿"，使死门禁无法继续被 wiring 检查背书。

## 非目标

- **不**撤销 FR-127 的闭环，也**不**推翻其台账结构、分类口径或 `enforcementKinds` / `providerIsolationModes` 定义——本 FR 只更换判定实现。
- **不**改变任何脚本的 `enforcement` 分类结论（12 ci-required / 33 manual-runbook 的归属经审计确认正确）。
- **不**扩大 `scripts/qa/` 的门禁数量或新增语义断言。
- **不**处理 DD-139 已显式披露的两项已知限制（退役措辞为 curated 字面清单、动态构造 bundle 路径可绕过）——前者归属 DD-138，后者由推广后的 stub 兜底覆盖。

## 需求

### 1. wiring 校验改为解析执行步骤

- `check_wiring_truth` 不再对 job block 做全文 `grep -F`，改为解析 YAML 步骤结构，仅当脚本出现在某个 step 的 `run:` 指令中（含通过 `invokedBy` 的间接调用）才判定为已接线。
- 注释、`name:` 文本、`if: false` 的步骤均不得满足该判定。
- 若引入 YAML 解析依赖，需与 `governance` job 的既有系统依赖声明保持一致。

### 2. path-shadow 校验改为运行时探针

- 不再用 `grep` 判断 `cp` 与 `export PATH` 两行文本是否存在。
- 改为可执行验证：在受控临时环境中把一个"被调用即失败并打印诊断"的 stub 放在 PATH 上真实 `claude`/`codex` 之前的位置之后（即模拟真实机器上存在真 CLI 的情形），执行被检门禁，断言 stub 未被触发。这与 QA-177 Scenario 3 已验证有效的方法同源，只是把它从一次性证据变为常驻检查。
- 若运行时探针在某些门禁上代价过高，允许保留文本检查作为**附加**条件，但不得作为**唯一**条件。

### 3. bundle 校验改为逐 agent 关联

- `bundle_has_unpinned_provider` 不再比较整文件的 `provider` 与 `binary: fake-` 计数。
- 改为解析 YAML 文档流，对每个 `kind: Agent` 单独判断：若其 `spec.driver.provider` 为 `claude`/`codex`，则该 agent 自身必须声明 `binary: fake-*`。
- 使实现与 `qa-gate-surface.json` 中 `fixture-pinned` 的契约文字（"Every claude/codex agent in the named fixture bundle also declares `binary: fake-*`"）真正一致。

### 4. stale-claim 扫描改为全集减豁免

- 扫描面由 `docs` + `.claude/skills` 改为 `git ls-files '*.md'`，减去带理由的显式豁免清单。
- 与 `test-agent-driver-documentation-alignment.sh` 的 `EXEMPT_PATTERN` 采用同一模式；两处若能共用实现则共用。
- 复核全集下是否出现既有误报（如 CHANGELOG 历史段落、`.agents` 镜像），对确需豁免者写明理由。

### 5. provider stub 兜底推广

- 把 `governance` job 的 `exit 97` stub 安装步骤提取为可复用步骤，应用到所有执行 provider 相关门禁的 job，至少包含 `coordination-strangler`。
- 记录哪些 job 无需 stub 及其理由。

### 6. 系统依赖一致性校验

- 新增检查：每个 `ci-required` 脚本的 `command -v` 前置依赖列表，必须是其声明 job 的依赖安装步骤所提供命令的子集。
- 该检查须能同时覆盖 apt 安装、runner 预装与工具 action 三类来源；无法静态判定的来源需在台账中显式声明。
- 修复现存不一致：`coordination-strangler` 与 `slack-certification-recorded` 两个 job 缺少 ripgrep。修复本身属实施范围，但**先补齐检查再修复**，以证明检查确实会在修复前失败。

### 7. Workspace 范围与诊断保真

- 新增检查：`ci-required` 门禁若执行 `cargo test`/`cargo clippy`，其 workspace 范围必须与 `ci.yml` 中同名 sibling job 的范围一致，或在台账中带理由声明差异。
- `test-filesystem-trigger.sh` 的范围差异按上述规则处置：对齐 sibling 的 `--exclude orchestrator-gui`，或声明该门禁要求完整 workspace 并由其 job 安装 Tauri 依赖（后者属 FR-076 需求 1 范围，本 FR 不实施，只需记录归属）。
- 禁止 `ci-required` 门禁把失败命令的输出丢弃：`>/dev/null 2>&1` 形态的调用须改为捕获到日志并在失败时回显，使 CI 日志足以定位根因而无需本地复现。

### 8. CI 存活性

**作用域为 `ci.yml` 的全部 job，而非仅台账中的 `ci-required` 门禁。** 这条区分是必要的：
`qa-gate-surface.json` 只分类 `scripts/qa/*`，因此 `boundary-coverage`、`Rust test`、`clippy`、
`miri`、`cross-compile` 等 job **都不在台账里**。`boundary-coverage` 已连红 6 次，
按原措辞的检查根本不会看它一眼。

这是本 FR 反复出现的那个模式的又一次实例：**存活性台账覆盖的是它认识的门禁，
而不是它实际运行其上的 CI**。因此对象须由发现得出——解析 `.github/workflows/*.yml` 的
job 列表——枚举只用于 `known-failing` 标注。

- 台账新增一个维度，记录**每个 CI job** 最近一次真实执行的结论与 run 引用。
- 新增检查：任何 job 不得处于已知持续失败状态；若确需在修复期内保持红色，必须显式标注为
  `known-failing` 并附 ticket 或 FR 引用与预期修复期限。
- job 清单由解析 workflow 得出；workflow 新增 job 而台账无对应条目即失败。
- 该维度的更新方式需可脚本化（例如从 `gh run` 拉取），不得依赖人工誊写——否则它会退化成
  与被治理对象同类的陈旧声明。
- FR-135 修复的是 `boundary-coverage` 这一处具体故障；本条扩容后才能保证**下一个**
  红了没人管的 job 会被发现。二者互不替代。

**环境等价性**——存活性不只是"有没有人在看"，还包括门禁能否在其运行环境中通过：

- 修复 `test-governance-ledger-tooling.sh:101` 的自锁：第 3 个用例应在清除 `CI` 的子进程中调用（`env -u CI ruby ...`），或改为断言"非 CI 环境下 no-op `--write` 不改动文件"并显式记录该用例的环境前提。
- 新增负向 fixture：以 `CI=1` 运行全部 `ci-required` 门禁，任一因环境变量差异而失败即报错。这是唯一能在本地捕获此类缺陷的方式——它们按定义在无 `CI` 的开发机上不可见。
- 门禁若存在"仅在特定环境下成立"的用例，须在脚本内显式声明该前提并在其他环境下跳过而非失败。
- 同理适用于 shell 版本：`boundary-coverage` 只跑在 macOS，其失败源于 bash 3.2 与 bash 5 的空数组语义差异（见 FR-135）。"在我机器上绿"与"在该 job 的 runner 上绿"是两个命题。
- **诊断可见性**：job 内串行步骤的首个失败会掩盖其后全部诊断。本 FR 的缺陷 B 当前就被账本工具的失败遮住。评估是否让 `governance` job 的各步骤独立报告（例如收集失败后统一退出），使一次运行能暴露多个问题。

### 9. 棘轮扫描的词法安全

修复点是 `scripts/lib/rust_source.rb` 的 `strip_test_modules`——FR-130 已将其抽取为共享库，因此单点修复同时覆盖 `coordination-governance.rb` 与 `core-boundary.rb` 两个台账，而单点遗漏也同时波及两者。

- 统计括号深度前须先剥除字符串字面量、字符字面量与行注释；不要求完整 Rust 词法器，但必须使 `format!("{err}")`、`"{{bad"`、`.body("{")` 三种已存在形态不再破坏深度计数。
- 补充负向 fixture 两条，因为两个台账数的是不同的东西：一条证明 `cfg(test)` 之后的生产 legacy 用量仍被计数，一条证明其后的生产 `rusqlite` 引用仍被计数。两条都须在修复前失败。
- 修复后重算全部棘轮，确认基线不变——协调侧应仍为 `53 / 30 / 9 / 0`，边界侧应仍为 `200 / 37` 与 `52 / 924 / 143`。任一变化都说明存在此前未发现的被吃区域，须逐项说明而非直接接受新数字。
- 附加防御：新增检查断言不存在"跑到文件末尾仍未闭合"的 `cfg(test)` 块。当前 core 中的 `error.rs`（283 行起）与 `source_task_template.rs`（363 行起）即为该形态，二者恰在 FR-130 提取的必改路径上。
- 该修复应排在 FR-130 需求 2 之前：提取会重度编辑那 37 个文件，在扫描器可靠之前得出的"收敛到 0"不构成证据。

### 10. 语义变更的陈旧陈述清偿

- 修正缺陷 Y 列出的 6 处 `monotonic` 表述，使其与精确相等语义一致。
- `docs/feature_request/FR-133-dependency-policy-gate.md:58` 引用"与 FR-124/125 的 `sourceBaseline` 棘轮同一模式"，同属过期，一并更正。
- 需求 4 的全集扫描落地后，须验证它能捕获这一类漂移；若捕获不到，说明其匹配模式只覆盖"CI 强制执行"一类声明，需评估是否扩展到语义契约类陈述，或明确记录该边界。

### 11. `--write` 的 CI 识别面

- `coordination-governance.rb:443` 与 `core-boundary.rb:141` 的写保护均仅判断 `ENV.key?("CI")`。未设置该变量的自托管 runner 不会被拦。新门禁复制了同一限制，说明这需要一处共用实现而非各写一遍。
- 扩展识别面（如 `GITHUB_ACTIONS`、通用 CI 变量集合），并补测试。实际风险低——CI 从不调用 `--write`——但这是"防止 review gate 沦为装饰"的唯一屏障，成本近零。

### 12. 镜像根的发现式覆盖

- `scripts/qa/test-skill-mirror-integrity.sh` 新增检查：发现仓库中所有指向 `.claude/skills/` 的被追踪符号链接，要求其所在根必须在 `mirrorRoots` 中声明。
- 实现提示：`git ls-files -s` 中 mode `120000` 的条目即全部被追踪的符号链接，按目标是否解析进源树筛选即可，无需遍历文件系统。
- 补负向 fixture：一个只含符号链接的未声明根必须使门禁失败；将其声明进 `mirrorRoots` 后，该根随即受覆盖率与形状检查约束（即错名错指的条目仍应失败）。
- 与需求 4 共享同一原则：覆盖面由发现得出，枚举只用于豁免。二者若能共用"全集减豁免"的实现骨架则共用。

### 13. 门禁分类的递归覆盖

- `test-qa-gate-surface.sh` 的待分类脚本枚举改为递归发现 `scripts/qa/**` 下的 `*.sh` / `*.rb`，而非 `ls scripts/qa/*.{sh,rb}`。
- 需一并决定 `scripts/qa/fixtures/**` 与潜在的 `scripts/qa/lib/**` 如何分类：作为受支持的 fixture / library 类别单独标注，或纳入豁免并写明理由。不得靠"glob 匹配不到"来表达豁免。
- 补负向 fixture：`scripts/qa/lib/hidden-gate.sh` 必须使分类检查失败。
- 与需求 4、12 合并考虑：三处的根因相同，若能共用"发现得出覆盖面、枚举仅用于豁免"的实现骨架则共用。

### 14. 修正 QA-177 的计数陈述

- QA-177 Scenario 3 记为"All 10 `ci-required` gates"，而台账声明 12（差额为两条 `invokedBy` 条目）。更正为准确表述，或说明 10 指直接调用数。

## 验收标准

- [ ] 缺陷 1 复现步骤（注释掉 `run:` 步骤）使 `check_wiring_truth` 失败
- [ ] 缺陷 2 复现步骤（注释掉 `export PATH`）使 provider isolation 失败
- [ ] 缺陷 3 复现步骤（未钉死 agent + 无关 fake 二进制）使 provider isolation 失败
- [ ] 缺陷 4 复现步骤（在 `README.md` 植入针对 manual-runbook 门禁的强制执行声明）使 stale-claim 检查失败

> 复现所用的具体措辞刻意不在本文档内逐字重现：现行 stale-claim 检查按行匹配"脚本名 + 强制执行措辞"，无法区分"作出声明"与"描述探针"。本 FR 撰写时即被该检查拦下，属其规则内的有效拦截。实施需求 4 时应一并考虑这一误报形态——描述性上下文是否需要可识别的转义机制，或由豁免清单承载。
- [ ] 上述四条各自成为常驻负向 fixture，且沿用 FR-127 的隔离断言（只打中目标 check，其余仍通过）
- [ ] 既有 7 条 fixture 与正向控制仍全部通过，`ci-required` 分类结论无变化
- [ ] `exit 97` stub 已覆盖 `coordination-strangler` 等全部 provider 相关 job，例外有书面理由
- [ ] stale-claim 扫描覆盖 `git ls-files '*.md'` 全集，豁免项各带理由，且全集下无误报
- [ ] 依赖一致性检查在修复 ripgrep 缺失**之前**即失败，指名 `coordination-strangler` 与 `slack-certification-recorded` 两个 job
- [ ] 修复后两个 job 的门禁能执行到各自的断言，而非停在 `command -v` 前置检查
- [ ] workspace 范围差异被检查捕获；`test-filesystem-trigger.sh` 的差异已对齐或带理由声明，归属记录清楚
- [ ] 无 `ci-required` 门禁以 `>/dev/null 2>&1` 丢弃失败命令输出；负向验证：故意使某条 cargo 命令失败，CI 日志足以定位根因
- [ ] 台账记录 **`ci.yml` 每个 job** 最近一次真实执行结论，且该维度可脚本化更新
- [ ] 存活性检查对处于持续失败且未标注 `known-failing` 的 job 失败，`boundary-coverage` 即为验证样本
- [ ] job 清单由解析 workflow 得出；负向 fixture 证明"workflow 新增 job 而台账无条目"会失败
- [ ] `CI=1 bash scripts/qa/test-governance-ledger-tooling.sh` 退出 0；修复前该命令退出 2
- [ ] 存在一条以 `CI=1` 运行全部 `ci-required` 门禁的检查，任一因环境变量差异失败即报错
- [ ] `governance` job 一次运行能同时暴露账本工具与 workspace 范围两个问题（诊断不再被首个失败掩盖）
- [ ] 含字符串花括号的 `cfg(test)` fixture 在修复前使棘轮漏计、修复后正确计数
- [ ] 修复后四项棘轮仍为 `53 / 30 / 9 / 0`；若变化则逐项说明被吃区域
- [ ] 存在检查断言无"跑到文件末尾仍未闭合"的 `cfg(test)` 块
- [ ] 6 处 `monotonic` 表述与 FR-133 的引用均已更正
- [ ] 已验证需求 4 的全集扫描能否捕获语义契约类漂移；捕获不到时其边界有书面记录
- [ ] `--write` 的 CI 识别面已扩展并有测试
- [ ] 只含符号链接的未声明镜像根使镜像门禁失败；声明后其错名错指条目仍被形状检查捕获
- [ ] 镜像根覆盖面由被追踪符号链接发现得出，而非由 `mirrorRoots` 枚举决定
- [ ] `scripts/qa/lib/hidden-gate.sh` 使分类检查失败；`scripts/qa/**` 下的子目录脚本各有分类或带理由的豁免
- [ ] 词法修复后 `core-boundary-ledger.json` 的 `200 / 37` 与 `52 / 924 / 143` 不变；变化项逐条说明
- [ ] `cfg(test)` 之后的生产 `rusqlite` 引用在修复前不可见、修复后被计数（独立于协调侧的那条 fixture）
- [x] QA-177 的门禁计数陈述与台账一致 —— 已由 `dd993346` 解决（11 次调用对应 13 个条目，两条经 `invokedBy` 间接接线）
- [ ] `cargo test --workspace`、strict Clippy 与全部既有 CI job 通过

## QA 计划

- **四条复现即四条 fixture**：本 FR 的验收与其发现方式同构——每个缺陷的复现步骤直接固化为负向 fixture，不另设结构性断言。这是唯一能证明修复真实有效的方式：修复前 fixture 必须失败于旧实现、通过于新实现。
- **变异类别扩展**：除"注释掉"外，补充 `if: false`、步骤被 `name:` 提及但无 `run:`、脚本名出现在 job 内 heredoc 文本中三种形态，确认均被判为未接线。
- **运行时探针自证**：对 `test-agent-driver-production-parity.sh` 故意移除隔离后运行探针，stub 必须被触发（exit 97 可见），恢复后不触发。
- **全集扫描误报基线**：在改为全集后先做一次干跑，记录所有命中项并逐条判定为真阳性或需豁免，避免用扩大豁免清单的方式让门禁变绿。
- **依赖缺失的先证后修**：在补 ripgrep 之前先跑依赖一致性检查，必须失败并指名两个 job；补齐后转绿。顺序不可颠倒，否则无法证明检查有效而非恒真。
- **存活性检查自证**：把一个当前为绿的 `ci-required` 门禁临时标为持续失败态，存活性检查必须失败；恢复后通过。反向亦须验证：真实红门禁未标注 `known-failing` 时不得放行。
- **诊断保真验证**：临时让某个 `ci-required` 门禁中的 cargo 命令失败，断言 CI 日志包含足以定位根因的编译器输出，而非仅一行 `FAIL:`。本 FR 的发现 B 正是因为缺少这一点而需要本地复现才能定位。
- **词法安全先证后修**：先加入含字符串花括号的 `cfg(test)` fixture，确认它在当前实现下漏计，再修复。同时对三处已存在的失衡块做回归——它们目前是尾部模块因而无害，修复后须确认基线读数不变，以区分"修好了"与"换了一种错法"。
- **陈旧陈述的双向验证**：修正 6 处表述后，再故意在任一设计文档写回 `monotonic source baseline`，确认需求 4 的扫描能否捕获。捕获不到即说明扫描只覆盖"CI 强制执行"类声明，该边界须写入设计记录而非默认成立。
- **发现式覆盖的双向验证**：未声明根必须失败；声明后必须立刻受既有三项检查约束。只验证前者会让"声明即豁免"成为绕过路径。
- **CI 实证**：修复后推送并观察真实 workflow 运行结果，而非仅本地执行。本 FR 的两个新发现均只在真实 CI 中可见——发现 A 因本地已装 ripgrep 而不可见，发现 B 因 macOS 提供 Tauri 系统框架而不可见。**本地绿不等于 CI 绿，CI 绿不等于门禁在守，门禁被引用也不等于门禁能跑。**
