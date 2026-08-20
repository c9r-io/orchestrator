---
lifecycle: active
related_fr: FR-174
---

# DD-192: Tiering Meta-Verification By When Its Answer Can Change

**Status**: In Progress (requirement 2 outstanding — see *What is not done*)

## The problem

Twelve jobs in `ci.yml`, one `needs` edge between them, so feedback latency is
the longest job and not the sum. Measured at run `32099510921` (head
`e6081c6d`):

| job | seconds | |
|---|---|---|
| **governance** | **1335** | governance |
| ci-environment-parity | 577 | governance |
| boundary-coverage (+18 prerequisite) | 408 | governance |
| test | 324 | product |
| miri | 127 | product |
| coordination-strangler | 121 | governance |
| clippy | 114 | product |
| cross-compile | 88 | product |
| the remaining four | 44 | mixed |

A two-line change waits 22 minutes, and 1335 of the critical path's seconds are
one job. Within it, **19 of the 54 steps are meta-verification** — the
`* negative fixtures` steps that prove each gate can still block what it claims
to block — and they cost **561s, 42% of the job**.

Those 19 answer a different question from the rest. "Did this PR break the
product" changes with every commit. "Can this gate still block what it claims
to" changes only when a gate changes. Running the second question's checks
against a PR that touches no gate spends nine minutes verifying a property that
PR cannot affect.

Nothing here says those steps are less important. DD-181's bidirectional ratchet
and §4.4 shape 1 both exist because a permanently-green gate is worse than no
gate. The claim is narrower: **their schedule is wrong, not their value.**

## The rule, and why it is derived

A PR runs the full set when its changeset touches `scripts/qa/`, `scripts/lib/`,
`config/governance/` or `.github/workflows/`. Otherwise the 19 steps are skipped
and `nightly-governance.yml` runs them instead.

Derived from the changeset, never curated. The alternative — rank the gates by
importance and drop the unimportant ones — degrades into a subjective argument
per gate, and its output is a list, which is §4.4 shape 2: it guards exactly what
someone remembered on the day they wrote it. "Can this change affect this
property" is mechanical and has no list.

`.github/workflows/` is not in FR-174's three roots. It is here because a PR
editing a workflow is editing the tiering mechanism, and a mechanism that can
exempt its own edits from verification is the one shape this must not have.

Measured payoff: **9 of the last 39 merges to `main` touched a tiered root —
23%.** So 77% of PRs take the fast path, and the expected critical path is
0.77·774 + 0.23·1335 ≈ **903s**.

## The part that could have gone wrong silently

`ci.yml`'s aggregation step accepted any outcome that was `success` **or
`skipped`**. That tolerance cost nothing while no gate in the job was
conditional — a skip could only mean a cancelled job.

Nineteen conditional gates turn it into §4.4 shape 5. A tier predicate wrongly
returning `deferred` skips all nineteen, the aggregator prints nineteen
untroubled lines, the job is green, and no meta-verification has run anywhere.
The waiting would be gone and so would the checking, and nothing in the log would
distinguish that from success. **This is the failure mode of FR-174, and the FR
does not mention it.**

So the aggregator asserts the expected set rather than tolerating what it is
handed, and the rule is two-sided and per gate:

- under `deferred`, every rostered gate must be `skipped` — a **`success` is as
  much a violation as a `failure`**, because the tier is a claim about what
  executed and an unexpected success falsifies it exactly as an unexpected
  failure does;
- under `full`, none of them may be `skipped`;
- a gate outside the roster must always succeed — `skipped` is no longer
  tolerated for it, because a skip now carries a declared meaning and an
  unrostered gate has no licence to one;
- reading nothing fails, and an unrecognised tier fails.

### Why both scripts left the workflow

`scripts/qa/ci-tier.sh` and `scripts/qa/governance-result.sh` are files, not
`run:` blocks, for three reasons that all turn out to be the same reason.

1. The interesting behaviour is what they do when git will not answer and when
   the outcome table is empty. A block inside a workflow can only be checked by
   reading it, which §4.4 calls a proxy. As scripts they are driven by
   `test-ci-tier.sh` against throwaway repositories and synthetic tables, and
   observed.
2. `bash32-compat.rb` and `pipefail-short-circuit.rb` glob `*.sh`. Neither can
   see inside a workflow — a scope that was sufficient only while no workflow
   carried non-trivial shell, which is §4.4 shape 9's scope-predicate corollary.
   Both now scan them.
3. That scanning immediately paid. It found FR-145's defect in `ci-tier.sh`:
   `printf '%s\n' "$changed" | grep -qE …` inside the condition. The reader
   leaves on the first match, the producer takes EPIPE, `pipefail` reports the
   pipeline failed, and a **successful match reads as no match** — inverting the
   verdict to `deferred`. That is the failing-open direction, the one that
   silently drops meta-verification, and it would not have been visible in a
   workflow.

