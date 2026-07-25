# 用户指南编译验证对齐

**Related FR**: `FR-018`
**Related QA**: `docs/qa/orchestrator/guide-alignment.md`

## 背景与目标

项目用户指南（`docs/guide/`）记录了 CLI 命令、子命令、参数、输出格式等信息。随着代码快速迭代，指南中的命令示例、参数名、子命令结构容易与实际实现产生漂移。此前已多次出现指南中记录的命令在实际编译产物中不存在、参数名已更改、或输出格式已变化的情况。

FR-018 建立了一个**编译驱动的文档治理流程**：实际编译项目，运行 CLI `--help` 输出，逐条对比指南内容，自动发现并修正漂移。

目标：

- 建立可重复执行的文档对齐流程，以实际编译产物为 ground truth。
- 覆盖所有 `docs/guide/` 中引用的 CLI 命令和参数。
- 输出对齐报告，明确列出差异项和建议修正。
- 对已确认的差异直接修正文档，中英文同步。

非目标：

- 不修改 CLI 本身的命令结构或参数命名。
- 不把完整的 LLM/编译驱动指南比对改造成 CI 自动门禁；稳定且高风险的局部语义可以由独立确定性 ratchet 覆盖。

## 2026-07-25 Follow-up：Agent Driver 语义子门禁

FR-126 的第三轮闭环审计证明：只检查 CLI `--help` 无法发现资源模型和执行语义漂移。EN/ZH 指南仍可能把已删除的 `runner.executor: streaming` 描述为可用配置，或把 typed driver 信号归因于旧 runner。第四轮审计进一步证明，只检查指南本身也不够：指南链接到的可执行 showcase 仍可能保留相反的旧语义。

因此新增 `scripts/qa/test-agent-driver-documentation-alignment.sh`，只冻结高风险且可确定性判断的 driver 不变量：

- `streaming` executor 不得作为可用配置出现；
- `shell` executor 仅是 parse-only round-trip 兼容；
- `driver_terminal` 和 normalized typed artifacts 是信号来源；
- 新 Agent 与回滚都使用显式 driver；
- production admission 与 runtime compatibility 分层明确。
- `docs/showcases/**/*.md` 不得把退役 executor 描述为当前运行路径；
- EN/ZH CEL 指南必须链接到存在且具备 `claude/cli`、normalized driver
  events 与 `driver_terminal` 正向语义的 mark-done showcase；
- `CHANGELOG.md` 的 `[Unreleased]` 段必须记录退役（`### Removed`、被删除的
  runner selection seam）与破坏性 manifest 变更（`legacy_runner_executor_removed`、
  `legacy_agent_command_deprecated`）。已发布段落属历史记录，不受此约束。

该脚本由 `scripts/qa-doc-lint.sh` 和 FR-126 默认 release gate 调用。

**覆盖策略**：retired-semantics 扫描以 `git ls-files '*.md'` 为默认全集，再减去
带理由的豁免清单（当前为空），而不是逐轮增长的目标文件白名单。前四轮审计
依次暴露 guide → 设计文档 → showcase → CHANGELOG，每轮都是"白名单之外的那一个"；
默认全集消除了这一类漏网面。DD-102/DD-103 等增量设计记录保留历史正文，只要求
显式标注已被当前 typed-driver seam 取代；DD-58 的事故背景和 DD-137 已完成的移交
记录同理，其措辞本身不构成当前配置主张，因此无需豁免即可通过。完整 CLI 命令树、
翻译质量和叙述性内容仍由本设计的五阶段 Skill 流程负责。

## 设计方案

### Skill 驱动的对齐流程

对齐流程以 Claude Code skill（`.claude/skills/guide-alignment/SKILL.md`）实现，通过 `/guide-alignment` 触发。选择 skill 而非独立脚本的原因：

1. CLI `--help` 输出为自然语言，适合 LLM 解析，无需编写专用解析器。
2. 文档修正涉及上下文理解（保留叙述文本、翻译中文），LLM 天然擅长。
3. Skill 可随 CLI 结构变化自适应，无需维护解析规则。

### 五阶段流程

| 阶段 | 输入 | 输出 |
|------|------|------|
| Phase 1: 编译与收集 | `cargo build --release` | 全量 `--help` 输出 |
| Phase 2: 文档解析 | `docs/guide/*.md`, `docs/guide/zh/*.md` | 文档命令映射表 |
| Phase 3: 比对分类 | Phase 1 + Phase 2 | 差异列表（Missing-in-doc / Missing-in-code / Mismatch） |
| Phase 4: 自动修正 | 差异列表 + 文档文件 | 更新后的文档 |
| Phase 5: 对齐报告 | 全部结果 | Markdown 报告 |

### 设计决策

1. **EN 为基准**：英文文档是 source of truth，中文版按段落映射同步。
2. **隐藏命令不文档化**：clap 中标记 `hide = true` 的命令不纳入对齐范围。
3. **幂等性**：连续执行两次在无代码变化时不产生 diff。
4. **仅修正事实性内容**：保留文档中的叙述性文本不变，仅修正命令/参数的事实性描述。

## 首次执行修正项

首次执行（FR-018 治理闭环）发现并修正了以下差异：

| 类别 | 详情 |
|------|------|
| Missing-in-doc | `agent` 命令组（list/cordon/uncordon/drain）及别名 `ag`/`agent ls` |
| Missing-in-doc | `secret key list` → `secret key ls`、`db migrations list` → `db migrations ls` 别名 |
| Missing-in-doc | `task list --verbose`、`task start --latest`、`task logs --timestamps` |
| Missing-in-doc | `task watch --interval`、`task trace --verbose/--json` |
| Missing-in-doc | `delete --dry-run`、`get -l/--selector`、`version --json` |
| Missing-in-doc | `store list --limit/--offset`、`store put --task-id`、`manifest validate --project` |
| Missing-in-doc | `check --workflow/--project` 标志（部分已文档化） |
| Missing-in-doc | C/S 命令列表缺少 agent 生命周期命令 |
