# FR-156: pipelineVariables 清单授权面退役

## 优先级: P2

## 状态: In Progress

## 背景

这是协调坍缩账本**自己点名要求却从未立项**的后续 FR。`config/governance/coordination-collapse-ledger.json`
记录 `consumerInventory.pipelineVariables` 处于 `deprecated-blocked`、`productionConsumerCount: 2`、
`sourceBaseline.pipelineVariables: 30`，其 `next` 字段写明 "govern a follow-up FR after store and public
manifest compatibility migration"——审计确认 FR-127→149 中不存在该 FR。协调坍缩序列
（FR-118→124→125→149，DD-130/136/137）已完成 `capturesOrJsonPath`（removed，consumer 0）与
`shellRunnerExecutor`（removed，legacy agent 0），`pipelineVariables` 是最后一个未走完 strangler 流程的坐标。

## 事实重建（fr-governance Phase 2 步骤 0，at `aafe322d`）

本 FR 初稿的六项事实中**四项不成立**。每个数字都用两条独立路径推导。

| 初稿主张 | 重建结果 | 判定 |
|---|---|---|
| 账本引用与 `sourceBaseline: 30` | 一致 | ✔ |
| 2 个生产消费者 | `docs/workflow/promotion.yaml#gather_updates` 与 `docs/workflow/self-evolution.yaml#evo_apply_winner`，均为 `store_inputs`（网关 JSON 报告 + `grep -c 'store_inputs:' docs/workflow config` 双路推导，均得 2） | ✔ 身份已确认 |
| 4 项 `blockers` 与 `consumerCount: 2` 存在数量关系 | 计数器统计的是 `docs/workflow`+`config` 下的**清单级 step 触点**；blockers 描述的是 **Rust 代码级**面。DD-137:95 已用散文写明。两者无算术关系，把计数打到 0 **不解除任何 blocker** | ✘ 类别混淆 |
| 退役目标是 `PipelineVariables` 符号 | 该类型是**存活载体**：全仓 `PreservedExecutionChannels` 仅 3 处引用且全在 `pipeline.rs` 内，即它只能经 `PipelineVariables.preserved` 抵达；`ExecutionSignals` 同理。死路是清单授权面，不是类型 | ✘ 倒置退役目标 |
| 棘轮 30 将降至新事实值 | 逐行枚举 30 处，全部是存活载体的 import 与函数签名。迁移两个消费者移除 **0 行**。唯一可达的下降来自删除 `step_vars` 覆盖层（30 → 27） | ✘ 原表述不可支撑 |
| 验收 1 "由再生工具产出而非手改" | `--emit-inventory` 只产出 `retirement.shellRunnerExecutor.productionAgents`（`coordination-governance.rb:433,449`），`consumerInventory` 无 emitter | ✘ 按原文不可满足 |

另有两项发现位于网关本身而非本 FR：

- `pipeline_consumer_kinds`（`coordination-governance.rb:546`）列了六种 kind。`outputs` 与 `pipe_to`
  **不是 `WorkflowStepSpec` 的字段**（`cli_types.rs:1070–1170`），根本无法抵达 apply 校验；`capture`
  已被硬拒绝且与 `capturesOrJsonPath` 坐标重复计数。六种中只有三种存活。
- `PostAction::StorePut { store, key, from_var }`（`config/step.rs:148`）文档自述为"把一个 pipeline
  变量写入 workflow store"，却**不被任何坐标计数**——`capture_consumers` 只在 `post_action` 带
  `json_path` 时才收。这是一个存活但未计数的通用变量消费者，即技能 §4.4 shape 2 出现在网关自己的枚举里。

因此真实的存活清单面是四种：`store_inputs`、`store_outputs`、`step_vars`、`post_actions[].store_put`。
除两处 `store_inputs` 外，其余生产消费者数为 0。

## 需求

### 1. 消费者迁移

- 迁移上表确认的 2 个生产消费者，改为步骤自取：`orchestrator store get <store> <key> --project {project_id}`；
- 为此新增 `{project_id}` 上下文模板变量（与 `{task_id}`/`{workspace_root}` 同类，**不**经 `PipelineVariables.vars`）；
- 每个迁移对象按技能 §4.3 记录 pre-migration baseline 并做 per-object 对比。

### 2. 拒绝语义落地

- manifest 校验对四种存活清单形态给出 `[legacy_pipeline_variables_removed]` 拒绝（与既有
  `[legacy_*_removed]` 前缀一致），诊断中点名具体字段；
- 按 DD-137 既定原则，spec 类型保持可反序列化，使诊断稳定而非退化为 unknown-field 错误；
- 同步 FR-152 的错误码词汇表（`docs/guide/error-codes.md` 与 `zh/`，由 `test-error-code-glossary.sh`
  从 Rust 源双向断言）。

### 3. 账本与棘轮收敛

- `consumerInventory.pipelineVariables` 的 state 迁移至 `removed`、consumerCount 归零、`next` 闭环；
- 新增 `retainedCarrier` 字段写明 `PipelineVariables` 类型留存的理由与边界，参照
  `celInterpreter: 9` 的先例（保留构件允许非零 baseline 并附 `dependency` 说明）；
- 四项 `blockers` 改名为 `codeLevelBlockers`，使其不再被读成消费者计数的欠账；
- `sourceBaseline.pipelineVariables` 30 → 27，由 `--emit-baseline --write` 产出并以 diff 审阅，不得手改；
- 扩展 `--emit-inventory` 使其同时产出 `consumerInventory`（补上原验收 1 缺失的再生路径）；
- 修正 `pipeline_consumer_kinds`：移除不可达的 `outputs`/`pipe_to` 与归属另一坐标的 `capture`，
  补入 `store_put` post-action。

### 4. 保留边界显式化

- `PreservedExecutionChannels` 作为长期保留载体，在 DD 中写明其边界与不再扩张的约束（与 CEL 的
  "deterministic governance 保留"同等待遇，防止成为新的隐性遗留层）。

## 验收标准

- [ ] 账本 `pipelineVariables.state == "removed"` 且 `productionConsumerCount == 0`；计数由扩展后的
      `--emit-inventory` 产出并以 diff 审阅
- [ ] `sourceBaseline.pipelineVariables == 27`，由 `--emit-baseline --write` 产出
- [ ] 每个被迁移对象有 per-object baseline 对比证据（路径记录于 QA 文档），非单一聚合通过
- [ ] 负向 fixture：四种形态各自被 apply 拒绝且诊断点名该字段。由
      `config/governance/fixture-bundle-validity.json` + `core/src/fixture_corpus_tests.rs` 的既有
      机制承载——它断言**精确诊断文本**而非退出码，满足 §4.4 shape 7；并附 before-run
- [ ] 移除 commit 可机械 revert（记录证据位置）
- [ ] 端到端：`promotion#gather_updates` 在 store 键存在时仍输出自该 SHA 起的 git log、在键缺失时走
      无条件兜底——两个分支都断言（非计数断言）
- [ ] `retainedCarrier` 与 `codeLevelBlockers` 落账，DD 写明保留边界

## 依赖与关联

- 前置阅读：`docs/design_doc/orchestrator/130-coordination-collapse-mcp-tools.md`、
  `136-coordination-strangler-completion.md`、`137-legacy-coordination-decommission.md`、
  `140-governance-ledger-regeneration.md`。
- 用户记忆约束：不引入 LangGraph 式 typed pipeline state（该方向已在 DD-130 关闭，本 FR 是拆除而非替换）。
