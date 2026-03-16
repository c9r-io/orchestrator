---
self_referential_safe: false
---

# QA 92: Dynamic Items Cycle Overflow — max_cycles Proactive Enforcement

**关联 FR**: FR-037
**关联 Design Doc**: `docs/design_doc/orchestrator/49-dynamic-items-cycle-overflow.md`
**Mock Fixture**: `fixtures/manifests/bundles/cycle-overflow-test.yaml`
**日期**: 2026-03-13

---

## Preconditions (all scenarios)

```bash
# 1. Build latest CLI
cd core && cargo build --release && cd ..

# 2. Reset project scope
orchestrator delete project/qa-fr037 --force 2>/dev/null || true

# 3. Deploy mock fixture
orchestrator apply -f fixtures/manifests/bundles/cycle-overflow-test.yaml --project qa-fr037
```

Mock agents used:
- `mock_architect`: `printf` deterministic JSON with `regression_targets` (capabilities: plan, qa_doc_gen, implement)
- `mock_tester`: `echo` success JSON (capabilities: qa_testing, loop_guard, self_test)

---

## Scenario 1: Fixed mode + dynamic items 不超过 max_cycles

**Workflow**: `fixed_with_dynamic_items` (`mode: fixed, max_cycles: 2`)

**步骤**:
```bash
orchestrator task create \
  --workflow fixed_with_dynamic_items \
  --project qa-fr037

orchestrator task start <task_id>
# Wait for completion
orchestrator task logs <task_id>
```

**验证**:
```bash
# 1. Check cycle_started events — must only have cycle=1 and cycle=2
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, json_extract(payload_json,'$.cycle') as cycle
   FROM events WHERE task_id='<task_id>' AND event_type='cycle_started'
   ORDER BY created_at"

# 2. Check max_cycles_enforced event exists
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events
   WHERE task_id='<task_id>' AND event_type='max_cycles_enforced'"

# 3. Verify NO degenerate_cycle_detected
sqlite3 data/agent_orchestrator.db \
  "SELECT COUNT(*) FROM events
   WHERE task_id='<task_id>' AND event_type='degenerate_cycle_detected'"
```

**预期结果**:
- `cycle_started` 事件仅出现 cycle=1 和 cycle=2
- 存在 `max_cycles_enforced` 事件，payload 包含 `"current_cycle":2,"max_cycles":2`
- 不出现 cycle=3 的 `cycle_started`
- `degenerate_cycle_detected` 计数为 0

---

## Scenario 2: Dynamic items 在 max_cycles 内完成 qa_testing

**Workflow**: 同 Scenario 1 的 task (复用同一 task_id)

**验证**:
```bash
# 1. Check items_generated event — dynamic items were created
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, payload_json FROM events
   WHERE task_id='<task_id>' AND event_type='items_generated'"

# 2. Check qa_testing step_finished events for dynamic items
sqlite3 data/agent_orchestrator.db \
  "SELECT event_type, task_item_id, json_extract(payload_json,'$.step') as step
   FROM events WHERE task_id='<task_id>'
   AND event_type='step_finished'
   AND json_extract(payload_json,'$.step')='qa_testing'"

# 3. Check task_items status
sqlite3 data/agent_orchestrator.db \
  "SELECT id, source, status, qa_file_path FROM task_items
   WHERE task_id='<task_id>'"
```

**预期结果**:
- `items_generated` 事件存在，count=2
- `qa_testing` 的 `step_finished` 事件出现（dynamic items 被处理）
- Dynamic items 状态为 `resolved`（mock agent 返回 success）

---

## Scenario 3: Infinite mode + dynamic items 正常运行

**Workflow**: `infinite_with_dynamic_items` (`mode: infinite`, `stop_when_no_unresolved: true`)

**步骤**:
```bash
orchestrator task create \
  --workflow infinite_with_dynamic_items \
  --project qa-fr037

orchestrator task start <task_id>
orchestrator task logs <task_id>
```

**验证**:
```bash
# 1. Task should complete (items resolve, stop_when_no_unresolved triggers)
sqlite3 data/agent_orchestrator.db \
  "SELECT status FROM tasks WHERE id='<task_id>'"

# 2. No max_cycles_enforced event (infinite mode, no cap)
sqlite3 data/agent_orchestrator.db \
  "SELECT COUNT(*) FROM events
   WHERE task_id='<task_id>' AND event_type='max_cycles_enforced'"

# 3. loop_guard_decision shows no_unresolved_items stop
sqlite3 data/agent_orchestrator.db \
  "SELECT payload_json FROM events
   WHERE task_id='<task_id>' AND event_type='loop_guard_decision'"
```

**预期结果**:
- Task 状态为 `completed`
- `max_cycles_enforced` 计数为 0（proactive gate 未触发）
- `loop_guard_decision` 显示 `no_unresolved_items` 或 `continue` 正常决策

---

## Scenario 4: Fixed mode 无 dynamic items — 回归基线

**Workflow**: `fixed_no_dynamic` (`mode: fixed, max_cycles: 2`, 无 generate_items)

**步骤**:
```bash
orchestrator task create \
  --workflow fixed_no_dynamic \
  --project qa-fr037

orchestrator task start <task_id>
orchestrator task logs <task_id>
```

**验证**:
```bash
# 1. cycle_started only cycle=1 and cycle=2
sqlite3 data/agent_orchestrator.db \
  "SELECT json_extract(payload_json,'$.cycle') as cycle FROM events
   WHERE task_id='<task_id>' AND event_type='cycle_started'
   ORDER BY created_at"

# 2. max_cycles_enforced event exists
sqlite3 data/agent_orchestrator.db \
  "SELECT COUNT(*) FROM events
   WHERE task_id='<task_id>' AND event_type='max_cycles_enforced'"

# 3. No items_generated (no generate_items in this workflow)
sqlite3 data/agent_orchestrator.db \
  "SELECT COUNT(*) FROM events
   WHERE task_id='<task_id>' AND event_type='items_generated'"

# 4. Task completes or fails (not stuck in loop)
sqlite3 data/agent_orchestrator.db \
  "SELECT status FROM tasks WHERE id='<task_id>'"
```

**预期结果**:
- `cycle_started` 仅 cycle=1 和 cycle=2
- `max_cycles_enforced` 存在
- `items_generated` 计数为 0
- Task 终态为 `completed` 或 `failed`（不是 `running`/`paused`）

---

## 单元测试覆盖

| 测试 | 覆盖 |
|------|------|
| `proactive_max_cycles_fixed_mode` | Fixed mode, max_cycles=2，边界值验证 |
| `proactive_max_cycles_fixed_mode_default` | Fixed mode 无 max_cycles 默认为 1 |
| `proactive_max_cycles_infinite_mode_with_cap` | Infinite mode + max_cycles=5 |
| `proactive_max_cycles_infinite_mode_no_cap` | Infinite mode 无上限返回 u32::MAX |
| `proactive_max_cycles_once_mode_passthrough` | Once mode 不干预（u32::MAX） |

**运行命令**: `cargo test --workspace --lib -- loop_engine::tests::proactive_max_cycles`

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
