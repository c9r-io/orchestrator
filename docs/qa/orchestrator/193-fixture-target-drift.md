---
lifecycle: active
related_fr: FR-143
---

# Orchestrator - A Negative Fixture Must Prove It Applied A Mutation

**Module**: CI / Governance
**Scope**: `scripts/lib/gate_fixture.sh`, the scanner
`scripts/qa/fixture-target-drift.rb`, and the conversion of 48 premise and
mutation sites across the ten ci-required shell gates that build fixture trees
**Scenarios**: 5
**Priority**: Medium

## Background

FR-129 and FR-134 each built two meta assertions over a gate's check registry:
every check is registered, and every registered check has a negative fixture.
Neither asks whether that fixture applied a mutation.

Nine times a fixture's target moved out from under it. Eight of the nine stayed
green — the fixture aborted the run, or passed vacuously, or mutated a file the
gate was seeing for the first time and reported through the wrong branch. The
worst of them announced that the gate under test had failed to notice a removal
nobody had made.

Design record: `docs/design_doc/orchestrator/155-fixture-target-drift.md`.
Sibling: `docs/qa/orchestrator/192-jq-status-observed.md` is the same shape one
layer down — a gate that reports PASS having read nothing, where this is a
fixture that reports having changed nothing.

**Safety**: read-only against the working tree. Every case builds a scratch tree
under `$TMPDIR`; no daemon starts, no database is touched, no provider is
invoked, and nothing contacts the network. Safe to run against this repository.

## Why the assertions are shaped the way they are

**Every rule gets a fixture, and every fixture gets its opposite.** A rule nobody
has tried to trip is a rule nobody knows can fire — FR-144 found two of its five
rules had no fixture by listing rules against cases. And a rule that fires on
correct code gets switched off long before it catches anything, so each "must
fire" case is paired with a "must not fire" one on the same probe.

**Two of the five rules have no violation in the repository today.**
`exit-code-only` and `restated-expectation` are regression guards: FR-141 cleared
the last restated ledger value, and no gate reports a pass on a bare exit code.
That makes their fixtures the *only* evidence they work, which is why both are
asserted on synthetic probes rather than assumed from a clean scan.

**The library is driven through a child shell.** Half of what is under test is
what happens to the *run*: an abort takes the summary line with it, and a run
that stopped early is indistinguishable from one that finished. That is only
observable from outside the process.

---

## Scenario 1: A stale premise costs one assertion, not the run

**Steps**

```bash
bash scripts/qa/test-fixture-target-drift.sh
```

Read the first two cases.

**Expected result**

- A `fixture_premise` whose command aborts fails **that case**, quotes the
  premise's own words in the diagnostic, lets the next case run, and reaches the
  summary line `harness: 1 passed, 1 failed`.
- A premise that still holds costs nothing.

**Mutation targeted**: the exit code alone is a proxy — a crashed interpreter
produces one too. The assertion requires the abort's own message to reach the
reader, which is what distinguishes "this fixture's anchor moved" from "something
went wrong".

*What the first case would still pass on*: a library that fails every premise.
The second case is what rules that out, and it is not decoration — nineteen
premises in this repository hold today and must keep costing nothing.

---

## Scenario 2: The recorded incident, replayed

**Steps**

Read case 3 of the same script. Then reproduce the original directly:

```bash
git log -1 --format=%B 75dcf68c   # the commit that first repaired it
```

**Expected result**

A `fixture_mutate` whose substitution matches nothing fails, names the file, says
`the fixture proves nothing`, and — asserted explicitly — **the case's own
accusation against the gate never prints**.

**Mutation targeted**: an in-place substitution whose pattern no longer matches,
not a deleted file. Deletion is the case the author has in mind and it already
fails loudly. A substitution matching nothing is the one that reports success,
and it is the one that happened: `core/src/db.rs` became a re-export shell whose
only rusqlite token sat inside `mod tests`, where the scanner does not count it.

