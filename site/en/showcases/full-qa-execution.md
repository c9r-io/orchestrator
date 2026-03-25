# Full QA Regression Test Execution Plan

This document is for performing a **single-iteration, full QA regression test** against the current codebase, with no code modifications involved.
Applicable scenarios: after large-scale refactoring, before releases, or for periodic regression verification.

---

## 1. Task Objective

> Topic name: `Full QA Regression Test`
>
> Background:
> A comprehensive scenario-level regression test needs to be performed against all QA documents
> in the current codebase (docs/qa/orchestrator/ + docs/qa/self-bootstrap/)
> to confirm all feature points are working correctly.
>
> Task objective for this round:
> Iterate through all QA documents, execute scenario verification one by one, create tickets for failures,
> have ticket_fix attempt repairs, then execute align_tests and doc_governance to close out.
>
> Constraints:
> 1. No proactive code changes in this round; only fix issues discovered by QA via ticket_fix.
> 2. Preserve all existing behavior unchanged.
> 3. Final goal: all QA scenarios pass or have explicitly recorded reasons for failure.

### 1.1 Expected Output

1. Execution results for all QA scenarios (pass/fail/skipped).
2. Tickets (docs/ticket/) corresponding to failed scenarios.
3. Automated fixes by ticket_fix for repairable items.
4. align_tests ensures unit tests are consistent with code.
5. doc_governance ensures no documentation drift.

### 1.2 Execution Pipeline

```text
qa_testing(item) → ticket_fix(item) → align_tests(task) → doc_governance(task) → self_test → loop_guard
```

Single cycle, no plan/implement/self_restart.

---

## 2. Safety Mechanisms

### 2.1 Dual-Layer Safety Protection

The full-qa workflow uses **dual-layer marking** to ensure dangerous operations (kill daemon, restart processes, recompile binaries) are never executed:

**Layer 1 — YAML Workflow prehook (CEL expression)**

`full-qa.yaml`'s `qa_testing` step prehook:
```yaml
prehook:
  engine: cel
  when: >-
    qa_file_path.startsWith("docs/qa/")
    && qa_file_path.endsWith(".md")
    && (self_referential_safe || size(self_referential_safe_scenarios) > 0)
```

That is: documents with `self_referential_safe: true` are **fully executed**,
documents with `self_referential_safe_scenarios` are **partially executed** (only the listed scenarios),
and documents satisfying neither are **completely skipped**.

**Layer 2 — QA Document frontmatter Marking**

Dangerous QA documents declare in their file header:
```yaml
---
self_referential_safe: false
---
```

When the workspace has `self_referential: true` set, the system reads the QA document's frontmatter.
Documents with `self_referential_safe: false` (and no `self_referential_safe_scenarios`) are skipped by the prehook and will not be executed by the agent.

### 2.2 Documents Marked as Unsafe (33 total)

The following documents contain dangerous or disruptive operations such as kill daemon, restart processes, recompile binaries, create tasks, or modify resources,
and have been marked as `self_referential_safe: false`.

Of these, **26 are completely skipped** (no `self_referential_safe_scenarios`),
and **7 are partially executed** (only the listed safe scenarios).

#### docs/qa/orchestrator/ (28 total)

**Completely skipped (21):**

| File | Dangerous Operations |
|------|---------------------|
| `01-cli-agent-orchestration.md` | force delete, task create/start, apply resources |
| `02-cli-task-lifecycle.md` | force delete, task create/start, apply resources |
| `15-workflow-multi-target-files.md` | force delete, task create/start, apply resources |
| `19-scheduler-repository-refactor-regression.md` | force delete, task create/start, apply resources |
| `26-self-bootstrap-workflow.md` | `cargo build --release`, force delete, apply resources |
| `28-self-bootstrap-pipeline.md` | force delete, apply resources |
| `41-project-scoped-agent-selection.md` | force delete, task create/start, apply resources |
| `45-cli-unsafe-mode.md` | force delete, `--unsafe` mode |
| `51-primitive-composition.md` | `cargo build --release`, task create/start |
| `55-sandbox-write-boundaries.md` | force delete, task create/start, apply resources |
| `56-sandbox-denial-anomaly-trace.md` | force delete, task create/start, apply resources |
| `56-sandbox-resource-network-enforcement.md` | `cargo build --release`, kill daemon |
| `57-sandbox-resource-limits-extended.md` | `cargo build --release`, kill daemon |
| `58-control-plane-security.md` | `cargo build --release`, kill daemon |
| `60-daemon-lifecycle-runtime-metrics.md` | `cargo build --release`, kill daemon, signal ops |
| `65-grpc-control-plane-protection.md` | `cargo build --release`, kill daemon |
| `84-generate-items-regression-narrowing.md` | force delete, task create/start, apply resources |
| `87-self-referential-daemon-pid-guard.md` | kill daemon |
| `96-self-restart-socket-continuity.md` | `cargo build`, `exec()` self-replacement |
| `100-agent-subprocess-daemon-pid-guard.md` | kill daemon |
| `smoke-orchestrator.md` | `cargo build --release` |

