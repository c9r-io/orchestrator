---
lifecycle: active
related_fr: FR-076
---

# GUI CI Integration

**Status**: Released (FR-076 requirement 1 only; requirements 2–4 — packaging,
release workflow, signing — remain deferred and the FR stays open)
**FR**: FR-076（GUI 正式发布 — 需求 1：CI 集成）
**QA**: docs/qa/orchestrator/203-gui-ci-integration.md

## The problem

`crates/gui` (5592 lines / 22 files, `find`+`wc` at `9e2c54f6`) was the one
crate awaiting user-facing release and the one crate the lint and test jobs
never saw: `ci.yml` ran clippy and test with `--exclude orchestrator-gui`.
The FR called this an inverse priority — the cost of restoring coverage
only grows — and asked for the exclusion's removal with negative
verification that a GUI compile error actually fails CI.

## What the FR got wrong, and what that taught

Phase 2 step 0 rebuilt the FR's claims at `9e2c54f6` before planning.

### The exclusion was triple, not double

The FR's 2026-07-25 supplement counted two exclusion sites (clippy, test).
The tree had three: the cross-compile job's `cargo check --workspace
--exclude orchestrator-gui --target …` matrix. The third site survives this
FR deliberately — see the decision below — but a requirement written as
"remove both" against a three-site surface would have certified completion
one site early. The recurring lesson: enumerate the surface from the tree,
not from the document proposing to change it.

### "The only crate with no CI coverage" had a one-day shelf life

True when written on 2026-07-25. One day later (`c9ada747`, 2026-07-26) the
`boundary-coverage` job began building `gui/dist` and running `cargo
llvm-cov --workspace --all-targets --all-features` on macOS — compiling
*and testing* orchestrator-gui on every push since. What remained genuinely
missing was narrower than the FR believed: clippy lint coverage (nothing
ran clippy over the crate anywhere) and any Linux build (macOS supplies the
Tauri frameworks as system libraries; Linux needs webkit2gtk/gtk packages
nothing installed). A bare claim in a funded document decays without a
pinned revision; this one decayed in 24 hours.

### The anticipated lint debt was zero

The FR budgeted for "累积的 lint 债" to be repaid at restoration time. At
`9e2c54f6`, `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` — both GUI-inclusive — pass clean on macOS. The
boundary-coverage job compiling the crate on every push is the likely
reason the debt never accumulated: the crate was excluded from the *lint*
gate but never from *compilation*. The FR's cost model assumed total
abandonment; actual abandonment was partial.

## Decisions

### Widen the existing jobs in place, not a separate GUI job

The acceptance criterion asked for the exclusion's removal from the clippy
and test jobs, and that is also the technically stronger shape: a separate
`-p orchestrator-gui` job would lint the crate under package-scoped feature
unification, which differs from the workspace build — observed directly
when `cargo clippy -p orchestrator-gui --all-targets -- -D warnings` at
`9e2c54f6` failed on dead code in *orchestrator-persistence*
(`configure_conn`, `crates/orchestrator-persistence/src/db.rs:152`) that
the workspace-unified build compiles as live. The workspace command in the
workspace jobs exercises exactly the graph every other crate is verified
under. Cost: both jobs pay the webkit apt install, the Node setup, the npm
build, and the Tauri dependency tree compile. The budgeted governance pair
is untouched (the budget in `config/governance/ci-step-cost.json` names
`governance` and `ci-environment-parity` only); the new step timings enter
the cost ledger as attribution on the next refresh.

### Prerequisites are copied from the job that already proved them

`boundary-coverage` established the recipe (Node 22, `npm ci`, `npm run
build` producing the `gui/dist` that `tauri::generate_context!` reads at
compile time) on macOS. The clippy/test jobs add the Linux half:
`libwebkit2gtk-4.1-dev libgtk-3-dev` — the pkg-config surface for tauri 2
with no tray-icon or global-shortcut features (no appindicator, no libxdo).

### The cross-compile exclusion survives, with its reason in the workflow

On the foreign-target legs the gtk-rs sys crates cannot link against a
cross `--target` (dev packages exist for the build host); on the two
host-target legs the job builds no `gui/dist`. Removing the exclusion there
buys a fourth compilation of the same crate at the price of webkit
cross-toolchains that do not exist on the runners. The exclusion is now
commented at the site; an uncommented survival of a flag this FR exists to
remove would read as an oversight forever.

### `workspaceScope` changes premise, not mechanism

`config/governance/qa-gate-surface.json`'s `workspaceScope.excludes` keeps
`orchestrator-gui` and check 7 (`check_workspace_scope`) stays armed,
because its real subject was never "match the siblings" — it is "a
ci-required gate must not run cargo over a crate that cannot build in the
job the gate runs in", and the gate-hosting jobs (coordination-strangler,
governance, ci-environment-parity) still install none of the GUI
prerequisites. Both manifest reason texts were rewritten because their
premises ("no job installs the dependencies", "the only place the crate is
compiled") are now false. Fixture 18 remains valid and armed: a gate
widening past its job's capabilities without a declared reason still fails.

## Known limits

- ~~**Package-scoped clippy of the GUI crate fails today**: `cargo clippy -p
  orchestrator-gui --all-targets -- -D warnings` dies on
  `orchestrator-persistence`'s `configure_conn` becoming dead code under
  narrowed feature unification (observed at `9e2c54f6`). Out of this FR's
  scope — the CI shape is workspace-wide — but anyone adding a `-p`-shaped
  GUI job inherits it. The symbol belongs to persistence governance.~~
  **Fixed at `c1748df3`**, where persistence governance took it as predicted:
  `db::configure_conn` was a pass-through to `sqlite::configure_conn` reachable
  only from the `test-support` feature and a cfg(test) module, so both callers
  now name the target directly and the pass-through is gone. The package-scoped
  invocation exits 0. The observation above about *why* the two invocations can
  disagree stands and is the reason the jobs stay workspace-wide; what is no
  longer true is that this particular symbol is waiting for anyone.
- **The GUI is still absent from the cross-compile matrix**, so a
  GUI-only breakage of a foreign target (e.g. aarch64-linux) is invisible
  until requirement 2 builds real bundles per platform.
- **`gui/dist` is a build product, not a tracked input**: the compile-time
  contract is only as reproducible as `npm ci` + `npm run build` on
  `gui/package-lock.json`. A frontend build failure now fails the clippy
  and test jobs too — three jobs share the single point of failure that
  previously only `boundary-coverage` carried.
- **Requirements 2–4 remain deferred**: no packaging, no release assets,
  no signing. The FR document stays open under `docs/feature_request/`
  with requirement 1 marked done and this record as its evidence.
