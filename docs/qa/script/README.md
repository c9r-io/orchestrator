# QA Test Scripts

This directory contains shell scripts for testing advanced orchestrator features that require background process management.

## Available Scripts

| Script | Description |
|--------|-------------|
| `test-task-pause-resume.sh` | Tests task pause and resume functionality |
| `test-task-retry.sh` | Tests task item retry functionality |
| `test-three-phase-workflow.sh` | Tests QA + Fix + Retest workflow execution |

## Debug Command

The orchestrator provides a debug command for QA testing:

```bash
# Show all debug options
orchestrator debug

# Show MessageBus information
orchestrator debug --component messagebus

# Show active configuration
orchestrator debug --component config

# Show runtime state
orchestrator debug --component state
```

## Prerequisites

Before running any script:

1. **Check binary exists**:
   ```bash
   ls -la ../orchestrator/src-tauri/target/release/agent-orchestrator
   ```

2. **Rebuild if needed**:
   ```bash
   cd ../orchestrator/src-tauri && cargo build --release
   ```

## Running Tests

```bash
# Make scripts executable (if needed)
chmod +x *.sh

# Run individual tests
./test-task-pause-resume.sh
./test-task-retry.sh
./test-three-phase-workflow.sh
```

## Script Requirements

- Bash shell (`#!/usr/bin/env bash`)
- SQLite3 for database verification
- `jq` for JSON parsing
- Network access (if testing remote features)

## Troubleshooting

**Issue**: Script fails with "Binary not found"
- **Fix**: Rebuild the orchestrator binary

**Issue**: Tests hang or timeout
- **Fix**: Check if there's a stuck task in the database and delete it:
  ```bash
  cd ../orchestrator
  ./src-tauri/target/release/agent-orchestrator task list
  ./src-tauri/target/release/agent-orchestrator task delete <task-id> --force
  ```

**Issue**: Database locked
- **Fix**: Reset the database:
  ```bash
  cd ../orchestrator
  rm -f data/agent_orchestrator.db
  ```
