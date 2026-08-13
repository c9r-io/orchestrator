# FR-167: Delete 路径的审计缺口——具名与信封双缺

## 优先级: P1

## 状态: Proposed

## 背景

计数 at `318444f3`（FR-164 闭环后），方法注明；治理时 step 0 重建。

FR-164 关闭了 apply 路径的无信封审计缺口与具名缺口。其 Phase 5 自检提出
"什么状态能满足全部验收标准而目标仍未达成"，答案是 **delete**——删除同样是
配置变更，且不可逆，但它的审计条件比修复前的 apply 更弱。

`crates/daemon/src/server/resource.rs` 的 `delete`：

```rust
let attempt = if force_references || is_source_task_binding {
    Some(super::action_audit::begin(...).await?)
} else {
    None
};
```

两个观察：

1. **11/12 个 kind 的普通删除完全不产生 `control_action_audit` 行。**
   只有 `SourceTaskBinding`（记 `source.binding.delete`）与
   `force_references` 清理（记 `delete_references`）被审计。删除一个
   SecretStore、Workflow、Agent、Trigger 或 Workspace 不留任何审计行。

2. **该条件里根本没有 `context.is_some()`——比修复前的 apply 更严重。**
   apply 旧条件至少包含该析取项，故携带信封的客户端仍被审计；delete 不包含，
   因此**即便客户端正确携带了信封，普通删除仍不记录任何行**。信封被接收、
   被忽略、被丢弃。

   **治理时重建（at `d7ef4faf`）：本条比原文更严重。** 被丢弃的不是某个假想的
   规矩客户端的信封，而是 CLI 每一次删除都发送的那一个——
   `crates/cli/src/commands/resource.rs:150-166` 无条件填充
   `audit: Some(ActionAuditContext { reason_code: "operator_resource_delete", .. })`。
   默认路径就是被丢弃的那条。

3. 与 apply 同理，`begin` 是 `resolve_context` 的唯一调用者，故
   `action_audit_mode: enforced` 对普通删除同样不可达——enforced 模式既不
   审计也不拒绝删除。DD-111 §21 称信封是"每一次 process-console mutation 的
   持久事实来源"；删除是 mutation，该断言对 delete 路径不成立。

唯一残留痕迹与 apply 相同且更弱：`delete_resource` 写入一条墓碑
`resource_versions` 行（`version = -1`、`spec_json = '"deleted"'`，
`crates/orchestrator-persistence/src/config_store.rs:403-422`），`author` 为硬编码
字面量——不可归因，且不含被删除资源的原始 spec。

> **治理时更正（at `d7ef4faf`）**：原文写作 `core/src/persistence/repository/config.rs:417-419`
> 与 `author = "daemon-apply"`，两处均不准确。该行号指向 `load_config`，不是墓碑写入；
> 墓碑由 `ConfigStore::delete_resource` 写出，author 由调用方传入，delete 路径传的是
> `"daemon-delete"`（project 删除传 `"project-delete"`，
> `core/src/service/resource/delete.rs:184,200,243,277`）。`"daemon-apply"` 是 apply 路径的
> 字面量（`core/src/service/resource/mod.rs:293`）。**结论不变**：仍是硬编码、不可归因的
> author。

## 需求

### 1. 变更性 delete 无条件产生审计行

`attempt` 对非 dry-run 的 delete 恒有；无信封时沿用 `legacy_client` 理由码。
行为断言：普通 delete 一个 SecretStore → `control_action_audit` 存在具名行。
派生断言：`enforced` 模式下无信封 delete 被拒绝。

### 2. 全 kind 具名删除动作

沿用 FR-164 确立的 `resource.<snake_kind>.delete` 规范，由无 `_` 通配的穷尽
match 给出——第 13 个 `ResourceKind` 变体须无法编译。既有
`source.binding.delete` 保留原拼写（同 FR-164 对两个 source.* apply 名的处理
理由：已入既有审计行）；`delete_references` 作为跨资源清理动作单独保留，
不并入按 kind 的命名面。

**治理时裁决（at `d7ef4faf`）三项，均为永久决定：**

- **SourceTaskTemplate 的删除名取 `source.template.delete`**，而非字面规则给出的
  `resource.source_task_template.delete`。取"一 kind 一族"而非"一规则贯穿"：apply
  已是 `source.template.apply`，分裂前缀会让"查这个模板的一切"需要两个前缀。
  即本 FR 有两个具名例外，而非一个。
- **`RuntimePolicy` 事实上不可删除**，故"12 kind 各有具名删除动作"不可能是 12 次成功。
  `canonical_project_kind`（`core/src/service/resource/delete.rs:402-423`）没有
  RuntimePolicy 分支，删除以 `unknown resource type for project delete: runtimepolicy`
  失败。审计行在执行前预留，故该动作名仍成立，只是断言的是一条 `status = failed` 行。
  记录这条不对称，而不是掩盖它。
- **删除面比 12 个 kind 更大。** `crd` / `customresourcedefinition` 与每一个 CRD 定义的
  自定义 kind 今天同样可删且同样零审计行（`core/src/service/resource/delete.rs:176-203`），
  原文未提及。对齐 apply 的既有做法：解析不到单一 builtin kind 时记通用
  `resource.delete` / `resource_manifest`（apply 侧对应 `resource.apply` /
  `resource_manifest`），一次关闭整个面。

