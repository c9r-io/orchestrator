---
lifecycle: active
related_fr: FR-135
self_referential_safe: true
---

# Orchestrator - bash 3.2 Compatibility And Coverage Main Path Recovery

**Module**: CI / Governance
**Scope**: bash 3.2 hazard elimination and its gate, the coverage-governance shell main path, diagnostic fidelity of the artifact upload step, and the observed recovery of the `boundary-coverage` job
**Scenarios**: 5
**Priority**: High

## Background

`scripts/coverage-governance.sh` expanded an empty array under `set -u`, which
bash 3.2 rejects. The `boundary-coverage` job runs on `macos-latest`, where
`/bin/bash` is 3.2.57 and the runner image ships nothing newer, so the job died
on its first real command on every run it ever had. The `##[error]` in the
summary named the artifact upload step instead of the generation step, and the
sibling `coverage-policy-fixtures` job — green throughout — covers a path that
`exec`s away on line 16 and shares no line with the failing one.

FR-135 rewrote every such expansion, removed the other bash-4-only constructs,
added `scripts/qa/bash32-compat.rb` with an executed fixture corpus, added a
stubbed main-path smoke, and changed `if-no-files-found` to `warn`. See DD-146.

All scenarios below are read-only against the working tree or operate inside
`$TMPDIR`. None starts a daemon, touches the runtime database, or invokes a
provider.

Primary entry points:

```bash
./scripts/qa/test-bash32-compat.sh                     # 9 cases, 23 assertions on macOS
./scripts/qa/test-coverage-governance-mainpath.sh      # 6 cases, 10 assertions
ruby scripts/qa/bash32-compat.rb                       # the static scan alone
ruby scripts/qa/bash32-compat.rb --list-files          # the scanned set
```

---

## Scenario 1: The Defect Reproduces Before The Fix And Not After

This is the acceptance criterion stated as an experiment. It needs a real bash
3.2, which macOS provides as `/bin/bash`.

### Preconditions

- macOS host, or any host where `/bin/bash --version` reports 3.2.
- Clean worktree.

### Steps

1. `/bin/bash --version | head -1` — confirm 3.2.
2. `/bin/bash -c 'set -euo pipefail; a=(); printf "%s\n" "${a[@]}"'`
3. `/bin/bash -c 'set -euo pipefail; a=(); printf "%s\n" ${a[@]+"${a[@]}"}'`
4. Restore the defect in a scratch copy — put `"${branch_args[@]}"` back on line
   39 of `scripts/coverage-governance.sh` — and run
   `./scripts/qa/test-coverage-governance-mainpath.sh`.
5. Revert and run it again.

### Expected Result

- Step 2 exits 1 with `a[@]: unbound variable`. Step 3 exits 0.
- Step 4 fails, naming `the main path did not complete under /bin/bash`. This is
  mutation M15 in DD-146's table, and it is the direct evidence that the
  wrapper observes the defect rather than describing it.
- Step 5 passes, 10 of 10.
- On a bash 4+ host steps 2 and 4 both succeed, which is why this scenario
  states its precondition. `BASH_COMPAT=3.2` does not change that — measured
  against bash 5.3 for every class in DD-146.

---

## Scenario 2: No Tracked Shell File Carries A bash 3.2 Hazard

### Preconditions

- Clean worktree, repository root is the working directory.

### Steps

1. `ruby scripts/qa/bash32-compat.rb`
2. `ruby scripts/qa/bash32-compat.rb --list-files | wc -l`
3. `git ls-files '*.sh' | wc -l`
4. Confirm no file anywhere enumerates the scanned set: read `shell_files` and
   confirm it calls `git ls-files`.
5. `git ls-files '*.sh' | while read -r f; do /bin/bash -n "$f" || echo "$f"; done`

### Expected Result

- Step 1 exits 0 with `bash 3.2 compatibility: PASS (95 shell file(s) scanned,
  0 finding(s))`.
- Steps 2 and 3 agree. The scanned set includes `.claude/skills/**`, which is
  where three of the five bash-4-only constructs were found.
- Step 4 confirms coverage is walked. A roster would guard only what existed
  when it was written.
- Step 5 prints nothing: every tracked shell file also parses under bash 3.2.

---

## Scenario 3: Each Rejection The Gate Claims Is Real, And Each Rule Is Too

### Preconditions

- `ruby` and `git` available. Every fixture lands under `$TMPDIR`.

### Steps

1. `bash scripts/qa/test-bash32-compat.sh > /tmp/bash32.log 2>&1; echo $?`
2. Read the log and confirm all nine cases are named and passing.
3. Confirm case 5 reports `hazardous form fails and the prescribed replacement
   works` for all seven classes, and that the run reports `0 skipped`.
4. Confirm each case is isolated by a targeted defect, per DD-146's mutation
   table.

### Expected Result

- Exit 0 with `FR-135 bash 3.2 compatibility: 23 passed, 0 failed, 0 skipped` on
  a macOS host.
- On Linux the same command reports `15 passed, 0 failed, 8 skipped` and prints
  a warning naming the interpreter version. The skips are the executed half;
  they are reported, never counted as passes.
- Case 2 is the coverage assertion — the fixture lands in a directory that did
  not exist when the gate was written, and deliberately uses a class case 3 does
  not, so one mutation cannot fail both.
- Case 5 is isolated by no mutation and cannot be: it observes bash rather than
  the gate. It is what makes the static rules mean anything.
- Case 7 asserts here-document bodies are not scanned. That is load-bearing —
  the wrapper writes every fixture as a here-document, because the gate scans
  the wrapper.
- Case 8's fixture must be rejected by `bash -n`; the case asserts that first,
  because its earlier fixture parsed cleanly and the case was passing on a
  premise that was not true.

