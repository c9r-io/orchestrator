---
lifecycle: active
related_fr: FR-143
---

# DD-155: a negative fixture must prove it applied a mutation

**Status**: Released
**FR**: FR-143
**QA**: `docs/qa/orchestrator/193-fixture-target-drift.md`

## The problem

FR-129 built two meta assertions over a gate's check registry, and FR-134 built
them again independently in a second gate: every check is registered, and every
registered check has a negative fixture. They answer *does this check exist* and
*has anyone tried to make it fail*.

Neither asks whether that attempt applied a mutation.

A negative fixture names a specific statement in a specific file and changes it.
**The target is enumerated** — and the governed code moving is precisely what
these gates exist to permit. When the target leaves, the fixture stops testing
anything, and it usually does not fail.

## The nine

Each verified against the commit that repaired it, at `9e250e7f`.

| # | where | what happened | result |
|---|---|---|---|
| 1 | `test-core-boundary.sh` case 3 | pinned `pubMod 52 → 53`; FR-130 Phase A moved it to 50 | **failed loudly** |
| 2 | `test-core-boundary.sh` case 5 | stripped rusqlite from a `core/src/db.rs` that had become a re-export shell whose only token sat in `mod tests`, where the scanner does not count it | mutation mutated nothing, gate correctly reported no change, and **the case reported that the gate had failed to notice a removal** |
| 3 | `test-core-boundary.sh` case 5, later | took the removal target from `rusqlite.files.keys.min`; FR-141 B4 emptied that map | the read returned nothing and the case **wrote to a directory** |
| 4 | `test-persistence-dependency.sh` case 8 | neutralised `SELECT COUNT(*) FROM command_runs` in the scheduler; FR-141 B3 moved it | **aborted** with `no statement to neutralise` |
| 5–7 | `test-persistence-dependency.sh` cases 12/13/14 | probed `crates/daemon/src/server/attention.rs` with a hardcoded count of 1; FR-141 B2 emptied it out of the ledger | all three mutated a file the gate saw for the first time and **reported through the new-file branch** while claiming to test the changed branch |
| 8 | `test-persistence-dependency.sh` case 16 | stripped the category from a daemon file no longer in the ledger | inert |
| 9 | `test-persistence-extraction.sh` case 6 | `git log --grep 'FR-130 A1'` fell through to `pass()` when empty | **an empty grep counted toward the pass total** |

Evidence: `75dcf68c` (1, 2), `e6b5fd70` (3–8), `ef6f439f` (9).

**Only the first failed loudly.** The other eight were green.

The second is the one worth remembering. A fixture that goes quiet is a gap; a
fixture that *reports a defect that is not there* spends an auditor's attention
on the gate instead of on itself, and the gate was innocent.

## What FR-143 itself got wrong

Filed from a real observation during FR-141 governance and not rechecked. Three
claims did not survive, and the corrections are as instructive as the FR.

**The two meta assertions are two, in two places, and neither is where the drift
happened.** FR-129 built them in `test-skill-mirror-integrity.sh`; FR-134
(`445fa9ed`) built them again in `test-qa-gate-surface.sh`, which carries a third
besides. Both key on `ALL_CHECKS`, a registry of check functions — and **none of
the three gates the FR cites as evidence has a registry**. They are linear
`Case N` scripts. Registering a third assertion "beside FR-129's two", as the FR
asked, would have covered the two gates that never drifted and none of the three
that did.

**Requirements 2 and 3 had no backlog.** Measured over the 27 ci-required shell
gates: zero assertions report PASS on a bare exit code as the only condition, and
zero expected diagnostics restate a ledger value as a literal `N -> M`. FR-141
cleared the last of those. They needed regression guards, not repairs — which
changes what their fixtures must prove, since a rule with no current violation is
a rule nobody has watched fire.

**The backlog that existed was 21 + 27, across ten gates, not three.**

## The measurement corrections made while writing this

Recorded because Phase 6 rule 1 exists for exactly this, and both happened here.

