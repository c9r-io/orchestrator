---
lifecycle: active
related_fr: FR-140
---

# DD-153: What Governance Costs, And Where The Time Actually Went

**Module**: CI / Governance
**Status**: Implemented (FR-140)
**Related Plan**: FR-140
**Related QA**: `docs/qa/orchestrator/191-governance-execution-cost.md`
**Related**: DD-145 (gate enforcement surface), DD-149 (governance aggregation),
DD-152 (shell lexical state — the same class of defect in the shell lexer),
FR-134's `scripts/lib/rust_lexer.rb` (the function this FR rewrote)

## The problem

Fourteen FRs governed whether gates are *correct*. None asked what they cost. So
the enforcement surface grew from 45 entries to 65 in three days and the two
governance jobs grew from 45 minutes to 80, and no artifact in the repository
could say which gate the minutes belonged to.

`config/governance/ci-job-liveness.json` records each job's last **conclusion**.
`config/governance/qa-gate-surface.json` records each gate's **classification**.
Neither records a duration. "Add one more gate" was therefore a zero-cost
decision every time — until the pipeline was slower than the product it guards
by an order of magnitude.

## What FR-140 got wrong, and why it mattered

FR-140 named the cost source as the fixture-isolation convention's
implementation: each fixture case copies the source tree, so 22 cases means 22
copies. Its proposed remedies were all about copying — `git worktree`, declared
paths, shallow clones.

Measured, on this host:

| what | cost |
|---|---|
| all 22 `new_case` copies in `test-persistence-dependency.sh` | **6.2 s** |
| the whole script | **195 s** |
| **one** invocation of `persistence-dependency.rb` | 13.2 s |
| one invocation of `coordination-governance.rb` | 22.1 s |
| one invocation of `core-boundary.rb` | 15.8 s |
| one invocation of `persistence-api-boundary.rb` | 8.8 s |
| `RustLexer.mask_literals` over 415 files / 6.4 MB | **37.4 s** |
| `RustSource.strip_test_modules`, given those masks | 0.099 s |

Copying is **3.2%**. The isolation convention is not expensive because each case
gets its own tree; it is expensive because each tree gets a full gate execution,
and four of those gates spent 9–22 seconds each inside one function.

On the largest single tracked file (116 804 characters): `mask_literals` cost
2726 ms, a bare `String#[]` walk doing **no work at all** cost 892 ms, and the
same walk over a `chars` array cost 15 ms. The cost was not the masking. It was
the stepping — `String#[]` on a multibyte-capable string is not the O(1)
operation it looks like, and `raw_string_start` was called once per character on
top of it.

A second correction bounds the whole exercise. Classifying every governance gate
step by whether it declares `cargo` (`gate_required_commands`) and joining to
real step durations:

```
cargo-declaring gate steps : 1449s   (63.6%)   ← FR-140 non-goal
no-cargo gate steps        :  831s   (36.4%)
```

FR-140 excludes cargo compile time as a non-goal. So requirement 2 as written
was aimed at 3% of the 36% it was permitted to touch.

## Design

### `ci-cost.rb` and `config/governance/ci-step-cost.json`

Deliberately the same shape as `ci-liveness.rb`, down to the helpers: FR-140
asked for no new mechanism and there was no reason to invent one.

- **Discovery from two ledgers at once.** A step is in scope when it executes a
  `ci-required` gate — read out of `ci.yml` and `qa-gate-surface.json` together,
  using the same executable-text predicate the surface gate uses for wiring
  truth. A commented-out `run:`, an `if: false` step, a `name:` mention and a
  heredoc body all fail to count here exactly as they fail to count there.
  Enumerating the steps would have guarded exactly the steps that existed the
  day it was written, which is the failure mode §4.4 lists second.
- **Coverage runs both ways.** A gate whose step has no number fails; a record
  naming a step the workflow no longer defines fails.
