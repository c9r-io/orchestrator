# FR-127: 治理门禁执行面补完 — QA 门禁 CI 接线与脚本执行面分类

## 优先级: P0

## 状态: Proposed

## 背景

2026-07-25 的技术负债深挖发现：本仓库的治理体系在**编写侧**极其严格（209 篇 QA 文档、46 个 `scripts/qa/` 门禁脚本、逐条可证伪断言），但在**执行侧**几乎没有接线。实测：

```
scripts/qa/*.{sh,rb} 总数            46
被 .github/workflows/ 引用            3   （其中 2 个是 Slack live 认证）
仅被文档引用（人工 runbook）          38
完全无人引用（孤儿）                   5
```

三个直接后果：

1. **`scripts/qa-doc-lint.sh` 不在任何 workflow 中**。FR-126 第四轮审计新建的 `scripts/qa/test-agent-driver-documentation-alignment.sh`（全量 Markdown 退役语义扫描 + 4 条 CHANGELOG 断言）因此只在有人手动执行时生效，push/PR 不会失败。
2. **文档已记载一个不存在的门禁**。`docs/design_doc/orchestrator/guide-alignment.md:42` 声明"该脚本由 `scripts/qa-doc-lint.sh` 和 FR-126 默认 release gate 调用"，但 `.github/workflows/release.yml` 中不存在该 gate。这是一条自我认证式的失效声明——治理文档本身成了漂移源。
3. **FR-126 的其余门禁同样未接线**：`test-agent-driver-execution-migration.sh`、`test-agent-driver-production-parity.sh`。

孤儿脚本清单（无任何文档或 workflow 引用）：`auto-regress.sh`、`test-coordination-governance.sh`、`test-filesystem-trigger.sh`、`test-per-trigger-webhook-auth.sh`、`test-qa83-mixed-text.sh`。

本质问题不是"缺少某个门禁"，而是**门禁的执行状态本身无人治理**：新写的脚本默认落在"只有作者知道要跑"的状态，而这正是 FR-126 连续四轮审计每轮都能发现新漂移的结构性原因。

## 目标

- 让确定性、可在 CI 环境无副作用执行的 QA 门禁全部进入 `.github/workflows/`。
- 为每一个 `scripts/qa/` 脚本建立**显式执行面分类**：CI 强制 / 人工 runbook（带理由）/ 应删除。分类本身进入门禁，新增脚本未分类即失败。
- 修正 `guide-alignment.md` 中关于 release gate 的失效声明。

## 非目标

- **不**把需要真实凭证的 live 认证脚本（Slack managed live、Codex session resume 认证）改为无条件 CI 执行——它们保留手动/定时触发，但必须在分类清单中显式标注理由。
- **不**把需要长时间运行 daemon 的重型 QA 脚本一次性全塞进 PR 门禁；允许分层（PR 快门禁 / nightly 重门禁），但分层归属必须显式记录。
- **不**在本 FR 内新增任何新的语义断言——只解决既有断言不执行的问题。
- **不**处理 `orchestrator-gui` 的 CI 排除，该项由 FR-076 承载。

## 需求

### 1. 门禁执行面清单

- 建立机器可读的执行面清单（如 `config/governance/qa-gate-surface.json`），为每个 `scripts/qa/*.{sh,rb}` 标注 `enforcement`：`ci-required` / `manual-runbook` / `scheduled`，`manual-runbook` 与 `scheduled` 必须带 `reason` 与 owner 文档路径。
- 清单与磁盘实际脚本集合双向比对：磁盘上有而清单中无 → 失败；清单中有而磁盘上无 → 失败。
- 该比对本身进入 CI。

### 2. 确定性门禁接入 CI

- 新增 `governance` job，至少覆盖 `scripts/qa-doc-lint.sh`（含其调用的 `test-agent-driver-documentation-alignment.sh`）、`test-agent-driver-execution-migration.sh`、`test-agent-driver-production-parity.sh`。
- job 需声明其系统依赖（`jq`、`ruby`、`rg`），并与既有 `coordination-strangler` job 保持一致的缓存与 `PROTOC` 约定。

### 3. 孤儿脚本处置

- 对 5 个孤儿脚本逐个做出书面决策：接入 CI、绑定到某篇 QA 文档作为 runbook、或删除。
- 不允许"暂时保留待定"——未分类脚本使需求 1 的门禁失败。

### 4. 失效治理声明修正

