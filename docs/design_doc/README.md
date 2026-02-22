# Design Docs

This directory contains design documents captured from confirmed plans (plan mode output). They preserve context after implementation (goals, scope, tradeoffs, risks, observability, acceptance criteria) to reduce future iteration overhead.

Generation entry point:
- Before generating `docs/qa/**`, `qa-doc-gen` generates the corresponding `docs/design_doc/**` design docs (same module-based structure).

## Suggested Directory Structure

```
docs/design_doc/
├── README.md
├── <module>/
│   ├── 01-<topic>.md
│   └── 02-<topic>.md
└── ...
```

## Document Rules (Strict)

- Write everything in English. Keep technical details (API paths, SQL, field names, metric names) as-is.
- Each design doc must include:
  - Background and goals (including non-goals)
  - Scope (in/out)
  - Interfaces/data changes (if applicable)
  - Key design and tradeoffs
  - Risks and mitigations
  - Observability and operations (include at least default recommendations)
  - Testing and acceptance (must point to the related QA doc path)

## Index (Recommended)

| Module | Doc | Related QA | Notes |
|--------|-----|------------|-------|
| example | `docs/design_doc/example/01-sample.md` | `docs/qa/example/01-sample.md` | skeleton |
| orchestrator | `docs/design_doc/orchestrator/01-cli-agent-orchestration.md` | `docs/qa/orchestrator/01-cli-agent-orchestration.md` | CLI testing with mock agents |
| orchestrator | `docs/design_doc/orchestrator/08-project-namespace.md` | `docs/qa/orchestrator/08-project-namespace.md` | Project namespace for resource isolation |
| orchestrator | `docs/design_doc/orchestrator/09-scheduler-repository-refactor.md` | `docs/qa/orchestrator/19-scheduler-repository-refactor-regression.md` | P0/P1 scheduler data-layer refactor and error observability |