**Partially executed (7, only the listed safe scenarios):**

| File | Safe Scenarios | Dangerous Operations (skipped scenarios) |
|------|---------------|------------------------------------------|
| `20-structured-output-worker-scheduler.md` | S1, S2, S3 | kill daemon, task create/start |
| `22-performance-io-queue-optimizations.md` | S1, S2, S3 | kill daemon, task create/start |
| `54-step-execution-profiles.md` | S2, S3 | force delete, task create/start |
| `64-secretstore-key-lifecycle.md` | S5 | apply resources |
| `94b-trigger-resource-advanced.md` | S2 | apply resources |
| `99-long-lived-command-guard.md` | S5 | task create/start |
| `111-daemon-proper-daemonize.md` | — | kill daemon, signal ops, daemon stop |

> Note: `111-daemon-proper-daemonize.md` is marked as false with no scenarios, categorized as completely skipped.
> It is listed here for easy cross-reference; the actual skipped count is 22 orchestrator documents.

#### docs/qa/self-bootstrap/ (5 total)

**Completely skipped (4):**

| File | Dangerous Operations |
|------|---------------------|
| `01-survival-binary-checkpoint-self-test.md` | `cargo build --release`, `exec()` self-replacement |
| `04-cycle2-validation-and-runtime-timestamps.md` | `cargo build --release`, `exec()` self-replacement |
| `07-self-restart-process-continuity.md` | `cargo build --release`, `exec()` self-replacement |
| `smoke-self-bootstrap.md` | smoke test (includes daemon interaction) |

**Partially executed (1):**

| File | Safe Scenarios | Dangerous Operations (skipped scenarios) |
|------|---------------|------------------------------------------|
| `02-survival-enforcement-watchdog.md` | S1, S2, S3 | kill daemon, signal ops, file deletion |

### 2.3 Safe QA Documents (approximately 124)

| Category | Count | Description |
|----------|-------|-------------|
| Explicit `self_referential_safe: true` | 89 | Fully executed |
| No frontmatter marking (default safe) | 28 | Fully executed |
| `false` + has `scenarios` | 7 | Partially executed (safe scenarios only) |
| **Total executable** | **124** | |

Executable documents include:
- Pure unit test documents (`cargo test --lib`)
- CLI command verification (`orchestrator get/apply/check` and other read-only operations)
- Database query verification (`orchestrator event list` / `orchestrator db status`, etc.)
- Configuration verification documents
- Documentation format/structure verification

> **Note**: Documents with `self_referential_safe_scenarios` are **partially executed** (only the listed scenarios).
> In metrics reporting, these documents count as "executed".

---

## 3. Execution Steps

### 3.1 Build and Confirm Daemon Is Running

```bash
cd "$ORCHESTRATOR_ROOT"   # your orchestrator project directory

# Confirm daemon is running
ps aux | grep orchestratord | grep -v grep

# If not running:
# nohup ./target/release/orchestratord --foreground --workers 4 > /tmp/orchestratord.log 2>&1 &
```

### 3.2 Load full-qa Workflow Resources

```bash
# Clean up old project (if starting fresh)
# orchestrator delete project/full-qa --force

# Initialize
orchestrator init

# Load secrets and execution profiles
orchestrator apply -f your-secrets.yaml           --project self-bootstrap
# apply additional secret manifests as needed      --project self-bootstrap
orchestrator apply -f docs/workflow/execution-profiles.yaml --project self-bootstrap

# Load self-bootstrap StepTemplates (full-qa reuses these templates)
orchestrator apply -f docs/workflow/self-bootstrap.yaml --project self-bootstrap

# Load full-qa workflow
orchestrator apply -f docs/workflow/full-qa.yaml --project self-bootstrap
```

### 3.3 Create Task (Full Scan)

```bash
orchestrator task create \
  -n "full-qa-regression" \
  -w full-qa -W full-qa \
  --project self-bootstrap \
  -g "Run scenario-level regression tests on all QA documents under docs/qa/, create tickets for failures and attempt fixes, ultimately ensuring all scenarios pass or have explicitly recorded reasons for failure"
```

> Without specifying `-t`, the system automatically scans all `.md` files under `docs/qa/` as configured in `qa_targets`.
> Approximately 150 items are expected, of which about 26 will be completely skipped by prehook (`self_referential_safe: false` with no scenarios),
> about 7 will be partially executed (safe scenarios only), and approximately 117 will be fully executed.

