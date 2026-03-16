---
self_referential_safe: false
---

# QA 94: Trigger Resource — Cron & Event-Driven Task Creation

**关联 FR**: FR-039
**关联 Design Doc**: `docs/design_doc/orchestrator/51-trigger-resource-cron-event-driven-task-creation.md`
**日期**: 2026-03-14

---

## Preconditions (all scenarios)

```bash
cd core && cargo build --release && cd ..
```

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
- `dispatch_trigger_manifest`: Trigger manifest 正确分派为 TriggerResource
- `validate_trigger_cron_ok`: 合法 cron trigger 验证通过
- `validate_trigger_event_ok`: 合法 event trigger 验证通过
- `validate_trigger_both_cron_and_event_rejected`: cron + event 同时设置被拒绝
- `validate_trigger_neither_cron_nor_event_rejected`: 两者都不设置被拒绝
- `trigger_apply_and_get`: apply 后可通过 get 取回
- `trigger_delete_removes`: delete 后不可再 get
- `trigger_yaml_roundtrip_cron`: cron trigger YAML 序列化/反序列化一致
- `trigger_yaml_roundtrip_event`: event trigger YAML 序列化/反序列化一致

---

## Scenario 3: Resource registry integration

**步骤**:
```bash
cargo test --package agent-orchestrator resource_registry_has_expected_count
cargo test --package agent-orchestrator migration_count_matches
```

**预期**:
- Registry count = 10（含 Trigger）
- Migration count = 18（含 m0018_trigger_state）

---

## Scenario 4: Full test suite regression

**步骤**:
```bash
cargo test 2>&1 | grep "^test result:"
```

**预期**:
- 所有 crate 测试通过，0 failures

---

## Scenario 5: Trigger manifest apply (手动验证)

**步骤**:
```bash
cat <<'EOF' > /tmp/test-trigger.yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: test-cron
spec:
  cron:
    schedule: "0 0 2 * * *"
    timezone: "Asia/Shanghai"
  action:
    workflow: default
    workspace: default
  concurrencyPolicy: Forbid
  suspend: false
EOF

orchestrator apply -f /tmp/test-trigger.yaml
orchestrator get trigger
orchestrator delete trigger/test-cron --force
```

**预期**:
- apply 成功，显示 `trigger / test-cron: created`
- get 输出包含 test-cron 的信息
- delete 成功移除

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |

See also: `docs/qa/orchestrator/94b-trigger-resource-advanced.md` for suspend/resume and preflight scenarios.
