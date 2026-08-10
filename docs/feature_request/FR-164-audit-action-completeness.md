# FR-164: 审计动作具名化与无信封缺口

## 优先级: P1

## 状态: Proposed

## 背景

计数 at `6678144d`，方法注明；治理时 step 0 重建。

- `single_builtin_apply_descriptor`（`crates/daemon/src/server/resource.rs:248`，
  单一消费点 `:30`）只为 **3 类** kind 给出具名审计动作
  （`source.template.apply` / `source.binding.apply` / `agent.driver.raw_args.apply`
  仅当 rawArgs 在场，`resource.rs:69-83`）；`cli_types.rs` 声明 **40 个 `*Spec`**
  （`rg -c '^pub struct '`）——Workflow、Agent（非 rawArgs）、SecretStore、
  Trigger、RuntimePolicy 等其余全部记通用 `resource.apply`；多文档 bundle 记
  `resource_manifest` + 内容哈希。FR-160 系列的 binding 门禁误报
  （QA 157 注记）即此机制的表征。
- **更重的缺口**：`audited_mutation`（`resource.rs:46-55`）仅当调用方带审计
  信封、或 rawArgs、或两个 Source kind 时为真——**无信封客户端 apply 一个
  SecretStore 或 Workflow，可以完全不产生审计行**。对照：非 resource 路径
  24 个动作全部具名（source/attention/session/handoff/source_connection 族）。
- FR-157 建立的动作词汇治理（`action_audit_mode`、reason code 单一定义点）
  覆盖了模式与理由码，未覆盖 apply 的动作命名面。

## 需求

### 1. 变更性 apply 无条件产生审计行

`audited_mutation` 对非 dry-run 的 apply 恒真；无信封时沿用既有
`legacy_client` 理由码路径（FR-157 已建：`FALLBACK_REASON_LEGACY_CLIENT`，
`resource.rs:110`）。行为断言：无信封 apply SecretStore → 审计表存在具名行。

### 2. 全 kind 具名动作

descriptor 覆盖全部 12 个枚举 kind（`<domain>.<kind>.apply` 命名规范成文）；
多文档 bundle 的语义单独设计：逐文档展开记行、或保留聚合行但附 kind 清单——
二选一入 DD，并同步更新 QA 157 的口径注记（它现在教的是"bundle 记通用名
是设计"）。

### 3. 词汇表与既有治理面的对齐

新动作名进入 FR-157 的词汇治理面（如适用其派生扫描）；`audit list --action`
的过滤文档同步。

## 验收标准

- [ ] 行为测试：无信封 + SecretStore apply → 审计行在（负夹具：断言修 1 前
      的旧行为会失败）
- [ ] 12 kind 各有具名动作的行为断言（逐 kind apply → 查审计行动作名），
      集合由 ResourceKind 枚举派生非手写
- [ ] bundle 语义决策成文 + 对应断言；QA 157 注记更新且 doc lint 绿
- [ ] 既有审计相关测试与 FR-157 词汇门禁全绿

## 依赖与关联

- 直接承接 QA 157 的口径注记与 FR-157 的审计词汇治理；关联 DD-111
  （action audit envelope）——step 0 核对信封可选性是否为该 DD 的设计意图，
  若是则需求 1 属设计修订，按此措辞。

## 未核验项（明确标注）

- 40 个 Spec 中多少个实际可经 apply 变更（部分为嵌套 spec 非顶级资源）——
  step 0 以 ResourceKind 枚举为准重derive需求 2 的集合。
- GUI/Tauri 客户端是否总是带信封未核验；若是，需求 1 的现网暴露面主要是
  裸 CLI 与脚本。
