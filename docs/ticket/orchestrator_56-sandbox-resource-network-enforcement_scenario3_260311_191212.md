# Ticket: CLI Panics On Broken Pipe During Piped Task Creation

**Created**: 2026-03-11 19:12:12
**QA Document**: `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md`
**Scenario**: #3
**Status**: FAILED

---

## Test Content

Create a sandbox allowlist task using the documented shell pipeline that extracts the task id from CLI output.

---

## Expected Result

`orchestrator task create ... | grep -oE ... | head -1` should return a task id cleanly without printing a Rust panic.

---

## Actual Result

The task is created successfully, but the CLI prints a panic to stderr:

---

## Repro Steps

1. Start the daemon with the latest release build.
2. Apply `fixtures/manifests/bundles/sandbox-execution-profiles.yaml` into a test project.
3. Run:
   ```bash
   ./target/release/orchestrator task create --project qa-fr006-sandbox --workflow sandbox-network-allowlist --name "sandbox allowlist unsupported" --goal "sandbox allowlist unsupported" --no-start | grep -oE '[0-9a-f-]{36}' | head -1
   ```
4. Observe stderr.

---

## Evidence

**UI/CLI Output**:
```text
thread 'main' (12908733) panicked at library/std/src/io/stdio.rs:1165:9:
failed printing to stdout: Broken pipe (os error 32)
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```

**Service Logs**:
```text
No daemon-side error. The task is still enqueued and later runs.
```

**DB Checks (if applicable)**:
```sql
SELECT task_id, status, workflow_id
FROM tasks
WHERE workflow_id = 'sandbox-network-allowlist'
ORDER BY created_at DESC
LIMIT 1;
```

---

## Analysis

**Root Cause**: The CLI does not handle a closed stdout pipe gracefully when downstream commands terminate after consuming the task id.
**Severity**: Medium
**Related Components**: Backend / CLI
