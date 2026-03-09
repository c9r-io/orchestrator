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
| `test-exec-interactive.sh` | Simulate `exec -it` interaction via pipe and here-doc, and verify non-tty rejection |

## Shared Library

- Common helpers: `docs/qa/script/lib/common.sh`
- Capabilities:
  - repo root detection
  - binary existence check
  - common args parsing (`--workspace`, `--project`, `--json`)
  - auto project isolation (`qa_resolve_project`)
  - additive fixture setup (`qa_apply_fixture_additive`)
  - project-local scaffold recreation (`qa_recreate_project`)

## Prerequisites

1. Binary exists:
   ```bash
   ls -la target/release/orchestrator
   ```

2. Build when missing:
   ```bash
   cargo build --release -p orchestrator-cli
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
./docs/qa/script/test-exec-interactive.sh --json
```

## Determinism Notes

- Each script uses an isolated QA project by default (auto-generated when `--project` is not provided).
- Each script recreates only that project's local scaffold with:
  - `orchestrator delete project/<project> --force`
  - `rm -rf workspace/<project>`
  - `orchestrator apply -f <fixture> --project <project>`
- The SQLite DB file is preserved across runs.
- Each script uses a dedicated fixture to avoid cross-scenario config drift:
  - `test-task-pause-resume.sh` -> `fixtures/manifests/bundles/pause-resume-workflow.yaml`
  - `test-task-retry.sh` -> `fixtures/manifests/bundles/retry-workflow.yaml`
  - `test-three-phase-workflow.sh` -> `fixtures/manifests/bundles/three-phase-forced.yaml`
- Fixtures now use structured JSON outputs for strict phase validation (`qa`/`fix`/`retest`/`guard`).
- Scripts apply fixtures additively and must not clear orchestrator config as part of normal setup.
- Run scripts serially. They still update shared active config via `apply`, so parallel runs can overwrite each other's fixture setup.
- `test-exec-interactive.sh` applies an inline temporary manifest (`workspace/exec-interactive-flow.yaml`) for deterministic `plan` + `qa` capability coverage.

## Troubleshooting

- Binary not found:
  - rebuild release binary.
- Task stuck:
  - inspect `orchestrator task list`
  - force delete stale task if needed.
- DB lock:
  - stop concurrent runs and retry.

## Checklist

| # | Item | Status | Date | Tester | Notes |
|---|------|--------|------|--------|-------|
| 1 | test-task-pause-resume.sh runs successfully | | | | |
| 2 | test-task-retry.sh runs successfully | | | | |
| 3 | test-three-phase-workflow.sh runs successfully | | | | |
| 4 | test-worker-throughput.sh runs successfully | | | | |
| 5 | test-log-tail-latency.sh runs successfully | | | | |
| 6 | test-exec-interactive.sh runs successfully | | | | |
