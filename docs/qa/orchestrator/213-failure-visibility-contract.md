---
lifecycle: active
related_fr: FR-162
self_referential_safe: true
---

# QA 213: 失败可见性契约

验证 FR-162：任务完成不吞失败证据（含同批次场景）、webhook 认证/路由失败进入
收件箱且按键合并、收件箱关闭期留下具名 gap、路由表文档由代码派生并有漂移门禁。

所有场景使用隔离守护进程（gRPC `127.0.0.1:19213`、webhook `127.0.0.1:19214`、
独立 `ORCHESTRATORD_DATA_DIR` 与 `HOME`），不触碰运行中的 orchestratord。

## 场景 1：端到端行为门禁（QA 脚本）

**Steps**

```bash
cargo build -p orchestratord -p orchestrator-cli
bash scripts/qa/test-failure-visibility.sh > /tmp/qa213.log 2>&1; echo "exit=$?"
tail -1 /tmp/qa213.log
```

**Expected result**

- exit=0，`Failure Visibility QA: 13 passed, 0 failed`；
- 场景 1 段：真实 timeline_failure 运行产生 open 的 `step_failed` 项；随后
  `approval_requested` 与 `task_completed` 两事件在**单个 SQLite 事务**内提交
  （保证同一投影批次），approval 项 resolved 且 `resolution_json` 含
  `task_completed`，而 `step_failed` 项**仍为 open**——这是 FR-162 验收 1 的
  负夹具：完成事件到达后证据项仍可见，非"曾经存在过"。
- 场景 2 段：三次错误签名合并为**恰好一条** open `source_auth_failed` 项；
  正确签名投递后该项自动 resolved；未知 trigger 产生 `source_route_missing`
  且 summary/title/dedupe_key 均不含攻击者可控名称；未知项目零分配。
- 场景 3 段：`attention_inbox_enabled: false` 下游标推进但零物化；重开后恰好
  一条 open `inbox_projection_gap` 项，summary 含非零事件计数；
  `attention_projection_gaps` 表被 flush 清空。
- 场景 4 段：全部收件项的 summary/title 不含 payload 或密钥内容。

**闭环实测（2026-08-11）**：13 passed, 0 failed（HEAD 二进制）。

**实现前基线目击红**（同日，worktree at `5de1eba9`，用基线二进制跑本门禁，
`ORCHD`/`ORCH` 覆写）：exit=1，六条 FAIL 逐一具名——
`completion swept the failure evidence`（全任务清扫，无 kind 过滤）、
`swept condition reason is not task_completed`（旧硬编码 `condition_cleared`）、
`source_auth_failed`/`source_route_missing` 零项、auth 项无自动 resolve、
`inbox_projection_gap` 零项。已知限制：基线 schema 无
`attention_projection_gaps` 表，最后一条 sqlite 断言使运行在汇总行前截断
（exit 仍为 1，且六条 FAIL 已在截断前打印）——按 §4.4 shape 7 记录：该截断
只出现在基线方向，HEAD 方向的运行以完整汇总行终止。

## 场景 2：同批次保留语义的存储级断言

**Steps**

```bash
cargo test -p orchestrator-persistence same_batch_completion_preserves_evidence -- --nocapture
cargo test -p orchestrator-scheduler completed_task_preserves_failure_evidence resumed_task_sweeps_evidence
```

**Expected result**

- 存储级：`Upsert(step_failed)` + `Upsert(approval_required)` + `ResolveTask`
  在**同一** `apply_projection_batch` 内应用后，step_failed 项 state=open 且
  被 `active_only` 过滤器匹配；approval 项 resolved、reason=`task_completed`。
- 策略级：`task_completed` 产出的 `ResolveTask` 携带
  `preserve_kinds == ["step_failed","low_confidence","task_spawn_failed"]`、
  reason=`task_completed`；`resume_executed` 为空保留集（全清扫，理由见 DD-176）。

