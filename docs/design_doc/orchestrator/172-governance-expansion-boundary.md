---
lifecycle: active
related_fr: FR-158
---

# DD-172: The governance expansion boundary

**Status**: Released

## Why this exists

Between FR-127 and FR-149 this repository produced 23 feature requests in six
days, and most of them were governance work: gates, ledgers, ratchets, and gates
guarding gates. Every one was individually defensible. Nothing ever asked what
the whole thing cost, or what would have to be true for the answer to be "enough".

FR-158 was filed to close three structural weaknesses in that machinery and then
to write down the boundary. The boundary is the point. The three repairs are in
[DD-173](173-ratchet-masking-and-surface-closure.md); this document is the rule
that decides whether the next one gets built.

## The three rules

### 1. The expansion budget already exists, and it is the job-time ceiling

`config/governance/ci-step-cost.json` carries a 2700-second ceiling on the
`governance` and `ci-environment-parity` jobs combined, decided in
[DD-153](153-governance-execution-cost.md) with a written reason, and enforced by
`ci-cost.rb`. **That is the expansion budget.** It did not need to be invented;
it needed to be named, because it was being read as a cost-control measure
rather than as the thing that says how much governance this repository is
willing to run.

Measured at FR-158's close (run `30792774882`, `c1fd4dd5`, ledger
`ci-step-cost.json` refreshed from that run):

| | seconds |
|---|---|
| `governance` | 1248 |
| `ci-environment-parity` | 545 |
| **budgeted total** | **1793 / 2700** |
| all twelve ci.yml jobs | 3113 |

FR-158's own step, `Manual-runbook gate freshness`, measures **0s** — it reads two
JSON files and prints 35 lines, and disappears into the per-step overhead. That
was the intent: a rule about the cost of governance should not arrive with a
bill.

FR-158's own filing claimed the 2700s figure had been breached, reading the
all-job sum of 2709s against a ceiling that covers two jobs. It had not: the pair
was at 1793s with 907s of headroom, and the number to compare is the one the
ledger's own `budget.jobs` field names. The correction matters beyond
arithmetic — the FR proposed a *new* cap on the strength of a breach that had not
happened, and the honest finding is that the existing ceiling binds and is not
close.

`budget.reviewWhen` already states the rule: a new gate that does not fit is not
grounds for raising the ceiling quietly. Whoever adds it makes room or raises the
line in that file, in writing, with a date. Nothing here changes that; this
document records that the sentence is the expansion budget and not merely
housekeeping.

### 2. A new ci-required gate names the shape it catches

Every `ci-required` entry added from FR-158 onward carries a `shape` field naming
the failure mode from the `fr-governance` skill's §4.4 catalogue that requires it.
Enforced by `check_new_gates_name_their_shape` in `test-qa-gate-surface.sh`.

The point is not taxonomy, and the field is not a label. A ci-required gate is
a permanent cost paid on every push by everyone, forever, and the question
*which recorded way of being wrong does this catch* is the cheapest available
filter against adding one out of unease. A check that cannot answer it is
usually a test — and a test belongs in `cargo test`, where it costs a fraction
as much and fails faster.

**The 52 exemptions are closed and may only shrink.** They are the ci-required
gates that existed when the rule was written. This is deliberately the inverse
of the enumeration §4.4 shape 2 condemns: a guard-list is wrong the moment
something new lands outside it, whereas this list is a statement about a past
commit and cannot go stale. Two properties keep it honest:

- An exemption naming a path that is no longer a ci-required gate **fails the
  check**, so the list cannot outlive what it excuses. Without that, it would
  become an amnesty by attrition — gates retire, entries remain, and eventually a
  new path inherits an exemption by colliding with a dead one.
- It grows only by someone editing the manifest, which appears in a diff. That
  visible, reviewable act is the whole mechanism. It is not tamper-proof and is
  not meant to be; it is the same discipline every ledger here runs on.

### 3. The retirement rule, and what it still needs

A gate that has caught nothing across N audit cycles should be demoted to
`manual-runbook` rather than kept on the push path.

**This rule is stated and not yet enforceable, and saying so is the point.** The
data it needs — how often a gate ran and what it concluded — did not exist for
the 35 human-run gates until FR-158 built
`config/governance/manual-gate-freshness.json`, and that ledger starts empty. It
records `exitStatus` per run, so after enough history a gate that has never once
failed becomes visible as a candidate. Until then N cannot be chosen from
evidence, and choosing it from intuition would be exactly the move this document
exists to stop.

What is enforced today is the ledger's *set*: it and the manifest must agree
about which gates are manual-runbook, because a gate missing from the ledger is
missing from every report and that reads identically to a gate that is fresh.
Staleness itself is reported and never fails the build — a gate that goes red
because a human has not followed a runbook lately gets answered by running the
cheapest thing that clears it, which is not the same as running the runbook.

## What this does not do

- It does not cap the number of gates. The cost ceiling is time, because time is
  what CI actually spends and what a reviewer actually waits for. Twenty cheap
  gates and one expensive one are not the same problem.
- It does not apply to `manual-runbook` gates. They cost nothing per push, and
  the constraint on them is human attention, which the freshness report measures
  and does not enforce.
- It does not retroactively justify the 52 exemptions. Writing 52 shape
  attributions for gates designed before the catalogue existed would be
  fabrication, and fabricated rationale is worse than none — it would read as
  review that never happened.

## Known limits

- **The shape field is prose and nothing validates its content.** A gate can
  name a shape that does not fit, and the check will accept it. What the check
  buys is that the question was asked in a diff someone reviewed; it cannot buy
  a good answer. Validating against a fixed token list was considered and
  rejected: the catalogue grows as new failure modes are recorded, and a closed
  token list would be §4.4 shape 2 aimed at the rule itself.
- **The exemption list's "may only shrink" property is a convention, not a
  ratchet.** Nothing computes the previous length and compares. A committed
  baseline would need `git show` at a pinned revision, which is unavailable in
  the fixture trees where every check must also run. The self-cleaning property
  is enforced; monotonic shrinkage is not.
- **The budget covers two jobs, not the whole pipeline.** A governance gate moved
  into `test` or `boundary-coverage` leaves the budgeted set and stops counting
  against the ceiling. `check_wiring_truth` would still require it to declare the
  job it runs in, so the move is visible, but the ceiling would not notice.
- **N in the retirement rule is unset.** See above; it needs history the ledger
  has only just begun collecting.
