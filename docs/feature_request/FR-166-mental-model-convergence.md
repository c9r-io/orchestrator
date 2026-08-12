# FR-166: 概念面收敛 —— 双词汇表、重叠 kind 与概念预算

## 优先级: P2

## 状态: Proposed

## 背景

原始计数 at `6678144d`，方法：产品分析的心智模型地图（探索代理，file 证据在案；
单一派生）。**Step 0 已于 `62ee2906` 重建全部事实，四条未通过——下列文字为
更正后版本，更正记录见文末《Step 0 事实核验记录》。** 本 FR 多数需求的合法产出
是**书面决策**而非代码——按 FR-160 需求 4 的先例，"决定不做"只要理由成文即为
有效产出。

- **概念负荷**：12 个枚举 kind（`cli_types.rs:146-171`，`ResourceKind`）+ 3 个
  文档专属 kind（WorkflowStore/CRD/StoreBackendProvider，
  `05-advanced-features.md:7,11,131`——该处"built-in kinds"列表与枚举**不一致**：
  列出 10 项，漏 Project/SourceTaskTemplate/SourceTaskBinding 三个 kind（两族）、
  多 WorkflowStore（后者是 CRD 而非内建 kind））+ ~30 运行时名词（原文未给方法，
  step 0 无法重建，见文末）+ 127 命令（`cli-surface.json` 中 `leaf==true` 的条目；
  二次派生：153 条目 − 26 非叶 = 127）+ 19 管道变量（`02-resource-model.md:175-194`
  表行计数）。
- **双词汇表**：GUI 用 Process/Wish/Attention/Sources（`gui/src/App.tsx:25-31`、
  `WishPool.tsx`），CLI/文档用 Task/Workflow；路由 `{page:"processes"; taskId?}`
  一行内两套（`gui/src/lib/routes.ts:5`）；"Wish" 从未在 docs/guide 获得定义，
  却已**以未定义形态泄漏进文档**：`docs/guide/zh/08-agent-process-console.md:482,495`
  出现小写 `wish`（"wish-pool drafting"、"**Modify wish**"）——大写 `Wish` 确为 0，
  但"文档中不存在"不成立；GUI 指南**只有中文**，且 EN TOC 直链中文内容的行数为
  **5 行而非 1 行**（`docs/guide/README.md:27` 直链 `zh/08`，另有 4 篇顶层中文
  Slack 指南列在 EN TOC 中：`slack-reaction-skill-automation.md`、
  `slack-managed-connections.md`、`slack-dedicated-app-provisioning.md`、
  `slack-managed-sandbox-certification-runbook.md`，合计 1740 行中文）。
- **重叠与歧义**：Trigger 一身**四**职——cron、task 生命周期事件、webhook
  （凭据持有者）、filesystem watcher，由 `TriggerEventSpec`
  （`cli_types.rs:605-620`，`source` + 可选 `webhook` + 可选 `filesystem`）给出；
  文档中 Trigger 章（`02-resource-model.md:308-373`）只写了 cron 与生命周期事件
  两职，webhook 一职的 YAML 反而出现在 `## 12. SourceTaskBinding` 章内
  （`02:434-445`）——即文档把 Trigger 的一个职能记在了另一个 kind 的标题下。
  EnvStore≈SecretStore（两者 spec 逐字段相同，均为 `data: HashMap<String,String>`，
  `cli_types.rs:533-546`；文档只给 "intended for" / "semantically designated"，
  无行为差异陈述，`02:294`、`05:94`——而加密与密钥轮换的真实差异存在
  （`orchestrator-security/src/{secret_store_crypto,secret_key_lifecycle,secret_key_audit}.rs`）
  却未接到 kind 选择上：`rg -i 'encrypt|rotat'` 在 guide 02 与 05 中命中 **0** 次）；
  "Store" 一词四用（EnvStore / SecretStore / WorkflowStore / StoreBackendProvider）；
  步骤执行模式 magic-by-id（`03:63-66`；机制在
  `crates/orchestrator-config/src/config/step_conventions.rs`——注册表接受*任意*
  step ID，未命中者回落 `required_capability = step_id`，故 `id:` 笔误静默改变
  执行模式且不报错）；资源模型全章显式"X 不是 Y"句**仅 1 处**
  （rg `separate from|not the same as` = 1，`02:376`）。