- 修正 `docs/design_doc/orchestrator/guide-alignment.md` 中关于"FR-126 默认 release gate"的表述，使其与实际 workflow 一致。
- 检查是否存在同类失效声明（其他 DD/QA 文档声称某脚本"由 CI 执行"但实际未接线），一并修正。

### 5. CI 执行面的真实 provider 隔离不变量

2026-07-25 的补充核查确认：拟接入 CI 的四个脚本均不消耗真实 provider token——两个是纯静态文本扫描，一个是 `cargo test` + 静态断言（不启 daemon），一个通过 `cp fake-claude → $QA_ROOT/bin/claude` + `export PATH` 遮蔽真实 CLI。92 个 fixture bundle 中仅 4 个声明 `provider: claude|codex`，其中 2 个以 `binary: fake-*` 显式钉死，1 个仅被 `apply` 而从不执行。

但这份安全性目前**无门禁保护**：`agent-driver-production-parity.yaml` 未覆盖 `binary`，其隔离完全依赖脚本中的单行 `export PATH`。若该行在重构中丢失，测试仍会通过，只是静默改走真实 `claude`——没有任何机制会报警。

- 建立不变量：任何 `enforcement: ci-required` 的脚本不得可达真实 provider 二进制。
- 判定方式二选一或并用：fixture 显式声明 `binary: fake-*`；或脚本在启动 daemon 前遮蔽 PATH 且门禁断言该遮蔽存在。
- 推荐补强：为 CI job 设置一个不含真实 `claude`/`codex` 的 PATH，或注入一个会立即失败并打印明确诊断的 stub，使"意外调用真实 provider"成为**可见失败**而非静默消耗。
- `certify-codex-session-resume.sh`（调用真实 `codex`，版本钉死 `0.144.5`）与 5 个 Slack 凭证脚本永久标注为非 `ci-required`，其分类理由需明确写为"消耗真实凭证/配额"。

## 验收标准

- [ ] `config/governance/qa-gate-surface.json`（或等价物）覆盖全部 46 个脚本，无未分类项
- [ ] 清单与磁盘脚本集合的双向比对在 CI 中执行，且负向 fixture 证明新增未分类脚本会失败
- [ ] `qa-doc-lint.sh` 与两个 agent driver 门禁在 CI 中执行并能使构建失败（以一次故意破坏的验证记录为证）
- [ ] 5 个孤儿脚本各自有书面处置结论并已落实
- [ ] `guide-alignment.md` 的 release gate 声明与 `.github/workflows/` 实际内容一致
- [ ] 全仓不存在"声称由 CI 执行但实际未接线"的门禁声明
- [ ] 每个 `ci-required` 脚本的 provider 隔离方式被门禁断言（`binary: fake-*` 或已验证的 PATH 遮蔽）
- [ ] 负向 fixture：删除 `test-agent-driver-production-parity.sh` 的 `export PATH` 行会使隔离门禁失败
- [ ] `certify-codex-session-resume.sh` 与 5 个 Slack 凭证脚本被标注为非 `ci-required`，理由为"消耗真实凭证/配额"
- [ ] `cargo test --workspace`、strict Clippy、既有 CI job 全部通过

## QA 计划

- **执行面清单负向 fixture**：新建一个空的 `scripts/qa/test-unclassified.sh`，比对门禁必须失败；加入清单后恢复通过。
- **门禁真实生效证明**：在临时分支上故意引入一处退役语义（如在任意 Markdown 中写入 `runner.executor: streaming` 可用配置），确认 CI 的 `governance` job 失败而非通过。此为区分"接线了"与"看起来接线了"的唯一证据。
- **依赖可用性**：确认 CI runner 上 `jq`/`ruby`/`rg` 均已安装，脚本的 `command -v` 前置检查不会因环境缺失而误报。
- **分层归属回归**：若采用 PR/nightly 分层，验证 nightly job 确实被调度且失败可见（非静默）。
- **provider 隔离负向 fixture**：临时移除 `test-agent-driver-production-parity.sh` 的 `export PATH` 行，隔离门禁必须失败；恢复后通过。这是本 FR 中唯一防止"CI 静默消耗真实 token"的检查，不能只做正向验证。
- **无真实 CLI 环境验证**：在 PATH 中不存在 `claude`/`codex` 的环境下运行全部 `ci-required` 脚本，应全部通过——若有脚本因此失败，说明它实际依赖真实 provider，分类有误。
