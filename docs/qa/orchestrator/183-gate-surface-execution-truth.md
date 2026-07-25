---
lifecycle: active
related_fr: FR-134
self_referential_safe: true
---

# Orchestrator - Gate Surface Execution Truth

**Module**: Governance / CI
**Scope**: execution-fact assertions in the enforcement surface gate, discovered rather than
enumerated coverage, CI job dependency and liveness, environment equivalence, and lexical safety in
the shared ratchet scanner
**Scenarios**: 5
**Priority**: High

## Background

FR-127 built the enforcement surface ledger. A mutation audit of the gate that guards it found four
ways to break the repository while it reported **5 passed, 0 failed** — three of them the same
error the ledger existed to eliminate, text describing a fact standing in for the fact. Two real CI
runs then showed that gates FR-127 counted as its coverage had never executed an assertion: their
jobs did not install ripgrep, so they exited on their own `command -v` preamble.

Every scenario below is a defect that was live in this repository, not a hypothetical.

All scenarios are read-only against the working tree or operate on copies under `$TMPDIR`. Nothing
starts a daemon, touches the runtime database, or invokes a provider — the gates under test are
themselves the provider-isolation machinery, and their fixtures use stubs. See DD-145.

Primary entry points:

```bash
./scripts/qa/test-qa-gate-surface.sh                   # 12 checks
./scripts/qa/test-qa-gate-surface.sh --fixture-test    # 27 assertions: 21 negative fixtures, two
                                                       # positive controls, a behavioural case,
                                                       # and three meta-assertions
./scripts/qa/test-skill-mirror-integrity.sh --fixture-test
./scripts/qa/test-ci-liveness.sh                       # 9 liveness fixtures
./scripts/qa/test-ci-environment-parity.sh             # and --fixture-test
./scripts/qa/test-core-boundary.sh                     # cases 10-12 are the lexer
```

---

## Scenario 1: The Four Reproduced Defects Are Rejected

### Preconditions

- Clean worktree; repository root is the working directory.
- `jq`, `rg`, `git`, `ruby` on PATH.

### Steps

```bash
# 1. Each defect as a resident fixture, with its isolation assertion.
./scripts/qa/test-qa-gate-surface.sh --fixture-test

# 2. End to end: apply all four to a throwaway copy and run the gate against it.
WORK="$(mktemp -d)"; git archive HEAD | (cd "$WORK" && tar xf -)
cd "$WORK" && git init -q . && git add -A && git -c user.email=q@l -c user.name=q commit -qm base

perl -pi -e 's{^(\s*)run: \./scripts/qa/test-filesystem-trigger\.sh$}{$1# disabled: was flaky}' \
  .github/workflows/ci.yml
perl -pi -e 's{^assert_provider_shadow}{# assert_provider_shadow};
             s{^export PATH="\$QA_ROOT/bin:\$PATH"}{# export PATH="\$QA_ROOT/bin:\$PATH"}' \
  scripts/qa/test-agent-driver-production-parity.sh
cat >> fixtures/manifests/bundles/coordination-strangler-parity.yaml <<'YAML'
---
apiVersion: orchestrator/v1
kind: Agent
metadata: {name: fr134-unpinned}
spec: {driver: {provider: claude}}
---
apiVersion: orchestrator/v1
kind: Agent
metadata: {name: fr134-decoy}
spec: {driver: {provider: mock, binary: fake-decoy}}
YAML
printf '\nEnforced by the release gate via test-webhook-trigger.sh.\n' >> README.md
git add -A && bash scripts/qa/test-qa-gate-surface.sh
```

### Expected Result

- Step 1: `FR-127 gate surface fixtures: 27 passed, 0 failed`. Fixtures 8-16 are FR-134's;
  each reports `(isolated to <check>)`, meaning it fails its target and leaves the others passing.
- Step 2 exits 1 with three checks red — wiring, provider isolation, stale claims. Before this FR
  the same tree produced `5 passed, 0 failed`.
