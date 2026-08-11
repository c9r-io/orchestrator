---
lifecycle: active
related_fr: FR-162
---

# DD-176: 失败可见性契约 —— 完成清扫的证据豁免、入站失败落账与关闭期 gap

**Status**: Released

记录 FR-162 的五项需求的设计决策、被 step 0 证伪的原始声明、逐事件裁决表与
留给 FR-167 的边界。验证证据在
[QA 213](../../qa/orchestrator/213-failure-visibility-contract.md)。

## Step 0 更正记录（原 FR 声明 vs 代码实况）

FR-162 原文四处声明未通过重建，按治理契约先改 FR 再实现；此处存档：

1. **"步骤退出码非零不影响任务"对主执行路径是反的。** 驱动器路径
   （所有 Agent 步骤，legacy 回退已在 `spawn.rs` bail）在
   `validate_driver_events_stage` 由退出码直接合成 `validation_status="failed"`
   → item unresolved → task failed。静默缺口只在 builtin/直连命令路径
   （`validate_phase_output_stage`）+ 默认 `on_failure: continue`。
2. **QA 211 的 wp05 目击不成立。** QA 211 实记 FR-156 拒收 apply（夹具残留
   `store_put`）后脚本静默截断，与"store put exit 1 + 任务 completed"无关。
   证词已删除，行为改由存储级测试与 QA 213 场景 1 直接建立。
3. **"先建后删"动词错误。** `ResolveTask` 是审计化 UPDATE 至
   `state='resolved'` 并盖 `resolution_json`，非 DELETE；但对 `active_only`
   过滤器等价于消失，症状成立。
4. **路径错误。** 投影器在 `crates/orchestrator-scheduler/src/service/attention.rs`，
   存储在 `crates/orchestrator-persistence/src/attention_store.rs`。

DD 依赖核对：DD-106 从未声明"完成即清空"为设计意图（其原则是条件域清除与
"preserve truth and history"，`task_completed` 在 DD-106 中零出现）；DD-112 与
attention 无关。故需求 1 定性为**缺陷修复 + DD-106 增补**，非设计翻案。

## R1：完成清扫的证据豁免（三选一取"保留"）

`ResolveTask` 增加 `preserve_kinds` 与 `reason`；策略层持有
`TASK_SWEEP_PRESERVED_KINDS = ["step_failed", "low_confidence", "task_spawn_failed"]`。

- **为何不取"降级"**：降级需发明第三severity或改写 kind，破坏 dedupe 键与
  `--kind` 过滤词汇的稳定性。
- **为何不取"标记为已随任务完成"**：`active_only`（`attention_store.rs`
  `attention_filter_matches`）排除 `state='resolved'`，任何 resolved 变体都
  通不过 FR 的可见性验收，除非连带改过滤语义波及 CLI/GUI/gRPC。保留是唯一
  结构性可见的选项。
- **保留集的判据**：证据类 kind 记录**已经发生**且等待人审的事实；条件类
  kind（approval、stalled、agent_question 等）描述任务终止即自然失效的等待。
  `task_spawn_failed` 入保留集：绿任务 + 未派生的子任务正是本 FR 的动机症状。
- **`resume_executed` 保持全清扫（含证据）**，理由：resume 是操作员动作，
  通常正是从证据项上发起（`retry_failed_item`/`resume_task`）；保留会让已被
  行使的项挂着不走，违反 DD-106"resolve 响应于 durable 状态变更"的既有语义。
  此为有意决策，非泄漏。
- 被清扫的条件项理由从 `condition_cleared` 分化为 `task_completed`，审计可
  区分"条件被清除"与"随任务完成"。
- **接受的后果**：完成任务的证据项无自动过期——这正是目的；出口是人工
  resolve、该步重试成功（ResolveStep 不变）、或显式 resume。

## R2：webhook 认证/路由失败落账

八个分支植入（Slack 源路径 5 处、通用路径 3 处），HTTP 响应逐字节不变：

- 复用 `upsert_external_candidate`（任务无关、按 `(project_id, dedupe_key)`
  合并计数），仿 `managed_source.rs` 的 revocation 形状。任务无关项结构性
  免疫 ResolveTask（resolve 按 task_id 匹配）。
- 两个 kind：`source_auth_failed`（Intervention，键
  `source-auth-failed:{project}:{trigger}`——所有认证分支的 trigger 名都经过
  配置解析；全局密钥路径下未解析的名字降为 `:-`，不进键）与
  `source_route_missing`（Attention，键 `source-route-missing:{project}`，
  攻击者可控名称永不进键/标题/summary，只以 `short_digest` 出现）。未知
  项目零分配。**项基数被配置基数封顶。**
- **有意偏离 FR 措辞"至少落 source event"**：不写 events/source_events 行。
  attention 行即持久投影，携带 provenance（dedupe 键、occurrence、change
  history）；空 task_id 的 events 行对投影器结构性不可见（SQL JOIN tasks），
  source_events 保留给已验证的 provider 载荷，而未通过认证的投递没有可信
  provenance。
- **风暴控制**：进程内每键 30s 最小写间隔（`INGEST_FAILURE_WRITE_INTERVAL`），
  窗口内的重复只留 `warn!`；攻击成本封顶为每键每窗口一次 SQLite 写，代价是
  `occurrence_count` 在风暴下为**下界**（记录在案，参数可调）。节流表键随
  配置基数有界。