This is the only case that reproduces the property that makes the defect worse
than a gap — the fixture did not go quiet, it **reported a defect that was not
there** and sent the auditor to read an innocent gate.

*Three live instances of this shape were converted*: `test-core-boundary.sh`
cases 6 and 11 and `test-governance-ledger-tooling.sh`'s scope-fidelity case all
assert that a baseline does **not** move, so an inert mutation passed them in
silence.

---

## Scenario 3: The other preconditions, both directions

**Steps**

Read cases 4 to 6.

**Expected result**

- A mutation that lands is **not** reported, so the check is about the change and
  not about the edit.
- A target that is a directory fails before the mutation runs. This is what an
  emptied ledger read produces: `core-boundary` case 5 took its target from
  `rusqlite.files.keys.min` after FR-141 B4 took core to zero, and wrote to a
  directory.
- A producer that leaves an empty file fails, and a non-empty one passes. Zero
  bytes and a correct derivation are the same exit code — the FR-144 lesson one
  layer over.

---

## Scenario 4: The scanner parses, and is not a grep

**Steps**

Read cases 7 to 15, including 7b.

**Expected result**

- Case 7: a correctly written gate produces no findings.
- Case 7b: a manifest that yields **no** gates is a failure, not a clean run.
  This was found by the closure self-check: the positive control used to be an
  empty surface, which made it pass on a scanner that examined nothing — a
  control that cannot tell "no findings" from "nothing looked at" is not a
  control, and §4.4 shape 5 is the FR immediately before this one.
- Case 8: an unwrapped in-place rewrite is a finding at its own line, and a
  wrapped one on the next line is not.
- Case 9: `(cd "$DIR" && ruby "$GATE")` and `COUNT=$(ruby -rjson -e ... )` are
  **not** mutations. Without the `-e` requirement the rule reported 96 findings
  on this repository where there are 43.
- Case 10: of four occurrences of the forbidden word — a shell comment, a
  here-document body, a Ruby comment inside the block, and the real premise —
  **exactly one** is a finding, anchored where the wrapper goes and naming the
  abort's line.
- Case 11: the same block wrapped is not a finding, because the abort is then the
  diagnosis rather than the defect.
- Case 12: an exit code alone is a finding; a diagnostic match is not, and
  neither is a recorded before-run.
- Case 13: a literal `sql 8 -> 7` is a finding; `sql $N -> $((N - 1))` is not.
- Case 14: a file ending inside a here-document is reported, and one that
  closes — in the control probe from case 7 — is not.
- Case 15: a scratch tree named nothing like the others is still followed.

**All five rules the scanner defines are proven by a case here**, and each is
paired with its opposite.

**Mutation targeted**: cases 9, 10 and 11 are what separate a parse from a
`grep`, and they are not hypothetical — DD-155, this document and the fixture
script all quote the forbidden shapes by necessity. A grep-based scanner passes
case 8 and fails case 10, and the natural way to silence it is to stop writing
the rule down.

Case 15 is the one that caught a real defect: it is the finding the measurement
prototype for this FR **missed**, because that prototype used a hand-listed
roster of scratch-variable names (`DIR|d|BASE|PROBE`) and
`test-coordination-strangler.sh` calls its scratch tree `QA_ROOT`. Deriving the
roots from the assignments found it. §4.4 shape 2, inside the tool built to
measure §4.4 shape 2.

---

## Scenario 5: Coverage follows the manifest

**Steps**

Read cases 16 and 17.

**Expected result**

Registering a new `ci-required` shell gate grows the scanned set by exactly one,
with no edit to the scanner; and the scanner passes on this repository.

**Mutation targeted**: §4.4 shape 2 — a hand-listed scope guards exactly what was
known the day it was written. The tell is a list that grows by one entry per
audit round.

---

## Recorded measurement

