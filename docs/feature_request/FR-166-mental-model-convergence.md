# FR-166: 概念面收敛 —— 双词汇表、重叠 kind 与概念预算

## 优先级: P2

## 状态: Proposed

## 背景

计数 at `6678144d`，方法：产品分析的心智模型地图（探索代理，file 证据在案；
单一派生，step 0 重建）。本 FR 多数需求的合法产出是**书面决策**而非代码——
按 FR-160 需求 4 的先例，"决定不做"只要理由成文即为有效产出。

- **概念负荷**：12 个枚举 kind（`cli_types.rs:137-162`）+ 3 个文档专属
  kind（WorkflowStore/CRD/StoreBackendProvider，`05-advanced-features.md:7,11,131`
  ——该处"built-in kinds"列表与枚举**不一致**：漏 Project/SourceTask 两族、
  多 WorkflowStore）+ ~30 运行时名词 + 127 命令 + ~19 管道变量。
- **双词汇表**：GUI 用 Process/Wish/Attention/Sources（`gui/src/App.tsx:25-31`、
  `WishPool.tsx`），CLI/文档用 Task/Workflow；路由 `{page:"processes"; taskId}`
  一行内两套；"Wish" 在 docs/guide 全树 **0 次出现**（rg）；GUI 指南
  **只有中文**（EN TOC 直链 zh/08）。
- **重叠与歧义**：Trigger 一身三职（cron/生命周期钩子/webhook 凭据持有者，
  `02-resource-model.md:310,441-452`）；EnvStore≈SecretStore（文档只给
  "intended for"，无行为差异陈述，`02:294`、`05:94`——而加密与密钥轮换的
  真实差异存在却未接到 kind 选择上）；"Store" 一词四用；步骤执行模式
  magic-by-id（`03:65-68`——`id:` 笔误静默改变执行模式）；资源模型全章
  显式"X 不是 Y"句**仅 1 处**（rg `separate from|not the same as` = 1）。
- **对照**：治理面有概念预算（DD-172 的 shape 自证规则），产品概念面没有
  对应物。

## 需求

### 1. 词汇表二选一

Process↔Task 择一为准（GUI 改词或 CLI/文档改词——后者波及 127 命令面，
大概率选前者，但须评估 GUI 用户存量）；"Wish" 要么进文档给定义、要么改名
为已有概念的修饰形态。路由/载荷字段命名随裁决对齐（可保留兼容别名，废弃
路径走 DD-137 的可解析拒绝模式）。

### 2. GUI 指南英文化

`docs/guide/08`（EN）成文，EN TOC 不再直链中文；纳入 cli-doc-parity 族门禁
的适用范围评估。

### 3. 重叠 kind 逐一裁决（书面产出即合法）

- EnvStore vs SecretStore：合并、或给出行为差异并写进 02 章；
- "Store" 命名去歧义方案（至少文档层重命名区分）；
- Trigger 三职：拆分评估（webhook 凭据持有者是否独立 kind）——产出可以是
  "不拆，理由 X"；
- magic-by-id：显式 `type:` 必填化评估或 lint 警告；
- `05-advanced-features.md:7` 的 kinds 列表与枚举对齐（这条是纯缺陷，直接修）。

**下游消费者（治理本需求时必须一并裁决）**：审计动作词汇按 kind 派生——
FR-164 已为 apply 建立 `resource.<snake_kind>.apply`（DD-177），FR-167 需求 2
将为 delete 建立对应面。**动作名一旦写入 `control_action_audit` 即不可更改**：
FR-164 正因此被迫保留 `source.template.apply` / `source.binding.apply` 的原
拼写，重命名会使已记录的审计历史失真。故本需求对 EnvStore/SecretStore 合并
与 Trigger 拆分的任何裁决，都等于永久固化相应的审计动作名——裁决产出中须
显式写明这一后果。FR-167 需求 2 已排在本 FR 之后，正是为此。

### 4. 概念预算规则

新 kind / 新顶级命令组的提案须自证"为什么不是现有概念的参数或子命令"——
一段进 CONTRIBUTING 或 orchestrator-guide skill 的规则文字 + 评审清单项。
是否门禁化按 DD-172 之问裁决（预期答案：不门禁，规则成文即可）。

## 验收标准

- [ ] 需求 1 裁决成文 + 首批对齐落地（至少路由字段与页面标题一致）
- [ ] EN 08 章存在且 TOC 修正；doc lint 与 parity 门禁绿
- [ ] 需求 3 五项各有书面裁决；kinds 列表缺陷修复
- [ ] 概念预算规则进入指定文档；后续 FR 模板引用它
- [ ] 全部文档门禁绿；改词若涉代码，兼容语义按 DD-137 模式并有测试

## 依赖与关联

- 与 FR-162/163/164 无代码耦合；需求 1 的裁决建议在治理计划阶段以
  AskUserQuestion 呈交（产品方向决策）。
- 关联 DD-172（预算哲学）、DD-137（可解析拒绝模式）、cli-doc-parity 基础
  设施（FR-154）。

## 未核验项（明确标注）

- GUI 存量用户对 Process/Wish 词汇的依附度未知（无遥测）——需求 1 的
  评估以文档与代码一致性为主要论据。
- 127 命令中受改词波及的确切子集未清点（选定方向后 step 0 派生）。
