# Orchestrator (CLI Wrapper)

`orchestrator/` now contains runtime data, config, and convenience scripts.
The executable implementation is in `core/`.

## CLI Entry Points

```bash
# Recommended wrapper (builds core binary if needed)
./orchestrator/scripts/orchestrator.sh <command> [options]

# Direct binary
./core/target/release/agent-orchestrator <command> [options]
```

## Build And Test

```bash
cd core
cargo build --release
cargo test --lib --bins
```

## Runtime Paths

- Config: `orchestrator/config/default.yaml`
- Database: `orchestrator/data/agent_orchestrator.db`
- Logs: `orchestrator/data/logs/`

## Common Commands

```bash
./orchestrator/scripts/orchestrator.sh task list
./orchestrator/scripts/orchestrator.sh task create --name "qa-run" --goal "run QA"
./orchestrator/scripts/orchestrator.sh task start --latest
./orchestrator/scripts/orchestrator.sh task logs <task-id>
./orchestrator/scripts/orchestrator.sh config view
./orchestrator/scripts/orchestrator.sh config validate orchestrator/config/default.yaml
```
- path escape (`..`) is rejected
- existing paths are canonicalized and must remain inside workspace root

## Existing Scripts Compatibility

Existing scripts remain usable:

- `scripts/run-qa-tests.sh`
- `scripts/fix-tickets.sh`

Use `--orchestrator` on either script to launch this UI workflow.
