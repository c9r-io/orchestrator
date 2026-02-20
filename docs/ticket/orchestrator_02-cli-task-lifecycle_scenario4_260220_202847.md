# Ticket: Task Logs Command Returns No Output

**Created**: 2026-02-20 20:28:47
**QA Document**: `docs/qa/orchestrator/02-cli-task-lifecycle.md`
**Scenario**: #4
**Status**: FAILED

---

## Test Content
Test the `task logs` command with various options (basic, --tail, --timestamps) to view task execution logs.

---

## Expected Result
- Logs display command output
- Tail limit works correctly
- Timestamps are shown when requested

---

## Actual Result
The `task logs` command runs without error but produces no output, even though:
1. Log files exist in the filesystem at `/Volumes/Yotta/ai_native_sdlc/orchestrator/data/logs/`
2. Log files contain content (verified via direct file read)
3. Database references to log files are correct in the `command_runs` table

All tested flags (basic, --tail, --timestamps, --verbose) return empty output.

---

## Repro Steps
1. Create and start a task:
   ```bash
   cd /Volumes/Yotta/ai_native_sdlc/orchestrator
   ./src-tauri/target/release/agent-orchestrator task create --name "latest-test" --goal "Test --latest flag" --no-start
   ./src-tauri/target/release/agent-orchestrator task start --latest
   ```

2. Wait for task to execute at least one item (verify with `task info`)

3. Try to view logs:
   ```bash
   ./src-tauri/target/release/agent-orchestrator task logs 3d5cea4f
   ./src-tauri/target/release/agent-orchestrator task logs 3d5cea4f --tail 10
   ./src-tauri/target/release/agent-orchestrator task logs 3d5cea4f --timestamps
   ./src-tauri/target/release/agent-orchestrator task logs 3d5cea4f --verbose
   ```

4. All commands return no output

---

## Evidence

**CLI Output**:
```
$ ./src-tauri/target/release/agent-orchestrator task logs 3d5cea4f
(no output)

$ ./src-tauri/target/release/agent-orchestrator task logs 3d5cea4f --verbose
(no output)
```

**Database Verification**:
```bash
$ sqlite3 data/agent_orchestrator.db "SELECT stdout_path, stderr_path FROM command_runs WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '3d5cea4f-7dd4-450a-97ff-d42dfe764698') LIMIT 2;"

/Volumes/Yotta/ai_native_sdlc/orchestrator/data/logs/qa-2e2922d0-4459-4989-84ca-2d0077676da0-stdout.log|/Volumes/Yotta/ai_native_sdlc/orchestrator/data/logs/qa-2e2922d0-4459-4989-84ca-2d0077676da0-stderr.log
/Volumes/Yotta/ai_native_sdlc/orchestrator/data/logs/qa-0d5ce767-d920-43e8-9c1f-3e8b18136e49-stdout.log|/Volumes/Yotta/ai_native_sdlc/orchestrator/data/logs/qa-0d5ce767-d920-43e8-9c1f-3e8b18136e49-stderr.log
```

**File Verification**:
```bash
$ ls -lh /Volumes/Yotta/ai_native_sdlc/orchestrator/data/logs/qa-2e2922d0-4459-4989-84ca-2d0077676da0-stdout.log
-rw-r--r--@ 1 chenhan  staff    50B Feb 20 20:26 /Volumes/Yotta/ai_native_sdlc/orchestrator/data/logs/qa-2e2922d0-4459-4989-84ca-2d0077676da0-stdout.log

$ cat /Volumes/Yotta/ai_native_sdlc/orchestrator/data/logs/qa-2e2922d0-4459-4989-84ca-2d0077676da0-stdout.log
我来读取指定的QA文档并执行QA测试。
```

**Help Output** (shows conflicting flags):
```
$ ./src-tauri/target/release/agent-orchestrator task logs --help
Options:
  -f, --follow           Follow logs in real-time
  -t, --tail <TAIL>      Show last N lines [default: 100]
  -t, --timestamps       Include timestamps
```

Note: There's also a CLI design issue - both `-t` short flags conflict (used for both --tail and --timestamps).

---

## Analysis

**Root Cause**: The `task logs` command implementation is not reading or displaying the log files referenced in the database. The command completes successfully but does not output any content.

**Severity**: High - This prevents users from viewing task execution logs, which is critical for debugging and monitoring task progress.

**Related Components**: Backend (CLI command implementation, log file reading)

**Additional Issue**: CLI flag conflict - `-t` is defined for both `--tail` and `--timestamps`, which is a clap configuration error.
