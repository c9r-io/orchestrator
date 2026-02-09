# AI Dev Platform Template

This repository is a minimal, AI-first development platform scaffold:
- Shared agent rules (`.cursor/rules`, `.gemini/rules`)
- Reusable skills (`.claude/skills`) that drive an end-to-end workflow

Open this repo in your AI dev tool (Claude Code, Cursor, Gemini) and it should automatically discover the rules and skills.

## Workflow

1. Bootstrap a new project skeleton with `project-bootstrap`.
2. Use Plan mode for new feature development (explicit scope, acceptance criteria, test plan).
3. Implement with conventions (use `rust-conventions` when applicable).
4. Generate reproducible QA test docs under `docs/qa/` via `qa-doc-gen`.
5. Start a new chat and execute QA via `qa-testing`; failures produce tickets under `docs/ticket/`.
6. Optionally start a new chat and run security testing/review (use `security-best-practices`) or run a UI/UX pass (use `design-system-guidance` and/or the repo's UI/UX docs under `docs/uiux/`).
7. Start a new chat and fix tickets end-to-end via `ticket-fix`.
8. Iterate until QA produces no new tickets, then return to Plan mode for the next feature and repeat.

## Quick Start (Bootstrap)

Generate a new project by prompting your agent to run the `project-bootstrap` skill (example prompt):

```text
Initialize a new project named "acme".
```

Generated projects follow these conventions:
- Local runtime: `docker/docker-compose.yml`
- Reset: `./scripts/reset-docker.sh`
- K8s: `k8s/base/` + `deploy/*.sh`
- QA docs: `docs/qa/`
- QA tickets: `docs/ticket/`

## Docs Used as Constraints

- Architecture reference: `docs/architecture.md`
- Design system reference: `docs/design-system.md`

When a task touches architecture or design system decisions, use the corresponding guidance skills:
- `arch-guidance`
- `design-system-guidance`

## Skills Index

See `SKILLS.md`.