1. **"eleven gates" was nine was ten.** The first figure came from counting rows
   in a per-gate table, two of which had a count of zero. A second route —
   `grep -lE` over the manifest's gates, no lexer involved — gave nine. The tenth
   arrived when the scanner's scratch-root discovery replaced the prototype's
   hand-listed variable names and found `test-coordination-strangler.sh:59`,
   which a roster of `DIR|d|BASE|PROBE` could not see. **The enumeration defect,
   in the tool built to measure the enumeration defect.** The site counts, 21 and
   27, agreed across both routes throughout; only the gate count moved.
2. **`ruby` is not an in-place editor.** Requiring `-e` is what separates
   `ruby -e '...' "$DIR/f"` from `(cd "$DIR" && ruby "$GATE")`, which runs the
   gate under test. Without it the rule reported 96 findings where there are 43.
   A scanner reporting defects that are not there is worse than the silence it
   replaces, and it is the failure mode that gets a gate switched off.

## Design

### `scripts/lib/gate_fixture.sh`

Three contracts:

| | requires | proves |
|---|---|---|
| `fixture_premise <label> <cmd...>` | the command succeeds | the case's premise still holds |
| `fixture_mutate <label> <file> <cmd...>` | the target is an existing regular file, the command succeeds, and the file changes | the mutation landed |
| `fixture_produce <label> <file> <cmd...>` | the command succeeds and leaves a non-empty file | the derivation has content |

**The `abort` was never the defect.** It is the diagnosis: it names, in the
fixture author's own words, which premise stopped holding. What was wrong is that
nothing caught it — `set -e` ended the run, the summary line never printed, and a
truncated run is indistinguishable from a complete one. So the aborts stay, and
their words now reach the reader through `fixture_premise`'s stderr.

Every function is called in condition position, which disables `set -e` for
everything beneath it. Nothing in the library leans on `set -e`, and no status is
read from `$?` after an assignment — where ERR is live the assignment leaves
before `$?` is consulted and the record goes with it. FR-144 shipped that defect
inside its own fix.

Generalised from `inject()` in `test-qa-gate-surface.sh`, which exists because
two pattern-based fixtures there stopped matching the moment `ci.yml`'s steps
gained `id:` lines. `inject()` now calls the shared version: two copies of that
logic in one repository is the drift the extraction exists to prevent.

### `scripts/qa/fixture-target-drift.rb`

Scope derived from `qa-gate-surface.json`, never listed. Five rules:

| rule | rejects | at closure |
|---|---|---|
| `aborting-premise` | `abort`/`raise` in a fixture's `ruby -e` body that nothing wraps | was 15 blocks / 21 lines |
| `unproven-mutation` | an in-place rewrite of a scratch file not routed through the library | was 28 |
| `exit-code-only` | a `pass` whose only condition is a non-zero exit code | 0 |
| `restated-expectation` | a literal `N -> M` in an expected diagnostic | 0 |
| `unclosed-heredoc` | a file that ends inside a here-document | 0 |

**Two readers, each right for its half.** Whether a block is wrapped is a
question about the *statement*, asked of the lexer-joined statement — `if
fixture_produce "..." "$AGGREGATE" \` and the `ruby -e '` on the next line are one
statement, and a reader that looked at the ruby line alone reported both
correctly wrapped blocks in this repository as findings. That was the first thing
the rule got wrong. Whether the block *can* abort is a question about the body,
which the lexer blanks — correctly, it is a single-quoted region — so the body is
read from the raw lines inside the statement's own extent. **The parse decides
where to look; the raw text decides what is there.**

`unclosed-heredoc` is the backstop that does not depend on the other four being
right: a clean result over a file the scanner half-read is an artefact of how much
was read. FR-138 is that failure in the bash 3.2 scanner, so it is asserted rather
than inherited on trust.

## Why not run every gate twice

FR-143 permitted a lighter form given a written justification and a fixture that
could falsify it. The justification is derived from the nine, not from cost.

Each way a fixture reports without proving is closed by one rule: it did not
mutate (`unproven-mutation`), its premise is gone (`aborting-premise` and
`fixture_premise`), it reported through another branch (`exit-code-only` and
`restated-expectation`), its target is absent or is not a file
(`fixture_mutate`).

