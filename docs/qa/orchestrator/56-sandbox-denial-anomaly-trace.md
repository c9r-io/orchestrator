# Orchestrator - Sandbox Denial Anomaly & Empty-Change Guard

**Module**: orchestrator
**Scope**: Verify that sandbox denials surface as trace anomalies and that self_test fails on empty changes
**Scenarios**: 2
**Priority**: High

---

## Background

FR-044 identified that sandbox EPERM errors were silently swallowed, allowing implement steps to report exit_code=0 while failing to write any files. This created a false-positive chain where self_test passed because no code changed, and the pipeline looped indefinitely.

Two detection layers are now in place:
1. `task trace` anomaly detection scans for `sandbox_denied` events and reports them as `SandboxDenied` anomalies (severity: error, escalation: intervene).
2. `self_test` runs `git diff --stat HEAD` before `cargo check`; if no changes are detected, the step fails immediately.

Entry point: `orchestrator`

---

## Scenario 1: Sandbox Denial Appears in Task Trace Anomalies

### Preconditions

- macOS environment with `/usr/bin/sandbox-exec` available.
- Runtime initialized.

### Goal

Ensure that when a sandbox denies file writes during an implement step, `task trace` includes a `sandbox_denied` anomaly.

### Steps

1. Trigger a sandbox denial (reuse the deny fixture from QA-55):
   ```bash
   QA_PROJECT="qa-sandbox-anomaly"
   orchestrator delete "project/${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply --project "${QA_PROJECT}" -f fixtures/manifests/bundles/sandbox-execution-profiles.yaml
   TASK_ID=$(orchestrator task create --project "${QA_PROJECT}" --workflow sandbox-deny-root-write --name "sandbox anomaly check" --goal "sandbox anomaly check" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}" || true
   ```
2. Inspect the task trace:
   ```bash
   orchestrator task trace "${TASK_ID}" 2>/dev/null | jq '.anomalies[] | select(.rule == "sandbox_denied")'
   ```

### Expected

- The `task trace` output contains at least one anomaly with `rule: "sandbox_denied"`.
- The anomaly severity is `error` and escalation is `intervene`.
- The anomaly message identifies the step and denial count.

---

## Scenario 2: Self-Test Fails on Empty Changes

### Preconditions

- Runtime initialized with a self-bootstrap-style workflow that includes `self_test`.
- The implement step produced no code changes (simulated or real).

### Goal

Ensure self_test detects the absence of code changes and reports failure.

### Steps

1. Run the unit test that validates empty-change detection:
   ```bash
   cargo test --lib -p agent-orchestrator -- sandbox_denied
   ```
2. Alternatively, inspect self_test behavior manually:
   ```bash
   # In a clean workspace with no pending changes:
   # self_test should fail with exit_code=1 and message containing "empty_change_check"
   ```

### Expected

- When `git diff --stat HEAD` returns empty output, self_test returns `exit_code=1`.
- Error output contains `[empty_change_check] no code changes detected after implement step`.
- A `self_test_phase` event is emitted with `{"phase": "empty_change_check", "passed": false}`.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Sandbox Denial Appears in Task Trace Anomalies | ☑ | 2026-03-14 | Claude | sandbox_denied anomaly detected with severity=error, escalation=intervene |
| 2 | Self-Test Fails on Empty Changes | ☑ | 2026-03-14 | Claude | 6 unit tests pass; git diff empty → exit_code=1 verified |