## 场景 3：路由修复的单元断言（R5）

**Steps**

```bash
cargo test -p orchestrator-scheduler degenerate_cycle_detected_routes_as_emitted \
  sandbox_siblings_route_to_sandbox_denied \
  output_validation_failure_materializes_step_failed \
  task_spawn_failure_routes_as_preserved_evidence
```

**Expected result**

四个测试全绿：`degenerate_cycle_detected`（发射者的真实拼写，修复前该臂从未
触发）路由为 `degenerate_loop`；沙箱三兄弟同路由 `sandbox_denied`；
`output_validation_failed` 物化为 `step_failed` 且 summary 不含 payload 的
error 字段；`task_spawn_failed` 路由且属于完成清扫的保留集。

## 场景 4：webhook 候选构造的脱敏与键界

**Steps**

```bash
cargo test -p orchestratord auth_failure_candidate_is_keyed_per_trigger_with_safe_fields_only \
  route_missing_candidate_excludes_the_attacker_controlled_name
```

**Expected result**

认证失败项按 `source-auth-failed:{project}:{trigger}` 键（trigger 未经配置
校验时降为 `:-`，不进键）；route-missing 项按项目键，敌意名称既不进键也不进
summary，只以 `short_digest` 出现。项基数由此被配置基数封顶。

## 场景 5：gap 记账的存储级生命周期

**Steps**

```bash
cargo test -p orchestrator-persistence projection_gaps_accumulate_flush_once_and_reopen
```

**Expected result**

两批 drop 折叠为单行（min/max/求和）；落后于行水位的 flush 是 no-op（fence，
防止并发折叠丢失）；当前水位 flush 产出一条 open 项并清空行；再次关闭窗口
reopen 同一项（reopen_count=1）而非第二条。

## 场景 6：路由表文档漂移门禁（ci-required）

**Steps**

```bash
bash scripts/qa/test-attention-routing-doc.sh --fixture-test; echo "exit=$?"
```

**Expected result**

exit=0，`8 passed, 0 failed`。四个负夹具及其变异选择（§4.4"选实现最不可能
捕获的变异"）：

1. **加臂而非删臂**于策略源私有副本——文档只含真行，仅派生侧移动，诊断具名
   `phantom_kind_fr162`；
2. **注释掉而非删除**EN 表一行——kind 名仍在页面上，只有锚定行提取看得见差异；
3. ZH 副本同变异——EN/ZH 行集比对拒绝并具名；
4. 无臂源文件——派生 abort（fail closed），不把空集交给比对（§4.4 shape 5）。

## 场景 7：既有行为保留（验收 6）

**Steps**

```bash
bash scripts/qa/test-attention-inbox.sh > /tmp/qa143.log 2>&1; echo "exit=$?"; tail -1 /tmp/qa143.log
```

**Expected result**

exit=0，`Attention Inbox QA: 10 passed, 0 failed`——QA 143 的全部既有断言
（step 成功事件自动 resolve 原生项、`condition_cleared` 理由、并发 claim 排他、
审计身份 2:2 等）在 FR-162 语义下继续成立。`task_completed` 对条件类项
（approval、stalled 等）的清扫行为保留，仅证据类三 kind 除外。

**闭环实测（2026-08-11）**：10 passed, 0 failed。

## Checklist

- [ ] 场景 1 端到端门禁 13 passed, 0 failed，且基线方向的目击红已留档
- [ ] 场景 2~5 的 cargo 单测全绿（保留语义、路由修复、webhook 键界、gap 生命周期）
- [ ] 场景 6 漂移门禁 `--fixture-test` 8 passed（四个负夹具各具名其变异）
- [ ] 场景 7 QA 143 复跑 10 passed（既有清扫与自动 resolve 行为保留）
- [ ] `manual-gate-freshness.json` 记录了 `test-failure-visibility.sh` 的本次运行
