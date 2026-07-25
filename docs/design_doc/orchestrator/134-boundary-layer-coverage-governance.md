---
lifecycle: active
related_fr: FR-122
---

# Orchestrator - Boundary Layer Coverage Governance

**Module**: Orchestrator

**Status**: Approved

**Related Plan**: FR-122 CLI, daemon, and Tauri boundary-layer coverage governance

**Related QA**: `docs/qa/orchestrator/172-boundary-layer-coverage-governance.md`

**Created**: 2026-07-25
**Last Updated**: 2026-07-25

## Background

Historical reports showed strong domain coverage but weak user and adapter
boundaries. React coverage and Playwright journeys could not prove daemon
authorization/error mapping, CLI parameter/output behavior, or Tauri command
serialization. Raw Rust reports also displayed no meaningful branch data.

FR-122 establishes a reproducible coverage contract without equating one
percentage with product correctness. Numeric non-regression and explicit
boundary scenarios are independent release signals.

## Goals

- Generate machine-readable Rust, React, and Playwright evidence with one command.
- Compare component and key-module results with an approved baseline.
- Exercise real daemon, CLI, and Tauri adapter boundaries.
- Represent unsupported Rust branch coverage honestly.
- Make FR-095 through FR-118 evidence types discoverable.

## Non-goals

- Requiring public providers, Slack credentials, or paid agents in ordinary CI.
- Setting a repository-wide 100% target.
- Raising percentages by excluding production commands or deleting code.
- Treating browser scenarios as Rust line coverage.

## Scope

- In scope: LLVM JSON/LCOV, Vitest JSON, Playwright scenario reports, baseline
  comparison, CI artifacts, five daemon boundary matrices, CLI transport tests,
  a real CLI gRPC adapter, and a real Tauri invoke/gRPC adapter.
- Out of scope: live Slack recertification, mutation testing, production
  telemetry, and changing product RPC behavior.

## Interfaces

The canonical command is:

```bash
./scripts/coverage-governance.sh
```

It writes:

- `target/coverage-governance/summary.json`;
- `target/coverage-governance/rust.json`;
- `target/coverage-governance/rust.lcov`;
- `target/coverage-governance/frontend.json`;
- `target/coverage-governance/playwright.json`.

`coverage/boundary-baseline.json` is the reviewed policy input. The schema
separates `core/domain`, `daemon adapter`, `CLI`, `Tauri Rust`, React, and
Playwright. Rust key modules include Attention, Handoff, Session,
SourceConnection, Action Audit, CLI commands, and Tauri commands.

## Database Changes

None. All boundary tests use temporary SQLite state owned by `TestState` or
`TestHarness`. They never connect to the developer daemon or active database.

## Key Design

1. LLVM file summaries are normalized to repository-relative paths before
   aggregation. Windows separators, macOS/Linux absolute roots, generated
   output, build scripts, standalone tests, and fixture infrastructure have
   explicit rules.
2. The approved baseline gates line and function percentages with a small
   rounding tolerance. Playwright gates scenario count and failures.
3. Stable Rust reports branch coverage as a structured `unsupported` object.
   `COVERAGE_BRANCH_MODE=required` is available only with nightly plus real
   `cargo-llvm-cov --branch` support.
4. Daemon tests invoke the production `OrchestratorServer` functions with
   isolated state and real authorization policy. They assert status codes,
   request correlation, optimistic conflicts, and fail-closed dependencies.
5. `TestHarness` exposes a cloned raw tonic `Channel`. CLI dispatch and Tauri
   mock runtime use that channel to cross real serialization boundaries without
   duplicating a fake client trait.

## Alternatives And Tradeoffs

- A global percentage threshold was rejected because it rewards low-risk line
  execution and can hide missing authorization or conflict assertions.
- A generated-client trait for every Tauri command was rejected as a broad
  production abstraction solely for tests. Channel injection keeps the
  production client unchanged.
- Nightly branch coverage in every PR was rejected until its output and build
  stability are certified across platforms. Explicit unsupported state plus
  scenario matrices is safer than ambiguous zero data.
- Counting Playwright as line coverage was rejected. It remains an executed
  scenario layer with its own non-regression count.

## Risks And Mitigations

- Risk: full coverage CI is slower than normal tests.
  - Mitigation: parser fixtures run on Linux and macOS; the full instrumented
    report runs once on the approved macOS lane and artifacts are retained.
- Risk: platform-specific code changes the denominator.
  - Mitigation: the baseline records its platform, path fixtures are
    cross-platform, and platform-specific baselines require review.
- Risk: embedded test code affects LLVM file denominators.
  - Mitigation: counts and percentages are both retained and scenario
    assertions remain mandatory.
- Risk: an exclusion broadens unnoticed.
  - Mitigation: exclusions are centralized in the normalizer and documented in
    `coverage/README.md`.

## Observability

- Logs: the shell command emits named collection phases and final artifact paths.
- Metrics: `summary.json` stores count, covered, and percent for every supported
  metric plus Playwright total/passed/failed/skipped.
- Tracing: not applicable; this is a build/QA boundary.
- CI artifacts: raw and normalized files are uploaded even when the gate fails.

## Operations / Release

- Config: `COVERAGE_OUTPUT_DIR`, `COVERAGE_BASELINE`,
  `COVERAGE_BRANCH_MODE`, `COVERAGE_SKIP_FRONTEND`, and
  `COVERAGE_SKIP_PLAYWRIGHT`.
- Tooling: CI pins `cargo-llvm-cov 0.8.5`; frontend versions remain locked by
  `gui/package-lock.json`.
- Rollback: remove the coverage CI jobs and script while retaining ordinary
  tests. No runtime state or database rollback is required.
- Compatibility: reports are additive and do not alter product binaries,
  manifests, RPCs, or persisted data.

## Test Plan

- Unit tests: normalization, aggregation, baseline pass/regression failure, and
  unsupported branch fixtures.
- Adapter tests: five production daemon boundaries, CLI UDS/TLS/TCP failures,
  CLI mutation dispatch over tonic, and Tauri invoke over tonic.
- Integration tests: the reusable in-process `TestHarness` owns temporary state.
- E2E: all Playwright scenarios execute and remain a separate scenario metric.

## QA Docs

- `docs/qa/orchestrator/172-boundary-layer-coverage-governance.md`
- FR-095 through FR-118 evidence index in `docs/qa/README.md`

## Acceptance Criteria

- One command produces machine-readable workspace/component/module artifacts.
- CI compares the approved baseline and uploads raw plus normalized evidence.
- Five daemon boundary families cover success and rejection/conflict classes.
- CLI and Tauri have reusable real gRPC adapter templates.
- Rust branch data is a real percentage or explicit `unsupported`.
- FR-095 through FR-118 DD/QA evidence types are indexed.
- Workspace tests, Clippy, Vitest, Playwright, formatting, and doc lint pass.

## Major Code Touchpoints

- `scripts/coverage-governance.sh`
- `scripts/coverage/coverage-governance.mjs`
- `coverage/boundary-baseline.json`
- `.github/workflows/ci.yml`
- `crates/daemon/src/server/boundary_contract_tests.rs`
- `crates/cli/src/grpc_adapter_tests.rs`
- `crates/cli/src/client.rs`
- `crates/gui/src/state.rs`
- `crates/gui/src/lib.rs`
- `crates/integration-tests/src/lib.rs`
