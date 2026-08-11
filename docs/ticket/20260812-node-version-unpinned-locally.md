# The GUI unit suite fails on Node 26, and nothing local says which Node to use

- **Observed during**: FR-165 R1 triage, 2026-08-12, running the never-run manual gates for the first time
- **Severity**: medium (12 tests fail for every developer on Node ≥26; three manual gates are unrunnable as a result, and CI cannot see any of it)
- **Status**: open

## Symptom

`npm test` in `gui/` fails 12 tests across 2 files, all with the same error:

```
TypeError: Cannot read properties of undefined (reading 'clear')
 ❯ src/hooks/usePreferences.test.tsx:12:18   localStorage.clear();
 ❯ src/pages/source-connections/SourceConnections.test.tsx:47:18
```

Node prints the cause itself, on stderr, above the failures:

```
(node:68757) ExperimentalWarning: localStorage is not available
because --localstorage-file was not provided.
```

Node 26 ships a native `localStorage` global. It shadows the one jsdom installs,
and it is `undefined` unless the process was started with `--localstorage-file`.
The two files that touch `localStorage` are exactly the two that fail; the other
23 test files pass, which is why this reads as a product defect at first glance.

**Not a product bug.** Measured on this machine, same worktree, same commit:

| Node | `npx vitest run src/hooks/usePreferences.test.tsx` |
|---|---|
| v26.7.0 | 3 failed |
| v24.19.0 | 3 passed |

## Why it was invisible

Two independent gaps, and either one alone would have caught it:

1. **`ci.yml` never runs the GUI unit suite.**
   `rg 'test:coverage|npm test|npm run test|vitest' .github/workflows/ci.yml`
   returns nothing — the GUI steps are `npm ci`, `npm run build` and
   `npx playwright install`, and no vitest at all.

   The only things that execute `npm test` are three `manual-runbook` gates,
   none of them enforced anywhere:

   ```text
   scripts/qa/test-process-console-ui.sh
   scripts/qa/test-process-console-metrics.sh
   scripts/qa/test-slack-managed-shared-oauth.sh
   ```

   All three were `lastRun: null` — never executed since the ledger was created.

2. **Nothing local declares the Node version.** All eight `node-version:` keys across the three workflows say `22`. There is no `.nvmrc`, no `engines` field in `gui/package.json`, and no check anywhere. A developer's Node is whatever their machine has; this one had 26.7.0. (Related: the 2026-08 dev-machine rebuild.)

The pairing matters more than either half. CI pins a version it never uses for
this suite, and the suite that would expose the mismatch is only reachable by
gates nobody runs.

## For ticket-fix

1. **Declare the version where a human and a tool both see it.** Add `gui/.nvmrc`
   with `22` and an `engines.node` field to `gui/package.json`. Prefer both:
   `.nvmrc` is what `nvm use` reads, `engines` is what `npm ci` can warn on.
   Decide explicitly whether to set `engine-strict=true` — that turns the
   advisory into a refusal, which is the point, but it will also refuse for
   people currently working fine on 24.

2. **Decide whether `ci.yml` should run the GUI unit suite.** It is the real
   question behind this ticket. Today the frontend's 120 unit tests are enforced
   by nothing on a push, while `coverage/boundary-baseline.json` records a
   frontend line coverage of 89.21% — a number no CI job re-derives. Adding
   `npm test` to the existing GUI job is cheap; note the cost against the
   DD-172 budget either way, and if the answer is no, write down why, because
   the current state reads as an oversight rather than a decision.

3. **A version check is not a substitute for either of the above.** A gate that
   greps `.nvmrc` and compares it to `node -v` asserts that two strings match,
   not that the suite passes — §4.4 shape 1. If a guard is wanted here, the
   guard is running the tests.

4. Note for whoever closes this: the three gates above will keep recording
   `exitStatus: 1` into `config/governance/manual-gate-freshness.json` on any
   Node ≥26 machine, and since FR-165 a recorded failure counts as not-fresh.
   That is working as intended — but it means this ticket blocks those three
   gates from ever going green locally, and `release.yml` now runs
   `manual-gate-freshness.rb --strict`.