- **Only workflow-defined steps are recorded.** GitHub injects `Set up job`,
  `Post <name>` and `Complete job`; recording those would put names in the
  ledger that no edit here can change. The seconds they consume are recorded per
  job as `unattributed` rather than dropped — a breakdown whose parts do not
  reach the total reads like a full accounting and is not one. Measured, it is
  8 s of 3286 s for `governance` and 5 s of 1512 s for `ci-environment-parity`.
- **`--refresh` from `gh run`, `--write` refused unattended** via
  `CiEnv.refuse_unattended_write!`, like the other five governance writers.
- **Matrix jobs take the slowest leg, not the sum.** Legs run concurrently, so
  the job's contribution to wall clock is when its last leg finishes.

### Why a threshold here, and exact equality in `sourceBaseline`

FR-128 tightened `coordination-governance.rb`'s `sourceBaseline` from monotonic
to exact equality, and that was right. A reference count is a deterministic
function of the tree: any drift without a reviewed ledger update is a defect by
definition, and a monotonic bound lets the true number sink unnoticed.

A duration is not that. It is a sample from a distribution — runner hardware,
cache state, the other tenants on the box. Measured across six successful runs
at a fixed gate count, the two governance jobs vary by about ±7% run to run.
Exact equality on a random variable is a gate that fails on noise; a gate that
fails on noise acquires a `knownFailing` annotation and is then ignored, which
is strictly worse than not having it. So the recorded numbers are attribution,
and the only thing that can fail is the budget.

This is the first ledger in this repository that legitimately uses a threshold,
and the distinction is the one to carry forward: **exact equality for quantities
derived from the tree, thresholds only for quantities that are measured.**

### Provenance instead of workflow-changed staleness

`ci-liveness.rb` invalidates a record when the workflow has changed since it was
taken. That rule is right for a conclusion, which any edit can invalidate, and
wrong for a duration: bumping a `runs-on` or fixing a comment does not make a
recorded second a lie, and a rule that says it does trains everyone to refresh
without reading the diff.

What actually invalidates a cost record is the **step set** moving, and the
coverage check observes that directly, in both directions. What remains is
provenance: the measurement's `headSha` must be an ancestor of `HEAD`, or the
numbers describe someone else's pipeline.

### `pendingMeasurement`, and why the budget can switch itself off

A step added today has never run and cannot have a number until CI executes it.
That window is acknowledged by name and reason in `pendingMeasurement`, and
while any entry is outstanding the **budget is not enforced** — a total that is
knowingly missing steps cannot be compared to a ceiling without reporting
headroom that does not exist.

This is not a switch anyone can leave off. Entries are dropped automatically by
the refresh that measures them, and an entry left on a step that *has* a number
is itself a failure. The bootstrap friction is the point: FR-140's third open
question was "what should adding a gate cost?", and a new gate now has to say
in writing that nobody knows yet.

### The budget

`governance` + `ci-environment-parity` ≤ **2700 s (45 minutes)**, combined.

Not derived from the measured value, which FR-140 forbids — that would only
freeze the status quo. 45 minutes is the top of the range the **whole pipeline**
occupied when FR-140 was filed (24–45 minutes end to end). The line therefore
says: governance alone may not cost what the entire pipeline used to. At the
moment it was set the pair measured 4798 s, so it binds immediately and by a
wide margin.

Two independent routes reached the same number, which is worth recording because
only one of them was permitted. The arithmetic route — the smallest multiple of
five minutes clearing the post-fix measurement with headroom of at least twice
the measured run-to-run spread (±7%, so ≥15%) — gives 2317 × 1.15 = 2664 s,
rounded up to 2700 s. That route was **not** used to set the ceiling, because it
is derived from the current value; it is recorded as a check that the
independently chosen line is achievable rather than aspirational. Measured at
closure: **2317 s against 2700 s, 14% headroom** — the ceiling sits 16.5% above
the measured total.

