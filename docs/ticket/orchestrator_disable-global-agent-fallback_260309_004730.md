# Ticket: Disable Global Agent Fallback for Project-Scoped Tasks

**Created**: 2026-03-09 00:47:30
**QA Document**: N/A (design decision from QA incident)
**Scenario**: N/A
**Status**: OPEN

---

## Test Content

When a project-scoped task requires a capability that no project-scoped agent provides, the engine silently falls back to global agents. This should be an error, not a fallback.

---

## Expected Result

If a task belongs to a project (`project_id` is set), agent selection should **only** consider agents registered in that project's scope. If no matching agent is found, the engine should return a clear error:

```
Error: no agent with capability 'implement' found in project 'my-project'.
Hint: apply a manifest with the required agent using --project my-project
```

---

## Actual Result

The engine silently searches global agents as fallback, which can select unintended agents (e.g., real `claude -p` agents registered globally) for a project that was designed to use only mock agents.

---

## Repro Steps

1. Create a project with only a `mock_echo` agent (capabilities: `qa, fix`)
2. Create a task in that project with a workflow requiring `implement` capability
3. Start the task
4. Engine falls back to global `evo_coder` (real `claude -p`) instead of erroring

---

## Design Rationale

Project isolation is a first-class feature. The `--project` flag on `apply`, `get`, `describe`, `delete`, and `task create` exists specifically to create isolated environments. Silently breaking that isolation during agent selection defeats the purpose.

Key principles:
- **Explicit over implicit**: If a project needs an agent, it should be explicitly registered in that project
- **Fail-fast over silent fallback**: A missing agent is a configuration error, not something to auto-resolve
- **Security boundary**: Global agents may have elevated permissions (API keys, paid services) that should not leak into arbitrary projects
- **QA safety**: Mock-only projects must guarantee no real agents are invoked

---

## Proposed Implementation

1. In `agent_selection` (or equivalent capability matching logic), check if the task has a `project_id`
2. If yes, only search `config.projects[project_id].agents` — never fall through to `config.agents`
3. If no matching agent found, return `AgentSelectionError::NoAgentInProject { capability, project_id }`
4. The error message should include a hint about how to register the missing agent

**Scope**: Agent selection engine (`core/src/` — likely in agent selection or dispatch logic)

---

## Analysis

**Root Cause**: Agent selection does not enforce project boundary — treats project agents as "preferred" rather than "exclusive"
**Severity**: High
**Related Components**: Agent Selection Engine / Project Isolation / Config Resolution
