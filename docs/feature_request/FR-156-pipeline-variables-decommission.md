# FR-156: pipelineVariables 退役收尾

## 优先级: P2

## 状态: Proposed

## 背景

这是协调坍缩账本**自己点名要求却从未立项**的后续 FR。`config/governance/coordination-collapse-ledger.json`（已复核 at `9bcfaa96`）记录：

```json
"pipelineVariables": {
  "state": "deprecated-blocked",
  "productionConsumerCount": 2,
  "preservedCarrier": "PreservedExecutionChannels",
  "blockers": [
    "public immutable initial and item variable bindings",
    "step-local store_inputs bindings in promotion and self-evolution",
    ...
  ]
}
```

其 `next` 字段写明 "govern a follow-up FR after store and public manifest compatibility migration"——审计确认 FR-127→149 中不存在该 FR。协调坍缩序列（FR-118→124→125→149,DD-130/136/137）已完成 `capturesOrJsonPath`（removed,consumer 0）与 `shellRunnerExecutor`（removed,legacy agent 0）,`pipelineVariables` 是最后一个未走完 strangler 流程的坐标,棘轮 baseline 中其源引用计数冻结在 30（`sourceBaseline.pipelineVariables: 30`,已复核）。

按 fr-governance Phase 2 步骤 0 的要求,实施时必须首先重建两个事实,不得信任本 FR 或账本的陈述：
- 2 个生产消费者的**当前**准确身份与用法（账本 blockers 列出 4 项,与 consumerCount 2 的关系需核实——可能存在类别混淆）;
- 退役目标的方向性:确认要移除的符号确实是死路,存活分支确实是 `PreservedExecutionChannels`（防"倒置退役目标"）。

## 需求

### 1. 消费者迁移
- 逐个迁移 2 个（以重建后的数字为准）生产消费者至 typed driver/tool 结果或 PreservedExecutionChannels;
- 每个迁移对象按技能 §4.3 记录 pre-migration baseline 并做 per-object 对比。

### 2. 拒绝语义落地
- manifest 校验对 `pipelineVariables` 形态给出 `[legacy_*_removed]` 类拒绝（错误码命名与既有 10 个前缀一致,并同步 FR-152 的错误码词汇表）。

### 3. 账本与棘轮收敛
- `coordination-collapse-ledger.json` 中 state 迁移至 `removed`、consumerCount 归零、`next` 字段闭环;
- 棘轮 `sourceBaseline.pipelineVariables` 从 30 降至新事实值——注意精确相等棘轮拒绝无声下降,需按 DD-140 的流程走账本再生工具而非手改数字。

### 4. 保留边界显式化
- `PreservedExecutionChannels` 作为长期保留载体,在 DD 中写明其边界与不再扩张的约束（与 CEL 的"deterministic governance 保留"同等待遇,防止成为新的隐性遗留层）。

## 验收标准

- [ ] 账本 `pipelineVariables.state == "removed"` 且 `productionConsumerCount == 0`,由再生工具产出而非手改
- [ ] 每个被迁移对象有 baseline 对比证据（路径记录于 QA 文档）
- [ ] 负向 fixture:含 pipelineVariables 的 manifest apply 被拒且诊断命名该字段（fixture 按 §4.4 shape 7 断言诊断而非退出码,并附 before-run）
- [ ] 移除 commit 可机械 revert（记录证据位置）
- [ ] 端到端:至少一个曾依赖该路径的 workflow 行为在迁移后仍成立（非计数断言）

## 依赖与关联

- 前置阅读:`docs/design_doc/orchestrator/130-coordination-collapse-mcp-tools.md`、`136-coordination-strangler-completion.md`、`137-legacy-coordination-decommission.md`、`140-governance-ledger-regeneration.md`。
- 用户记忆约束:不引入 LangGraph 式 typed pipeline state（该方向已在 DD-130 关闭,本 FR 是拆除而非替换）。