**The residue, stated rather than hidden.** A gate *already failing before the
mutation* satisfies every rule above. A diagnostic match narrows it — an
unrelated pre-existing failure cannot produce a message naming the object this
case just mutated — but does not close it. That is why `core-boundary` case 9 and
`persistence-dependency` case 10 carry explicit before-runs, and why
`exit-code-only` is written as *a diagnostic match **or** a before-run* rather
than as *a diagnostic match*.

Cost is recorded and is not the argument: headroom is 330s of 2700s (12%), and
`test-persistence-extraction.sh` alone costs 200s because its cases run
`cargo check` over `git archive` copies. A blanket before-run there would consume
the remaining headroom by itself. The scanner is one Ruby pass over 27 files.

## Accepted costs

- **Indentation.** Wrapping a mutation in `if fixture_mutate ...; then` indents
  the case body. The alternative is a case whose assertion runs on an unmutated
  tree and blames the gate.
- **A double report at ten call sites.** `set_field` in
  `test-doc-lifecycle.sh` is converted inside the helper rather than at its ten
  call sites, so a caller added later inherits the proof without anyone asking
  for it. Because the callers do not test its return, a stale premise there
  prints twice: the real diagnosis, then the case's own "the gate accepted it".
  The first names the file and key and comes first. Threading a skip through ten
  call sites would buy the tidier log at the cost of the coverage that made the
  helper the right place.
- **One setup failure exits early.** `test-coordination-strangler.sh` derives its
  tool fixture before every contract case reads it; there is no scope to skip. It
  stops — but by printing the summary line, which is the whole difference from
  the abort it replaced.

## Known limits

- **Appends and creates are out of scope.** `cat >> "$DIR/file"` always changes
  the file; it cannot silently fail to apply, which is the only thing the landing
  proof observes. Covering the ~57 append sites would buy nothing against any of
  the nine and would train people to add exemptions. A rule that fires on a shape
  already guarded elsewhere is how exemption lists start.
- **Incident 9's shape is not mechanically guarded.** An empty capture in *shell*
  — `FIRST_MOVE="$(git log ...)"` returning nothing and a `pass` following — is
  closed by the repair in `ef6f439f` and by `fixture_premise` being available, not
  by a rule. Detecting the absence of a premise check is not the same problem as
  detecting a premise check that aborts, and only the second is mechanical.
- **`check_job_dependencies` does not follow sourced libraries.** It derives a
  gate's requirements from that gate's own `command -v` preamble, so a command
  introduced in `scripts/lib/*.sh` is invisible to it. `gate_fixture.sh` uses only
  `shasum`, `mktemp` and `awk`, all already in the governance job's declared
  `runnerBaseline`, deliberately — this file stays inside the baseline rather than
  relying on a check that cannot see it. The limit belongs to that gate and is
  recorded here because this FR is what found it.
- **The wrapper check is positional.** A statement is wrapped when it *begins*
  with a library call. A mutation buried mid-statement behind `&&` would not be
  seen. Every call site in this repository is written the first way, and a rule
  that tried to find the call anywhere in the statement would match the word in a
  comment describing it.
- **Only shell gates.** The Ruby gates build their fixtures in Ruby, where a
  raised exception is already loud and already ends with a backtrace naming the
  line. The rules would need rewriting rather than widening.

## What the fixtures caught that review did not

1. **The library's own test wrote into the working tree.** The harness bodies
   used `$PWD` and the child shell ran in the repository root, leaving `db.rs`,
   `agent.yaml` and three others behind — a fixture doing something other than
   what its own safety paragraph claimed, which is this gate's subject. Found by
   `git status` after the first run, not by reading it.
2. **The `aborting-premise` finding is anchored at the block opener, not the
   abort.** The first version of the case asserted the abort's line, and the
   assertion was wrong rather than the scanner: the wrapper goes where the block
   begins.
3. **Line 7 of the probe is legitimately two findings.** Counting per file made a
   correct scanner look wrong. Rules are counted per rule.
