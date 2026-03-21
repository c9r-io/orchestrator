---
self_referential_safe: true
---

# QA 94: Trigger Resource — Cron & Event-Driven Task Creation

**关联 FR**: FR-039
**关联 Design Doc**: `docs/design_doc/orchestrator/51-trigger-resource-cron-event-driven-task-creation.md`
**日期**: 2026-03-14

---

## Scenario 1: Unit tests — trigger engine cron scheduling

**步骤**:
```bash
cargo test --package agent-orchestrator trigger_engine
```

**预期**:
- `compute_next_fire_utc`: UTC cron expression 计算下次触发时间正确（02:00）
- `compute_next_fire_with_timezone`: Asia/Shanghai 02:00 对应 UTC 18:00
- `compute_next_fire_rejects_invalid_schedule`: 无效 cron 表达式返回错误
- `compute_next_fire_rejects_invalid_timezone`: 无效时区返回错误
- `next_cron_sleep_empty_returns_1h`: 无 cron 触发器时 sleep 1 小时
- `collect_due_triggers_finds_past_entries`: 只收集已到期的触发器

---

## Scenario 2: Unit tests — trigger resource YAML roundtrip

**步骤**:
```bash
cargo test --package agent-orchestrator trigger -- --include-ignored
```

**预期**:
- `trigger_dispatch_and_kind`: Trigger manifest 正确分派为 TriggerResource
- `trigger_validate_accepts_valid_cron`: 合法 cron trigger 验证通过
- `trigger_validate_accepts_valid_event`: 合法 event trigger 验证通过
- `trigger_validate_rejects_both_cron_and_event`: cron + event 同时设置被拒绝
- `trigger_validate_rejects_neither_cron_nor_event`: 两者都不设置被拒绝
- `trigger_apply_created_then_unchanged`: apply 创建后再次 apply 为 unchanged
- `trigger_get_from_and_delete_from`: get 取回后 delete 移除
- `trigger_yaml_roundtrip_cron`: cron trigger YAML 序列化/反序列化一致
- `trigger_yaml_roundtrip_event`: event trigger YAML 序列化/反序列化一致

---

## Scenario 3: Resource registry integration

**步骤**:
```bash
cargo test --package agent-orchestrator --lib returns_eleven_definitions
```

**预期**:
- Builtin CRD count = 11（含 Trigger、WorkflowStore、StoreBackendProvider）
- Migration 存在由 `m0018_trigger_state` 负责的 trigger 状态表

---

## Scenario 4: Full test suite regression

**步骤**:
```bash
cargo test 2>&1 | grep "^test result:"
```

**预期**:
- 所有 crate 测试通过，0 failures

---

## Scenario 5: Trigger manifest apply/get/delete round-trip (unit test)

**步骤**:
```bash
cargo test --package agent-orchestrator --lib trigger_apply_created_then_unchanged
cargo test --package agent-orchestrator --lib trigger_get_from_and_delete_from
cargo test --package agent-orchestrator --lib trigger_yaml_roundtrip_cron
```

**预期**:
- `trigger_apply_created_then_unchanged`: apply 创建后再次 apply 为 unchanged
- `trigger_get_from_and_delete_from`: get 取回后 delete 成功移除
- `trigger_yaml_roundtrip_cron`: cron trigger YAML 序列化/反序列化一致

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ⚠️ | 2026-03-21: S1=6 ✅, S2=20 ✅, S3=1 ✅, S4=2049+1 doctest FAIL (qa120), S5=3 ✅ |

See also: `docs/qa/orchestrator/94b-trigger-resource-advanced.md` for suspend/resume and preflight scenarios.
