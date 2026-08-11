# FR-164: 审计动作具名化与无信封缺口

## 优先级: P1

## 状态: Proposed

## 背景

计数 at `84569018`（治理 step 0 重建；原始计数 at `6678144d`，两次修订间
`resource.rs` 与 `cli_types.rs` 无变更，差异全部来自计数方法错误）。

- `single_builtin_apply_descriptor`（`crates/daemon/src/server/resource.rs:248`，
  单一消费点 `:30`）只为 **3 类** kind 给出具名审计动作
  （`source.template.apply` / `source.binding.apply` / `agent.driver.raw_args.apply`
  仅当 rawArgs 在场，`resource.rs:69-83`）；`ResourceKind` 共 **12 个变体**
  （`crates/orchestrator-config/src/cli_types.rs`）——Workflow、Agent（非
  rawArgs）、SecretStore、Trigger、RuntimePolicy 等其余全部记通用
  `resource.apply`；多文档 bundle 记 `resource_manifest` + 内容哈希。
  FR-160 系列的 binding 门禁误报（QA 157 注记）即此机制的表征。
- **更重的缺口**：`audited_mutation`（`resource.rs:46-55`）仅当调用方带审计
  信封、或 rawArgs、或两个 Source kind 时为真——无信封客户端 apply 一个
  SecretStore 或 Workflow，**不产生任何 `control_action_audit` 行**。
  对照：非 resource 路径 **26 个**动作全部具名
  （source/attention/session/handoff/resume/source_connection 族，由
  `ActionDescriptor` 构造点派生、剔除测试文件；含测试专用
  `task.boundary_test` 为 27）。
- FR-157 建立的动作词汇治理（`action_audit_mode`、reason code 单一定义点）
  覆盖了模式与理由码，未覆盖 apply 的动作命名面。

### step 0 核验修正

以下四项是 step 0 重建后对原文的更正，其中第 2、3 项改变了本 FR 的性质。

1. **计数方法错误**：原文"`cli_types.rs` 声明 40 个 `*Spec`
   （`rg -c '^pub struct '`）"两处不成立——所述方法得 **42**（统计的是全部
   struct 而非 `*Spec`），`*Spec` 后缀结构体实为 **34**
   （`grep -o '^pub struct [A-Za-z]*Spec\b' | wc -l`，两条路径互证）；文件路径
   为 `crates/orchestrator-config/src/cli_types.rs`。需求 2 的集合以
   `ResourceKind` 枚举（12）为准，该数已独立核验无误，故此项不影响方案。
   原文"非 resource 路径 24 个动作"实为 26。

2. **"完全不产生审计行"表述不准，且该不准是承重的**：无
   `control_action_audit` 行属实；但每次非 dry-run 成功 apply **无条件**写入
   `resource_versions`（每资源一行）与 `orchestrator_config_versions`，其
   `author` 为硬编码字面量 `"daemon-apply"`
   （`core/src/service/resource/mod.rs:293`）。即变更留有修订轨迹，但**不含
   actor、transport、reason code、request id**——不可归因，而非不存在。
   因此验收标准必须点名 `control_action_audit`：原措辞"审计表存在具名行"
   可被一条无论如何都会写入的 `resource_versions` 行满足，届时断言什么也
   证明不了（§4.4 代理断言，缺陷落在验收标准自身）。

3. **缺口大于原述，并回答了原文留给 step 0 的 DD-111 问题**：DD-111 §21 将
   信封定为"**每一次** process-console mutation 的持久事实来源"，§92 述
   enforced 模式"在 mutation 前拒绝缺失的 context"，`resolve_context`
   （`action_audit.rs:196-198`）确实如此实现。但其唯一调用者是 `begin`，而
   `resource.rs` 仅在 `audited_mutation` 为真时调用 `begin`——该条件的第一个
   析取项正是 `context.is_some()`。**故在 `action_audit_mode: enforced` 下，
   无信封 apply 既不被审计也不被拒绝**：恰在应当触发时，enforcement 不可达，
   开启 enforced 模式得到的是虚假保证。信封可选性**不是** DD-111 的设计意图，
   需求 1 属**一致性修复**（并修补 enforced 模式绕过），不属设计修订；原文
   "若是则需求 1 属设计修订"分支不适用。

