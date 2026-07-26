---
lifecycle: active
related_fr: FR-137
---

# DD-149: Governance Aggregation Completeness

**Status**: Implemented (FR-137)
**Related**: [DD-145](145-gate-surface-execution-truth.md) (the FR whose own fix this repairs),
[DD-139](139-qa-gate-enforcement-surface.md) (the enforcement surface this check joins)

## The defect

FR-134 made every gate step in the `governance` job `continue-on-error: true` so that one run
reports every problem instead of stopping at the first, and added a final `Governance result` step
that reads each step's outcome and fails the job on any of them. The structure is right, and it
paid for itself on its first run by surfacing a gate that had been failing behind two others since
the day it was wired.

What guarded the aggregate was a hand-written list:

```yaml
        env:
          OUTCOMES: |
            liveness=${{ steps.liveness.outcome }}
            surface=${{ steps.surface.outcome }}
            ...
```

Nothing asserted that the list was complete. Insert a step with `continue-on-error: true` and
`run: exit 1`, leave it out of `OUTCOMES`, and the gate reports `12 passed, 0 failed` while that
step fails on every single run.

The realistic path needs no malice. Adding a gate, three existing checks fire in sequence: the
classification check forces it into `qa-gate-surface.json`, the wiring check forces a real `run:`,
the dependency check forces the commands to be installed. All three go green. Then the `OUTCOMES`
line is forgotten and the gate is dead.

This is the enumeration shape — **a list guards exactly what was known when it was written** —
which FR-134 removed in six other places, reappearing inside the fix FR-134 wrote for diagnostic
visibility. DD-145's Known Limits did not record it.

The list's growth is the argument. It has been **19 → 20 → 21 → 22** entries across four FR cycles,
one per cycle, and at the moment FR-137 was governed the three documents describing it had each
stopped at a different number: DD-145 recorded nineteen outcomes from the first run, FR-137's own
body said twenty, and `docs/feature_request/README.md` said twenty-one. The true count was
twenty-two. Nothing was wrong in CI — the sets did agree — but no reader could have told you that,
and no check could either.

**The defect was latent, not active.** All 22 swallowed steps were aggregated and all 22 records
named a real step; the difference in both directions was empty. This closes it before it fires
rather than repairing a failure that already happened.

## What the FR got wrong

Four claims did not survive rebuilding from the repository.

**The counts were stale in three places at once.** Twenty-two, not twenty. Recorded above, because
the drift is better evidence for the FR's thesis than the number it was trying to state.

**The specified check would have walked past the more likely accident.** Requirement 1 asked for
the steps "with an `id:` **and** `continue-on-error: true`". A step with `continue-on-error: true`
and *no* `id` cannot be aggregated by anything — there is nothing to put on the left of `.outcome`
— and it is invisible to a check that only looks at steps which have one. It is also the likelier
lapse: an `id` gets typed only when someone already intends to read the outcome, so forgetting the
`id` and forgetting the `OUTCOMES` line are one mistake, not two. The rule here is
`continue-on-error` ⇒ has an `id` ⇒ that `id`'s outcome is read.

**The reverse direction's stated reason was backwards.** FR-137 asked for a dangling check because
renaming a step "leaves a record that resolves to empty forever, whose effect is the same as the
omission". Measured against the real aggregate script, it is the opposite:

| `OUTCOMES` fed to the real `Governance result` script | Exit |
|---|---|
| `liveness=success` / `surface=skipped` | 0 |
| `liveness=success` / `surface=failure` | 1 |
| `liveness=success` / `fr137-ghost=` (what a dangling reference produces) | **1** |

`${{ steps.ghost.outcome }}` evaluates to the empty string, the loop finds a value that is neither
`success` nor `skipped`, and the job fails. A dangling record is **loud and permanent**, not
silent: the job can never go green again, and it fails naming a gate that no longer exists. The
rule survived the correction and its reason was rewritten — a stale record makes the job
unsatisfiable, *and* it hides that the renamed step is now unaggregated in the first sense. The
three behavioural assertions exist to pin this down so the next reader does not have to re-derive
it.

**The non-goal would have reintroduced what the FR removes.** FR-137 listed "do not extend to other
jobs" as a non-goal, on the grounds that only `governance` uses this pattern today. Honouring it
means writing `.github/workflows/ci.yml` and `governance` into the check as literals — precisely
the shape the FR exists to abolish, one level up. The general form was measured first: `governance`
is the only job in any workflow with a `continue-on-error` step at all, so the general check passes
on this repository unchanged and costs nothing. The requirement is specific and the non-goal is
general, so the requirement wins — the same resolution DD-145 recorded when FR-134's requirement 8
forced three new gates past its own non-goal.

## Design

### Coverage is discovered

The scan is every job of every workflow found by globbing `.github/workflows/*.{yml,yaml}`. No
workflow path, job name or step id appears as a literal anywhere in the check. The only enumeration
left is the one GitHub imposes: the spelling of `steps.<id>.outcome`.