Membership is a `case` over a newline-delimited list rather than `declare -A`:
macOS ships bash 3.2 and the repository enforces it, and being runnable here is
what let all eleven aggregator states be exercised before anything was pushed.

## The critical path is now recorded

`ci-step-cost.json` held per-job and per-step seconds. Feedback latency is
neither, and every reader had to re-derive it from the workflow's `needs` graph.

That is not a hypothetical cost. **FR-174's own acceptance criterion asked for
the post-tiering critical path to be compared against the sum of five parallel
jobs** — a quantity that bounds no critical path — two paragraphs after its
background states the principle correctly. A number an argument rests on belongs
in the file the argument cites.

`ci-cost.rb` now computes it as the longest chain by seconds through the `needs`
graph, records both tiers, and **recomputes and compares on every run**. A stored
latency nobody re-derives is a duration pinned to a graph that has since moved.
The chain is memoised over the DAG rather than special-cased for today's single
edge, because one edge is a fact about the workflow now and not about the shape
of the answer.

```
critical path: 1335s full / 774s deferred (19 tiered steps, 561s)
  longest chain: governance
```

### Measured after merge, and the bound moved as predicted

Second sample, run `32384727129` (head `f1637d4a`), the first CI run on `main`
carrying the tiering:

```
critical path: 1153s full / 820s deferred (19 tiered step(s), 454s)
  full     chain=governance
  deferred chain=ci-environment-parity
```

The numbers below were taken at `32099510921` and are left as they were — they
name their run and their head, and a sample is not a function of the tree
(FR-140). What matters is not that they moved but **which way the chain moved**:
under `deferred`, `governance` falls to 699s and the longest chain is no longer
`governance` at all. It is `ci-environment-parity`.

That is the bound this document predicted, arriving one sample later and without
anyone re-deriving it. `criticalPath` is computed from the `needs` graph on every
run rather than stored, so the handover showed up in the gate's own output the
first time it was true. A recorded 774s would have been a number nobody
recomputed, describing a chain that had already changed.

It also sharpens the next step for whoever takes it: tiering more of
`governance` now buys nothing until `ci-environment-parity` moves, because the
critical path stops being governance's to spend.

### What the remaining gap is made of, since the FR asked

`774s` is still `governance`. The comparison the criterion should have asked for
is against the longest **product** job, `test` at 324s, and the chain from here
is:

| | seconds | what closes it |
|---|---|---|
| today | 1335 | — |
| after this FR | 774 | — |
| next bound | **577** | `ci-environment-parity`, untouched here |
| floor | 324 | `test`, the longest product job |

`ci-environment-parity` is deliberately not tiered. It is a **third category**:
its answer changes with the runner environment, not with the PR and not with the
gates, so FR-174 requirement 1's binary rule does not classify it — the FR's own
未核验 section says so. Deferring it would mean PRs trusting an unverified
environment, which is a different bargain from deferring a gate's self-test, and
it is not made here.

## The budget was off for eight days

Not caused by this FR, found by it, and worth recording because the mechanism is
working correctly the whole time.

DD-153's `pendingMeasurement` disarms the ceiling rather than compare it against
a total knowingly missing steps, and the gate prints `NOT ENFORCED` with the
outstanding steps named. Every prior window in that file's history closed within
0–1 days. **This one ran 2026-08-11 to 2026-08-19 — 8 days, 5 commits, growing
2→3→4→6** — because FR-163 and FR-165 each added steps and neither refreshed.
FR-174 was written on day six of it, and states that the budget 「做了它承诺的事」.

The line is printed by a step whose outcome is `success`, in a job that was green
throughout. A fail-safe that reports honestly and then waits indefinitely is, in
a green log, indistinguishable from one that is enforcing. Nothing bounds how
long it stays disarmed, and that is the residue this FR did not close.

Re-armed at 1912/2700 — and immediately disarmed again by this FR's own two new
steps, which is the mechanism doing exactly what it should until CI measures
them.

DD-153 gains a sixth sample: **14 → 13 → 12 → 11 → 9 → 29%**. The monotonic
decline it recorded across five samples reversed, by more than every prior sample
moved combined, because two gates retired. Its projection that the review
condition was "one FR away" was overtaken in the other direction. Five samples
established a direction and could not establish that the direction was a property
of the system rather than of the period.

## What is not done

**Requirement 2 is outstanding.** `nightly-governance.yml` has no
`ci-job-liveness.json` record, because it has never run. Every record in that
ledger is a real run, and writing a `headSha` for a run that did not happen is
precisely the entry the ledger exists to make impossible.

This is the requirement that decides whether FR-174 removed work or removed
checking, and DD-159 is its precedent: a gate dead since 2026-03-26, exiting
before its first scenario, with nothing looking. Until the nightly has fired once
and the ledger records it, the 19 deferred gates have a declared home and no
evidence of arriving there.

