# Orchestrator - CLI Agent Orchestration Testing

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Create comprehensive test documentation for CLI interface with agent orchestration capabilities using simple bash commands (echo, sleep) to simulate agent execution
**Related QA**: `docs/qa/orchestrator/01-cli-agent-orchestration.md`
**Created**: 2026-02-20
**Last Updated**: 2026-02-20

## Background And Goals

### Background

The Agent Orchestrator provides a kubectl-like CLI interface for managing tasks, workspaces, configurations, and resources. The orchestrator executes agent templates as shell commands, supporting workflow phases: init_once, qa, fix, retest, and loop_guard. Testing the full agent orchestration pipeline requires a comprehensive test suite that validates:

- CLI command parsing and execution
- Task lifecycle management (create, start, pause, resume, delete, retry)
- Workspace and configuration management
- Agent template rendering with placeholder substitution ({rel_path}, {ticket_paths})
- Workflow execution with multiple phases

### Goals

- Validate all CLI command parsing (init, apply, get, describe, task, workspace, manifest, edit, db, completion, debug)
- Verify task lifecycle state transitions (pending -> running -> paused -> completed/failed)
- Test agent template rendering with placeholder substitution
- Validate workflow execution with multiple agents and phases
- Test error handling and edge cases

### Non-goals

- Real AI agent integration (use bash mock commands instead)
- UI dashboard testing (separate test suite)
- Database migration testing (covered by integration tests)

## Scope And User Experience

### Scope

- In scope:
  - CLI command parsing and execution
  - Task lifecycle management
  - Workspace listing and info retrieval
  - Configuration management (view, validate, set)
  - Agent template rendering with placeholders
  - Workflow execution with bash mock agents

- Out of scope:
  - Real AI agent execution (opencode, claude)
  - UI dashboard interactions
  - Database migrations

### CLI Interactions

Entry points:
- `orchestrator <command> [options]` (CLI client)
- `orchestratord` (daemon)

Key commands:
- `apply -f <file>` - Apply YAML manifests
- `task list/create/info/start/pause/resume/logs/delete/retry` - Task management
- `workspace list/info` - Workspace management
- `manifest export/validate` - Manifest artifact operations
- `edit export/open` - Resource editing
- `db reset` - Database reset
- `debug --component <name>` - Runtime diagnostics

## Interfaces And Data

### CLI Command Structure

```
orchestrator [global-options] <command> [command-options]

Global Options:
  --verbose, -v    Enable verbose output

Commands:
  init [--root <path>] [--force]
  apply -f <file> [--dry-run]
  get <resource> [-o table|json|yaml]
  describe <resource> [-o table|json|yaml]
  task list|create|info|start|pause|resume|logs|delete|retry
  workspace list|info
  manifest export|validate
  edit export|open
  db reset
  completion bash|zsh|fish|powershell
  debug [--component <name>]
```

### Agent Template Placeholders

- `{rel_path}` - Current QA/security markdown file path
- `{ticket_paths}` - Space-separated ticket file paths for current item
- `{task_id}` - Task ID (for loop guard)
- `{cycle}` - Current loop cycle number (for loop guard)
- `{unresolved_items}` - Number of unresolved items (for loop guard)

### Workflow Phases

| Phase | Purpose | Typical Command |
|-------|---------|----------------|
| init_once | One-time initialization | `echo "init"` |
| qa | Run QA tests | `echo '{"confidence":0.9,"quality_score":0.86,"artifacts":[{"kind":"analysis","findings":[{"title":"qa","description":"qa for {rel_path}","severity":"info"}]}]}'` |
| fix | Fix tickets | `echo '{"confidence":0.82,"quality_score":0.78,"artifacts":[{"kind":"code_change","files":["fix.patch"]}]}'` |
| retest | Re-run QA after fix | `echo '{"confidence":0.9,"quality_score":0.88,"artifacts":[{"kind":"test_result","passed":1,"failed":0}]}'` |
| loop_guard | Decide loop continuation | `echo '{"continue":true,"should_stop":false,"reason":"continue"}'` |

## Key Design And Tradeoffs

### Design Decisions

1. **Mock Agent Approach**: Use simple bash commands (echo, sleep) instead of real AI agents to enable fast, deterministic testing without external dependencies.

2. **Template Rendering**: Agent templates support placeholder substitution at runtime, allowing flexible command generation based on task context.

3. **Task State Machine**: Tasks follow a deterministic state lifecycle: pending -> running -> (paused | completed | failed), enabling clear state verification.

4. **SQLite-backed Runtime Config**: Runtime config is persisted in SQLite. YAML manifests are applied via `apply -f`, and YAML is also used for export/edit workflows.

### Alternatives And Tradeoffs

- Option A: Use real AI agents
  - Pros: End-to-end validation
  - Cons: Slow, non-deterministic, requires API keys

- Option B: Use bash mock commands (chosen)
  - Pros: Fast, deterministic, no external dependencies
  - Cons: Doesn't test real AI integration

### Risks And Mitigations

- Risk: Mock commands don't validate real AI integration
  - Mitigation: Separate integration test suite for real agents

- Risk: Template rendering may differ from real execution
  - Mitigation: Verify placeholder substitution in unit tests

## Observability And Operations

### Logs

- Task execution logs stored in `data/logs/{task_id}/{phase}.log`
- CLI operations logged to stdout/stderr
- Database operations logged with timestamps

### Metrics

- Task execution time per phase
- Success/failure rates by workflow
- Agent selection and execution state (from debug/log events)

### Operations

- Runtime config store: `data/agent_orchestrator.db` (`resources` table; `orchestrator_config_versions` for audit history)
- Database: `data/agent_orchestrator.db`
- Logs: `data/logs/`

## Testing And Acceptance

### Test Plan

- Unit tests: CLI parsing, template rendering, state transitions
- Integration tests: Full workflow execution with mock agents
- E2E: Critical CLI command paths

### QA Docs

- `docs/qa/orchestrator/01-cli-agent-orchestration.md`

### Acceptance Criteria

- All CLI commands parse correctly with valid and invalid inputs
- Task lifecycle state transitions work as expected
- Agent templates render with correct placeholder substitution
- Workflow executes all enabled phases in order
- Error cases are handled gracefully with appropriate error messages
