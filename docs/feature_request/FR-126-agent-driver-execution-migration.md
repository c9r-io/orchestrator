# FR-126: Agent 执行路径迁移 — 用户指南语义与闭环门禁补强

## 优先级: P1

## 状态: In Progress

## 背景

FR-126 已完成 typed driver 迁移、legacy runner 退役、生产对象离线 parity 与完整仓库测试，但 2026-07-25 的第三轮严格审计发现：闭环门禁只覆盖 `docs/qa`、`docs/security`、`docs/uiux`，没有验证 `docs/guide`、架构说明、仓库内 Orchestrator authoring skill 与历史设计文档的当前状态说明。

因此 EN/ZH 用户指南仍把 `runner.executor: streaming` 描述为可用后端，并把 `tools_called` 等信号归因于已删除的 streaming executor；DD-101、DD-127、架构文档和 authoring skill 也保留了会误导新配置的当前时态描述。生产 governance negative fixture 同时缺少“生产清单层拒绝 / 外部运行时兼容层接受并提升”的机器可读分层说明。

本轮重新打开 FR-126，不改变 runtime 行为，只修复规范性文档并让相同漂移在默认 release gate 中自动失败。

## 目标

- EN/ZH 用户指南只把 per-Agent typed driver 描述为当前执行模型。
- 明确 `runner.executor` 是 parse-only 兼容字段：`shell` 仅用于 round-trip，`streaming` 以 `[legacy_runner_executor_removed]` 拒绝。
- 明确 structured signals 来自 typed driver artifacts（包括 `driver_terminal`），而不是已删除的 global streaming executor。
- 修正架构、authoring skill 与已发布设计文档中的当前时态漂移；历史设计内容保留为明确标注的 decision record。
- 给 governance fixture 增加 production admission 与 runtime compatibility 的分层、理由和预期结果，并由测试强制校验。
- 新增确定性文档语义门禁，接入 `qa-doc-lint` 和 FR-126 默认 release gate。

## 非目标

- 不改变 command-only manifest 的 runtime 兼容行为。
- 不移除 `runner.executor: shell` 的 parse/round-trip 兼容。
- 不重写 DD-101 的历史设计过程。
- 不新增 driver、provider、数据库字段或 UI。
- 不把 LLM 驱动的全量 guide-alignment skill 替换为静态脚本；本轮脚本只冻结 FR-126 的关键执行语义。

## 需求

### 1. 规范性文档对齐

- `docs/guide/02-resource-model.md` 与中文镜像不得把 `streaming` 文档化为可选 executor。
- `docs/guide/04-cel-prehooks.md` 与中文镜像必须把信号来源描述为 typed driver artifacts。
- `docs/guide/agent-driver-model.md`、`docs/architecture.md` 与 `.claude/skills/orchestrator-guide/` 不得建议创建新的 command-only Agent 或以其作为回滚目标。
- EN/ZH 必须同步说明 stable diagnostic、runtime promotion 和 explicit `shell/cli` 回滚。

### 2. 历史设计文档状态

- DD-101 必须明确 global streaming executor 已由 FR-126 删除；其余旧 runner 叙述是历史决策记录。
- DD-127 必须说明 command-only ingress 会告警并提升，global compatibility bridge 已删除，回滚使用 explicit `shell/cli`。
- DD-138 必须记录本轮文档门禁补强和根因。

### 3. Governance 分层

- `new-command-only-agent-is-rejected` fixture 必须声明其判定层是 production manifest governance。
- 同一 fixture 必须记录 runtime compatibility 的 `accepted=true`、稳定 warning code 与 persisted driver。
- Ruby fixture runner 必须拒绝缺少 layer/rationale 或与兼容契约不一致的 execution case。
- `execution_document_accepted?` 必须改名或注释为 production admission 判定，避免与 daemon Apply 混淆。

### 4. 自动化门禁

- 新脚本必须验证 EN/ZH、架构、authoring skill、DD-101/DD-127 和 governance fixture 的关键不变量。
- `scripts/qa-doc-lint.sh` 必须执行该脚本，使 `docs/guide` 进入常规文档 lint。
- FR-126 默认 aggregate 必须显式报告 guide/driver documentation alignment 结果。
- `FR126_FAST=1` 仍不得称为 release certification。

## 验收标准

- [ ] EN/ZH resource model 不再提供 `streaming` executor 配置，且说明 `[legacy_runner_executor_removed]`
- [ ] EN/ZH CEL 文档将 structured signals 绑定到 typed driver / `driver_terminal`
- [ ] Agent driver 指南、架构和 authoring skill 只推荐显式 driver；兼容入口与回滚边界准确
- [ ] DD-101、DD-127 当前状态说明与 FR-126 退役事实一致
- [ ] governance fixture 与 Ruby runner 机器可读地区分 production admission 和 runtime compatibility
- [ ] 文档语义检查脚本能对旧文案 negative fixture fail closed
- [ ] `qa-doc-lint` 和 FR-126 默认 aggregate 均包含该文档语义门禁
- [ ] DD-138、QA-176 和索引更新；QA/security/UIUX 影响扫描完成
- [ ] 完整 FR-126 release gate 全部通过

## 依赖与参考

- `docs/design_doc/orchestrator/138-agent-driver-execution-migration.md`
- `docs/qa/orchestrator/176-agent-driver-execution-migration.md`
- `docs/design_doc/orchestrator/guide-alignment.md`
- `docs/qa/orchestrator/guide-alignment.md`
- `scripts/qa-doc-lint.sh`
- `scripts/qa/test-agent-driver-execution-migration.sh`
