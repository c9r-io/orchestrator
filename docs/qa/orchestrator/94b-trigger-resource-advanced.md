# QA 94b: Trigger Resource — Advanced (Suspend/Resume & Preflight)

**关联 FR**: FR-039
**Split from**: `docs/qa/orchestrator/94-trigger-resource-cron-event-driven.md`
**日期**: 2026-03-15

---

## Preconditions (all scenarios)

```bash
cd core && cargo build --release && cd ..
```

---

## Scenario 1: Trigger suspend/resume (手动验证)

**步骤**:
```bash
# Apply trigger first
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
# Suspend
orchestrator trigger suspend test-cron
# Verify suspended
orchestrator get trigger  # should show suspend=true or similar
# Resume
orchestrator trigger resume test-cron
# Fire manually
orchestrator trigger fire test-cron
```

**预期**:
- suspend 后 trigger 不再自动触发
- resume 后恢复
- fire 创建一个 task 并返回 task ID

---

## Scenario 2: Preflight check validates trigger references

**步骤**:
```bash
cat <<'EOF' > /tmp/bad-trigger.yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad-ref
spec:
  cron:
    schedule: "0 0 * * * *"
  action:
    workflow: nonexistent-workflow
    workspace: nonexistent-workspace
EOF

orchestrator apply -f /tmp/bad-trigger.yaml
orchestrator check
```

**预期**:
- `orchestrator check` 输出包含 trigger_workflow_ref 和 trigger_workspace_ref 错误，指出引用不存在的 workflow/workspace

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
