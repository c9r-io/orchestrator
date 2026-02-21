# Agent Orchestrator

Tauri + React based workflow orchestrator for agent-driven operations.

## Features

- SQLite-backed `task -> task_item` lifecycle tracking
- Workspace isolation (`workspace`) for root path and document scope
- Agent-driven command templates (`agent`) bound by `workflow` phase mapping
- Full shell command passthrough (`/bin/zsh -lc` by default)
- Auto-resume latest unfinished task on startup
- Real-time dashboard for task list, item progress, and command logs
- Config Center with `Form`/`YAML` switch for workspace/workflow/agent editing
- Config persistence in SQLite with hot reload for new tasks
- **Structured AgentOutput** with artifacts, confidence, and quality scores
- **MessageBus** for agent-to-agent communication
- **Artifact parsing** from agent stdout/stderr (JSON and text markers)
- **Dynamic Orchestration** with PrehookDecision (Run/Skip/Branch/DynamicAdd/Transform)
- **DAG Execution Engine** with topological sort and cycle detection
- **Dynamic Step Pool** for runtime step selection based on context

## Directory

- `config/default.yaml`: command templates and runtime config
- `data/agent_orchestrator.db`: SQLite database (runtime)
- `data/logs/`: command stdout/stderr logs
- `src-tauri/`: orchestrator backend and scheduler
- `src/`: React dashboard

## Run

```bash
cd orchestrator
npm install
npm run tauri:dev
```

## Test & Coverage

Install test dependencies once:

```bash
cd orchestrator
npm install -D vitest @vitest/coverage-v8
```

```bash
cd orchestrator
npm run test
npm run test:coverage
npm run test:tauri
npm run test:tauri:coverage
```

Coverage requirement (unit scope): `>= 90%` for lines/functions/branches/statements.

Current coverage gates:

- Frontend: `vitest.config.ts` (>=90%)
- Tauri: `src-tauri/Makefile` using `cargo llvm-cov --fail-under-lines 90`

## CLI Usage (kubectl-style)

### New CLI Interface

The orchestrator provides a kubectl-like CLI interface:

```bash
# Main entry point
./scripts/orchestrator.sh <command> [options]

# Or use the binary directly
./src-tauri/target/release/agent-orchestrator <command> [options]
```

### Task Management

```bash
# List all tasks
./scripts/orchestrator.sh task list
./scripts/orchestrator.sh task list --status running
./scripts/orchestrator.sh task list -o json

# Create a new task
./scripts/orchestrator.sh task create --name "my-task" --goal "run QA"
./scripts/orchestrator.sh task create --workspace default --workflow qa_fix_retest
./scripts/orchestrator.sh task create --target-file docs/qa/user/01-crud.md

# Get task details
./scripts/orchestrator.sh task info <task-id>

# Start a task
./scripts/orchestrator.sh task start --latest  # Auto-select latest resumable task
./scripts/orchestrator.sh task start <task-id>

# Pause/Resume
./scripts/orchestrator.sh task pause <task-id>
./scripts/orchestrator.sh task resume <task-id>

# View logs
./scripts/orchestrator.sh task logs <task-id>
./scripts/orchestrator.sh task logs <task-id> --tail 50

# Delete a task
./scripts/orchestrator.sh task delete <task-id> --force

# Retry failed item
./scripts/orchestrator.sh task retry <task-item-id>
```

### Workspace Management

```bash
# List workspaces
./scripts/orchestrator.sh workspace list

# Get workspace details
./scripts/orchestrator.sh workspace info <workspace-id>
```

### Configuration

```bash
# View current configuration
./scripts/orchestrator.sh config view
./scripts/orchestrator.sh config view -o yaml

# Validate configuration file
./scripts/orchestrator.sh config validate <config-file>

# Update configuration
./scripts/orchestrator.sh config set <config-file>

# List available workflows
./scripts/orchestrator.sh config list-workflows

# List available agents
./scripts/orchestrator.sh config list-agents
```

### UI vs CLI behavior

- UI startup (`npm run tauri:dev` or `scripts/open-ui.sh`):
  - does **not** auto-resume and does **not** auto-start QA
  - shows existing tasks and waits for user action (`Start`/`Resume`)
- CLI startup (`scripts/run-cli.sh`):
  - auto-resumes latest unfinished task (`running/interrupted/paused/pending`)
  - if no unfinished task exists, auto-creates a new task and starts execution

Legacy CLI examples (still supported):

```bash
./orchestrator/scripts/run-cli.sh
./orchestrator/scripts/run-cli.sh --workspace default --workflow qa_fix_retest
./orchestrator/scripts/run-cli.sh --target-file docs/qa/user/01-crud.md
./orchestrator/scripts/run-cli.sh --no-auto-resume --workflow qa_only
```

