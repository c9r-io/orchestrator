---
lifecycle: active
related_fr: FR-165
---

# 179. Ledger-driven manual-gate freshness

**Status**: Released

FR-158 built `config/governance/manual-gate-freshness.json` and stopped one step
short of using it. This record is about that step, and about the two ways the
ledger was lying before anyone could take it.

## The criterion was recency, and the ledger recorded three other things

`scripts/lib/gate_runlog.sh` has written four fields per run since the ledger
existed: `date`, `revision`, `exitStatus`, `worktreeDirty`. The reader consulted
one of them.

```ruby
stale = rows.select { |_, age, _| age.nil? || age > stale_after }
```

`exitStatus` and `worktreeDirty` appeared in the printed note beside each row
and nowhere else, so a gate whose last run *failed* reported `ok`, and `--strict`
passed it. This is §4.4 shape 6 in the form the skill describes it: a status
field reporting something other than what you are asking. The ledger's subject
is "has this runbook been exercised, and did exercising it establish anything";
recency answers the first half only.

It was not hypothetical when FR-165 opened it. `test-attention-inbox.sh` carried
`exitStatus: 1` dated 2026-08-11 and read `ok` the next morning.

`worktreeDirty` voids a record for the reason §4.6 condition 1 voids a whole
certification: a gate exercised against uncommitted edits observed a state that
is not in the repository. Ten of the 38 records carried it.

The repaired criterion — a record counts only if it exited 0 on a clean tree,
within `staleAfterDays` — moved the ledger from 15 stale to 23 not fresh, out of
38. That number is the finding, not a regression introduced by the change.

The four states are labelled separately (`never`, `FAILED`, `dirty`, `aged`)
rather than collapsed into `STALE`, because the operator's response to a broken
gate is not their response to an unrun one, and one marker for both hides which.

## Enforcement at the release, not the push

FR-158's argument for keeping staleness advisory is correct and is preserved: a
gate that goes red on every push because a human has not followed a runbook
lately gets answered by running whatever clears it fastest, which is not the
same as running the runbook. `ci.yml` still runs the script bare.

A release is the one moment the answer changes what anyone does. `release.yml`
gains a `manual-gate-freshness` job that runs `--strict`, and `build` and
`gui-build` name it in `needs:`.

The `needs:` edge *is* the enforcement. A job that runs, goes red, and is
depended on by nothing publishes the release anyway — §4.4 shape 1 wearing a
workflow's clothes, and the failure mode is silent because the job's own log
looks exactly like a job that mattered. The fixture parses `release.yml` and
asserts the edge; it does not grep for the job's name, because a `needs:` inside
a comment satisfies a grep.

## Exemptions, and why they are per-gate

Some manual gates cannot be a release precondition. `scripts/watchdog.sh` is the
unarguable case: its own manifest entry describes an unbounded foreground loop
that overwrites `target/release/orchestratord`, so "run it before every release"
asks a human to start an infinite loop that clobbers the artifact being
released. A blocking gate nobody can satisfy is not enforcement — it is a thing
people learn to route around, and a routed-around gate is worse than an advisory
one, because it still reads as enforcement to everyone who has not tried it.

`releaseBlocking: false` with a mandatory `releaseBlockingReason`, and the design
constraints are all §4.4 material:

- **Per-gate keys only.** No pattern, prefix, glob or subtree form exists, and
  none should be added. Shape 8 is exactly that: a `skip-tree` goes on absorbing
  instances *that do not exist yet* and never produces a line in any log. An
  exemption here can only name one gate that already exists.
- **It cannot outlive what it excuses.** Exemptions are keys in `gates`, and the
  set-agreement check forces that map to equal the manifest's manual-runbook
  set. A retired gate's exemption is a hard error, not a lingering line.
- **A reason is mandatory, and an orphaned reason is also an error.** Both
  directions, both with fixtures. FR-133 measured `--deny unmatched-skip`
  covering less than its name suggests and drew the general lesson: an exemption
  ratchet nobody has tried to trip is one whose reach you are guessing at.
- **Exempt gates are always printed with their reason.** An exemption that does
  not appear in the output is the enumeration failure wearing a different hat.

## Three restatements of a number that had already moved

The gate FR-158 built to make a set derivable restated that set's size in prose,
in three places, and all three were stale — the manifest had moved 35 → 38:

| Site | Said | Derived |
|---|---|---|
| `manual-gate-freshness.rb` fail-closed diagnostic | "35 are expected" | 38 |
| `gate_runlog.sh` header | "52 of 87 … the other 35" | 56 of 94 … 38 |
| `qa-gate-surface.json` reason for the runlog library | "all 35 of them … 30 of the 35" | 38 … 33 of 38 |
| `ci.yml` comment above the step | "35 human-run gates" | 38 |

The first is now derived from the ledger, which the set-agreement check keeps
honest, and has a fixture: the empty-read case reads the expected count out of
the fixture ledger rather than restating it, so a re-restated literal fails.
The other three are prose and remain prose — they are annotated with their
derivation command instead, which is the honest treatment for a comment. This
is §4.4 shape 7's third practice ("derive the expected value from the ledger,
never restate it") failing inside the guard built to embody it.

## What the first execution found

Twelve gates had `lastRun: null`. Running them is the whole point of the ledger,
so FR-165 ran them, and the results are the argument for the change better than
the change is.

Three sweeps were needed and only the third is evidence — recorded here because
the first two failed in ways this repository has documented before and reached
for anyway:

1. No login `PATH`: `cargo` and `npm` were absent, and all eleven gates died on
   their prerequisite checks in about a second. Eleven `exitStatus` values that
   were facts about the runner, written into the ledger and reverted.
