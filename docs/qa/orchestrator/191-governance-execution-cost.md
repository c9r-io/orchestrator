---
lifecycle: active
related_fr: FR-140
self_referential_safe: true
---

# Orchestrator - Governance Execution Cost And Budget

**Module**: CI / Governance
**Scope**: the per-step cost ledger `config/governance/ci-step-cost.json` and its gate `scripts/qa/ci-cost.rb`, the ceiling on the two governance jobs, and the `RustLexer.mask_literals` rewrite that the ceiling was made reachable by
**Scenarios**: 5
**Priority**: Low

## Background

Fourteen FRs governed whether gates are correct; none asked what they cost. The
enforcement surface grew from 45 entries to 65 in three days while the two
governance jobs grew from 45 minutes to 80, and no artifact recorded which gate
the minutes belonged to.

FR-140 attributed the cost to per-case tree copying. Measured, that is 3.2% of
the gate it named; the cost was `RustLexer.mask_literals` walking 6.4 MB of Rust
one `String#[]` at a time, re-run once per fixture case by four gates.

Design record: `docs/design_doc/orchestrator/153-governance-execution-cost.md`.
Sibling: `docs/qa/orchestrator/186-ci-job-liveness.md` covers the ledger this one
is modelled on.

**Safety**: read-only against the working tree. Scenarios 1–4 build scratch git
repositories under `$TMPDIR`; no daemon is started, no database is touched, no
provider is invoked, and nothing contacts GitHub — only `--refresh`, which no
scenario calls, talks to the API. Safe to run against this repository.

## Why the assertions are shaped the way they are

A cost ledger that only checks coverage — every step has a number — passes on a
pipeline of **any** length. It would have reported PASS on all 80 minutes that
prompted this FR. So scenario 3 is not optional decoration: it is the only
assertion that observes the budget doing arithmetic on recorded seconds rather
than merely existing.

The same reasoning applies to the lexer. "The suites that use it still pass" is
satisfied by a lexer that masks slightly differently in a region no current
fixture inspects. Scenario 5 therefore asserts the masking directly.

Each fixture also asserts that no other check fired on the same tree (the FR-127
isolation convention), and the mutation each one applies is named, because a
fixture that also passes on the broken implementation is not evidence.

---

## Scenario 1: A gate whose cost nobody recorded is a failure

**Steps**

```bash
bash scripts/qa/test-ci-cost.sh
```

Read the positive control and cases 1 and 2.

**Expected result**

- Control: a complete, in-budget ledger passes. Without it every case below
  could be passing because the gate is broken rather than because the defect is
  detected.
- Case 1: a `ci-required` gate added to the workflow with no cost record fails,
  and the diagnostic names **both** the step and the gate it runs.
- Case 2: a record naming a step the job no longer defines fails.

**Mutation targeted**: an **added** step, not a deleted record. Deletion is the
case the author has in mind and a hand-maintained list survives it; a new gate
landing outside the list is how this decays in practice, and it is the exact
shape FR-140 was filed about — the surface grew by twenty entries while nothing
was obliged to notice. Case 2 is the same rule in the other direction, so a
renamed step cannot leave a number attributed to nothing.

---

## Scenario 2: The ceiling is a decision, and the measurement is from this history

**Steps**

Read cases 3, 4 and 5 of the same script.

**Expected result**

- Case 3: a budget with no written `reason` fails.
- Case 4: a measurement whose `headSha` is not an ancestor of `HEAD` fails.
- Case 5: a `pendingMeasurement` annotation left on a step that **has** been
  measured fails.

**Mutation targeted**: FR-140 requires the ceiling to be a decision and forbids
deriving it from the current value. A number with no rationale and no review
condition is indistinguishable from whatever the cost happened to be the day
someone wrote it down, so the absence of a reason is a failure and not a
warning. Case 5 is the counterpart to `knownFailing` outliving its failure:
without it the budget stays unenforced forever behind a note nobody revisited.

---

## Scenario 3: The budget is evaluated against real recorded seconds

**Steps**

Read cases 6 and 6b.

**Expected result**

The recorded total is 400 s. With the ceiling lowered to 399 s the gate fails
with `governance costs 400s against a 399s budget, over by 1s`, and the
diagnostic breaks the overage down per job and per step.

**Mutation targeted**: the **ceiling is lowered**, not the durations raised —
FR-140 asks specifically for proof that the limit is evaluated against real
recorded time rather than spinning.

*What a coverage-only ledger would still pass on*: every other scenario in this
document, on a pipeline of any duration whatsoever. That is why this one exists,
and why 6b requires the failure to say where the time went — "over budget" with
no breakdown leaves the reader to redo the attribution by hand, which is the
state this FR started from.

---

## Scenario 4: `--write` is refused when no human is present

**Steps**

Read case 7.

**Expected result**

`CI=1 ruby scripts/qa/ci-cost.rb --refresh --write` exits non-zero **and** prints
`refusing --write under CI`.

**Mutation targeted**: the exit code alone is a proxy — exit 2 is also what a
crashed interpreter produces. Asserting the diagnostic is what distinguishes a
refusal from a failure. This matches the refusal in the five other governance
writers, all routed through `CiEnv.refuse_unattended_write!`.

---

## Scenario 5: The lexer rewrite did not change what the lexer means

**Steps**

Read case 8, then reproduce the corpus differential:

