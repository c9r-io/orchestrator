# Orchestrator - Project Namespace

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Add project concept to constrain resource naming spaces, similar to Kubernetes namespace. A project can contain multiple workspaces, and workspaces within the same project can share project-level workflows and agents.
**Related QA**: `docs/qa/orchestrator/08-project-namespace.md`
**Created**: 2026-02-21
**Last Updated**: 2026-02-21

## Background

The orchestrator originally had flat global namespaces for workspaces, workflows, and agents. This caused resource name collisions when multiple teams/projects shared the same orchestrator instance. There was no way to group related resources or provide resource isolation.

## Goals

- Add Project as top-level namespace concept
- Allow project-level workspace, workflow, agent definitions
- Enforce strict project isolation — no fallback to global resources

## Non-goals

- Cross-project resource sharing (strict isolation)
- Project-level permission/auth (future consideration)

## Scope

- In scope: Project CRUD in config, resource resolution, database schema, CLI support
- Out of scope: Project-level permissions, project quotas, cross-project references

## Key Design

1. **ProjectConfig Structure**: Contains workspaces, agents, workflows — project-scoped tasks use only these resources
2. **Strict Isolation**: No fallback to global resources; if a project lacks a required resource, the operation fails with a clear error
3. **Explicit Projects**: Every project must be explicitly created via `apply --project`; there is no implicit "default" project
4. **Database Schema**: project_id added to tasks and command_runs tables

## Alternatives And Tradeoffs

- Option A (chosen): Project contains all resource types (workspaces, agents, workflows)
- Option B: Project only contains workspaces, agents/workflows remain global
- Why chosen: Provides complete isolation and flexibility for teams

## Risks And Mitigations

- Risk: Strict isolation may break workflows that relied on global resources
  - Mitigation: Clear error messages guide users to apply resources into the project scope

## Observability

- No new metrics/logs required for this feature
- Existing task/workspace logging sufficient

## Operations / Release

- Migration: Add project_id column to existing tables (default empty string)
- Rollback: Remove project_id column (data loss acceptable for rollback)
- Compatibility: Project-scoped tasks require all resources in the project; non-project tasks use top-level config

## Test Plan

- Unit: Config parsing, resource resolution logic
- Integration: CLI --project flag, task creation with project_id
- E2E: Full workflow with project isolation

## QA Docs

- `docs/qa/orchestrator/08-project-namespace.md`

## Acceptance Criteria

- [ ] Tasks store project_id in database
- [ ] CLI --project flag works
- [ ] Project-level resources are exclusive — no global fallback
- [ ] Missing project resource fails with clear error
- [ ] Projects must be explicitly created via `apply --project`