## Workflow Model

- Workflow is a configurable step pipeline: `init_once`, `qa`, `ticket_scan`, `fix`, `retest`
- Each step can be enabled/disabled and mapped to an agent
- `ticket_scan` is a built-in step (no agent required) that scans `ticket_dir` and maps active tickets to task items
- Each step can define optional `prehook` rules to decide run/skip per item
- Workflow supports `finalize.rules[]` to decide final item status (`skipped/qa_passed/fixed/verified/unresolved`) via CEL
- Loop policy is defined per workflow: `once` or `infinite`
- Loop guard supports rule-based stop conditions and optional guard agent decision (`loop.guard.agent_id`)

## Prehook (Low-friction mode)

- Default editor is **Visual Rules** (no CEL required)
- Built-in presets:
  - `ticket_scan`: always run (or guard by active ticket context)
  - `fix`: run only when `active_ticket_count > 0`
  - `retest`: run only when `active_ticket_count > 0 && fix_exit_code == 0`
- `Advanced CEL` mode is optional for power users
- Runtime still evaluates `prehook.when` (CEL); visual editor writes CEL automatically
- `Simulate` in both modes runs backend CEL evaluator (`simulate_prehook`) for parity with runtime
- UI-only metadata is stored under `prehook.ui` for round-trip editing
- Final state decisions can be configured with `workflow.finalize.rules[]` (first-match wins)

### Extended Prehook Decisions (Dynamic Orchestration)

When `prehook.extended: true` is set, the prehook can return complex decisions beyond simple run/skip:

- **Run**: Execute the step (default)
- **Skip**: Skip the step with a reason
- **Branch**: Jump to a different step with context
- **DynamicAdd**: Dynamically inject steps into the execution plan
- **Transform**: Replace templates for subsequent steps

Available visual fields:

- `active_ticket_count`, `new_ticket_count`, `cycle`
- `qa_exit_code`, `fix_exit_code`, `retest_exit_code`
- `qa_failed`, `fix_required`

## Config Model

`config/default.yaml` defines:

- `workspaces`: isolated roots and path scopes (`root_path`, `qa_targets`, `ticket_dir`)
- `agents`: step templates (`init_once`, `qa`, `fix`, `retest`, `loop_guard`)
- `workflows`: step array + loop policy
- `defaults`: default `workspace` and `workflow`

Runtime source of truth:

- active config is stored in SQLite (`orchestrator_config` tables)
- `config/default.yaml` is updated on every save as mirror/export
- config changes hot-reload for new task creation; running tasks keep their own snapshots

Template placeholders:

- `{rel_path}`: current QA/security markdown file path
- `{ticket_paths}`: space-separated ticket file paths for current item
- loop guard template placeholders: `{task_id}`, `{cycle}`, `{unresolved_items}`
- **Enhanced placeholders**: `{phase}`, `{upstream[0].exit_code}`, `{upstream[0].confidence}`, `{shared_state.key}`

## Dynamic Steps (Optional)

Workflows can define a pool of dynamic steps that are selected at runtime based on context:

```yaml
workflows:
  adaptive:
    steps:
      - id: qa
        type: qa
        enabled: true
    dynamic_steps:
      - id: quick_fix
        step_type: fix
        trigger: "qa_confidence > 0.8 && active_ticket_count < 3"
        priority: 10
        max_runs: 1
      - id: deep_retest
        step_type: retest
        trigger: "cycle > 2 && active_ticket_count > 5"
        priority: 5
```

- **trigger**: CEL condition that determines when this step is eligible
- **priority**: Higher priority steps are selected first when multiple match
- **max_runs**: Maximum times this step can execute per item

## DAG Execution Engine

The orchestrator includes a DAG (Directed Acyclic Graph) execution engine for advanced workflows:

- **WorkflowNode**: Represents a step in the workflow
- **WorkflowEdge**: Directed connections between nodes with optional CEL conditions
- **Topological Sort**: Validates execution order and detects cycles
- **Conditional Edges**: Paths branch based on CEL conditions evaluated at runtime

This enables complex workflows with dynamic branching and conditional execution paths.

Path safety rules:

- all task paths are resolved relative to the selected workspace root
- path escape (`..`) is rejected
- existing paths are canonicalized and must remain inside workspace root

## Existing Scripts Compatibility

Existing scripts remain usable:

- `scripts/run-qa-tests.sh`
- `scripts/fix-tickets.sh`

Use `--orchestrator` on either script to launch this UI workflow.