- **对照**：治理面有概念预算（DD-172 的 shape 自证规则，`172:63-67`，由
  `check_new_gates_name_their_shape` 门禁化），产品概念面没有对应物。

## 需求

### 1. 词汇表二选一

Process↔Task 择一为准（GUI 改词或 CLI/文档改词——后者波及 127 命令面，
大概率选前者，但须评估 GUI 用户存量）；"Wish" 要么进文档给定义、要么改名
为已有概念的修饰形态。路由/载荷字段命名随裁决对齐（可保留兼容别名，废弃
路径走 DD-137 的可解析拒绝模式）。

### 2. GUI 指南英文化

`docs/guide/08`（EN）成文，EN TOC 不再直链中文；纳入 cli-doc-parity 族门禁
的适用范围评估（现有 `scripts/qa/test-cli-doc-parity.sh:71-72` 只覆盖
`07-cli-reference.md` 的 EN/ZH 两份，08 不在其视野内）。

**Step 0 修正的范围**：EN TOC 直链中文的是 5 行而非 1 行（见背景）。仅补 EN 08
只关闭 1/5 的缺陷，其余 4 篇（1740 行）仍留在 EN TOC 里。故本需求须显式裁决
覆盖面：全译、仅译 08、或"译 08 + 其余 4 篇在 EN TOC 中标注语言并移出主表"。
裁决理由须成文——按背景段的先例，"只做 08，理由 X"是合法产出，但"没注意到
另外 4 篇"不是。

### 3. 重叠 kind 逐一裁决（书面产出即合法）

- EnvStore vs SecretStore：合并、或给出行为差异并写进 02 章。**Step 0 已确定
  该差异真实存在且文档 0 次提及**（加密 `secret_store_crypto`、轮换
  `secret_key_lifecycle`、审计 `secret_key_audit`），故"合并"选项须先驳回该证据；
  最省力的合法产出是"不合并，因为 X"+ 把 X 写进 02:294 与 05:94；
- "Store" 命名去歧义方案（至少文档层重命名区分）；
- Trigger **四**职（cron / task 生命周期事件 / webhook 凭据持有者 / filesystem
  watcher，见背景）：拆分评估（webhook 凭据持有者是否独立 kind）——产出可以是
  "不拆，理由 X"。附带一条纯缺陷：Trigger 的 webhook 与 filesystem 两职在 02 章
  Trigger 节（308-373）中完全没有文档，webhook 的 YAML 只出现在 SourceTaskBinding
  节内（02:434-445）；无论拆不拆，02 章 Trigger 节须补齐四职；
- magic-by-id：显式 `type:` 必填化评估或 lint 警告；
- `05-advanced-features.md:7` 的 kinds 列表与枚举对齐（这条是纯缺陷，直接修）。
  **但只改文本不够**：今日无任何门禁把该列表接到 `ResourceKind` 上
  （`rg 'ResourceKind' scripts/ config/` = 0 命中），手改一次即按 §4.4 shape 2
  再次漂移。修复须同时把该列表接进 `scripts/qa/test-docs-reality-alignment.sh`
  （FR-155 已建立"高权威文档断言绑定源码"的机制），并带否定夹具。

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
- [ ] EN TOC 不再直链未标注的中文内容——**5 行全部处理**（译或标注+移出主表），
      裁决理由成文；EN 08 按裁决落地；doc lint 与 parity 门禁绿
