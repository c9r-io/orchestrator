# Orchestrator - Project-Scoped Agent Selection

**Module**: orchestrator
**Scope**: Validate project-scoped resource deployment (`apply --project`) and hierarchical agent selection
**Scenarios**: 5
**Priority**: High

---

## Background

Agent selection must be isolated per project: only agents deployed into a
project scope should participate in selection for that project's tasks. This
prevents QA fixture agents from interfering with bootstrap agents and vice versa.

The implementation uses hierarchical resolution: project-scoped agents take
priority when they have the required capability; otherwise selection falls back
to global agents.

Key CLI workflow:
```bash
# Reset project state (task data + config + auto-tickets)
./scripts/run-cli.sh qa project reset <project> --force

# Deploy resources into project scope
./scripts/run-cli.sh apply -f <fixture> --project <project>

# Create task in project (agent selection uses project-scoped agents)
./scripts/run-cli.sh task create --project <project> ...
```

Entry point: `./scripts/run-cli.sh <command>`

---

## Scenario 1: Apply --project Routes Agents to Project Scope

### Preconditions

- Reset previous QA state.

### Goal

Validate that `apply -f --project` deploys agents, workflows, and workspaces
into the project config scope, not into the global config.

### Steps

1. Reset and apply into project scope:
   ```bash
   ./scripts/run-cli.sh qa project reset qa-scope --force
   ./scripts/run-cli.sh apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-scope
   ```

2. Inspect global vs project config:
   ```bash
   ./scripts/run-cli.sh manifest export | python3 -c "
   import sys, yaml
   cfg = yaml.safe_load(sys.stdin)
   global_agents = list(cfg.get('agents', {}).keys())
   proj_agents = list(cfg.get('projects', {}).get('qa-scope', {}).get('agents', {}).keys())
   print('global_agents:', global_agents)
   print('project_agents:', proj_agents)
   "
   ```

### Expected

- `mock_echo_alpha` and `mock_echo_beta` appear in `projects.qa-scope.agents`
- Neither appears in global `agents` (unless they were already there from bootstrap)
- CLI output shows `(project: qa-scope)` suffix for each applied resource

---

## Scenario 2: Project-Scoped Agent Selection Isolation

### Preconditions

- Reset previous QA state.

### Goal

Validate that tasks created with `--project` use only project-scoped agents
for selection, even when global agents have the same capability.

### Steps

1. Reset and apply into project scope:
   ```bash
   ./scripts/run-cli.sh qa project reset qa-iso --force
   ./scripts/run-cli.sh apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-iso
   ```

2. Create and run task in project:
   ```bash
   TASK_ID=$(./scripts/run-cli.sh task create \
     --project qa-iso \
     --name "isolation-test" \
     --goal "Verify only project agents selected" \
     --workspace default \
     --workflow multi_agent_qa \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ./scripts/run-cli.sh task start "${TASK_ID}"
   ```

3. Query agent selection via DB:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT DISTINCT agent_id FROM command_runs
      WHERE task_item_id IN (SELECT id FROM task_items WHERE task_id = '${TASK_ID}');"
   ```

### Expected

- Only `mock_echo_alpha` and/or `mock_echo_beta` appear in `command_runs`
- No global/bootstrap agents (e.g. `probe_fallback`, `env-agent`) are selected
- Task status: `completed`

---

## Scenario 3: Global Fallback When Project Lacks Capability

### Preconditions

- Reset previous QA state.
- Global config has at least one agent with `fix` capability (from bootstrap).

### Goal

Validate that when a project's agents lack the required capability, selection
falls back to global agents.

### Steps

1. Reset and apply (project agents only have `qa` capability):
   ```bash
   ./scripts/run-cli.sh qa project reset qa-fallback --force
   ./scripts/run-cli.sh apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-fallback
   ```

2. Inspect project agents capabilities:
   ```bash
   ./scripts/run-cli.sh manifest export | python3 -c "
   import sys, yaml
   cfg = yaml.safe_load(sys.stdin)
   proj = cfg.get('projects', {}).get('qa-fallback', {})
   for name, agent in proj.get('agents', {}).items():
       print(f'{name}: {agent.get(\"capabilities\", [])}')
   global_agents = cfg.get('agents', {})
   fix_agents = [n for n, a in global_agents.items() if 'fix' in a.get('capabilities', [])]
   print(f'global fix-capable: {fix_agents}')
   "
   ```

### Expected

- Project agents have `qa` capability only
- If a step requires `fix` capability, the runtime would fall back to global agents
- This verifies the hierarchical resolution: project-first, global-fallback

---

## Scenario 4: QA Project Reset Cleans Auto-Tickets

### Preconditions

- A project with workspaces that have ticket directories containing auto-generated tickets.

### Goal

Validate that `qa project reset --force` removes `auto_*.md` files from
project workspace ticket directories.

### Steps

1. Set up project with ticket files:
   ```bash
   ./scripts/run-cli.sh qa project reset qa-tickets --force
   ./scripts/run-cli.sh apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-tickets
   ```

2. Create some auto-ticket files manually (simulating previous run):
   ```bash
   TICKET_DIR=$(./scripts/run-cli.sh manifest export | python3 -c "
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
   ./scripts/run-cli.sh qa project reset qa-tickets --force
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

## Scenario 5: Non-Project Tasks Use Global Agents

### Preconditions

- Global config has agents (from bootstrap or init).
- A project with scoped agents exists.

### Goal

Validate that tasks created WITHOUT `--project` (or with empty project) use
global agents, not project-scoped agents.

### Steps

1. Ensure project agents exist:
   ```bash
   ./scripts/run-cli.sh qa project reset qa-global --force
   ./scripts/run-cli.sh apply -f fixtures/manifests/bundles/multi-echo.yaml --project qa-global
   ```

2. Create task WITHOUT `--project`:
   ```bash
   TASK_ID=$(./scripts/run-cli.sh task create \
     --project default \
     --name "global-agent-test" \
     --goal "Verify global agents used" \
     --workspace default \
     --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   ```

3. Verify project_id in task record:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT project_id FROM tasks WHERE id = '${TASK_ID}';"
   ```

### Expected

- Task project_id is `default` (not `qa-global`)
- If task were started, global agents would be selected (not `mock_echo_alpha`/`mock_echo_beta`)

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Apply --project Routes to Project Scope | ☐ | | | |
| 2 | Project-Scoped Agent Selection Isolation | ☐ | | | |
| 3 | Global Fallback When Project Lacks Capability | ☐ | | | |
| 4 | QA Project Reset Cleans Auto-Tickets | ☐ | | | |
| 5 | Non-Project Tasks Use Global Agents | ☐ | | | |
