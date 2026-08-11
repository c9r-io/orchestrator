# FR-162: 失败可见性契约 —— 步骤失败、任务状态与收件箱的三方矛盾

## 优先级: P1

## 状态: In Progress

## 背景

所有代码坐标 at `91419ac6`（治理 step 0 已逐条重建；原 `6678144d` 坐标与 HEAD 零代码漂移）。
step 0 重建修正了四处原始声明——修正内容已并入下文，原始错误记录在 DD-176。

产品的失败传播链在多个环节倾向静默，合起来构成与用户预期的正面矛盾：

1. **builtin/直连命令步骤的退出码非零不影响任务**（step 0 修正：范围收窄）。
   驱动器执行的 Agent 步骤**已经**在 exit≠0 时硬失败——
   `validate_driver_events_stage`（`crates/orchestrator-scheduler/src/scheduler/phase_runner/validate.rs:195,199`）
   直接由退出码合成 `validation_status="failed"`，且 legacy 非驱动 Agent 路径已被移除
   （`spawn.rs:254-256` bail）。静默缺口仅存在于 builtin/直连命令路径：
   `validate_phase_output_stage`（`validate.rs:43-46`）下有效输出 + exit 1 放行，
   加上 `step.behavior.on_failure` 默认 `Continue`（`crates/orchestrator-config/src/config/step.rs:70-88`，
   `#[default]`）为无操作；硬失败仅当 `validation_status == "failed"`
   （`dispatch_builtin.rs:422-424`，消费于 `item_executor/apply.rs:301-306`）。
2. **任务终态推导只数 item 状态**（step 0 修正措辞：并非"从不看退出码"——驱动路径的
   退出码经 validation_status 间接到达 failed）：`failed` 当且仅当
   `unresolved + stale_pending > 0`（`loop_engine/mod.rs:359-394`），否则 completed。
3. **收件项先建后清**（step 0 修正：`ResolveTask` 是审计化 UPDATE 至 `state='resolved'`
   并盖 `resolution_json={"reason":"condition_cleared"}`，非 DELETE；但对 active 过滤器
   等价于消失）：`step_finished{success:false}` 确实生成 `step_failed` 候选
   （`crates/orchestrator-scheduler/src/service/attention.rs:216-224`），但 `task_completed`
   映射为**全任务范围** `ResolveTask`（`attention.rs:134-142` →
   `crates/orchestrator-persistence/src/attention_store.rs:748-751`，无 kind/step/item 过滤），
   两事件常落同一 500 条批次、单事务按序应用——用户看到绿任务 + 空收件箱。
   （step 0 修正：原引 QA 211 wp05 目击不成立——QA 211 记录的是 FR-156 拒收 apply，
   与本链路无关；该证词已删除。行为由 store 级测试与 QA 213 场景 1 直接建立。）
4. **整类入站失败不落账**：webhook 的 404/401/签名失败在
   `crates/daemon/src/webhook.rs:108,116-121,130-138,140-153,334-341,368-374,441-447`
   直接返回 HTTP、最多 `warn!`、零持久化——轮换了签名密钥的操作者得到零信号。
5. **收件箱关闭期永久失聪**：`attention_inbox_enabled=false` 时事件被过滤但游标
   照常推进（`attention.rs:109` 在过滤**前**取 `last_event_id`，`:112-124` 过滤），
   重开不回填、无警告。游标唯一写者为 `apply_projection_batch`，无任何回退路径。
6. **无路由臂的事件**（step 0 扩展：远多于原列举）：`output_validation_failed`、
   `ticket_created`、`task_spawned` 等约 50 种生产事件落 `_ => None`（`attention.rs:225`）；
   `spawn_task` 失败仅 `warn!`（`item_executor/apply.rs:182`）。另有约 9 条**死臂**
   （无任何生产发射者）：`step_failed` 事件型、`task_finished`、`agent_question`、
   `decision_required`、`policy_blocked`、`budget_threshold|budget_exhausted`、
   `task_stalled`、`retry_exhausted`；以及一处**近失配**：臂写 `degenerate_cycle`
   而发射者写 `degenerate_cycle_detected`（`loop_engine/mod.rs`）——该臂从未触发。
   附带发现：`trigger_error`/`trigger_skipped` 以 `task_id=""` 发射
   （`core/src/trigger_engine.rs:457-465,754-765`），而投影器 SQL JOIN tasks——
   结构性不可投影。
7. **文档覆盖缺口**（step 0 修正：`on_failure` 默认值本身有 YAML 注释级记载，
   `docs/guide/03-workflow-configuration.md:186-206`，但其**后果**、任务状态推导规则、
   收件箱路由清单三者均无任何陈述；`07-cli-reference.md:363-388` 只写了 `--kind`
   过滤，从未列举 kind 集合）。

对一个以 "Guardrails matter more than heroic prompting"（`docs/guide/00-vision.md`）
自我定位的产品，这是心智模型级别的缺口，不是孤立 bug。

## 需求

### 1. 任务完成不得吞掉未读的失败证据

`ResolveTask` 语义修订：任务完成时，`step_failed` / `low_confidence` 等
Intervention 级未读项**不被静默清除**——保留、降级或标记为"已随任务完成"，
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
默认值的后果（按驱动/builtin 路径正确分域）、任务状态推导规则、收件箱完整
路由清单（从 `attention.rs` 派生而非手写，防 §4.4 shape 2）。

### 5. 无臂事件的逐一裁决

`output_validation_failed`/`ticket_created`/`task_spawned`/`spawn_task` 失败：
每个要么获得路由臂、要么在 DD 记录"不路由，因为 X"。产出可以是"决定不路由"，
但理由成文。step 0 追加进裁决范围：`degenerate_cycle_detected` 近失配（修复）、
`sandbox_network_blocked`/`sandbox_resource_exceeded`（与 `sandbox_denied` 同族）、
死臂清单（保留并记录）。其余约 50 种未命名 kind 与 `trigger_error` 空 task_id
不可投影问题**降入 FR-167 候选**，DD-176 记录清单。

## 验收标准

- [ ] 行为测试：`on_failure` 缺省 + exit 1 + 任务完成 → 收件箱存在可见项
      （负夹具：完成事件到达后项仍在，同批次单事务场景）
- [ ] 行为测试：错误签名的 webhook 投递 → 收件箱出现认证失败项；重复投递
      合并而非刷屏
- [ ] 需求 3 的选择有行为测试覆盖（关闭→事件→重开 的端到端断言）
- [ ] 文档节存在且路由清单由代码派生（有 drift 检查或生成器）
- [ ] 需求 5 每个事件有裁决记录
- [ ] 既有测试全绿；`task_completed` 清理其余类别收件项的既有行为保留
      （本 FR 不是"永不清理"）

## 依赖与关联

- 源自 2026-08-11 产品分析（UX 摩擦审计）。原 wp05/QA 211 目击引用经 step 0
  证伪并删除（QA 211 实记 FR-156 apply 拒收）。
- step 0 已核对 DD-106（attention inbox 设计）与 DD-112（session 控制面）：
  **DD-106 未声明"完成即清空"为设计意图**（其原则是条件域清除 + "preserve truth
  and history"）；DD-112 与 attention 无关。故需求 1 按**缺陷修复 + DD-106 增补**
  措辞，非设计翻案。

## 未核验项（明确标注）

- webhook 失败的现网频率未测——需求 2 的去重参数（30s 节流窗口）按设计判断取值，
  DD-176 记录为可调；`occurrence_count` 在风暴下为下界。
- "两事件常落同一批次"已由 store 级单事务测试直接覆盖（不再依赖批边界推断）。