- [ ] 需求 3 五项各有书面裁决；05:7 kinds 列表缺陷修复**并接上门禁**
      （否定夹具：向 `ResourceKind` 加一个变体而不改文档 → 门禁红且诊断点名该 kind）
- [ ] 02 章 Trigger 节补齐四职（webhook 与 filesystem 今日在该节完全缺失）
- [ ] EnvStore/SecretStore 的真实行为差异（加密/轮换/审计）写进 02:294 与 05:94，
      或给出不写的理由——不得停留在 "intended for"
- [ ] 概念预算规则进入指定文档；后续 FR 模板引用它
- [ ] 全部文档门禁绿；改词若涉代码，兼容语义按 DD-137 模式并有测试

## 依赖与关联

- 与 FR-162/163/164 无代码耦合；需求 1 的裁决建议在治理计划阶段以
  AskUserQuestion 呈交（产品方向决策）。
- 关联 DD-172（预算哲学）、DD-137（可解析拒绝模式）、cli-doc-parity 基础
  设施（FR-154）。

## 未核验项（明确标注）

- GUI 存量用户对 Process/Wish 词汇的依附度未知（无遥测）——需求 1 的
  评估以文档与代码一致性为主要论据。**该项 step 0 无法关闭：无遥测即无数据，
  不是"没查"。**
- ~~127 命令中受改词波及的确切子集未清点~~ **Step 0 已派生**：127 个 leaf 中
  路径含 `task` 的为 **14** 个（`task create|delete|info|items|list|logs|pause|
  recover|resume|retry|start|timeline|trace|watch`），`about` 文案含 "task" 的为
  23 个。故"CLI 改词波及 127 命令面"高估约 9 倍——真实路径面是 14。
  **这不改变裁决方向**（Task 一词还固化在 `control_action_audit` 动作名、gRPC
  字段与全部文档里，改 CLI 的代价远不止路径数），但需求 1 的论据须换成后者，
  不能再引用"127 命令面"。
- ~30 运行时名词：原文未记方法，step 0 无可重建的派生路径，故该数**既未证实
  也未证伪**。它不支撑任何需求，保留为背景印象即可；若要用作论据须先给方法。

## Step 0 事实核验记录（at `62ee2906`）

**证实**（各经二次派生或直接源码比对）：12 个枚举 kind；127 leaf 命令；
19 管道变量；`{page:"processes"; taskId?}` 同行双词汇；EnvStore/SecretStore
spec 逐字段相同且文档只给意图性措辞；"Store" 四用；magic-by-id；
`separate from|not the same as` = 1；05:7 kinds 列表与枚举不一致；
DD-172 shape 自证规则；FR-164 的 `source.template.apply`/`source.binding.apply`
原拼写保留（`crates/daemon/src/server/resource.rs:258-275`）；
FR-167 需求 2 采用 `resource.<snake_kind>.delete`。

**未通过，已在正文更正**：

1. **"Wish 在 docs/guide 全树 0 次出现"** —— 大小写敏感为 0，不敏感为 2
   （`zh/08:482,495`）。结论（词汇未定义）不但成立且更强：未定义的词已经
   泄漏进用户文档。
2. **"Trigger 一身三职"，引 `02:441-452`** —— 实为四职（`TriggerEventSpec`
   带 `webhook` 与 `filesystem`）；且所引行段落属 `## 12. SourceTaskBinding`
   （02:410-467）而非 `## 10. Trigger`（02:308-373）。这正是 step 0 要找的
   category conflation：按原文裁决"三职拆不拆"会漏掉 filesystem 一职。
3. **"GUI 指南只有中文（EN TOC 直链 zh/08）"** —— 直链中文的是 5 行，
   需求 2 按原样只关闭 1/5。
4. **`cli_types.rs:137-162`** —— 实为 `146-171`；`03:65-68` 实为 `63-66`
   （文件自 `6678144d` 起有位移，非实质错误，一并订正以便重新派生）。
