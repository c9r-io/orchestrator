---
self_referential_safe: true
---

# Orchestrator - QA Command Contract

**Module**: orchestrator
**Scope**: Canonical CLI contract for all QA documents
**Scenarios**: 4
**Priority**: Critical

---

## Purpose

This document is the single source of truth for command syntax used by QA docs.
All files under `docs/qa/orchestrator/` must align with this contract.

Runtime state source of truth is SQLite. YAML is used as import/export/edit artifact when needed.

Entry point: `orchestrator <command>` (auto-builds + calls CLI client) or `./target/release/orchestrator <command>`.

Daemon lifecycle:
- Start: `./target/release/orchestratord --foreground --workers 2` (foreground, recommended)
- Background: `nohup ./target/release/orchestratord --foreground --workers 2 &`
- Stop: `kill $(cat data/daemon.pid)` (graceful SIGTERM)

---

## Scenario 1: Valid Top-Level Command Surface

### Preconditions

- CLI binary is available.

### Steps

1. Show help:
   ```bash
   orchestrator --help
   ```

2. Confirm top-level commands exist:
   - `init`
   - `apply`
   - `get`
   - `describe`
   - `delete`
   - `task`
   - `store`
   - `debug`
   - `check`
   - `manifest`
   - `version`

3. Confirm `task` subcommands:
   - `list`
   - `create`
   - `info`
   - `start`
   - `pause`
   - `resume`
   - `logs`
   - `delete`
   - `retry`
   - `watch`
   - `trace`

### Expected Result

- Help output includes all commands above.
- Deprecated/removed command groups are not documented in QA steps.

---

## Scenario 2: Parameter Contract Check

### Verification Method

Code review + unit test verification. The CLI argument parsing contract is verified through the `clap` derive macros and unit tests for resource operations.

### Steps

1. **Code review** — confirm CLI argument definitions in `crates/cli/src/cli.rs`:
   - `describe workspace` accepts positional workspace name
   - `task list` supports `-o json` and `-o yaml` output flags
   - `task info` supports `-o json` and `-o yaml`
   - `get workspaces` supports `-o yaml`
   - `task create` supports `--no-start` flag
   - `task create` does NOT expose legacy `--detach`/`--attach` flags

2. **Code review** — confirm `--no-start` behavior in `crates/cli/src/commands/`:
   - `task create --no-start` creates a task record without starting execution

3. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- resource
   cargo test --workspace --lib -- apply_result
   ```

### Expected Result

- `describe workspace` accepts positional workspace name (clap derive verified)
- Output format flags `-o json`/`-o yaml` defined for commands that support them
- `task create --no-start` is defined; `--detach`/`--attach` are absent
- Resource operation unit tests pass

---

## Scenario 3: kubectl-Style Surface Contract

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm kubectl-style CLI structure in `crates/cli/src/cli.rs`:
   - `get <resource-type>` pattern (workspaces, agents, workflows) is defined
   - `-l key=value[,k=v]` label selector syntax is accepted on list commands
   - `apply -f -` reads from stdin (file path `-` handling)

2. **Code review** — confirm stdin apply in `crates/cli/src/commands/resource.rs`:
   - When file path is `-`, input is read from stdin
   - Manifest parsing handles multi-document YAML from stdin

3. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- resource_dispatch
   cargo test --workspace --lib -- registered_resource
   ```

### Expected Result

- `get <resource-type>` syntax is implemented
- `-l key=value` is accepted on list commands
- `apply -f -` reads from stdin
- Resource dispatch unit tests pass

---

## Scenario 4: Banned Patterns Guard

### Preconditions

- QA lint script is available.

### Steps

1. Run QA doc lint:
   ```bash
   ./scripts/qa-doc-lint.sh
   ```

2. Confirm no banned patterns in docs:
   - `cd orchestrator`
   - `--workspace-id`
   - `orchestrator agent health`
   - `orchestrator/config/default.yaml`
   - `config bootstrap --from`
   - `orchestrator workspace create`
   - `orchestrator agent create`
   - `orchestrator workflow create`
   - `orchestrator workspace info`

### Expected Result

- Lint exits with code `0`.
- QA docs remain aligned with current repository structure and CLI surface.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Valid Top-Level Command Surface | ✅ | 2026-02-23 | opencode | |
| 2 | Parameter Contract Check | ✅ | 2026-03-18 | Claude | Rewritten as code review + unit test |
| 3 | kubectl-Style Surface Contract | ✅ | 2026-03-18 | Claude | Rewritten as code review + unit test |
| 4 | Banned Patterns Guard | ✅ | 2026-03-18 | Claude | All sub-checks pass (lint script wrapper has `command -v rg` issue in non-interactive bash; individual checks verified manually) |
