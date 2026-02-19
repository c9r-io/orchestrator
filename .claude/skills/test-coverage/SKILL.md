---
name: test-coverage
description: Run unit tests and measure test coverage for backend/frontend projects, identify coverage gaps, and improve tests based on project requirements. Use when the user asks to run tests, check coverage, increase coverage, or add missing unit tests.
---

# Test Coverage Skill

Run tests and measure coverage in a project-agnostic way, preferring existing project commands and conventions.

This skill aligns with `project-bootstrap` conventions (common layout: `core/`, `portal/`, `scripts/`, `docker/`, `docs/qa/`, `docs/ticket/`), but should work with other layouts by discovering commands.

## Workflow

1. Discover the test commands already defined by the project.
2. Run unit tests first (fast feedback).
3. If coverage is requested: run coverage tooling and identify high-value gaps.
4. Improve tests focusing on behavior and critical branches, not raw percentages.
5. Re-run tests and coverage to confirm improvement.

## Discover Existing Commands

From repo root, look for:
- Backend:
  - Rust: `core/Cargo.toml`
  - Makefile targets: `Makefile`, `core/Makefile`
- Frontend:
  - `portal/package.json`

Prefer running existing scripts over inventing new ones.

## Backend (Rust, if `core/` exists)

### Unit Tests

```bash
cd core
cargo test
```

### Coverage

Prefer `cargo llvm-cov` if the project uses it (or has Makefile wrappers). Typical options:

```bash
cd core
cargo llvm-cov
cargo llvm-cov --html
```

> **Why llvm-cov over tarpaulin?** tarpaulin has known issues with `#[async_trait]` macros (reports dramatically lower coverage for async trait code). `cargo-llvm-cov` uses LLVM's instrumentation and correctly tracks async trait coverage.

If a Makefile defines coverage targets, use those instead:

```bash
cd core
make coverage
make coverage-html
```

Notes:
- Coverage exclusions (migrations, generated code, thin wiring) are project-specific. Follow the project's existing exclusions if present.
- Document coverage exclusions so reviewers understand why files are skipped:

| Example Exclusion | Typical Reason |
|-------------------|---------------|
| `repository/*.rs` | Thin data mapping layer (ORM-equivalent), no business logic |
| `main.rs` | Program entry point |
| `migration/*.rs` | Database migration scripts |
| `src/index.ts` | Barrel re-export, no logic |
| `src/types/**/*.ts` | Pure type definitions, no runtime code |

- Treat coverage as a signal: prioritize service/business logic and error branches.

## Frontend (TypeScript, if `portal/` exists)

Run the project's test command (examples):

```bash
cd portal
npm test
```

If Vitest is used:

```bash
cd portal
npx vitest --run
```

Coverage (examples):

```bash
cd portal
npm test -- --coverage
```

## Coverage-Driven Improvement Guidance

When asked to "complete tests" or "increase coverage":
- Add tests for behaviors users rely on (API validations, authz checks, invariants, boundary conditions).
- Prefer testing pure business logic without Docker.
- Avoid brittle implementation-detail assertions.
- For E2E gaps, add only a few stable tests for critical flows (see `e2e-testing` skill).

## Coverage Targets (Guidance)

All test coverage targets follow a project minimum of `>=90%` where possible:

| Layer | Target | Notes |
|-------|--------|-------|
| Domain/Business logic | >=90% | Pure validation, no I/O |
| Service layer | >=90% | Core business logic |
| API handlers | >=90% | HTTP routing and request validation |
| gRPC handlers (if applicable) | >=90% | RPC service methods |
| Repository layer | N/A | Often excluded from tracking (thin data mapping) |
| Frontend utils/services | >=90% | Non-UI logic |

## HTTP Handler Testing with Dependency Injection

If the project uses a DI / `HasServices` trait pattern (generic handlers with swappable state), test production handler code by providing a test state that implements the same trait:

```
Production:  AppState (impl HasServices) → build_router() → Handlers<AppState>
Tests:       TestAppState (impl HasServices) → build_router() → Handlers<TestAppState>
                                                    ↑
                                          Same production handlers!
```

Key rules:
1. **Always use the trait** for new handlers — never use concrete `AppState` directly.
2. **Test production code** — `build_test_router()` should use production `build_router(state)`.
3. **Mock external services** with wiremock or equivalent.
4. **Use test repositories** — in-memory implementations of repository traits.

## Troubleshooting

- **Compilation errors**: Run `cargo clean` first
- **Mock expectations not met**: Check predicate conditions
- **Low coverage with tarpaulin**: Use `cargo llvm-cov` instead (tarpaulin can't track `#[async_trait]` macros properly)
- **llvm-cov not found**: Install with `cargo install cargo-llvm-cov`
