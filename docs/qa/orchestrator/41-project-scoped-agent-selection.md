# Orchestrator - Project-Scoped Agent Selection

**Module**: orchestrator
**Scope**: Validate project-scoped resource deployment (`apply --project`) and strict project-isolated agent selection
**Scenarios**: 5
**Priority**: High

---

## Background

Agent selection must be isolated per project: only agents deployed into a
project scope should participate in selection for that project's tasks. This
prevents QA fixture agents from interfering with resources belonging to other
projects.

Agent selection is strictly project-isolated: only agents deployed into a
project scope participate in selection for that project's tasks. There is no
cross-project fallback — if the project lacks an agent with the required
capability, the task fails with a clear error.

Key CLI workflow:
```bash
# Reset project state (task data + config + auto-tickets)
orchestrator project reset <project> --force --include-config

# Deploy resources into project scope
orchestrator apply -f <fixture> --project <project>

# Create task in project (agent selection uses project-scoped agents)
orchestrator task create --project <project> ...
```

Entry point: `orchestrator <command>`

---

## Scenario 1: Apply --project Routes Agents to Project Scope

### Preconditions

- Reset previous QA state.

### Goal

Validate that `apply -f --project` deploys agents, workflows, and workspaces
into the project config scope, not into any top-level global map.

### Steps

1. Reset and apply into project scope:
   ```bash
   orchestrator project reset qa-scope --force --include-config
   orchestrator apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-scope
   ```

2. Inspect manifest export for project-scoped placement:
   ```bash
   orchestrator manifest export | python3 -c "
   import sys, yaml
   cfg = yaml.safe_load(sys.stdin)
   proj_agents = list(cfg.get('projects', {}).get('qa-scope', {}).get('agents', {}).keys())
   print('project_agents:', proj_agents)
   "
   ```

### Expected

- `mock_echo_alpha` and `mock_echo_beta` appear in `projects.qa-scope.agents`
- No top-level `agents` map is required for these resources
- CLI output shows `(project: qa-scope)` suffix for each applied resource

---

## Scenario 2: Project-Scoped Agent Selection Isolation

### Preconditions

- Reset previous QA state.

### Goal

Validate that tasks created with `--project` use only project-scoped agents
for selection, even when another project has agents with the same capability.

### Steps

1. Reset and apply into project scope:
   ```bash
   orchestrator project reset qa-iso --force --include-config
   orchestrator apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-iso
   ```

2. Create and run task in project:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-iso \
     --name "isolation-test" \
     --goal "Verify only project agents selected" \
     --workspace default \
     --workflow multi_agent_qa \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator task start "${TASK_ID}"
   ```

3. Query agent selection via DB:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT DISTINCT agent_id FROM command_runs
      WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '${TASK_ID}');"
   ```

### Expected

- Only `mock_echo_alpha` and/or `mock_echo_beta` appear in `command_runs`
- No agents from other projects are selected
- Task status: `completed`

---

## Scenario 3: Missing Capability Fails Without Fallback

### Preconditions

- Reset previous QA state.
- Another project may contain agents with `fix` capability.

### Goal

Validate that when a project's agents lack a required capability, the task
fails with a clear error instead of silently falling back to another project.

### Steps

1. Reset and apply (project agents only have `qa` capability):
   ```bash
   orchestrator project reset qa-nofallback --force --include-config
   orchestrator apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-nofallback
   ```

2. Confirm project agents only have `qa`:
   ```bash
   orchestrator get agents --project qa-nofallback -o json
   ```

3. Attempt a workflow that requires `fix` capability:
   ```bash
   orchestrator task create \
     --project qa-nofallback \
     --name "no-fallback-test" \
     --goal "Should fail — project lacks fix agent" \
     --workspace default \
     --workflow qa_fix \
     --attach 2>&1; echo "EXIT=$?"
   ```

### Expected

- Task creation or execution fails with a clear error about missing `fix` capability.
- No agents from other projects are selected — strict project isolation is enforced.
- Exit code is non-zero.

---

## Scenario 4: QA Project Reset Cleans Auto-Tickets

### Preconditions

- A project with workspaces that have ticket directories containing auto-generated tickets.

### Goal

Validate that `project reset --force --include-config` removes `auto_*.md` files from
project workspace ticket directories.

### Steps

1. Set up project with ticket files:
   ```bash
   orchestrator project reset qa-tickets --force --include-config
   orchestrator apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-tickets
   ```

2. Create some auto-ticket files manually (simulating previous run):
   ```bash
   TICKET_DIR=$(orchestrator manifest export | python3 -c "
   import sys, yaml
   cfg = yaml.safe_load(sys.stdin)
   ws = cfg.get('projects', {}).get('qa-tickets', {}).get('workspaces', {})
   for name, w in ws.items():
       print(w.get('ticket_dir', ''))
       break
   ")
   mkdir -p "${TICKET_DIR}" 2>/dev/null
   echo "test" > "${TICKET_DIR}/auto_test1.md"
   echo "test" > "${TICKET_DIR}/auto_test2.md"
   ls "${TICKET_DIR}"/auto_*.md 2>/dev/null | wc -l
   ```

3. Reset project:
   ```bash
   orchestrator project reset qa-tickets --force --include-config
   ```

4. Verify tickets cleaned:
   ```bash
   ls "${TICKET_DIR}"/auto_*.md 2>/dev/null | wc -l
   ```

### Expected

- Before reset: auto-ticket files exist (count >= 2)
- After reset: auto-ticket files removed (count = 0)
- Reset output includes `tickets_cleaned=` count

---

## Scenario 5: Cross-Project Agent Isolation

### Preconditions

- Two separate projects exist, each with different agents.

### Goal

Validate that agents from one project never leak into another project's tasks.

### Steps

1. Create two isolated projects:
   ```bash
   orchestrator project reset qa-proj-a --force --include-config
   orchestrator project reset qa-proj-b --force --include-config
   orchestrator apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-proj-a
   orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project qa-proj-b
   ```

2. List agents in each project:
   ```bash
   orchestrator get agents --project qa-proj-a -o json
   orchestrator get agents --project qa-proj-b -o json
   ```

3. Create task in project B:
   ```bash
   TASK_ID=$(orchestrator task create \
     --project qa-proj-b \
     --name "cross-isolation-test" \
     --goal "Verify agents from project A are not used" \
     --workspace default \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ```

### Expected

- Project A agents (`mock_echo_alpha`, `mock_echo_beta`) do not appear in project B agent list.
- Task created in project B would only use project B agents (`mock_echo`).

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Apply --project Routes to Project Scope | ✅ | 2026-03-09 | Claude | Agents in project scope only, not global |
| 2 | Project-Scoped Agent Selection Isolation | ✅ | 2026-03-09 | Claude | Only project agents in command_runs |
| 3 | Missing Capability Fails Without Fallback | ✅ | 2026-03-09 | Claude | Strict isolation, clear error, no fallback |
| 4 | QA Project Reset Cleans Auto-Tickets | ✅ | 2026-03-09 | Claude | auto_*.md cleaned, manual tickets preserved |
| 5 | Cross-Project Agent Isolation | ✅ | 2026-03-09 | Claude | No agent leakage between projects |