2. Run while the sweep's author was editing `release.yml` in the same worktree.
   The three gates that require a clean tree *by design* failed on his diff.
   §4.6 condition 7 states that "nothing else may be writing to the repository"
   includes the person running the sweep; it was written after FR-133 lost a
   sweep the same way, and it did not prevent this one.
3. Clean tree, `HEAD bd0e2389` recorded before and after and equal, Node 24.

Running six parent gates also refreshed their sub-gates, taking the ledger from
23 not fresh to 17. `test-process-console-metrics.sh` passed — the first green
run on record for any of the twelve.

Two later sweeps at `b778c86b` took it to **10, with no dirty records left**.
The five gates whose only evidence was a dirty-worktree run were re-run on a
clean tree and all five passed, among them `test-wp05-integration.sh` — the
gate FR-149 found broken from 2026-03-26, four months, whose last record was
made against uncommitted edits. `test-attention-inbox.sh`, the `exitStatus: 1`
record that motivated this whole requirement, also passed at the new revision,
so its failure was environmental rather than a live defect. That is worth
stating plainly: the criterion's value is not that the gate was broken, it is
that nothing could tell the difference.

The six failures have **two** root causes:

- **`--wait-ready`, four gates.** `c1060338` centralised daemon readiness in
  `gate_daemon.sh` on a flag the *previous release* binary cannot accept, and
  `test-slack-skill-automation-vertical.sh` pins `PREVIOUS_REF` to the 0.5.0 cut
  precisely so it can test backward compatibility. Three parent gates wrap it;
  every other sub-gate in all three passes. This is the behavioural half of the
  forward-only rollback contract — the same contract FR-165's requirement 2
  found stated in 14 documents and no code — and it had been dead since
  2026-08-11. `docs/ticket/20260812-wait-ready-breaks-previous-release-gates.md`
- **`npm audit`, two gates.** Dev-only advisories (jsdom → undici);
  `--omit=dev` reports zero, so nothing vulnerable ships.

  **Resolved rather than exempted.** The first pass took the cheap route and
  gave `test-process-console-ui.sh` a `releaseBlocking: false`, which would have
  worked and cost more than it looked: that gate is
  `test:coverage && test:e2e && build && audit`, so exempting it to dodge one
  step also removes the GUI vitest suite and Playwright e2e from release
  blocking — and `ci.yml` runs no vitest, so nothing else covers them at all.
  An exemption sized to a whole gate when the objection is to one line inside it
  is the enumeration mistake in miniature: it takes out everything it happens to
  contain.

  Scoping line 27 to `npm audit --omit=dev` asks the question the gate is
  actually for — is anything *shipping* vulnerable — and answers it green today,
  so the gate re-arms and the four assertions around it go on blocking a
  release. Dev-tree advisories keep an owner: `dependabot.yml` covers `/gui`.

  The general form is worth keeping: when a composite gate is red for one
  reason, fix the reason's scope, not the gate's blocking status. The exemption
  mechanism exists for gates that *cannot* run before a release, not for gates
  that are inconvenient.

The first of those is the change justifying itself on a real object.
`test-slack-skill-automation-vertical.sh` has a recorded green run at `685525af`
dated 2026-08-10, and `c1060338` broke it on 08-11. Under the recency-only
criterion the ledger would have reported it `ok` — a one-day-old record is
recent whatever it exited with — and the gate would have gone on reading fresh
while the thing it certifies could not start.

Two further findings came out of the same sweep and belong to nobody's FR:
`ci.yml` runs no `vitest` at all, so the GUI's 120 unit tests are enforced by
nothing on a push while `boundary-baseline.json` carries a frontend coverage
figure no CI job re-derives; and `npm audit` appears in exactly one place in the
repository, inside one of the never-run gates. Both are in
`docs/ticket/20260812-node-version-unpinned-locally.md`.

## Known limits

- The three prose counts above are annotated, not enforced. `dependency-policy.rb`
  has a `prose-counts-derived` rule for `deny.toml`; there is no equivalent for
  `qa-gate-surface.json` reasons, and building one was not in FR-165's scope.
  They will go stale again.
- `CiEnv.unattended?` decides whether a run is recorded, and its own comment
  names "a locally driven agent" as the case `ENV["CI"]` misses — while an agent
  driving these gates locally is detected as *attended* and does record. The
  ledger's premise is "executed by a person following the owner QA document".
  Whether an agent's run satisfies that premise is not decided anywhere.
- **Seven release-blocking gates are not fresh as this ships**, so the next
  release is blocked until they are worked. The backlog was worked down rather
  than declared acceptable — 23 not fresh at the start, 10 now, and **no dirty
  records at all** — but what remains is not clearable by running things:

  | Remaining | Why |
  |---|---|
  | 4 × Slack gates, `FAILED` | one root cause, the `--wait-ready` ticket |
  | 3 × `never` | each needs an ambient daemon that none of them starts |

  The four Slack gates go green together when the `--wait-ready` ticket is
  fixed; they are one defect, not four. The three never-run gates need a
  decision that is not FR-165's to make: either they learn to start their own
  daemon like the other 33, or they are the wrong shape for a release
  precondition and should say so in a `releaseBlockingReason`.
- `npm audit` appears exactly once in the repository, inside this gate, so the
  npm supply chain has no other enforced check. It is now scoped `--omit=dev`
  (see below); dev-tree advisories are owned by Dependabot rather than by a
  release gate.
- Nothing yet asserts that a *non*-manual gate does not arm the runlog.
  `test-qa-gate-surface.sh` asserts the forward direction — every manual gate
  arms it — and FR-165 removed one ci-required gate that had picked up the
  boilerplate and was warning on every attended local run. The converse check
  was not built.