**Review condition.** A new gate that does not fit is not grounds to raise this
silently. Whoever adds it either makes room or raises the ceiling in
`ci-step-cost.json` with a written reason and a new date. Revisit
unconditionally if the runner class changes, since the number assumes
GitHub-hosted `ubuntu-latest`.

### The lexer rewrite

`RustLexer.mask_literals` now walks a character array. The algorithm — line
comments, nested block comments, raw strings at any hash depth, byte strings,
char-versus-lifetime — is untouched; only the storage changed. `Array#index`
takes no start offset, so the two searches the masker needs (`index_of`,
`index_of_terminator`) are spelled out rather than borrowed from `String`.

Nothing about the isolation convention changed: no fixture rewritten, no
assertion touched, no tree shared. That is a stricter reading of FR-140's own
constraint than the copy work would have been — swapping whole-tree copies for
worktrees changes what each fixture tree *contains*, and a fixture that still
passes on a different tree may be passing for a new reason.

## The evidence

Byte-identical output, three ways:

| check | result |
|---|---|
| all 415 tracked Rust files, SHA-256 per file, old vs new | **415/415 identical** |
| 26 hand-written adversarial constructs | **26/26 identical** |
| 7000 random inputs (2000 slices of real sources, 5000 strings over the driving alphabet) | **0 differences** |

Speed, over the same corpus: **37 416 ms → 1 498 ms (25×)**.

Per gate, single invocation:

| gate | before | after |
|---|---|---|
| `coordination-governance.rb` | 22 124 ms | 1 586 ms |
| `core-boundary.rb` | 15 752 ms | 1 087 ms |
| `persistence-dependency.rb` | 13 177 ms | 1 009 ms |
| `persistence-api-boundary.rb` | 8 815 ms | 1 487 ms |
| `bash32-compat.rb` (not a consumer) | 412 ms | 404 ms |
| `doc-lifecycle.rb` (not a consumer) | 115 ms | 112 ms |

Per suite, locally, **assertion counts unchanged** — which is FR-140's own
acceptance criterion that isolation must not regress:

| suite | assertions | before | after |
|---|---|---|---|
| `test-persistence-dependency.sh` | 22 | 195 s | 29 s |
| `test-core-boundary.sh` | 14 | 360 s | 57 s |
| `test-qa-gate-surface.sh` | 13 | 14 s | 11 s |
| `test-doc-lifecycle.sh` | 12 | 5 s | 5 s |

The last two rows are the control: neither is a `mask_literals` consumer, and
neither moved. A change that made everything faster would be a changed
measurement rather than a fixed defect.

### In CI, which is what the budget is enforced against

Run `30275254232` (before) against run `30288601535` (after):

| job | before | after |
|---|---|---|
| `governance` | 3286 s | **1938 s** |
| `ci-environment-parity` | 1512 s | **379 s** |
| combined | 4798 s | **2317 s** (−52%) |

`ci-environment-parity` falls furthest because it runs the affected gates
**twice** by construction, once with `CI` cleared and once with it set.

Per step, which is the discriminator. Every step that moved more than noise is a
`mask_literals` consumer, and every step that is not one did not move:

| step | before | after | change |
|---|---|---|---|
| Persistence API capability boundary | 292 s | 35 s | **−88%** |
| Persistence dependency chokepoint | 409 s | 55 s | **−87%** |
| Governance ledger regeneration tooling | 261 s | 39 s | **−85%** |
| Core crate boundary and schema snapshot | 610 s | 126 s | **−79%** |
| Legacy coordination decommission contracts | 205 s | 139 s | −32% |
| Agent driver execution migration contracts | 278 s | 271 s | −3% |
| Persistence crate extraction contracts | 202 s | 197 s | −2% |
| Filesystem trigger contracts | 337 s | 356 s | +6% |
| Verify gate enforcement surface negative fixtures | 484 s | 525 s | +8% |
| Agent driver production parity | 56 s | 62 s | +11% |

