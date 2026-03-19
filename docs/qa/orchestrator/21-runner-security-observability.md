---
self_referential_safe: true
---

# Orchestrator - Runner Security Boundary and Observability

**Module**: orchestrator
**Scope**: Validate runner execution boundary controls, log/output redaction, and task execution metrics observability
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates the runner boundary and observability coverage from the phase2/phase3 refactor:

- Runner policy model (`unsafe` / `allowlist`; `legacy` accepted as alias for `unsafe`) and runtime enforcement
- Pluginized runner entry (`spawn_with_runner`) behavior
- Sensitive text redaction for task logs and structured output
- Persistent task execution metrics (`task_execution_metrics`)
- `qa doctor` observability exposure for execution metrics

Default-policy initialization and backward-compatibility checks are covered in `docs/qa/orchestrator/31-runner-policy-defaults-compatibility.md`.
Step-level host/sandbox selection via `ExecutionProfile` is covered separately in `docs/qa/orchestrator/54-step-execution-profiles.md`.

Entry point: `orchestrator`

---

## Scenario 1: Allowlist Policy Schema Validation

### Preconditions

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Ensure `policy=allowlist` is rejected when `allowed_shells` or `allowed_shell_args` is empty, via unit tests.

### Steps

1. Run allowlist validation unit tests:
   ```bash
   cargo test -p agent-orchestrator --lib -- validate_rejects_allowlist_with_empty_shells --nocapture
   cargo test -p agent-orchestrator --lib -- validate_rejects_allowlist_with_empty_shell_args --nocapture
   ```

2. Run safety config tests:
   ```bash
   cargo test -p orchestrator-config --lib -- safety --nocapture
   ```

### Expected

- `validate_rejects_allowlist_with_empty_shells` passes — empty `allowed_shells` rejected
- `validate_rejects_allowlist_with_empty_shell_args` passes — empty `allowed_shell_args` rejected
- All 3 safety config tests pass (default, serde round-trip, deserialize minimal)

---

## Scenario 2: Runtime Policy Blocks Disallowed Shell

### Preconditions

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Verify runner policy enforcement logic denies disallowed shells before process spawn, via unit tests and code review.

### Steps

1. Run runner config unit tests covering policy enforcement:
   ```bash
   cargo test -p orchestrator-config --lib -- runner --nocapture
   ```

2. Code review: verify allowlist check occurs before spawn:
   ```bash
   rg -n "allowed_shells\|is_shell_allowed\|policy.*deny\|not in runner" core/src/runner/ crates/orchestrator-config/src/config/runner.rs
   ```

3. Run validation tests for runtime policy resource:
   ```bash
   cargo test -p agent-orchestrator --lib -- runtime_policy --nocapture
   ```

### Expected

- All 6 runner config tests pass (serde, defaults, policy model)
- Code review confirms allowlist check occurs in `spawn_with_runner` before `Command::new()`
- Runtime policy validation tests pass

---

## Scenario 3: Structured Output and Log Redaction

### Preconditions

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Verify redaction logic correctly replaces sensitive tokens in text output, via unit tests and code review.

### Steps

1. Run redaction unit tests:
   ```bash
   cargo test -p agent-orchestrator --lib -- redact_text --nocapture
   ```

2. Run streaming redactor tests:
   ```bash
   cargo test -p agent-orchestrator --lib -- streaming_redactor --nocapture
   ```

3. Run spawn-with-redaction integration test:
   ```bash
   cargo test -p agent-orchestrator --lib -- spawn_with_runner_and_capture_redacts_persisted_output --nocapture
   ```

4. Code review: verify redaction is applied before persistence:
   ```bash
   rg -n "redact_text\|pipe_and_redact\|redaction_patterns" core/src/runner/ core/src/output_capture.rs
   ```

### Expected

- 6 `redact_text` tests pass (pattern matching, case insensitive, multiple variants, secret values, empty patterns)
- 2 `streaming_redactor` tests pass (cross-chunk secrets, preserve visible text)
- `spawn_with_runner_and_capture_redacts_persisted_output` passes — end-to-end redaction verified
- Code review confirms redaction applied in `pipe_and_redact` before output persistence

---

## Scenario 4: task_execution_metrics Persistence

### Preconditions

- Repository root is the current working directory.
- Rust toolchain is available.

### Goal

Verify the scheduler terminal path includes metrics persistence logic, via code review and related unit tests.

### Steps

1. Code review: verify `task_execution_metrics` INSERT exists in the terminal path:
   ```bash
   rg -n "task_execution_metrics\|INSERT INTO task_execution_metrics" core/src/
   ```

2. Verify the metrics table schema is created by migration:
   ```bash
   rg -n "task_execution_metrics" core/src/persistence/migration*.rs core/src/migration.rs
   ```

3. Run scheduler terminal path unit tests:
   ```bash
   cargo test -p orchestrator-scheduler --lib -- loop_engine --nocapture
   ```

### Expected

- Code review confirms `INSERT INTO task_execution_metrics` in scheduler terminal path
- Migration creates `task_execution_metrics` table with expected columns (task_id, status, current_cycle, unresolved_items, total_items, failed_items, command_runs)
- Loop engine unit tests pass (terminal path coverage)

---

## Scenario 5: QA Doctor Exposes Observability Metrics

> **Skip**: `orchestrator qa doctor` command is not yet implemented. This scenario is blocked until the CLI subcommand is added.

### Preconditions

- `task_execution_metrics` contains records from prior runs.

### Goal

Ensure `qa doctor` exposes new metrics fields in JSON and table outputs.

### Steps

1. Run doctor in JSON mode:
   ```bash
   orchestrator qa doctor -o json
   ```
2. Run doctor in table mode:
   ```bash
   orchestrator qa doctor
   ```

### Expected

- JSON includes:
  - `observability.task_execution_metrics_total`
  - `observability.task_execution_metrics_last_24h`
  - `observability.task_completion_rate`
- Table output includes corresponding lines with non-error values.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Allowlist Policy Schema Validation | ☐ | | | Rewritten for safe mode: unit test validation |
| 2 | Runtime Policy Blocks Disallowed Shell | ☐ | | | Rewritten for safe mode: unit test + code review |
| 3 | Structured Output and Log Redaction | ☐ | | | Rewritten for safe mode: 9 redaction unit tests + code review |
| 4 | task_execution_metrics Persistence | ☐ | | | Rewritten for safe mode: code review + loop engine tests |
| 5 | QA Doctor Exposes Observability Metrics | ❌ | 2026-03-18 | claude | `orchestrator qa` subcommand does not exist (unrecognized subcommand); data preconditions met (514 rows in task_execution_metrics); ticket: qa21_s5_doctor_subcommand_20260318 |
