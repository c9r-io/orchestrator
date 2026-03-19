---
self_referential_safe: true
---

# QA 92: Dynamic Items Cycle Overflow — max_cycles Proactive Enforcement

**关联 FR**: FR-037
**关联 Design Doc**: `docs/design_doc/orchestrator/49-dynamic-items-cycle-overflow.md`
**Mock Fixture**: `fixtures/manifests/bundles/cycle-overflow-test.yaml`
**日期**: 2026-03-13

---

## Preconditions (all scenarios)

- Repository root is the current working directory.
- Rust toolchain is available.

---

## Scenario 1: Fixed mode + dynamic items 不超过 max_cycles

**Goal**: Verify fixed mode respects `max_cycles` boundary via proactive gate unit tests.

**步骤**:
```bash
# Run proactive max_cycles enforcement tests
cargo test -p orchestrator-scheduler --lib -- proactive_max_cycles_fixed_mode --nocapture
cargo test -p orchestrator-scheduler --lib -- proactive_max_cycles_fixed_mode_default --nocapture
```

**Code review**: verify proactive gate logic:
```bash
rg -n "fn proactive_max_cycles" crates/orchestrator-scheduler/src/scheduler/loop_engine/
```

**预期结果**:
- `proactive_max_cycles_fixed_mode` passes — max_cycles=2 returns 2
- `proactive_max_cycles_fixed_mode_default` passes — no max_cycles defaults to 1
- Code review confirms proactive gate checks cycle count before advancing

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

**Goal**: Verify infinite mode returns no cap (u32::MAX) when no max_cycles set, via unit tests.

**步骤**:
```bash
# Run infinite mode proactive gate tests
cargo test -p orchestrator-scheduler --lib -- proactive_max_cycles_infinite_mode_no_cap --nocapture
cargo test -p orchestrator-scheduler --lib -- proactive_max_cycles_infinite_mode_with_cap --nocapture
```

**Code review**: verify loop continuation logic for infinite mode:
```bash
rg -n "infinite\|LoopMode::Infinite\|stop_when_no_unresolved" crates/orchestrator-scheduler/src/scheduler/loop_engine/
```

**预期结果**:
- `proactive_max_cycles_infinite_mode_no_cap` passes — returns u32::MAX (no limit)
- `proactive_max_cycles_infinite_mode_with_cap` passes — respects explicit max_cycles=5
- Code review confirms infinite mode uses guard step / stop_when_no_unresolved for termination

---

## Scenario 4: Fixed mode 无 dynamic items — 回归基线

**Goal**: Verify fixed mode loop policy stops correctly at max_cycles and once mode passes through, via unit tests.

**步骤**:
```bash
# Run fixed mode loop continuation tests
cargo test -p orchestrator-scheduler --lib -- fixed_mode_stops_at_max_cycles --nocapture
cargo test -p orchestrator-scheduler --lib -- fixed_mode_defaults_to_one_cycle --nocapture
cargo test -p orchestrator-scheduler --lib -- proactive_max_cycles_once_mode_passthrough --nocapture
```

**Code review**: verify loop engine terminates properly:
```bash
rg -n "evaluate_loop_continuation\|should_continue\|LoopDecision" crates/orchestrator-scheduler/src/scheduler/loop_engine/
```

**预期结果**:
- `fixed_mode_stops_at_max_cycles` passes — loop stops when cycle reaches max_cycles
- `fixed_mode_defaults_to_one_cycle` passes — no explicit max_cycles defaults to 1 cycle
- `proactive_max_cycles_once_mode_passthrough` passes — once mode does not interfere
- Code review confirms loop engine produces terminal decision at max_cycles boundary

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
| 1 | S1: Fixed mode max_cycles | ☐ | Rewritten: proactive gate unit tests |
| 2 | S2: Dynamic items qa_testing | ☑ | Data-plane verification (DB only) |
| 3 | S3: Infinite mode | ☐ | Rewritten: loop_engine unit tests |
| 4 | S4: Fixed mode baseline | ☐ | Rewritten: loop continuation unit tests |