- **自愈**：该 trigger 的首次成功投递 resolve 认证项（Slack ingest 成功后、
  通用 fire 成功后各一处），轮换密钥事故自闭环。
- 不植入的分支及理由：503（suspend/ingest 关闭是操作员已知状态）、认证后的
  4xx 载荷错误（可信发送方的 bug，风暴倾向）、500 持久化失败（独立故障域）、
  `fire_trigger_canonical` 失败（属 `trigger_error` 裁决，见 R5 遗留）。

## R3：关闭期 gap 记账（取"推进 + 具名 gap"）

- **否决"不推进游标"**：flag 按项目、游标全局单行（`attention_projector_state`），
  一个关闭的项目会扣住所有项目的投影。
- m0038 `attention_projection_gaps`：每项目单行
  {first/last_event_id, first/last_occurred_at, dropped_count}，
  drop 折叠**与游标推进同事务**提交——崩溃重放收敛，游标唯一写者不变。
- 重开时 flush 为一条 `inbox_projection_gap` 项（Attention，任务无关，键
  `inbox-projection-gap:{project}`），summary 只含计数/id 区间/时间戳。重复
  关闭窗口经正常 dedupe reopen 同一项，无泛滥。flush 以行水位为 fence：
  读后又折叠进新 drop 则本轮不动，下轮以更全区间重建。
- **flush 在重开时而非 drop 时**：DD-106 的运维语义是关闭"停止新物化"，
  关闭期间写 gap 行属于记账（侧表），物化推迟到重开——契约不破。
- 否决的替代：每项目游标（长期正确但属投影器重构，gap 表 O(项目数) 且可逆）。

## R4：文档与派生门禁

《Where Failures Go / 失败去了哪里》落在 `docs/guide/03-workflow-configuration.md`
（EN+ZH），三个生成块（路由表、完成/清扫规则、来源侧 kind 清单）由
`scripts/qa/test-attention-routing-doc.sh` 从源码派生并双向比对，ZH 块与 EN
行集全等，空提取 fail closed，四个负夹具自证（加臂/注释行/ZH 分叉/无臂源）。
取 FR-152 直接派生形状而非 FR-154 中间工件形状：路由臂是单文件字面量提取目标，
无需新增 committed artifact。`07-cli-reference.md` 的 `--kind` 与内置 guide 均
指向该表，不再生长第二个手写枚举面。ci-required，`shape` 具名 §4.4 shape 2。

## R5：逐事件裁决表

| 事件 | 裁决 | 理由 |
|---|---|---|
| `output_validation_failed` | 路由 → `step_failed` | 补驱动路径缺口：`step_finished.success` 反映 agent 自报而非校验结果；payload 补 `step_id` 使其与同步骤证据合并 |
| `spawn_task` 执行失败 | 落账 + 路由 | 新持久事件 `task_spawn_failed`（`reason_code: spawn_error`），入保留集 |
| `spawn_task` 深度截断 | 同上 | `reason_code: depth_limit`，同一事件同一臂 |
| ticket 自动创建失败（`apply.rs` warn） | 不落账 | ticket 子系统自有队列与治理流；记录在案 |
| `ticket_created` | 不路由 | 信息性；收件箱按 DD-106 只收可行动例外 |
| `task_spawned` | 不路由 | 信息性血缘，无决策需求 |
| `degenerate_cycle_detected` 近失配 | 修复 | 臂原拼写 `degenerate_cycle` 无发射者，臂从未触发——真缺陷 |
| `sandbox_network_blocked` / `sandbox_resource_exceeded` | 路由 → `sandbox_denied` | 与已路由的 `sandbox_denied` 同发射槽位（`record.rs` sandbox_event_type），一族一 kind |
| 约 9 条死臂（`step_failed` 事件型、`task_finished`、`agent_question`、`decision_required`、`policy_blocked`、`budget_threshold\|budget_exhausted`、`task_stalled`、`retry_exhausted`） | 保留，具名 | 移除会churn `--kind` 词汇（CLI/GUI/QA 文档）零用户收益；标注"保留，当前无发射者"。`approval_required` 的活别名是 `approval_requested`（`record.rs` PermissionRequested） |
| `trigger_error` / `trigger_skipped`（`task_id=""` 发射，投影器 JOIN tasks 结构性不可见） | **遗留 FR-167 候选** | 正确修复是发射侧改 external candidate 或投影器 LEFT JOIN，超出本 FR；FR-163~166 均不覆盖 |
| 其余约 50 种 `_ => None` kind（`step_stall_killed`、`dynamic_plan_failed`、`auto_rollback_failed` 等） | 遗留 FR-167 候选 | 未被 FR 点名；派生路由表使"未路由"成为文档事实而非静默 |

## 留下的限度

- `occurrence_count` 在写节流窗口内为下界（见 R2）；节流是进程内存，daemon
  重启后首个失败必写一次。
- gap 项的 `dropped_count` 统计的是**到达投影器的任务事件**数，不区分其中
  多少本会产生收件项。
- 完成任务的保留证据无自动过期；若未来出现证据堆积，出口应是批量 resolve
  工具而非恢复全清扫。
- 基线方向的 QA 213 运行在旧 schema 上因缺 `attention_projection_gaps` 表而
  在汇总行前截断（记录于 QA 213 场景 1）；HEAD 方向以完整汇总行终止。
