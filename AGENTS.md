# AI Dev Platform Index

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

