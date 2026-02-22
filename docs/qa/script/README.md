# QA Test Scripts

Shell scripts for executable QA scenarios.

## Available Scripts

| Script | Description |
|--------|-------------|
| `test-task-pause-resume.sh` | Tests task pause and resume functionality |
| `test-task-retry.sh` | Tests task item retry functionality |
| `test-three-phase-workflow.sh` | Tests QA + Fix + Retest workflow execution |

## Shared Library

- Common helpers: `docs/qa/script/lib/common.sh`
- Capabilities:
  - repo root detection
  - binary existence check
  - common args parsing (`--workspace`, `--json`)

## Prerequisites

1. Binary exists:
   ```bash
   ls -la core/target/release/agent-orchestrator
   ```

2. Build when missing:
   ```bash
   (cd core && cargo build --release)
   ```

3. Optional tools:
   - `sqlite3` (for DB checks)
   - `jq` (for JSON parsing)

## Running Tests

```bash
./docs/qa/script/test-task-pause-resume.sh
./docs/qa/script/test-task-retry.sh --json
./docs/qa/script/test-three-phase-workflow.sh --workspace default
```

## Troubleshooting

- Binary not found:
  - rebuild release binary.
- Task stuck:
  - inspect `./scripts/orchestrator.sh task list`
  - force delete stale task if needed.
- DB lock:
  - stop concurrent runs and retry.
