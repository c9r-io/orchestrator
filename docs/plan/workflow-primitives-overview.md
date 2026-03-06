# Workflow Primitives for Self-Evolving SDLC

## Motivation

Orchestrator is a general-purpose AI-native SDLC engine. Self-bootstrapping (the orchestrator improving its own code) is one workflow pattern running on this engine — not a special built-in mode.

To enable workflows that can learn, explore, and safely evolve over time, the workflow language needs four new primitives. These primitives are general-purpose: they benefit any SDLC workflow, not just self-bootstrap.

## Current State

The workflow engine provides:
- **Steps** with types, scopes (task/item), prehooks (CEL), captures, post_actions
- **Pipeline Variables** for intra-task data flow between steps
- **Loop Modes** (once, fixed, convergence) for iterative execution
- **Builtins** (self_test, self_restart, loop_guard, ticket_scan)
- **CRDs / ResourceStore** for custom resource definitions (nascent)
- **Finalize Rules** for item terminal state decisions

## Gap Analysis

| Capability | Required For | Current Limitation |
|-----------|-------------|-------------------|
| Cross-task memory | Learning from past runs | pipeline_vars are task-scoped, lost after task completes |
| Goal generation | Self-directed improvement | goal is static, set at task creation |
| Candidate exploration | Evolutionary improvement | items are bound to QA files, single-path execution |
| Tamper-proof safety | Safe unbounded self-modification | all evaluation logic is mutable by the workflow itself |

## Four New Primitives

### 1. Persistent Store (docs/plan/wp01-persistent-store.md)
Declarative cross-task read/write interface for workflow steps. Enables long-term memory, metric tracking, and knowledge accumulation across task runs.

**Priority**: Highest — foundational for the other three primitives.

### 2. Task Spawning (docs/plan/wp02-task-spawning.md)
Post-action that creates new tasks from step output. Enables autonomous goal discovery, work decomposition, and cascading workflows.

**Priority**: High — enables self-directed workflows.

### 3. Dynamic Items + Selection (docs/plan/wp03-dynamic-items.md)
Runtime item generation with parallel execution and tournament selection. Enables multi-candidate exploration and evolutionary strategies.

**Priority**: Medium — powerful but depends on Persistent Store for metrics comparison.

### 4. Invariant Constraints (docs/plan/wp04-invariant-constraints.md)
Immutable safety assertions declared outside the workflow's mutable scope. Ensures self-modifying workflows cannot weaken their own evaluation criteria.

**Priority**: High — critical for safe unbounded self-modification.

## Execution Order

```
wp01 (Persistent Store)
 ├── wp02 (Task Spawning)        — uses store for cross-task context
 ├── wp04 (Invariant Constraints) — uses store for baseline metrics
 └── wp03 (Dynamic Items)         — uses store for candidate scoring
         └── Integration validation
```

## Integration Validation (docs/plan/wp05-integration-validation.md)
End-to-end scenario: a `self-evolving.yaml` workflow that uses all four primitives to demonstrate autonomous, measurable, safe self-improvement — expressed entirely in workflow YAML without any new built-in Rust code beyond the primitives themselves.

## Design Principles

1. **Workflow-level, not built-in**: Every capability is expressed in YAML step declarations, not hardcoded behavior
2. **Incremental**: Each primitive is independently useful; they compose but don't require each other
3. **Backward-compatible**: Existing workflows continue to work unchanged
4. **Minimal kernel**: Orchestrator adds the smallest possible engine support; intelligence lives in workflow definitions