Taken during governance at `5062f3e5`, macOS, system Ruby 2.6. **Certified at
`d17e27c4`** on a clean tree, pinned across the run, 29 of 30 green including
`cargo test --workspace` and `cargo clippy -D warnings`; every gate's final
summary line present in its log.

The one non-zero is `ci-liveness.rb`, and it is the known first pass of DD-146's
two-pass convergence rather than a defect: committing a `ci.yml` change stales
all fourteen job records at once, and the liveness step runs inside the job it
fails. It converges when a run at the new SHA lands and the ledger is refreshed.
Recorded here rather than omitted, because a certification that quietly drops
its one red line is the shape this whole FR is about.

**Scope, measured rather than counted from the FR:**

| | FR-143 as filed | measured |
|---|---|---|
| gates carrying the defect | 3 named | **10** |
| uncaught premises | not counted | **21 lines / 15 blocks** |
| unproven mutations | not counted | **28** |
| exit-code-only assertions | implied outstanding | **0** — a guard, not a repair |
| restated `N -> M` expectations | implied outstanding | **0** — a guard, not a repair |
| scanner findings before / after | — | **43 → 0** |

**Assertion counts, every affected gate** — the FR's own criterion that nothing
regresses. Baselines measured at `0fb2c5ef`, not quoted from an earlier commit
message: `75dcf68c` recorded core-boundary 14, persistence-dependency 20 and
persistence-extraction 9, and two of those three had moved.

| suite | before | after |
|---|---|---|
| `test-agent-driver-production-parity.sh` | 11 | 11 |
| `test-ci-cost.sh` | 10 | 10 |
| `test-coordination-strangler.sh` | 20 | 20 |
| `test-core-boundary.sh` | 14 | 14 |
| `test-doc-lifecycle.sh` | 12 | 12 |
| `test-governance-ledger-tooling.sh` | 8 | 8 |
| `test-jq-status-observed.sh` | 18 | 18 |
| `test-persistence-dependency.sh` | 22 | 22 |
| `test-persistence-extraction.sh` | 11 | 11 |
| `test-qa-gate-surface.sh` | 13 | 13 |
| `test-qa-gate-surface.sh --fixture-test` | 34 | 34 |

No count fell.

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | A stale premise costs one assertion, not the run | ☑ PASS | 2026-07-28 | Claude |
| 2 | The recorded incident, replayed | ☑ PASS | 2026-07-28 | Claude |
| 3 | The other preconditions, both directions | ☑ PASS | 2026-07-28 | Claude |
| 4 | The scanner parses, and is not a grep | ☑ PASS | 2026-07-28 | Claude |
| 5 | Coverage follows the manifest | ☑ PASS | 2026-07-28 | Claude |

## Certification Conditions

A run counts as closure evidence only when `git status --porcelain` is empty at
start and at end, `git rev-parse HEAD` matches across the run, nothing else is
writing to the repository while it runs, and each script's final summary line is
present in its log. Invoke as `bash script > log 2>&1` and read `$?` directly;
piping into a pager reports the pager's status and masks a failed script.

`test-persistence-extraction.sh` refuses to run on a dirty worktree by design,
because three of its cases build fixtures with `git archive HEAD`. That is also
why its conversion could only be verified in the run *after* its commit, not
before it.

## Related gates

- `scripts/qa/test-qa-gate-surface.sh` — asserts the new scripts are registered
  and wired into a CI job. It is also one of the ten converted gates: it is where
  `inject()` was written, which the shared library generalises.
- `scripts/qa/bash32-compat.rb` — `gate_fixture.sh` must stay bash 3.2 clean; the
  scanned set is `git ls-files '*.sh'`, which includes `scripts/lib`.
- `scripts/qa/jq-status-observed.rb` — the sibling rule from FR-144, and the
  precedent for deriving a scanner's scope from `qa-gate-surface.json`.
- `scripts/qa/ci-cost.rb` — carries the two new steps as `pendingMeasurement`
  until CI measures them.
