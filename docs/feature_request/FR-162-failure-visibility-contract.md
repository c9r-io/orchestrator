# FR-162: 失败可见性契约 —— 步骤失败、任务状态与收件箱的三方矛盾

## 优先级: P1

## 状态: Proposed

## 背景

所有代码坐标 at `6678144d`；链路由产品分析的 UX 审计逐环核验（file:line 逐条在案），
治理时按 step 0 重建。

产品的失败传播链在每一环都倾向静默，合起来构成与用户预期的正面矛盾：

1. **步骤退出码非零不影响任务**：`success = final_exit_code == 0`
   （`crates/orchestrator-scheduler/src/scheduler/phase_runner/validate.rs:42`），但
   `step.behavior.on_failure` 默认 `Continue`
   （`crates/orchestrator-config/src/config/step.rs:75-78`，`#[default]`），item
   状态不动；硬失败仅当 `validation_status == "failed"`
   （`dispatch_builtin.rs:422-424`）——有效 JSON + exit 1 直接放行。
2. **任务状态从不看退出码**：`failed` 当且仅当 `unresolved + stale_pending > 0`
   （`loop_engine/mod.rs:360-394`）。
3. **收件项先建后删**：`step_failed` 候选确实生成（`service/attention.rs:216-224`），
   但 `task_completed` 映射为**全任务范围** `ResolveTask`
   （`attention.rs:134-142` → `attention_store.rs:748-751`，无 step/item 过滤），
   两事件常落同一 500 条批次、按序应用——用户看到绿任务 + 空收件箱。
   实测目击：FR-160 治理期间 wp05 的 store put 步骤 exit 1，任务 completed
   （QA 211 记录）。
4. **整类入站失败不落账**：webhook 的 404/401/签名失败在
   `crates/daemon/src/webhook.rs:108,117,131,138,153,340,374,446,497,529-562,927-940`
   直接返回 HTTP、不写事件——轮换了签名密钥的操作者得到零信号。
5. **收件箱关闭期永久失聪**：`attention_inbox_enabled=false` 时事件被过滤但游标
   照常推进（`attention.rs:112-124`），重开不回填、无警告。
6. **无路由臂的事件**：`output_validation_failed`、`ticket_created`、`task_spawned`
   落 `_ => None`；`spawn_task` 失败仅 `warn!`（`apply.rs:182`）。
7. **文档零覆盖**：`docs/guide/` 中无任何一处陈述"非零退出码默认不失败任务"、
   任务状态推导规则、或收件箱路由清单（`07-cli-reference.md:364-380` 只写了
   `--kind` 过滤，从未列举 kind 集合）。

对一个以 "Guardrails matter more than heroic prompting"（`docs/guide/00-vision.md`）
自我定位的产品，这是心智模型级别的缺口，不是七个孤立 bug。

## 需求

### 1. 任务完成不得吞掉未读的失败证据

`ResolveTask` 语义修订：任务完成时，`step_failed` / `low_confidence` 等
Intervention 级未读项**不被静默删除**——保留、降级或标记为"已随任务完成"，
三选一并把理由写入 DD。验收必须含行为断言：exit 1 步骤 + 任务完成后，
收件箱**可见**该项（不是"曾经存在过"）。

### 2. 入站认证/路由失败进入可观测面

webhook 签名失败与 404（trigger 不存在）至少落 source event 并投影为
一类收件项（如 `source_auth_failed`，Intervention）。限流去重（同一 trigger
的重复失败合并计数）作为需求的一部分设计，避免收件箱风暴——这是设计题，
不是加一行 insert。

### 3. 收件箱关闭期的游标语义要么回填要么具名

二选一：过滤时不推进游标（重开回填），或推进但在重开时写入一条
"覆盖间隙 [t1,t2] 的事件未投影" 的系统收件项。静默丢失不再是选项。

### 4. 《失败去了哪里》文档一节

`docs/guide/03-workflow-configuration.md`（EN+ZH）：`on_failure` 三值语义与
默认值的后果、任务状态推导规则、收件箱完整路由清单（从
`attention.rs:165-243` 派生而非手写，防 §4.4 shape 2）。

### 5. 无臂事件的逐一裁决

`output_validation_failed`/`ticket_created`/`task_spawned`/`spawn_task` 失败：
每个要么获得路由臂、要么在 DD 记录"不路由，因为 X"。产出可以是"决定不路由"，
但理由成文。

## 验收标准

- [ ] 行为测试：`on_failure` 缺省 + exit 1 + 任务完成 → 收件箱存在可见项
      （负夹具：完成事件到达后项仍在）
- [ ] 行为测试：错误签名的 webhook 投递 → 收件箱出现认证失败项；重复投递
      合并而非刷屏
- [ ] 需求 3 的选择有行为测试覆盖（关闭→事件→重开 的端到端断言）
- [ ] 文档节存在且路由清单由代码派生（有 drift 检查或生成器）
- [ ] 需求 5 每个事件有裁决记录
- [ ] 既有测试全绿；`task_completed` 清理其余类别收件项的既有行为保留
      （本 FR 不是"永不清理"）

## 依赖与关联

- 源自 2026-08-11 产品分析（UX 摩擦审计，本会话记录）；wp05 实测目击见 QA 211。
- 触及 DD-106（attention inbox 设计）、DD-112（session 控制面）的既有语义——
  step 0 时核对这两份 DD 是否已声明"完成即清空"为设计意图；若是，本 FR
  需求 1 属于设计修订而非缺陷修复，按此措辞。

## 未核验项（明确标注）

- 各环 file:line 由探索代理单一派生，**未做第二派生**——治理 step 0 逐条重建。
- "两事件常落同一批次"基于批大小 500 的代码读取推断，未实测批边界跨越时
  的行为（跨批时项是否短暂可见）。
- webhook 失败的现网频率未测——需求 2 的去重设计参数需实测支撑。