```bash
ruby scripts/qa/bash32-compat.rb            # unaffected control, ~0.4s
bash scripts/qa/test-persistence-dependency.sh
bash scripts/qa/test-core-boundary.sh
```

**Expected result**

Case 8 asserts 15 known-answer masking cases: a brace inside a literal, nested
block comments, raw strings at hash depths 0/1/2, byte strings, a lifetime that
is not a char literal, literals spanning a newline, and literals unterminated at
end of file. The suites pass with **unchanged assertion counts** and materially
lower runtimes.

**Why known answers rather than a captured baseline**: a baseline captured from
the implementation asserts only that the implementation has not changed —
including from a state that was already wrong. Each expectation is written as
`"code" + " " * n` with the masked construct's length named in a comment, so the
count is the assertion and a reviewer can check it. Seven of the fifteen
disagreed with the first hand-written draft; in every case the implementation
was right and the count was wrong, which is what a real assertion does.

**Why not a differential against the previous implementation**: that comparison
was run during governance and is recorded below, but leaving it in the gate
would forbid every deliberate future change to `mask_literals` rather than
catching accidental ones.

---

## Recorded measurement

Taken during governance at `cc9631d9`, on macOS with system Ruby 2.6.

**Equivalence of the rewritten lexer** — three independent checks:

| check | result |
|---|---|
| all 415 tracked Rust files, SHA-256 of masked output, old vs new | **415/415 identical** |
| 26 hand-written adversarial constructs | **26/26 identical** |
| 7000 random inputs (2000 slices of real sources, 5000 strings over the driving alphabet) | **0 differences** |

**Cost attribution**, from real CI runs `30275254232` (before) and
`30288601535` (after):

| job | before | after |
|---|---|---|
| `governance` | 3286 s (8 unattributed) | **1938 s** (5 unattributed) |
| `ci-environment-parity` | 1512 s (5 unattributed) | **379 s** (5 unattributed) |
| combined | 4798 s | **2317 s** against the 2700 s ceiling, 14% headroom |

Per step, which is the discriminator that separates a fixed defect from a faster
runner — every step that moved beyond noise is a `mask_literals` consumer, and
every step that is not one did not move:

| step | before | after | change |
|---|---|---|---|
| Persistence API capability boundary | 292 s | 35 s | **−88%** |
| Persistence dependency chokepoint | 409 s | 55 s | **−87%** |
| Governance ledger regeneration tooling | 261 s | 39 s | **−85%** |
| Core crate boundary and schema snapshot | 610 s | 126 s | **−79%** |
| Agent driver execution migration contracts | 278 s | 271 s | −3% |
| Persistence crate extraction contracts | 202 s | 197 s | −2% |
| Filesystem trigger contracts | 337 s | 356 s | +6% |
| Verify gate enforcement surface negative fixtures | 484 s | 525 s | +8% |
| Agent driver production parity | 56 s | 62 s | +11% |

Both new gates passed on their first CI execution: `cost=success`,
`cost-fixtures=success` in run `30288601535`.

**The correction that reshaped the FR** — where the time was, and was not:

| what | cost |
|---|---|
| all 22 `new_case` copies in `test-persistence-dependency.sh` | 6.2 s |
| one invocation of `persistence-dependency.rb` | 13.2 s |
| `RustLexer.mask_literals` over 415 files / 6.4 MB | 37.4 s |
| the same corpus after the rewrite | **1.5 s** |

**Per suite, assertion counts unchanged** — FR-140's own criterion that isolation
must not regress:

| suite | assertions | before | after |
|---|---|---|---|
| `test-persistence-dependency.sh` | 22 | 195 s | 29 s |
| `test-core-boundary.sh` | 14 | 360 s | 57 s |
| `test-qa-gate-surface.sh` | 13 | 14 s | 11 s |
| `test-doc-lifecycle.sh` | 12 | 5 s | 5 s |

The last two rows are the control: neither is a `mask_literals` consumer and
neither moved. A change that made everything faster would be a changed
measurement rather than a fixed defect.

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | A gate whose cost nobody recorded is a failure | ☑ PASS | 2026-07-28 | Claude |
| 2 | The ceiling is a decision, and the measurement is from this history | ☑ PASS | 2026-07-28 | Claude |
| 3 | The budget is evaluated against real recorded seconds | ☑ PASS | 2026-07-28 | Claude |
| 4 | `--write` is refused when no human is present | ☑ PASS | 2026-07-28 | Claude |
| 5 | The lexer rewrite did not change what the lexer means | ☑ PASS | 2026-07-28 | Claude |

## Certification Conditions

A run counts as closure evidence only when `git status --porcelain` is empty at
start and at end, `git rev-parse HEAD` matches across the run, nothing else is
writing to the repository while it runs, and each script's final summary line is
present in its log. Invoke as `bash script > log 2>&1` and read `$?` directly;
piping into a pager reports the pager's status and masks a failed script.

Timings recorded here are from one host and are evidence of a ratio, not a
promise about any other machine. The numbers the budget is enforced against come
from `gh run`, never from a local measurement.

## Related gates

- `scripts/qa/ci-liveness.rb` — the ledger this one is modelled on; records each
  job's conclusion where this records its duration.
- `scripts/qa/test-qa-gate-surface.sh` — asserts this script is registered in
  `config/governance/qa-gate-surface.json` and wired into a CI job, and that a
  gate reading git history runs in a job checked out with `fetch-depth: 0`.