### Three ways a swallowed failure disappears

`check_continue_on_error_aggregated` reports each independently, so a run says which one happened:

1. a `continue-on-error` step with no `id` — unaggregatable by construction;
2. a `continue-on-error` step whose `id` is never read as `.outcome` in its job — the omission
   direction, silent, job green, gate dead;
3. an `.outcome` naming a step id the job does not define — the dangling direction, loud, job red
   forever.

Each has its own negative fixture, and the three fixtures are deliberately disjoint: fixture 23
touches only the `OUTCOMES` block so that it cannot also trip direction 2. A single fixture
satisfying two rules would leave either one free to be deleted without a test noticing.

### Referenced is not load-bearing

The check is structural: it proves each swallowed outcome is *referenced*. An aggregate step that
printed the table and exited 0 would satisfy it completely while every gate in the job became
decoration. So the real `Governance result` script is extracted from `ci.yml` through the workflow
model and executed against synthetic outcomes — all-pass exits 0, one `failure` exits non-zero and
names the gate, an empty outcome exits non-zero. That is the pairing DD-145 established for
`check_diagnostics_preserved`: a proxy may be an additional condition, never the only one.

### Facts in the library, the rule in the gate

`workflow_model.rb` gains `continue_on_error_steps`, `outcome_references` and a bulk `outcome_facts`
that emits `coe` / `step` / `ref` records for a whole checkout. All three report facts; the set
arithmetic that turns them into "this step's failure disappears" lives in the gate beside the
reason it is a rule, which is where the library's header says interpretation belongs.

`outcome_references` walks the **parsed** job — every string in the map, recursively — rather than
scanning the file. The distinction is the one this whole surface is about: the same text in a
neighbouring job, in a comment, or in a `name:` field is not this job consuming this step's
outcome, and a byte-level scan cannot tell the difference. Both index forms are matched, because
`steps['my-id'].outcome` is what anyone with a dot in an id is pushed toward.

`continue-on-error` counts as *on* unless it says `false` literally; an expression may evaluate
either way at run time. Same direction as the existing `disabled?` — erring toward "this failure is
swallowed" can only make the gate stricter.

### One process per scan

`outcome_facts` covers the whole checkout in a single ruby invocation (0.33s) rather than one per
job. The fixture harness runs every check against 24 separate trees, so a process per job would
have put minutes into a gate that FR-140 is currently open about.

## Decisions and their alternatives

**Assert the no-id case even though the FR did not ask for it.** The alternative was to implement
requirement 1 as written and note the gap. Rejected: the gap admits the exact failure the FR was
filed on, and a gate that certifies an enforcement it cannot observe is worse than no gate.

**Keep the dangling rule after disproving its stated reason.** The alternative was to drop it once
the "same effect as omission" argument fell. Rejected: a permanently-red job is a real defect, and
it co-occurs with an unaggregated renamed step. The reason was rewritten instead, and pinned by an
executed assertion rather than by prose.

**Generalise past the FR's non-goal.** Recorded above.

**Bulk fact command rather than a per-job call.** The alternative was reusing the two single-job
subcommands from the shell in a loop, which reads better and costs 45 ruby processes per tree
against 24 trees. Both single-job subcommands are kept — they are what the bulk command is built
from and what a person debugging one job will reach for.

## Known limits

- **Composite actions are not scanned.** `.github/actions/**/action.yml` can contain steps with
  `continue-on-error`, and a composite step's outcome is consumed inside the action rather than by
  the job. The one composite action in this repository has no such step. This is a place a
  swallowed failure could hide, and it is not covered.
- **The rule is "somebody reads the outcome", not "the reader decides the job".** A job with a
  second step that reads every outcome and ignores them would satisfy the structural check. The
  behavioural assertions cover the aggregate that exists; they do not generalise to an aggregate
  nobody has written yet.
- **`steps.<id>.outcome` is matched as text inside parsed strings.** The walk removes the
  cross-job and cross-file confusions, but an expression built by string concatenation at run time
  is beyond it. GitHub does not offer a form that would let the reference be computed, so this is
  currently theoretical.
- **The new fixtures sit in the region FR-138 reports as unscanned.** FR-138 (open) found that
  `bash32-compat.rb` resets quote state per line, so a `<<`-shaped token inside a multi-line single
  quoted string silently ends the scan — and it names `test-qa-gate-surface.sh` from line 900 as
  one of the two live instances. Fixtures 22-24 and the behavioural block land after that point, so
  the bash 3.2 gate does not see them. Not fixed here; it belongs to FR-138. Mitigated by running
  the gate under the real `/bin/bash` 3.2 on macOS, which is an observation rather than a scan.
- **`disabled?` is not consulted.** A `continue-on-error` step behind `if: false` is still required
  to be aggregated. Erring strict, and the situation does not currently arise.
