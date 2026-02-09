---
name: project-bootstrap
description: Initialize a brand-new project from scratch with a default full-stack scaffold (Rust core + React Router 7 portal + Docker + Kubernetes). Use when a developer asks to bootstrap an empty repo or start a new project and no specific tech stack is requested.
---

# Project Bootstrap

Create a ready-to-run project skeleton with core, portal, docker, and k8s resources.

## Quick Start

1. Choose a project name and target directory.
2. Run the initializer script.

```bash
.claude/skills/project-bootstrap/scripts/init_project.py \
  --name acme \
  --root /path/to/acme
```

If `--name` is omitted, the script will prompt (interactive) and default to the target directory name.

By default the initializer supports a safe merge into a non-empty directory (for example a repo root that already contains `README.md`). It refuses to overwrite existing files and will error if any template top-level paths already exist (for example `core/`, `portal/`, `.github/`).

```bash
.claude/skills/project-bootstrap/scripts/init_project.py \
  --name acme \
  --root /path/to/existing-repo \
```

If you want strict behavior, require an empty target directory:

```bash
.claude/skills/project-bootstrap/scripts/init_project.py \
  --name acme \
  --root /path/to/acme \
  --require-empty
```

Optional overrides:

```bash
.claude/skills/project-bootstrap/scripts/init_project.py \
  --name acme \
  --root /path/to/acme \
  --core-port 8080 \
  --portal-port 3000 \
  --namespace acme
```

If host ports conflict with other local stacks, override the Docker Compose bindings at runtime:
- `CORE_HOST_PORT=18080`
- `PORTAL_HOST_PORT=13000`

## Workflow

1. Confirm `--root` and ask the user for a project `--name` (used for naming, default namespace, and image identifiers). If the user has no preference, suggest a default based on the target directory name.
2. Run the initializer script with the confirmed parameters.
3. Verify local run instructions:
   - Core: `cd core && cargo run`
   - Portal: `cd portal && npm install && npm run dev`
   - Docker: `cd docker && docker-compose up -d`
4. Run `project-readiness` against the generated repo to validate local compose startup, basic tests, and k8s/gh sanity checks.
5. Hand off the generated skeleton and next steps.

## Suggested Dev Loop (After Bootstrap)

1. Use Plan mode for new feature development (explicit scope, acceptance criteria, test plan).
2. Ask the agent to generate QA test docs for the new feature (use `qa-doc-gen`).
3. Start a new chat and ask the agent to execute QA testing (use `qa-testing`) and wait until it completes.
4. Optionally start a new chat and ask the agent to run security testing/review (use `security-best-practices`) or run a UI/UX pass (use `design-system-guidance` and/or the repo's UI/UX docs under `docs/uiux/`).
5. Start a new chat and ask the agent to check for tickets and fix them end-to-end (use `ticket-fix`).
6. Iterate until the agent no longer produces new tickets from QA.
7. Return to Plan mode and start the next feature; repeat.

## What Gets Generated

- `core/` Rust service with `/health` and `/ready`
- `portal/` React Router 7 + Vite + TypeScript app
- `docker/` Dockerfiles and `docker-compose.yml`
- `k8s/` Base manifests for core and portal
- `scripts/` Local reset script
- `deploy/` K8s deploy/upgrade/cleanup scripts
- Basic unit test setup:
  - `core/`: `cargo test` with a small HTTP smoke test for `/health` and `/ready`
  - `portal/`: `vitest` + Testing Library with `npm run test` and `npm run test:coverage`
- GitHub Actions:
  - `.github/workflows/ci.yml` runs formatting, unit tests, and Docker build checks on PR/push to `main`
  - `.github/workflows/cd.yml` builds and pushes `core` and `portal` images to GHCR on push to `main` (uses `${{ github.repository }}` so no hardcoded username)

## Script Templates

- `scripts/reset-docker.sh` – reset local Docker environment
- `deploy/deploy.sh` – apply k8s base manifests
- `deploy/upgrade.sh` – restart k8s deployments
- `deploy/cleanup.sh` – cleanup k8s namespace

Each script supports optional project extras via `ENABLE_PROJECT_EXTRAS=true`.

## Templates

Templates live in:

```
assets/template/
```

Use placeholders:
- `{{project_name}}`
- `{{core_port}}`
- `{{portal_port}}`
- `{{namespace}}`

## References

- `references/stack.md` for optional extensions (Keycloak/DB/Redis/etc.)
