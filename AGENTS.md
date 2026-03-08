# AI Dev Platform Index

## Project Overview

This project is an **Agent Orchestrator** (code in `core/` folder) designed to provide both **workflow orchestration** and **agent orchestration** capabilities in a unified platform.

### Core Capabilities

- **Workflow Orchestration**: Define, manage, and execute complex multi-step workflows with built-in state management, error handling, and retry mechanisms
- **Agent Orchestration**: Coordinate multiple specialized agents to work together on complex tasks, with intelligent task delegation and result aggregation

### Architecture

The orchestrator combines workflow engines with agent coordination to enable:
- **Capability-driven orchestration**: Steps declare required capabilities, agents declare supported capabilities
- **Dynamic agent selection**: Based on capability matching and preference scoring
- **Declarative workflow definitions**: Steps can be builtin (`init_once`, `ticket_scan`, `loop_guard`) or capability-based
- **Repeatable steps**: Control whether steps execute in every loop cycle
- **Guard steps**: Steps that can terminate the workflow loop based on their output
- **Dynamic Orchestration**: PrehookDecision (Run/Skip/Branch/DynamicAdd/Transform) for runtime step control
- **DAG Execution Engine**: Topological sort, cycle detection, conditional edges
- **Dynamic Step Pool**: Runtime step selection based on context and priority
- Built-in observability and debugging (real-time logs, event tracking)

### Config Format

```yaml
agents:
  opencode:
    metadata:
      name: opencode
    capabilities:
    - qa
    - fix
    - retest
    templates:
      qa: "opencode run {rel_path}"
      fix: "opencode run {ticket_paths}"

workflows:
  my_workflow:
    steps:
    - id: init
      builtin: init_once
      repeatable: false
    - id: qa_test
      required_capability: qa
      repeatable: true
    - id: check_done
      builtin: loop_guard
      is_guard: true
      repeatable: true
    loop:
      mode: infinite
      guard:
        stop_when_no_unresolved: true
    # Optional: Dynamic steps pool for runtime selection
    dynamic_steps:
    - id: quick_fix
      step_type: fix
      trigger: "qa_confidence > 0.8 && active_ticket_count < 3"
      priority: 10
```

### Tech Stack

- **Backend**: Rust (Cargo workspace)
- **Database**: SQLite for task/item lifecycle tracking
- **CLI**: kubectl-style interface for task management
- **RPC**: gRPC (tonic + prost) for client/server mode
- **Transport**: Unix Domain Socket (default) or TCP

### Execution Modes

- **Client/Server**: `orchestratord` (daemon) + `orchestrator` (CLI client) — daemon holds state, CLI communicates via gRPC over UDS

---

This repo is an AI-first development scaffold. When a task touches architecture or UI design language, consult the corresponding docs before making decisions or changes:

- Architecture reference: `docs/architecture.md`
- Design system reference: `docs/design-system.md`

Recommended workflow:
1. Use `project-bootstrap` to generate a new project skeleton.
2. Create an explicit plan (scope, acceptance criteria, test plan).
3. Implement.
4. Generate reproducible QA docs under `docs/qa/` via `qa-doc-gen`.
5. Execute QA via `qa-testing`; file failures under `docs/ticket/`.
6. Fix end-to-end via `ticket-fix` and re-verify.

