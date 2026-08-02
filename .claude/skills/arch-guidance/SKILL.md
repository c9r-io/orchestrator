---
name: arch-guidance
description: Consult and apply the repository architecture reference when discussing system architecture, module boundaries, directory layout, service interfaces, Docker Compose, Kubernetes deployment, or operational entry points. Use when a user asks about architecture, refactors, project structure, deployment models, or infra layout.
---

# Architecture Guidance

When triggered:
1. Read `docs/architecture.md` first.
2. Use it as the source of truth for the Rust facade and crates (`core/`, `crates/`), Web frontend (`gui/`), scripts, fixtures, runtime data, and governance docs (`docs/qa/`, `docs/ticket/`).
3. If the requested change conflicts with the reference, call out the conflict and propose an updated, consistent design.

For a generated app, discover its `project-bootstrap` outputs from that target repository. Do not project its optional portal, Docker, or Kubernetes layout onto this control-plane repository.