**And it cannot be satisfied before this branch merges.** Measured, not assumed:
`gh workflow run nightly-governance.yml --ref feat/fr174-ci-governance-tiering`
returns `HTTP 404: workflow not found on the default branch`, and the workflow
does not appear in `gh api .../actions/workflows` at all — GitHub registers a
`workflow_dispatch` workflow only from the default branch, so `--ref` cannot
reach one that lives on a feature branch. A bare push to the branch runs nothing
either: `ci.yml` triggers on `pull_request` and on pushes to `main`/`master`, not
on arbitrary branches.

So the order is forced, and it is worth stating because it inverts the usual one:

1. Open the PR. That is what first exercises the tier predicate for real, and on
   this PR it must decide `full` — the changeset touches `scripts/qa/`,
   `config/governance/` and `.github/workflows/`, three of the four roots. A
   `deferred` verdict here would be the mechanism failing its own first case.
2. Merge. Only then does GitHub register the nightly.
3. Dispatch it, or wait for 03:17 UTC.
4. `ci-liveness.rb --refresh --write` and `ci-cost.rb --refresh --write`. The
   second also measures the two steps under `pendingMeasurement` and re-arms the
   ceiling.

The consequence worth naming: **between step 2 and step 4 the deferred gates have
a home that has never run**, and `main` is the first place that is true. The
window is one nightly cycle at most, and `ci-liveness.rb` is red for the whole of
it — which is the correct signal rather than a nuisance, since it is exactly the
claim "these gates run somewhere" going unevidenced.

## A red on this PR that is not this FR's

`core-boundary` and `persistence-api-boundary` are red on PR #131 and green on
`main`, and the cause is not the tiering. Both compare `--emit-baseline` against
their ledger with `cmp`, and `ledger_json` (`scripts/lib/rust_source.rb:299`)
renders an empty object differently across json gem versions — measured, CI is
json 2.21.2 and emits `{}` while the committed ledgers carry the multi-line form.
When that comparison fails, the same gate's Case 7 then **writes the ledger**,
so a read-only gate leaves a modified tracked file behind.

Neither defect is caused by this FR, and one of them this FR made visible: both
gates used to die at the `diff` line under `set -euo pipefail` — the FR-146 shape
— so Cases 3–12 never ran and the `--write` defect had never once been reached.
That truncation is fixed in `f4e93f8c`.

The apparent divergence — main green, this branch red — turned out to be a
measurement error of mine rather than a real difference, and it is worth naming
because it is §4.4 shape 6 again. I claimed both ran the same interpreter on the
strength of apt printing `ruby is already the newest version (1:3.2~ubuntu1)`.
That is the **apt package**; the quantity that decides the rendering is the
**json gem**, which moves with the runner image while the package version does
not. Main's run passed, so its json version had never been printed, and I read
"not measured" as "identical". The two runs were 34 hours apart. Re-running
main's own job on the current image fails identically, byte for byte.

Filed and fixed under
`docs/ticket/core-boundary_ledger-json-version-dependence_260820_223641.md`:
`ledger_json` now normalises all three empty-container renderings, both ledgers
were regenerated, and `test-core-boundary.sh` Case 8b asserts the property
directly — version-independently, which Case 2 cannot, since it compares one
machine's emit against a ledger that machine wrote.

## Accepted costs

- **A 24-hour window on `main`.** A PR that defers, merges, and breaks a gate's
  self-test is caught by the next nightly rather than at review. No merge queue
  exists in this repository — no `merge_group` trigger in any workflow — and
  adopting one is a branch-protection change well outside this FR. FR-174 lists
  nightly-only as an acceptable answer to its requirement 3.
- **Local habit is unmeasured.** If developers stop running meta-verification
  locally because CI no longer does on their PR, defect discovery shifts to the
  nightly. FR-174 flags this and it is still unquantified.
- **Cache behaviour is unmeasured.** `governance` and `test` use different
  `rust-cache` keys and the cold/warm split was never measured; if a meaningful
  share of the 1335s is cold compilation, the saving is smaller than 561s
  suggests. The nightly shares the `governance` key, which is a guess at the
  right trade and not a measurement.

## Known limits

- The roster lives in four places — the `if:` conditions, `ci.yml`'s `META`, the
  nightly's steps, the nightly's `META` — plus `tieredBy` in the gate manifest.
  `test-ci-tier.sh` cases 19–22 derive all five from the files and compare, so
  they cannot drift apart silently, but the duplication is real and a sixth copy
  would need a sixth comparison.
- The tier predicate reads `git diff base...HEAD`. A force-push that rewrites the
  base, or a merge commit that changes what `...` resolves to, changes the
  changeset it sees. Every such failure yields `full`, so the error is toward
  cost rather than toward coverage, but it is not proven exhaustive.
- `coverage-policy-fixtures` (15s) is an entire meta job feeding
  `boundary-coverage` and is not tiered. It is small and it gates a real
  measurement; leaving it is a judgement, not a measurement.
