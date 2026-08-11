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

3. 与 apply 同理，`begin` 是 `resolve_context` 的唯一调用者，故
   `action_audit_mode: enforced` 对普通删除同样不可达——enforced 模式既不
   审计也不拒绝删除。DD-111 §21 称信封是"每一次 process-console mutation 的
   持久事实来源"；删除是 mutation，该断言对 delete 路径不成立。

唯一残留痕迹与 apply 相同且更弱：`delete_resource` 写入一条墓碑
`resource_versions` 行（`version = -1`、`spec_json = '"deleted"'`，
`core/src/persistence/repository/config.rs:417-419`），`author` 同为硬编码
字面量 `"daemon-apply"`——不可归因，且不含被删除资源的原始 spec。

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
- [ ] dry-run delete 不产生审计行
- [ ] 既有 `source.binding.delete` 与 `delete_references` 断言不回归
      （`scripts/qa/test-source-task-binding.sh` 断言前者）

## 治理顺序（与 FR-166 的依赖，2026-08-11 裁决）

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

- 其余 RPC 是否存在同类"审计层入口条件包含了该层本应裁决的条件"实例，未做
  全面清点。`begin` 的全部调用点值得逐一核对其守卫条件——这是 FR-164 与本 FR
  共同的形状，两次出现说明它不是孤例。
- `crates/integration-tests` 的 `TestOrchestratorServer` 以重实现镜像多个 RPC
  （`apply` 已确认），故该 harness 对本类缺口结构性不可见；应清点它还重实现了
  哪些 RPC，此清点独立于本 FR 的价值。