---

## Scenario 4: The Shell Main Path Is Exercised, And Its Failure Is Not Masked

### Preconditions

- `ruby` available. `cargo`, `node`, `npm`, `npx`, `rustc` and `rg` are shadowed
  by stubs inside the test; nothing real is compiled or fetched.

### Steps

1. `bash scripts/qa/test-coverage-governance-mainpath.sh > /tmp/mainpath.log 2>&1; echo $?`
2. Confirm the reported interpreter and whether 3.2 semantics were in force.
3. Confirm the collection argv is asserted exactly, not by substring.
4. Confirm case 5 asserts `--fixture-test` never reaches the cargo path.
5. Read the `boundary-coverage` job's upload step in `.github/workflows/ci.yml`
   and confirm `if: always()` is retained while `if-no-files-found` is `warn`.
6. Set it back to `error`, re-run the wrapper, then revert.

### Expected Result

- Exit 0 with `FR-135 coverage governance main path: 10 passed, 0 failed`, on
  both macOS and Linux.
- Step 3: the assertion is string equality against
  `llvm-cov --workspace --all-targets --all-features --json --output-path
  <out>/rust.json`. A substring check would also accept a stray empty word or an
  unwanted `--branch`.
- Case 3 runs the nightly branch so a *non-empty* `branch_args` is exercised
  too: a rewrite that dropped the array entirely would pass case 1 and fail
  there.
- Step 4 confirms the FR's premise rather than assuming it. If the `exec` on
  line 16 ever stopped short-circuiting, the fixtures job would begin covering
  the main path and this gate's reason for existing would change.
- Step 5 holds. `always()` is kept deliberately: artifacts are most worth
  reading when the *comparison* fails, and that is a successful generation.
- Step 6 fails case 6, naming the job and step, and reverting restores a passing
  run. That case parses the workflow — the two settings are separate keys and
  grepping for either says nothing about the pair, which is what causes the
  masking.

---

## Scenario 5: The Job Actually Recovered On A Real Runner

The defect only appears on a macOS runner and had been masked by the upload step
for 77 commits. A local pass is not evidence that it is fixed.

### Preconditions

- The fix pushed to `main`.

### Steps

1. `gh run list --limit 5` and take the CI run for the pushed head.
2. `gh run view <id> --json jobs` and read the `Boundary coverage
   non-regression` conclusion.
3. Read the job log and confirm it passes line 38 — `[coverage] collecting
   instrumented Rust tests` is followed by real cargo output, not by
   `branch_args[@]: unbound variable`.
4. Confirm the coverage comparison ran.
5. Read the macOS leg of `Coverage policy fixtures and shell compatibility` and
   confirm `legacy interpreter: /bin/bash (3.2.57(1)-release)` and `0 skipped`.
6. Refresh `config/governance/ci-job-liveness.json` from that run and confirm
   the `knownFailing` entry for `boundary-coverage` is gone.

### Expected Result

- Step 3 shows the script proceeding past the expansion.
- Step 5 confirms the GitHub macOS image really does ship bash 3.2 as
  `/bin/bash`, and that the executed half of the compatibility gate ran there
  rather than being skipped. The ubuntu leg of the same job reports `5.2.21` and
  8 skips, which is the honest reporting this depends on.
- Step 6 leaves the ledger with no `knownFailing` entry for this job.
- If the comparison itself fails on its merits after the job is recovered, that
  is a separate issue: FR-135 does not adjust `coverage/boundary-baseline.json`.

### Observed

Run `30182612498` (`d1f878be`) — the bash 3.2 fix alone. `Boundary coverage
non-regression` ran two and a half minutes into `cargo llvm-cov` and failed on
`tauri::generate_context!`, which reads `frontendDist: ../../gui/dist` at
compile time; nothing in the job built the bundle. The upload step reported
`##[warning]No files were found`, not `##[error]` — requirement 4 verified on a
real runner. The macOS fixtures leg reported `legacy interpreter: /bin/bash
(3.2.57(1)-release)` and `23 passed, 0 failed, 0 skipped`; the ubuntu leg
reported `5.2.21` with `15 passed, 0 failed, 8 skipped` and its warning.

Run `30182768742` (`c9ada747`) — with the frontend build added. `Boundary
coverage non-regression`: **success**, the first in the job's existence. The log
reaches `coverage summary written to target/coverage-governance/summary.json`,
then `coverage governance passed`, and uploads a 3.9 MB artifact
(`boundary-coverage-macOS-ARM64`, id `8626122997`). The non-regression
comparison ran against the approved baseline and passed, so the separate-issue
clause did not arise.

---

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | The defect reproduces before the fix and not after | ☑ PASS | 2026-07-26 | Claude |
| 2 | No tracked shell file carries a bash 3.2 hazard | ☑ PASS | 2026-07-26 | Claude |
| 3 | Each rejection the gate claims is real, and each rule is too | ☑ PASS | 2026-07-26 | Claude |
| 4 | The shell main path is exercised, and its failure is not masked | ☑ PASS | 2026-07-26 | Claude |
| 5 | The job actually recovered on a real runner | ☑ PASS | 2026-07-26 | Claude |

## Certification Conditions

A run of these scenarios counts as closure evidence only when `git status
--porcelain` is empty at start and at end, `git rev-parse HEAD` matches across
the run, nothing else is writing to the repository, each script is invoked as
`bash <script> > log 2>&1` with `$?` captured directly rather than through a
pager, and each log ends with its own summary line.

Scenario 1 and the executed half of scenario 3 additionally require a host where
`/bin/bash` is 3.2. On any other host they are not evidence, and the wrapper
says so rather than passing.