4. **暴露面是随产品发布的一等命令，不止"裸 CLI 与脚本"**：已核验
   `orchestrator apply`（`cli/commands/resource.rs:26`）与 GUI
   （`gui/commands/resource.rs:152`）均发送 `audit: Some(..)`；但
   `orchestrator tool secret-rotate`（`cli.rs:1263` → `tool.rs:95`
   `secret_rotate_cmd` → `tool.rs:136` `audit: None`）以无信封方式改写
   **SecretStore 的密钥值**——正是需求 1 描述的场景，且在生产代码中。
   `secret_key_audit` 不覆盖此路径（它记录加密密钥生命周期，非 store 值写入）。

## 需求

### 1. 变更性 apply 无条件产生审计行

`audited_mutation` 对非 dry-run 的 apply 恒真；无信封时沿用既有
`legacy_client` 理由码路径（FR-157 已建：`FALLBACK_REASON_LEGACY_CLIENT`，
`resource.rs:110`）。行为断言：无信封 apply SecretStore → `control_action_audit`
存在具名行。

派生断言（来自核验修正 3）：`enforced` 模式下无信封 apply 被拒绝，诊断为
`action audit context is required`。此前该拒绝不可达。

配套：`secret_rotate_cmd` 补齐信封（理由码 `operator_secret_rotate`），使密钥
轮换可归因且在 enforced 模式下不被误伤。

### 2. 全 kind 具名动作

descriptor 覆盖全部 12 个枚举 kind。命名规范：`resource.<snake_kind>.apply`，
两个已发布名 `source.template.apply` / `source.binding.apply` 保持不变（它们已
出现在 DD-111、QA 157 与既有审计历史中，重命名会使这些记录失真），
`agent.driver.raw_args.apply` 保留为 Agent 在 rawArgs 在场时的覆盖名；两处例外
入 DD 记录。

实现约束：动作名与 `target_type` 各由一个**无 `_` 通配分支的穷尽 match**
给出——新增第 13 个枚举变体将无法编译。这是验收标准 2"集合由枚举派生非手写"
的实际担保，运行时数组不是。

多文档 bundle：保留单行聚合语义（`resource_manifest` / `resource.apply`），在
`canonical_request` 增补 `documents: [{kind, name}]` 清单。理由：
`control_action_audit` 以 `request_id` 为主键，DD-111 的预留/重放
（`should_execute`）是请求作用域的，逐文档展开需为单请求合成 N 个 request_id
并重做该路径。同步更新 QA 157 的口径注记（它现在教的是"bundle 记通用名
是设计"）。

### 3. 词汇表与既有治理面的对齐

`audit list --action` 的过滤文档同步（`docs/guide/07-cli-reference.md` 及 `zh/`
镜像）。FR-157 词汇门禁**无需扩展**：其 `TERMS` 治理的是模式与理由码字面量
（`"compatibility"` / `"enforced"` / `"legacy_client"`），动作名不在该面内；
此判断入 DD 记录，而非推测性地放宽门禁范围。

## 验收标准

- [ ] 行为测试：无信封 + SecretStore apply → **`control_action_audit`**（点名该表）
      存在 `action=resource.secret_store.apply`、`reason_code=legacy_client` 的行
      （负夹具：恢复 `context.is_some()` 析取项后该测试须失败，且诊断点名 SecretStore）
- [ ] `enforced` 模式 + 无信封 apply → 被拒绝，断言诊断字符串而非退出码
      （退出码无法区分拒绝分支）
- [ ] 12 kind 各有具名动作的行为断言（逐 kind apply → 查审计行动作名），
      集合由 `ResourceKind` 穷尽 match 保证完整；负夹具：重新引入
      `_ => "resource.apply"` 通配分支后须失败并点名具体 kind
- [ ] `tool secret-rotate` 携带信封的行为断言；负夹具采用**注释掉**而非删除
      该信封（§4.4：删除是作者预想中的情形，注释掉不是）
- [ ] bundle 语义决策成文 + `documents` 清单断言；QA 157 注记更新且 doc lint 绿
- [ ] 既有审计相关测试与 FR-157 词汇门禁全绿；5 个断言
      `control_action_audit` 的 QA 脚本与 5 个调用 apply 的脚本重跑通过

## 依赖与关联

- 直接承接 QA 157 的口径注记与 FR-157 的审计词汇治理；关联 DD-111
  （action audit envelope）——step 0 已核对：信封可选性非该 DD 设计意图，
  需求 1 为一致性修复，见核验修正 3。

## 未核验项（明确标注）

- 新增的每次 apply 一次信封预留带来的写入开销未实测；实施时对照既有 `begin`
  路径测量，不以推断断言。
- 此前未审计路径的响应体由 `Response::new(result)` 变为
  `attempt.response(result)`，是否有客户端依赖"缺少审计元数据"未核验。