- Fixtures 8 and 9 cover the wiring shapes that are *not* execution: a commented-out `run:`, an
  `if: false` step, a `name:` mention, and a heredoc body.
- Fixture 12 neuters `assert_provider_shadow` itself while leaving its call site intact; only
  executing the mechanism catches that.

---

## Scenario 2: The Gates That Were Wired Now Actually Run

### Preconditions

- Clean worktree. `gh` authenticated for the CI verification step.

### Steps

```bash
# The four environment checks, and the repair they were written against.
./scripts/qa/test-qa-gate-surface.sh | grep -E 'commands|workspace|discards|stubs'

# Reverting the repair must reproduce the failures the checks assert.
git revert --no-commit 34d3b582 && ./scripts/qa/test-qa-gate-surface.sh; git revert --abort || git reset --hard

# The behavioural half: output must survive, not just be un-discarded in source.
./scripts/qa/test-qa-gate-surface.sh --fixture-test | grep behavioural

# Real CI, which is the only place findings A and B were visible at all.
gh run view <run-id> --json jobs -q '.jobs[]|"\(.conclusion)\t\(.name)"'
```

### Expected Result

- All four checks pass on the repaired tree: dependencies provided, workspace scope aligned or
  declared, no discarded diagnostics, stubs present in every provider-capable job.
- On the reverted tree they fail, naming `coordination-strangler` and
  `slack-certification-recorded` for `rg` and `test-filesystem-trigger.sh` for the workspace.
- The behavioural case runs the trigger gate under a `cargo` that fails with
  `error[E0425] … fr134_sentinel` and requires that text to reach the gate's output. A gate that
  satisfied the source rule while still swallowing output passes the check and fails this.
- In real CI both repaired jobs reach their assertions instead of stopping at `command -v`.
- The governance job's step-level reporting prints every gate's outcome and a final step fails on
  any of them. Its first real run printed nineteen outcomes with three red, one of which —
  `test-agent-driver-production-parity.sh` — had been failing on every run since it was wired,
  behind two earlier failures in a job that stopped at the first. Note that `continue-on-error`
  makes GitHub report each step's `conclusion` as success while `outcome` holds the truth, so the
  step list alone reads green; the summary table is what makes the run legible.
- `check_git_history_available` covers the cause: `git cat-file`, `git merge-base` and the reverse
  `git apply` that carry FR-126's retirement-parity evidence all fail on the single commit
  `actions/checkout` fetches by default, and all pass on any developer machine. Fixture 21 reverts
  `fetch-depth: 0` and requires the failure.

---

## Scenario 3: Coverage Is Discovered, Not Enumerated

### Preconditions

- Clean worktree.

### Steps

```bash
# Mirror roots: an undeclared root of pure symlinks, and the reverse direction.
./scripts/qa/test-skill-mirror-integrity.sh --fixture-test | grep -E 'fixture 9'

# Classification reaches subdirectories.
./scripts/qa/test-qa-gate-surface.sh --fixture-test | grep 'fixture 15'

# Stale claims read every tracked Markdown file, not two directories.
./scripts/qa/test-qa-gate-surface.sh --fixture-test | grep -E 'fixture 13|fixture 14'

# CI liveness discovers jobs by parsing workflows.
./scripts/qa/test-ci-liveness.sh
```

### Expected Result

- Mirror fixture 9: a `.windsurf/skills/` holding one correct and one misnamed symlink fails
  discovery. Fixture 9b: declaring that root subjects it to coverage and shape rather than
  excusing it — without 9b the check would only teach people to write one line of JSON.
- Fixture 15: `scripts/qa/lib/hidden-gate.sh` fails classification. The live instance,
  `scripts/qa/lib/slack-live-certification-lib.sh`, was tracked and invisible before this FR.
- Fixture 13: a claim planted in `README.md` — one of 41 tracked files the old scope never read —
  is now inside the scan. Fixture 14: an exemption for a file that makes no claim is stale.
- `test-ci-liveness.sh` reports `9 passed, 0 failed`, including a job added to a workflow with no
  record, a new workflow file with no entry, and a record naming a commit outside this history.

