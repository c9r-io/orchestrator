# Boundary Coverage Governance

`./scripts/coverage-governance.sh` is the canonical command for FR-122. It runs
instrumented Rust tests, Vitest coverage, and Playwright, then writes auditable
artifacts under `target/coverage-governance/`:

- `summary.json`: normalized workspace, component, key-module, React, and
  Playwright scenario coverage;
- `rust.json`: raw LLVM JSON export;
- `rust.lcov`: raw LLVM LCOV export;
- `frontend.json`: Vitest JSON summary;
- `playwright.json`: Playwright JSON report.

The checked-in `boundary-baseline.json` is an approved non-regression baseline,
not a target. A changed risk-sensitive module must add scenario assertions even
when its percentage does not decrease.

## Component Boundaries

| Component | Source roots |
|---|---|
| `core/domain` | `core/` and reusable domain/client crates |
| `daemon adapter` | `crates/daemon/` |
| `CLI` | `crates/cli/` |
| `Tauri Rust` | `crates/gui/` |
| React | `gui/src/` through Vitest V8 coverage |
| Playwright | Executed browser scenarios; no synthetic line percentage |

The report also tracks the daemon Attention, Handoff, Session,
SourceConnection, and Action Audit modules plus CLI and Tauri command roots.

## Exclusions

The normalizer excludes:

- build output and generated sources under `target/`;
- standalone Rust test sources under `tests/`;
- Cargo `build.rs` scripts;
- `core/src/test_utils.rs`, which is fixture infrastructure.

Production `main.rs` files and command adapters remain in the denominator.
Tests embedded beside production code cannot be removed from LLVM's file-level
denominator; reviewers must use scenario assertions in addition to percentages.

## Branch Coverage

`cargo-llvm-cov 0.8.5 --branch` requires nightly Rust. The default stable
toolchain therefore emits:

```json
{
  "status": "unsupported",
  "count": null,
  "covered": null,
  "percent": null
}
```

It must never be interpreted as `0%` or complete coverage. On a pinned nightly
toolchain, set `COVERAGE_BRANCH_MODE=required`; the command fails if real branch
data cannot be produced. Stable CI uses the explicit unsupported state and the
boundary scenario matrix in QA-172.

## Baseline Updates

Baseline changes require review of both the numerator and denominator. Do not
approve a lower value solely because new code was added. Confirm that:

1. new or changed boundary behavior has success and rejection assertions;
2. exclusions did not broaden;
3. Playwright scenario count did not fall;
4. any intentional decrease is recorded in the owning FR and QA evidence.
