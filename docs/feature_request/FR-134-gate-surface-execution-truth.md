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

## 目标

- 把 `test-qa-gate-surface.sh` 中三处以文本存在性为代理的判定，替换为对执行事实的判定。
- 把 stale-claim 扫描的覆盖面从白名单改为全集减豁免，与同仓库既有门禁的策略统一。
- 把 provider stub 兜底推广到所有执行 provider 相关门禁的 job，使兜底不再依赖单个 job 的配置。

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

### 6. 修正 QA-177 的计数陈述

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
- [ ] QA-177 的门禁计数陈述与台账一致
- [ ] `cargo test --workspace`、strict Clippy 与全部既有 CI job 通过

## QA 计划

- **四条复现即四条 fixture**：本 FR 的验收与其发现方式同构——每个缺陷的复现步骤直接固化为负向 fixture，不另设结构性断言。这是唯一能证明修复真实有效的方式：修复前 fixture 必须失败于旧实现、通过于新实现。
- **变异类别扩展**：除"注释掉"外，补充 `if: false`、步骤被 `name:` 提及但无 `run:`、脚本名出现在 job 内 heredoc 文本中三种形态，确认均被判为未接线。
- **运行时探针自证**：对 `test-agent-driver-production-parity.sh` 故意移除隔离后运行探针，stub 必须被触发（exit 97 可见），恢复后不触发。
- **全集扫描误报基线**：在改为全集后先做一次干跑，记录所有命中项并逐条判定为真阳性或需豁免，避免用扩大豁免清单的方式让门禁变绿。
- **CI 实证**：修复后推送并观察真实 workflow 运行结果，而非仅本地执行——FR-127 的教训是本地绿不等于 CI 绿，而 CI 绿不等于门禁真的在守。