Record the returned `<task_id>`.

---

## 4. Monitoring Methods

### 4.1 Status Monitoring

```bash
orchestrator task list
orchestrator task info <task_id>
orchestrator task trace <task_id>
orchestrator task watch <task_id>
```

Key observations:

1. Item execution progress (completed / total)
2. pass/fail/skipped distribution of the qa_testing step
3. Whether ticket_fix is processing active tickets
4. Whether any items are stuck for an extended period
5. Whether the number of unsafe documents skipped by prehook matches expectations

### 4.2 Log Monitoring

```bash
orchestrator task logs --tail 200 <task_id>
```

Key observations:

1. Execution results for each QA document
2. Ticket creation and fix status
3. Self-referential unsafe documents skipped by prehook (should see `step_skipped` events)

### 4.3 Process Monitoring

```bash
# agent subprocesses
ps aux | grep "claude -p" | grep -v grep | wc -l

# Expected maximum 4 parallel (workflow max_parallel: 4; ticket_fix step max_parallel: 2)
```

### 4.4 Mid-Execution Check

When the item segment is approximately 50% complete, you can check:

```bash
# View created tickets
ls docs/ticket/

# Count tickets
ls docs/ticket/*.md 2>/dev/null | wc -l

# Verify unsafe documents were skipped (count step_skipped events from JSON output)
orchestrator event list --task <task_id> --type step_skipped -o json
```

---

## 5. Key Checkpoints

### 5.1 Safety Checkpoint

- [ ] `full-qa.yaml` workspace's `self_referential: true` is in effect
- [ ] 26 completely unsafe QA documents were skipped by prehook (`step_skipped` events)
- [ ] 7 partially safe documents executed only the specified scenarios
- [ ] Daemon process remained stable throughout execution (PID unchanged)
- [ ] No `cargo build --release -p orchestratord` was executed

### 5.2 QA Testing Phase

- [ ] All safe QA documents were executed (approximately 124)
- [ ] Each scenario has a clear pass/fail conclusion
- [ ] Failed scenarios have corresponding ticket files

### 5.3 Ticket Fix Phase

- [ ] Active tickets were attempted to be fixed
- [ ] Scenarios pass after fix re-verification
- [ ] Unfixable tickets are preserved with recorded reasons

### 5.4 Align Tests Phase

- [ ] cargo test all pass
- [ ] cargo clippy has no warnings
- [ ] Compilation has no warnings

### 5.5 Doc Governance Phase

- [ ] QA documents have no format drift
- [ ] README/manifest consistency

### 5.6 Self Test Phase

- [ ] `cargo test` compiles successfully
- [ ] No unit test regressions

---

## 6. Success Criteria

The full QA round is considered complete when all of the following conditions are met:

1. orchestrator completed the full `full-qa` workflow and exited normally at `loop_guard`.
2. Safe QA scenario pass rate >= 90% (some environment-dependent scenario failures are acceptable).
3. All 26 completely unsafe documents were correctly skipped, and 7 partially safe documents executed only safe scenarios.
4. All tickets were processed by ticket_fix (fixed or explicitly marked as unfixable).
5. `align_tests` confirms no unit test or compilation regressions.
6. `doc_governance` confirms no documentation drift.
7. `self_test` confirms compilation and tests pass.

---

## 7. Exception Handling

| Exception | Detection Method | Resolution |
|-----------|-----------------|------------|
| Unsafe documents not skipped | `step_skipped` count < 26 | Check workspace `self_referential` setting, QA document frontmatter |
| Large number of QA documents failing with same pattern | Same-pattern tickets exceed 10 | Likely a systemic issue; pause and investigate root cause |
| Agent process deadlocked | `claude -p` process has no output for over 10 minutes | Check API quota and network |
| ticket_fix introduces new issues | align_tests fails after fix | Check ticket_fix change scope |
| Daemon memory too high | Item concurrency causes memory pressure | Reduce max_parallel to 2 |
| Daemon unexpectedly killed | PID changes or connection lost | An unsafe document bypassed the prehook; immediately abort the task |

---

## 8. Estimated Execution Time

- **Approximately 117 fully executed + 7 partially executed** x **approximately 2-5 minutes each** = approximately 60-310 minutes (4 parallel)
- 26 unsafe documents skipped (< 1 second)
- ticket_fix depends on ticket count (max_parallel: 2)
- align_tests + doc_governance + self_test approximately 10-20 minutes

Total estimate: **1.5 - 6 hours**

---

## 9. Human Role Boundaries

In this plan, the human role is limited to:

1. Launching the workflow
2. Monitoring execution progress
3. Verifying unsafe documents were correctly skipped
4. Interrupting when systemic exceptions occur
5. Recording final results

No manual intervention in specific QA scenario execution or ticket fixing.