### 3. dry-run 与 apply 对齐

dry-run delete 不审计，与 FR-164 的 `!dry_run` 语义一致，并单独断言，避免
"审计一切"被过度套用。

## 验收标准

- [ ] 行为测试：无信封 + SecretStore delete → `control_action_audit`（点名该表）
      存在 `action=resource.secret_store.delete` 的行；负夹具：恢复
      `force_references || is_source_task_binding` 条件后须失败且诊断点名 SecretStore
- [ ] **携带信封**的普通 delete 亦记录（该分支今日即为红——旧条件下信封被丢弃），
      这是与 FR-164 不同的一条：apply 旧条件尚能审计带信封的调用方，delete 不能
- [ ] `enforced` 模式 + 无信封 delete → 被拒绝，断言诊断字符串而非退出码
- [ ] 12 kind 各有具名删除动作的行为断言，集合由穷尽 match 保证
      （11 个断言 `status=succeeded`；RuntimePolicy 断言 `status=failed` 且诊断点名它）
- [ ] 无法解析为 builtin kind 的删除（CRD、自定义资源）记通用 `resource.delete` /
      `resource_manifest`
- [ ] dry-run delete 不产生审计行
- [ ] 既有 `source.binding.delete` 与 `delete_references` 断言不回归
      （`scripts/qa/test-source-task-binding.sh` 断言前者）

## 治理顺序（与 FR-166 的依赖，2026-08-11 裁决）

> **依赖已解除（2026-08-13 治理时核验）。** FR-166 已闭环且 `ResourceKind` 未变——
> DD-182 决定 2（EnvStore 与 SecretStore 不合并）与决定 3（Trigger 不拆分）。本节
> 末尾的"反向要求"亦已兑现：两条决定都显式写出"审计动作名是其下游消费者，已记录的
> 动作名永不重命名"，正是它们成为永久决定的理由。故需求 1/2/3 一并治理。

**需求 1 与需求 3 可立即治理；需求 2 应排在 FR-166 之后。**

FR-166 需求 3 把 **EnvStore 与 SecretStore 的合并**与 **Trigger 三职拆分**列为
待裁决项，两者都可能改动 `ResourceKind` 集合。而需求 2 的产出是"每 kind 一个
`resource.<kind>.delete`"——**动作名一旦写入审计行即不可更改**：这正是 FR-164
被迫保留 `source.template.apply` / `source.binding.apply` 原拼写的原因（重命名
会使已记录的审计历史失真，见 DD-177 Key Design 3）。若需求 2 先落地、FR-166
再合并掉 EnvStore，就会凭空制造一个永久遗留例外——与 FR-164 继承的那种债务
同形，且这次是自己造的。

需求 1 不依赖 kind 命名（只是让 `attempt` 对非 dry-run 恒有），需求 3 同理。
两者关闭的是安全缺口——**带信封的普通删除同样零审计行**，且 `enforced` 模式
对删除不可达——不应被一个 P2 的词汇 FR 阻塞。故切分治理，避免优先级倒置。

**反向要求**：FR-166 需求 3 做 kind 裁决时须显式记录"审计动作词汇是其下游
消费者"——合并或拆分一个 kind 等于永久固化其动作名。该条应写进 FR-166 的
裁决产出，否则这个依赖只存在于本文件，FR-166 的治理者看不到。

## 依赖与关联

- 直接承接 FR-164（DD-177、QA 214）的机制与命名规范；同一 `resolve_context`
  可达性问题的第二处实例。
- 需求 2 依赖 FR-166 对 `ResourceKind` 集合的裁决，见上节。
- 关联 DD-111：其"每一次 process-console mutation"的目标对 delete 路径仍不成立，
  FR-164 已为 apply 补记一条 conformance 注，delete 闭环时应一并更新该注。

## 未核验项（明确标注）

- ~~其余 RPC 是否存在同类"审计层入口条件包含了该层本应裁决的条件"实例，未做
  全面清点。~~ **治理时已清点（at `d7ef4faf`）**：daemon 内 `action_audit::begin`
  的全部生产调用点（session/source/source_connection/attention/handoff/trigger/resource）
  中，`crates/daemon/src/server/resource.rs:504` 是**最后一处**条件守卫，其余均为无条件
  调用。本 FR 关闭后该形状在 daemon 内清零。
- `crates/integration-tests` 的 `TestOrchestratorServer` 以重实现镜像多个 RPC
  （`apply` 已确认；**治理时确认 `delete` 同样如此**——
  `crates/integration-tests/src/lib.rs:1365-1389` 直接调用 `delete_resource`，
  从不进入审计路径），故该 harness 对本类缺口结构性不可见；应清点它还重实现了
  哪些 RPC，此清点独立于本 FR 的价值。
- **`core/src/resource/parse.rs:71 delete_resource_by_kind` 无任何生产调用方**（仅其自身
  测试调用），却携带一张含 RuntimePolicy 与 CRD 的 13 分支别名表——未来作者最可能在此
  添加别名，而添加不会有任何效果。本 FR 记录不删除（删除是另一件事）。
