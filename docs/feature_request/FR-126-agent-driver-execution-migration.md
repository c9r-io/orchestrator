# FR-126: Agent 执行路径迁移 — Showcase 链接闭环与文档门禁补强

## 优先级: P1

## 状态: In Progress

## 背景

FR-126 已完成 typed driver 迁移、legacy runner 退役、生产对象离线
parity，以及三轮严格闭环审计。第三轮审计把 EN/ZH 用户指南、架构、
authoring skill、DD-101/DD-127 和 governance fixture 纳入了确定性文档语义
门禁。

2026-07-25 的后续审计发现，该门禁仍以 10 个已知文件为边界，没有覆盖
`docs/showcases/`。因此 EN/ZH CEL 指南链接到的
`docs/showcases/streaming-mark-done-convergence.md` 仍在可执行的 “Run it”
章节把已删除的 `streaming` executor 描述为当前执行方式，而对应 manifest
早已迁移为 per-Agent `claude/cli` driver。

本轮再次打开 FR-126，不改变 runtime 行为，只修复当前操作说明，并把
showcase 目录和指南下游链接纳入 fail-closed 的闭环门禁。

## 目标

- 将 mark-done showcase 改写为当前 `claude/cli` typed-driver 操作指南。
- 明确 normalized `driver_tool_use` / `driver_tool_result` 事件与
  `driver_terminal` artifact 是结构化信号来源。
- 扫描全部 `docs/showcases/**/*.md`，避免未来新增 showcase 绕过同类门禁。
- 验证 EN/ZH CEL 指南引用的 showcase 存在且保持 typed-driver 正向语义。
- 将已遗漏的 showcase 旧文案加入 negative fixture，证明门禁 fail closed。
- 为 DD-102/DD-103 增加 superseded execution seam 状态说明，同时保留历史正文。
- 同步 DD-138、guide-alignment 设计/QA 与 QA-176 的治理证据。

## 非目标

- 不改变 Agent driver、runner、scheduler、CEL 或 manifest 行为。
- 不重命名现有 showcase 文件或 workflow manifest。
- 不删除 DD-101～DD-103 的历史设计过程。
- 不因历史设计记录中的旧类型名而失败；静态门禁只拒绝当前指南、架构、
  authoring skill 和 showcase 中会误导操作的语义。
- 不修改 DD-58 的问题背景或 DD-137 已明确移交给 FR-126 的历史范围。

## 需求

### 1. 当前 Showcase 对齐

- mark-done showcase 必须将执行入口描述为 Agent 的 `claude/cli` typed driver。
- 操作说明不得把 global `streaming` executor、`StreamingAgentRunner` 或
  provider-owned compatibility bridge 描述为当前机制。
- 观测说明必须使用当前 normalized driver event 与 typed artifact 名称。
- 仍需说明 MCP 完整名称到 CEL bare name 的规范化关系。

### 2. 下游链接与目录级门禁

- 文档语义脚本必须扫描全部 `docs/showcases/**/*.md`。
- EN/ZH CEL 指南必须继续引用存在的 mark-done showcase。
- 该 showcase 必须包含 `claude/cli`、`driver_tool_use`、
  `driver_tool_result` 和 `driver_terminal` 的正向语义。
- negative fixture 必须包含本次遗漏的可执行旧文案。

### 3. 历史设计状态

- DD-102/DD-103 必须用显式横幅说明其 streaming runner 术语属于历史 first cut，
  当前执行缝已由 DD-127/DD-138 取代。
- 历史正文不重写，避免篡改当时决策。
- DD-58 与 DD-137 的历史语境应在影响扫描中记录为无需修改。

### 4. 闭环证据

- `scripts/qa-doc-lint.sh` 和 FR-126 默认 aggregate 必须继续执行扩展后的门禁。
- DD-138、QA-176、guide-alignment 设计与 QA 必须记录本轮根因和新边界。
- 完整 release gate 必须从 clean tree 执行；`FR126_FAST=1` 仍只用于迭代。

## 验收标准

- [ ] mark-done showcase 只描述 `claude/cli` typed-driver 当前路径
- [ ] showcase 使用当前 driver events/artifacts 解释一周期收敛
- [ ] 所有 `docs/showcases/**/*.md` 进入 retired-semantics 扫描
- [ ] EN/ZH 指南链接与 showcase typed-driver 正向语义均有确定性断言
- [ ] negative fixture 能捕获本轮遗漏的两类 showcase 旧文案
- [ ] DD-102/DD-103 有 superseded execution seam 横幅，历史正文保留
- [ ] DD-138、QA-176、guide-alignment 设计/QA 与 FR 索引同步
- [ ] `qa-doc-lint` 与 FR-126 完整 clean-tree release gate 全部通过

## 依赖与参考

- `docs/showcases/streaming-mark-done-convergence.md`
- `docs/workflow/streaming-mark-done-convergence.yaml`
- `docs/design_doc/orchestrator/138-agent-driver-execution-migration.md`
- `docs/qa/orchestrator/176-agent-driver-execution-migration.md`
- `docs/design_doc/orchestrator/guide-alignment.md`
- `docs/qa/orchestrator/guide-alignment.md`
- `scripts/qa/test-agent-driver-documentation-alignment.sh`
- `scripts/qa/test-agent-driver-execution-migration.sh`
