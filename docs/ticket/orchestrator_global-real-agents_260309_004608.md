# Ticket: Real AI Agents Registered in Global Scope — Risk of Unintended Invocation

**Created**: 2026-03-09 00:46:08
**QA Document**: N/A (discovered during full QA run)
**Scenario**: N/A
**Status**: FAILED

---

## Test Content

During full QA testing, real AI agents (`evo_coder`, `evo_architect`, `evo_reviewer`) were unexpectedly invoked, consuming API credits. These agents use `claude -p` commands and are registered in the **global** agent namespace, making them available as fallback candidates for any project-scoped task whose mock agents lack the required capability.

---

## Expected Result

Real AI agents (those invoking `claude -p` or other paid API services) should be registered under a specific project scope (e.g., `--project self-evolution`), not in the global namespace. Global agents should only contain safe, deterministic mock/echo agents.

---

## Actual Result

Three real AI agents exist in the global config:

| Agent | Capabilities | Command |
|-------|-------------|---------|
| `evo_coder` | implement, qa_testing, align_tests | `claude -p "{prompt}" --dangerously-skip-permissions --verbose --output-format stream-json` |
| `evo_architect` | plan, evaluate | `claude -p "{prompt}" --dangerously-skip-permissions --verbose --output-format stream-json` |
| `evo_reviewer` | doc_governance, review, loop_guard | `claude -p "{prompt}" --dangerously-skip-permissions --verbose --output-format stream-json` |

When a project-scoped task requires a capability (e.g., `implement`) that the project's mock agent doesn't have, the engine falls back to global agents and selects `evo_coder` — invoking a real `claude -p` process and consuming API credits.

---

## Repro Steps

1. Apply `echo-workflow.yaml` (mock agent with only `qa/fix/retest/loop_guard` capabilities) to a project scope
2. Create a task with a workflow that has an `implement` step
3. Start the task — engine selects global `evo_coder` because project-scoped `mock_echo` lacks `implement` capability
4. Real `claude -p` process spawns, consuming API credits

---

## Evidence

**Process list during QA**:
```
claude -p You are working on the Agent Orchestrator project...
  --dangerously-skip-permissions --verbose --output-format stream-json
```

**Global agent describe**:
```
$ orchestrator describe agent/evo_coder
capabilities:
- implement
- qa_testing
- align_tests
command: claude -p "{prompt}" --dangerously-skip-permissions --verbose --output-format stream-json
env:
- fromRef: claude-sonnet
- fromRef: minimax
```

---

## Analysis

**Root Cause**: The self-evolution workflow agents (`evo_coder`, `evo_architect`, `evo_reviewer`) were registered globally via `orchestrator apply` without `--project`. This makes them visible to all projects as fallback candidates during capability-based agent selection.

**Recommended Fix**:
1. Move `evo_coder`, `evo_architect`, `evo_reviewer` to project-scoped registration: `orchestrator apply -f <manifest> --project self-evolution`
2. Remove these agents from the global config: `orchestrator delete agent/evo_coder --force`, etc.
3. Consider adding a safety mechanism: agents with `claude -p` or other paid-API commands should warn or require explicit opt-in when selected as fallback from global scope
4. Alternatively, add an agent-level `scope: project-only` flag to prevent global fallback selection

**Severity**: High
**Related Components**: Agent Selection Engine / Config Management / Global vs Project Scope
