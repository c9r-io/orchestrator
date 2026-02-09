---
name: arch-guidance
description: Consult and apply the repository architecture reference when discussing system architecture, module boundaries, directory layout, service interfaces, Docker Compose, Kubernetes deployment, or operational entry points. Use when a user asks about architecture, refactors, project structure, deployment models, or infra layout.
---

# Architecture Guidance

When triggered:
1. Read `docs/architecture.md` first.
2. Use it as the source of truth for directory conventions (`core/`, `portal/`, `docker/`, `k8s/`, `deploy/`, `scripts/`, `docs/qa/`, `docs/ticket/`) and platform workflow.
3. If the requested change conflicts with the reference, call out the conflict and propose an updated, consistent design.

If the project is a generated app (from `project-bootstrap`), assume the conventions exist unless the repo shows otherwise.

