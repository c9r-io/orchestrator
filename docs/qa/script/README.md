# QA Test Scripts

Shell scripts for executable QA scenarios.

## Available Scripts

| Script | Description |
|--------|-------------|
| `test-task-pause-resume.sh` | Tests task pause and resume functionality |
| `test-task-retry.sh` | Tests task item retry functionality |
| `test-three-phase-workflow.sh` | Tests QA + Fix + Retest workflow execution |
| `test-worker-throughput.sh` | Baseline detached queue throughput and multi-worker drain time |
| `test-log-tail-latency.sh` | Baseline `task logs --tail` latency on large log files |

## Shared Library

- Common helpers: `docs/qa/script/lib/common.sh`
- Capabilities:
  - repo root detection
  - binary existence check
  - common args parsing (`--workspace`, `--project`, `--json`)
  - auto project isolation (`qa_resolve_project`)
  - per-project cleanup (`qa_reset_project_data`)

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
./docs/qa/script/test-three-phase-workflow.sh --project qa-manual-1 --workspace qa-manual-1-ws
./docs/qa/script/test-worker-throughput.sh --workers 6 --tasks 30 --json
./docs/qa/script/test-log-tail-latency.sh --append-lines 120000 --samples 5 --json
```

## Determinism Notes

- Each script uses an isolated QA project by default (auto-generated when `--project` is not provided).
- Each script resets only that project's task/runtime rows with:
  - `orchestrator qa project reset <project> --keep-config --force`
- The SQLite DB file is preserved across runs.
- Each script uses a dedicated fixture to avoid cross-scenario config drift:
  - `test-task-pause-resume.sh` -> `fixtures/manifests/bundles/pause-resume-workflow.yaml`
  - `test-task-retry.sh` -> `fixtures/manifests/bundles/retry-workflow.yaml`
  - `test-three-phase-workflow.sh` -> `fixtures/manifests/bundles/three-phase-forced.yaml`
- Fixtures now use structured JSON outputs for strict phase validation (`qa`/`fix`/`retest`/`guard`).
- Scripts perform config-level isolation with `db reset --include-config` before applying their fixture.
- Run scripts serially. They update shared active config via `apply`, so parallel runs can overwrite each other's fixture setup.

## Troubleshooting

- Binary not found:
  - rebuild release binary.
- Task stuck:
  - inspect `./scripts/orchestrator.sh task list`
  - force delete stale task if needed.
- DB lock:
  - stop concurrent runs and retry.