---

## Scenario 4: A Gate's Environment Is Part Of The Gate

### Preconditions

- Clean worktree.

### Steps

```bash
# The self-lock, which made this gate green locally and dead in CI.
bash scripts/qa/test-governance-ledger-tooling.sh; echo "no CI: $?"
CI=1 bash scripts/qa/test-governance-ledger-tooling.sh; echo "CI=1: $?"

# Generalised: every in-scope gate, both worlds, same exit code.
./scripts/qa/test-ci-environment-parity.sh
./scripts/qa/test-ci-environment-parity.sh --fixture-test

# The widened unattended-write guard.
env -u CI GITHUB_ACTIONS=true ruby scripts/qa/core-boundary.rb --emit-baseline --write; echo $?

# Liveness freshness, and its annotation lifecycle.
ruby scripts/qa/ci-liveness.rb
```

### Expected Result

- Both ledger-tooling invocations exit 0 with `8 passed, 0 failed`. Before the fix, `CI=1` exited 2
  after the second case: case 2 verifies that `--write` refuses under CI, case 3 then called
  `--write` and was killed by the mechanism case 2 had just confirmed. That gate had never
  succeeded once in the job it was wired into.
- Parity reports `6 passed, 0 failed` in fixture mode, including a gate that exits 2 only under CI
  (detected), a gate that fails identically in both (correctly not reported — that is a different
  problem), and the two that stop this gate running itself. Its first version did select itself:
  the CI job sat at 52 minutes, and a hang looks like nothing at all because it produces no failure
  output. Self-exclusion is derived from `BASH_SOURCE`, and a sentinel variable closes the indirect
  case that path exclusion cannot see. Verification mode takes about 7.5 minutes for 15 gates.
- `core-boundary.rb --write` exits 2 under `GITHUB_ACTIONS` alone, and `test-core-boundary.sh`
  case 7b holds it. Against the pre-FR-134 guard that case reports
  `--write ran with only GITHUB_ACTIONS set (exit 0)` — it wrote the reviewed ledger with no
  human present. Case 7c is the other direction: `CI=false` must be treated as interactive, or the
  recovery path this ledger depends on is unusable on a machine that sets it; the old guard tested
  for presence and refused. Both cases fail on the old implementation and pass on the new one,
  which is what makes them evidence rather than decoration.
- `ci-liveness.rb` passes only when every job of every in-scope workflow has a record that is
  fresh against its workflow, and every non-success record names a reference and a reason.

---

## Scenario 5: A Brace Inside A Literal Is Not A Brace

### Preconditions

- Clean worktree. `ruby` and `cargo` on PATH.

### Steps

```bash
./scripts/qa/test-core-boundary.sh                       # cases 10, 11, 12

ruby scripts/qa/coordination-governance.rb --emit-baseline
ruby scripts/qa/core-boundary.rb | tail -2

ruby -e '$LOAD_PATH.unshift "scripts/lib"; require "rust_source"; require "pathname"
p RustSource.unclosed_test_modules(Pathname.new(Dir.pwd))'
```

### Expected Result

- `FR-130 core boundary: 12 passed, 0 failed`.
- Case 10: production `rusqlite`, `captures` and `PipelineVariables` lines placed after a
  `cfg(test)` module containing `format!("{err}")`, `"{{bad"` and `String::from("{")` move **both**
  baselines. Before the fix they moved neither — the module's range ran to end of file and every
  line after it left the scan silently, which is invisible precisely because no number changes.
- Case 11: a tail module containing the multi-line raw string `r#"{"items": [` moves **neither**
  baseline. This is the regression a per-line regex fix produces: it cannot see a raw string
  spanning lines, closes that module 245 lines early, and moves `capturesOrJsonPath` from 53 to 60
  by handing test fixtures to the ratchet. The obvious fix is worse than the defect.
- Case 12 and the inline scan both report no unclosed module.
- Baselines unchanged: `53 / 30 / 9 / 0`, `200 / 37`, and `52 / 924 / 143`.

