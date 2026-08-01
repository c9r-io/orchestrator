---
lifecycle: active
related_fr: FR-076
---

# Orchestrator - The GUI Crate Is Built, Linted, And Tested By CI

**Module**: CI enforcement surface / `crates/gui` (orchestrator-gui)
**Scope**: the `clippy` and `test` jobs of `.github/workflows/ci.yml` after
FR-076 requirement 1 removed `--exclude orchestrator-gui` from both; the
GUI build prerequisites those jobs install (webkit2gtk/gtk dev packages,
Node 22, the npm-built `gui/dist` that `tauri::generate_context!` reads at
compile time); the deliberate survivals of the exclusion (the cross-compile
job, and the ci-required gates that run in jobs without those
prerequisites, governed by `workspaceScope` in
`config/governance/qa-gate-surface.json`).
**Scenarios**: 5
**Priority**: High

## Background

FR-076 requirement 1 (P1, landed independently of the deferred packaging
requirements 2–4): the one crate awaiting release was excluded from the
lint and test jobs. Phase 2 fact verification corrected the FR's premises
before implementation: the exclusion was triple (clippy, test, and the
cross-compile `cargo check`), not double; and the crate was not wholly
uncovered — the `boundary-coverage` job has compiled and tested it on macOS
via `cargo llvm-cov --workspace` since `c9ada747` (2026-07-26), one day
after the FR's supplement was written. What was genuinely missing was
clippy coverage and any Linux build. Design record:
`docs/design_doc/orchestrator/165-gui-ci-integration.md`.

**Safety**: scenarios 1, 4 and 5 are read-only derivations against the
working tree. Scenario 2 compiles and tests the workspace. Scenario 3 is a
recorded one-time verification performed on a throwaway branch via
`workflow_dispatch`; re-running it is optional and never touches `main`.

## Scenario 1: the de-exclusion and its prerequisites are derived from the workflow, not asserted

Steps:

```bash
for job in clippy test; do
  ruby scripts/lib/workflow_model.rb run-commands .github/workflows/ci.yml "$job"
  ruby scripts/lib/workflow_model.rb step-names .github/workflows/ci.yml "$job"
done
```

Expected result: in both jobs the workspace cargo command carries no
`--exclude orchestrator-gui` (clippy: `cargo clippy --workspace
--all-targets -- -D warnings`; test: `cargo test --workspace`), and the
step list contains `Install GUI system dependencies`, `Install frontend
dependencies`, and `Build the frontend bundle the Tauri crate compiles
against` **before** the cargo step — a job that dropped the exclusion
without the prerequisites would fail on webkit pkg-config or on the missing
`gui/dist`, not silently narrow.

## Scenario 2: the widened commands pass with the GUI crate in scope

Steps (requires the GUI prerequisites: macOS, or Linux with
`libwebkit2gtk-4.1-dev libgtk-3-dev`; plus `npm --prefix gui ci && npm
--prefix gui run build`):

```bash
cargo clippy --workspace --all-targets -- -D warnings > /tmp/fr076-clippy.log 2>&1; echo $?
cargo test --workspace > /tmp/fr076-test.log 2>&1; echo $?
```

Expected result: both exit 0, and `/tmp/fr076-test.log` contains
`test result:` lines for the `orchestrator_gui_lib` targets (the crate is
in scope, not skipped) — verified at `9e2c54f6` before the ci.yml change
landed, establishing there was no accumulated lint or test debt to defer.

## Scenario 3: a GUI compile error fails CI (recorded negative verification)

Steps (one-time, recorded; reproduction recipe):

```bash
git checkout -b fr076-negative-verification
echo 'compile_error!("FR-076 negative verification");' >> crates/gui/src/lib.rs
git commit -am "test: FR-076 negative verification" && git push origin HEAD
gh workflow run ci.yml --ref fr076-negative-verification
gh run list -w ci.yml --branch fr076-negative-verification
gh run view <run-id> --json jobs \
  --jq '.jobs[] | select(.name=="Rust clippy" or .name=="Rust test") | {name, conclusion}'
git push origin :fr076-negative-verification
```

Expected result: both the `Rust clippy` and `Rust test` **job conclusions**
are `failure` (per §4.4 shape 6 the evidence is the job conclusion, never a
step conclusion), while the same run's `Rust fmt` concludes `success` — so
the run reached the jobs and failed through the compile error, not through
setup. Recorded evidence: pending — the run ID and branch head are pinned
here by the verification pass that executes this scenario.

## Scenario 4: the exclusion that survives is still enforced where it must be

Steps: `bash scripts/qa/test-qa-gate-surface.sh > /tmp/fr076-surface.log 2>&1; echo $?`

Expected result: exit 0 with the suite's final summary line present.
Fixture 18 (a ci-required gate widening to the full workspace in a job
without the GUI prerequisites, with no declared `workspaceScopeReason`)
still fires — the de-exclusion of the sibling jobs did not disarm
`check_workspace_scope`, because `workspaceScope.excludes` deliberately
still names `orchestrator-gui`.

## Scenario 5: the CI ledgers converge on the widened pipeline

Steps: `ruby scripts/qa/ci-liveness.rb; echo $?` and
`ruby scripts/qa/ci-cost.rb; echo $?`

Expected result: both exit 0 at the closure revision — every ci.yml job
including the widened `clippy` and `test` concluded `success` on a run
whose `headSha` postdates the last ci.yml change, and the cost ledger's
budget check holds (the widened jobs are ledgered but outside the budgeted
governance pair).

## Checklist

- [ ] the negative evidence cites a **job** conclusion from a real run at a
      pinned revision, never a green step list (§4.4 shape 6)
- [ ] scenario 1 derives commands and step order from the parsed workflow,
      not from a grep of the YAML text
- [ ] the cross-compile job's surviving exclusion carries its reason in a
      comment, and the clippy/test jobs carry none
- [ ] `workspaceScope.excludes` stays non-empty while any ci-required gate
      runs in a job without the GUI prerequisites (fixture 18 stays armed)
- [ ] CONTRIBUTING.md and the PR template state the same commands ci.yml
      runs, with the local-prerequisite escape hatch marked as the
      exception, not the canonical form
