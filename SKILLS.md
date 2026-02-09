# Skills

This repo stores skills under `.claude/skills/`. Other tools discover them via symlinks under:
- `.cursor/skills/`
- `.gemini/skills/`
- `.agents/skills/`

## Platform Loop

Use these skills together for an end-to-end iteration loop:
1. `project-bootstrap`: create a new project skeleton.
2. Plan explicitly, then implement.
3. `qa-doc-gen`: convert the approved plan into reproducible QA docs under `docs/qa/`.
4. `qa-testing`: execute QA docs and create failure tickets under `docs/ticket/`.
5. `ticket-fix`: reproduce, fix, reset, re-test, and close tickets.

## Skills (By Purpose)

### Bootstrap

- `project-bootstrap`
  - Create `core/`, `portal/`, `docker/`, `k8s/`, `deploy/`, `scripts/`, `docs/qa/`, `docs/ticket/`.
  - Includes basic unit test setup for backend and frontend.
  - Typical prompt: "Bootstrap a new project named acme in /path/to/acme."

### Conventions

- `rust-conventions`
  - Apply Rust service conventions when working in `core/`.
  - Typical prompt: "Refactor this service logic to match Rust conventions and add unit tests."

### QA Loop

- `qa-doc-gen`
  - After the plan is approved, generate QA docs under `docs/qa/` following the repo format.
  - Typical prompt: "The plan is approved. Generate QA docs."

- `qa-testing`
  - Execute scenarios from `docs/qa/**/*.md` with optional UI automation, optional DB validation, and ticket creation.
  - Uses `.claude/skills/tools/qa-api-test.sh` for optional API calls.
  - Typical prompt: "Execute docs/qa/user/01-crud.md step-by-step and create tickets for failures."

- `ticket-fix`
  - Take a ticket under `docs/ticket/`, reproduce quickly, implement fix, reset env, re-run steps, then close the ticket.
  - Typical prompt: "Fix docs/ticket/user_01-crud_scenario2_*.md and rerun verification."

### Security Docs

- `security-test-doc-gen`
  - Generate and complete `docs/security/**` based on the current project and/or confirmed plan, using OWASP ASVS 5.0 (default L2) as the baseline.
  - Typical prompt: "This feature is implemented. Complete the relevant security test docs and scenarios."

### UI/UX Docs

- `uiux-test-doc-gen`
  - Generate and maintain `docs/uiux/**` based on the project UI implementation and `docs/design-system.md` constraints.
  - Typical prompt: "Align UI/UX constraints during development and update the relevant docs/uiux scenarios."

### Testing and Coverage

- `test-authoring`
  - Add unit tests and/or Playwright E2E tests for new features.
  - If asked to "complete tests", run coverage, find gaps, and improve tests based on project requirements.
  - Typical prompt: "This feature is implemented. Add missing unit tests and key E2E coverage."

- `test-coverage`
  - Run unit tests and measure coverage (backend and frontend) using project-defined commands.
  - Typical prompt: "Run coverage, identify the most valuable gaps, and fill them."

- `e2e-testing`
  - Run and author Playwright E2E tests (frontend-only or full-stack).
  - Full-stack runs should reset state first via `./scripts/reset-docker.sh` (or an existing `:full:reset` script).
  - Typical prompt: "Add a stable Playwright E2E test for this critical flow and make sure it is not flaky."

- `performance-testing`
  - Run quick load tests with `hey` against key endpoints and compare regressions.
  - Typical prompt: "Benchmark /health and /api/v1/items and report p50/p99 and QPS."

### Ops and Debugging

- `ops`
  - Triage local Docker Compose issues and Kubernetes rollouts: status, logs, restarts, rollout debugging.
  - Typical prompt: "Services fail to start. Triage compose logs and propose the smallest fix."

- `reset-local-env`
  - Reset local Docker Compose environment to a clean state (rebuild images, drop volumes).
  - Typical prompt: "The environment has dirty data. Reset it and rerun tests."

- `grpc-regression`
  - Run `grpcurl` from inside the Docker network when host-to-container ports are blocked/flaky.
  - Typical prompt: "Host-to-container gRPC is failing. Use grpcurl inside the Docker network to verify a method call."

### Doc Constraints (Auto-Load on Demand)

- `arch-guidance`
  - When discussing architecture/refactors/deployment models, read `docs/architecture.md` first and apply it.

- `design-system-guidance`
  - When discussing UI tokens/components/theming, read `docs/design-system.md` first and apply it.

## Notes

- Skills are intentionally project-agnostic and assume `project-bootstrap` conventions when present.
- For tool configuration, see `AGENTS.md` and `.CLAUDE.md`.