---

## Mutation Evidence

Every check was neutered by inserting `return 0` after its opening brace, and its fixtures were
required to fail. A check whose fixtures still pass when the check does nothing is not tested by
them.

| Check | Fixture failures when neutered |
|---|---|
| `check_surface_complete` | 3 |
| `check_support_files_declared` | 1 |
| `check_reason_and_owner` | 1 |
| `check_wiring_truth` | 3 |
| `check_provider_isolation` | 5 |
| `check_no_stale_claims` | 2 |
| `check_no_stale_claim_exemptions` | 1 |
| `check_job_dependencies` | 1 |
| `check_workspace_scope` | 1 |
| `check_diagnostics_preserved` | 1 |
| `check_provider_stub_coverage` | 1 |
| `check_git_history_available` | 1 |
| `check_mirror_roots_discovered` | 1 |
| `check_environment_parity` | 1 |

Every check reached a non-zero count, so no check is carried by fixtures that would pass without
it. The counts differ because some checks are targeted by several fixtures — `check_wiring_truth`
by fixtures 4, 8 and 9, `check_provider_isolation` by 5, 6, 10, 11 and 12 — and neutering one also
fails the isolation assertion of every fixture that names a *different* target but relies on this
one still passing.

Two mutations were caught by the fixture harness rather than by a check, and are the reason
`inject()` exists: fixtures 8 and 9 stopped matching when `ci.yml`'s steps gained `id:` lines, and
reported *"check_wiring_truth accepted the injected defect"* — accusing the check of the fixture's
own failure to inject anything. `inject()` hashes the file before and after and fails loudly when
nothing moved.

## Retirement And Migration Parity

FR-134 replaces four assertion implementations rather than removing a feature, so the parity
evidence is behavioural rather than inventory-based.

**Recorded baseline.** The pre-fix behaviour is recorded exactly: all four mutations applied to a
clean `git archive` of `HEAD` produced `FR-127 gate surface: 5 passed, 0 failed`. The pre-repair
run of the four environment checks is recorded in the commit message of `186521e2`, which lands the
checks *before* the repair for this reason.

**Per-object comparison.** Each of the 11 checks is compared individually by the mutation table
above, not in aggregate. Each of the four defects is compared individually by fixtures 8-13.

**Reverse-applicable removal.** `git revert 34d3b582` restores the pre-repair CI configuration and
reproduces the four environment-check failures; `git revert 445fa9ed` restores the pre-rewrite
assertions and reproduces `5 passed, 0 failed` under the four mutations. Both reverts are clean.

**End-to-end behaviour.** The thing the enforcement surface exists to do is keep a real provider
CLI away from CI and keep a disabled gate from looking enforced. Both are asserted by execution:
`assert_provider_shadow` resolves the provider through the PATH the run will use, and
`check_provider_isolation` executes that assertion in both directions rather than reading it.

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | The four reproduced defects are rejected | ☑ PASS | 2026-07-25 | Claude |
| 2 | The gates that were wired now actually run | ☑ PASS | 2026-07-25 | Claude |
| 3 | Coverage is discovered, not enumerated | ☑ PASS | 2026-07-25 | Claude |
| 4 | A gate's environment is part of the gate | ☑ PASS | 2026-07-25 | Claude |
| 5 | A brace inside a literal is not a brace | ☑ PASS | 2026-07-25 | Claude |

## Certification Conditions

A run of these scenarios counts as closure evidence only when `git status --porcelain` is empty at
start and at end, `git rev-parse HEAD` matches across the run, nothing else is writing to the
repository, each script is invoked as `bash <script> > log 2>&1` with `$?` captured directly rather
than through a pager, and each log ends with its own summary line.

This FR adds one condition the others did not need: **a local pass is not the evidence.** Both of
FR-134's CI findings were invisible on a developer machine — ripgrep is installed there, and macOS
supplies the Tauri frameworks as system libraries. The certifying observation is a real workflow
run read through `gh run view`, and the CI liveness ledger records its outcome.