This is FR-130's "ruler before measurement" discriminator applied to cost. A
faster runner would have moved every row; only the consumers moved, and the
non-consumers scatter either side of zero, which is what run-to-run variance
looks like.

## Accepted costs

- The ledger has to be refreshed by hand from a completed run. Automating it
  would mean a CI job writing a reviewed artifact, which is the thing
  `CiEnv.refuse_unattended_write!` exists to prevent.
- Adding a gate now takes two passes: one to land it with a
  `pendingMeasurement` note, one to record its cost. That friction is the
  feature.
- `index_of` and `index_of_terminator` are hand-rolled scans where `String`
  offered library methods. The library methods are what cost 892 ms per file.

## Known limits

- ~~**A malformed `qa-gate-surface.json` can make checks in
  `test-qa-gate-surface.sh` pass vacuously.**~~ **Fixed by FR-144; see
  [DD-154](154-jq-status-observed.md).** Found while implementing this FR:
  writing `"providerIsolation": "no-provider"` where the manifest wants
  `{"mode": "no-provider"}` makes `jq` exit 5, and because the loops are fed by
  `done < <(jq …)` the exit status is never observed — the check reads zero rows
  and returns success, and the real gate reported PASS on that manifest.

  Two numbers written here were wrong, and are corrected rather than left
  standing. **Six** fixtures caught it, not three, and one of them belongs to a
  second check that reads the same field. The shape was counted as "thirteen
  loops in that gate and four other gates" — that is the text `done < <(jq`,
  which is not the defect; counting whether the feed can *reach* jq gives **39**
  across the same five gates, and the worst is `test-docs-publishing-integrity.sh`
  with 22, listed here as though it were one of the four incidental ones.
  Deferring it out of a P3 cost FR was still the right call; the estimate of what
  was being deferred was not.
- **The budget covers wall clock, not money.** Two jobs running concurrently for
  20 minutes each cost the same wall clock as one for 20 minutes and twice the
  runner minutes. FR-140 asked for a limit on duration and that is what this is.
- **`unattributed` is not broken down.** It is checkout, toolchain install,
  cache restore and teardown. Measured at 8 s and 5 s, it is not currently worth
  attributing; if it grows, the number is already in the ledger to notice it.
- **Step names are the join key.** GitHub reports timings by rendered step name,
  so renaming a step orphans its record — loudly, by design, since a rename can
  also be a replacement.
- **The lexer is faster, not asymptotically better.** It is still a linear scan
  in Ruby. A corpus an order of magnitude larger would need a different answer.
- **The ±7% spread is measured over six runs on GitHub-hosted `ubuntu-latest`**,
  all of them before the lexer rewrite and at drifting gate counts. It is a
  sample, not a guarantee. The budget's headroom at closure is 14% (2317 s
  against 2700 s), which is twice that spread and not much more — the next
  expensive gate is likely to be the one that forces the written trade-off the
  review condition describes. That is the mechanism working, but it means the
  ceiling is not comfortable, and it should not be read as though it were.

  The first post-fix repeat at an unchanged pipeline shape, run `30291298712`,
  came in at 2348 s — 1.3% above the closure sample, leaving 13% headroom. Two
  points are not a spread, so this neither confirms ±7% nor replaces it. It is
  recorded because ±7% is the number the ceiling was reasoned from, and if the
  post-fix regime is in fact this stable then the ceiling is roomier than the
  paragraph above says. Which regime the figure describes should be settled by
  accumulated samples, not by ±7% having been written down first.

  Third sample, run `30301603134`: **2370 s, 12% headroom** — but the shape
  changed, so it is not a repeat. FR-144 added two gates costing 0 s and 25 s
  together, and the pair still landed within 1% of the previous sample once
  those are subtracted. Two FRs have now spent headroom without the ceiling
  binding, which is the mechanism working as intended rather than a warning;
  the note above stands, and the figure to watch is the trend 14% → 13% → 12%,
  not any single reading.
