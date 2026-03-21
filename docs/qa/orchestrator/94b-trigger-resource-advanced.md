---
self_referential_safe: false
self_referential_safe_scenarios: [S2]
---

# QA 94b: Trigger Resource — Advanced (Suspend/Resume & Preflight)

**关联 FR**: FR-039
**Split from**: `docs/qa/orchestrator/94-trigger-resource-cron-event-driven.md`
**日期**: 2026-03-15

---

## Scenario 1: Trigger suspend/resume (手动验证，天然不安全)

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

## Scenario 2: Preflight / validation checks for trigger references

**步骤**:

1. Code review — 验证 trigger validation 拒绝无效引用:
   ```bash
   rg -n "validate_trigger|trigger_workflow_ref|trigger_workspace_ref" core/src/config_load/validate/tests.rs core/src/service/resource.rs
   ```

2. Unit test — preflight sandbox 检查:
   ```bash
   cargo test -p agent-orchestrator --lib test_sandbox_backend_preflight_issues_reports_macos_allowlist_gap
   ```

3. Unit test — manifest validate 在 self-test 中可用:
   ```bash
   cargo test -p orchestrator-scheduler --lib test_execute_self_test_step_success_with_manifest_validate
   ```

4. Unit test — trigger 验证覆盖:
   ```bash
   cargo test --package agent-orchestrator --lib validate_trigger
   ```

**预期**:
- Trigger validation 逻辑拒绝引用不存在的 workflow/workspace
- Preflight / self-test pipeline 在 trigger 引用无效时报告错误
- 所有 unit test 通过

---

## Checklist

**S2 Results** (2026-03-21 re-verified):
- Code review: `trigger_workflow_ref` / `trigger_workspace_ref` validation found at `crates/orchestrator-scheduler/src/scheduler/check/mod.rs:258-280`
- `cargo test -p agent-orchestrator --lib test_sandbox_backend_preflight_issues_reports_macos_allowlist_gap` — **PASSED**
- `cargo test -p orchestrator-scheduler --lib test_execute_self_test_step_success_with_manifest_validate` — **PASSED**
- `cargo test --package agent-orchestrator --lib trigger` — **20/20 PASSED**
- `cargo test -p orchestrator-scheduler --lib check` — **29/29 PASSED**

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | S2 (preflight validation) executed | ✅ | S1 skipped (self-referential unsafe); S2 verified: code review + 4 unit test suites passed (1 + 1 + 20 + 29 = 51 tests) |
