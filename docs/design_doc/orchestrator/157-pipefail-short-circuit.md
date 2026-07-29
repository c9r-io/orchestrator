---
lifecycle: active
related_fr: FR-145
---

# DD-157: A Reader That Leaves Early, And A Defect Whose Direction The Data Chooses

**Status**: Released

## The mechanism

`set -o pipefail` makes a pipeline's exit status "any stage that was non-zero".
`grep -q` / `rg -q` leave on the first match. Where the producer still has more
than a pipe buffer to write, it is killed by EPIPE, and that non-zero becomes
the pipeline's answer — so **a successful match is reported to the caller as a
failed one**.

FR-145 was filed from one observation: during FR-133's certification sweep,
`scripts/qa/test-agent-driver-documentation-alignment.sh` announced that
`CHANGELOG`'s `[Unreleased]` section did not name `RunnerExecutorKind`. It does,
at byte 59273 of a 90047-byte section. Ten isolated re-runs passed.

## This shape was found three days earlier and recorded as fixed

The most useful thing the fact-check turned up was not in the FR or in the code.
It is in `CHANGELOG.md`'s `[Unreleased]` section, entered by **FR-134** on
2026-07-26:

> The surface gate could report a false failure under load (FR-134).
> `producer | grep -q` with `set -o pipefail` is a race … **Every such pipeline
> in the governance scripts is now a here-string**

Same mechanism, same diagnostic — that run had
`sed: couldn't write 80 items to stdout: Broken pipe` printed beside the
accusation — same remedy.

`DD-145` states it accurately: "Every such pipeline **in these files** is now a
here-string." The CHANGELOG entry widened *these files* to *the governance
scripts*, and that sentence was false the moment it was written: **63 sites
remained** — 61 of them under a lexically visible `pipefail`, and two more that
only the scope correction below made visible at all. Three days later one of the
61 produced another false failure.

This is a local repair recorded as a global one, and it is the mirror image of
§4.4 shape 2. A hand-written list at least *looks* suspicious when it stops
growing; an unscoped completion claim reads like a finished job and stops the
next person from looking. `[Unreleased]` entries are live statements rather than
history, so the entry has been corrected as part of this closure.

## The direction is set by the branch, not by the defect

FR-145 recorded this as a defect that fails **closed**: it produces a mystery
red, people re-run until it is green, and the gate is thereby taught to be
ignored.

That holds only where the match feeds the **passing** branch. Where the match
feeds the **failing** branch, the same code turns a real violation into a clean
report. Measured on the same input in the inverted shape: **2 / 200** matches
reported as "not found".

This repository had five sites of the second kind, and three of them are leak
assertions:

| site | producer | what it asserts |
|---|---|---|
| `test-agent-driver-production-parity.sh:266` | `sqlite3 "$DB" .dump` | provider session material never reaches the database |
| `test-coordination-collapse.sh:185` | `sqlite3 "$DB" '.dump'` | the per-run callback token never reaches the database |
| `test-slack-reaction-task-routing.sh:257` | `sqlite3` snapshot columns | credentials never reach a route snapshot |

All three were `! producer | grep -q SECRET`. A whole-database dump is the only
producer here with no bound on its size, and it grows with every scenario added
to the fixture. When the secret *is* in the dump, `rg` matches, `sqlite3` dies,
`pipefail` hands the non-zero to `!`, and the gate prints `pass` at exactly the
moment it must not.

The other two were `cargo test --workspace 2>&1 | grep -q "^test result: FAILED"`
in two manual gates, where a suite that printed `FAILED` early enough could be
reported as `PASS: cargo test --workspace`.

**The FR's author — the same effort that wrote this record — did not see the
inverted shape**, because the one observed instance happened to be the harmless
half. That is the finding worth carrying forward, and it is proposed as §4.4
shape 9.

## Why the rule is syntactic and has no exemption

FR-145's acceptance criterion 1 asked for a per-site classification: convert to
a here-string, **or** write down why the producer is bounded. Measured:

| producer | match at | fires |
|---|---|---|
| 90 KB, 131 lines | byte 59273 | **8–13 / 400** idle, **10 / 400** loaded |
| 1 MB, one line | byte 0 | **0 / 200** |

Size does not decide it. Match position and line structure do, and the readers
differ from each other — BSD `grep -q` measured **3 / 400** on the same input
where `rg -q` measured **8 / 400**.

And "this producer is bounded" is a claim about **today's data**, re-checked by
nobody. The CHANGELOG took years to cross 64 KB. So the annotation the FR asked
for is §4.4 shape 2 wearing a different hat, and criterion 1 was replaced.

The rule is therefore syntactic, and has **no escape hatch**, because the
alternative spelling

```sh
grep -q PATTERN <<< "$(producer)"
```

writes a temporary file, leaves no writer to signal, and is correct at every
size, every match position and every implementation of grep. A rule whose remedy
is always available does not need an exemption — and an exemption is how a rule
gets quietly widened (§4.4 shape 8).

## Scope: every tracked shell file, and why the obvious exemption is wrong

The first version of this scanner governed **tracked `*.sh` that enable
`pipefail`**, on the reasoning that without the option a dead producer is
invisible and there is nothing to guard. FR-145's corrected text said the same
thing, and named the two files it exempted.

That reasoning reads a **dynamic** property lexically, and the closure
self-check's question — *name a broken state this would still pass on* — found
the counterexample in the repository. `scripts/regression/run-cli-probes.sh`
sets `-euo pipefail` and then

```sh
source "$scenario_script"
```

for every file under `scenarios/`. Those files set no options at all; their
pipelines run under the runner's. Demonstrated by execution rather than
argument: a scenario sourced that way reports a pattern that **is present** as
absent. The two files FR-145 recorded as immune —
`probe-low-output.sh:41` and `probe-runtime-control.sh:52` — were live sites,
outside the governed set, for the whole of this FR until the self-check.

And a scanner cannot prove the negative: the sourcing site is
`source "$scenario_script"`, a variable. So the exemption goes, on the same
argument as the per-site one — the remedy costs nothing and is correct
everywhere, so there is no state worth exempting. **The governed set is the
tracked set.**

It is also deliberately wider than `config/governance/qa-gate-surface.json`:
the hazard has nothing to do with whether a script is ci-required, and three
scripts `ci.yml` executes are absent from that manifest, so a manifest-derived
scope would have missed the invoker of the run where this was first seen.

**63 executable sites across 22 files.** Two independent derivations agree on
the 61 that sit under a lexical `pipefail`: this scanner (parsing) and a `grep`
over tracked shell (text). Their six differences are the whole argument for
parsing — four are **comment lines describing the pattern**, and two are the
files above, which the text derivation and the first scanner both mistook for
exempt.

## The other four corrections to FR-145

- **"0 / 400 idle"** → 8–13 / 400 idle. Load raises the rate by about a quarter;
  it is not what makes it possible. The FR's explanation of why the defect had
  never been seen was therefore wrong, and wrong in the direction that makes it
  look rarer.
- **"42 sites / 9 gates"** → 35 executable sites / 7 gates. 42 was produced by
  `grep -c`, which counts lines including prose; four of the matched lines are
  comments about the pattern, one of them written by FR-133 to explain the first
  fix. §4.4 shape 1, committed by the FR about assertion strength.
- **Ten of the twelve "suspect" sites were structurally immune**:
  `scan() { (…) || true; }` returns 0 whatever the subshell did, SIGPIPE
  included, so `pipefail` never saw the producer. They were rewritten anyway,
  because a rule that exempts "immune by a `|| true` two functions away" is not
  a rule anyone can check.
- **The gate that reported the false failure** was
  `test-agent-driver-documentation-alignment.sh`, not `qa-doc-lint.sh`, which is
  its `invokedBy`.

## The deterministic assertion

A committed test that runs the buffer race 400 times and asserts "at least one
failure" is a coin flip on someone else's runner — measured 0/200 here on a 1 MB
producer. Case 16 of the fixtures removes the race instead of racing it:

```sh
{ printf 'MATCHME\n'; sleep 0.2; printf 'tail\n'; } | grep -q MATCHME
```

The producer is still writing **by construction** when the reader leaves.
Measured **10/10** piped and **0/10** through a here-string, independent of pipe
buffer, match position, grep implementation and machine load. The 400× field
measurement lives in QA-195 as the observation that found the defect.

## Certification

At `2b2e5cab`, on a clean worktree that was still clean at the end and a HEAD
that had not moved, **44 of 44 derived invocations green**. The invocations come
from `workflow_model.rb run-commands` over every job in `ci.yml`, not from a
list: §4.6.6's derived *path* is not a derived *invocation*, and this repository
has one gate — `certify-slack-managed-live.sh` — that exits 2 when run without
the subcommand CI gives it. The reconciliation is printed rather than assumed:
seven ci-required paths have no direct invocation, and all seven are the
manifest's `invokedBy` entries; three invoked paths are absent from the manifest,
and those three are FR-147.

CI at the same revision: **17 of 17 jobs green**, plus the Security workflow.

**The local sweep could not have caught the last defect this FR shipped**, and
that is worth recording next to the green line. Fixture case 9b compared a
sourced scenario's combined output for equality; on the Linux runner bash prints
`printf: write error: Broken pipe` to stderr first, and macOS bash 3.2 and 5.3.9
both do not. One shell on one platform is what a local sweep is, and CI is the
observer that is not. The case now writes its verdict to a file and is green
under all three shells.

## The second family: `head`, which needs no flag

FR-145 deferred `head` with a measurement obligation, and FR-146 discharged it. It is the same
EPIPE, and almost nothing else about it is the same.

**It needs no flag.** `grep` and `rg` short-circuit only when told to — `-q`, `--quiet`,
`--silent`, `-m N` — and otherwise read to end of input. Every `head` short-circuits. So the
scanner's `READERS` splits into `MATCH_READERS`, which get a flag test, and `ALWAYS_READERS`,
which do not.

**It fires roughly two orders of magnitude harder.** `X="$(seq 1 N | head -1)"` under
`set -euo pipefail`, ten runs per row:

| producer | died |
|---|---|
| ~6 B | 0 / 10 |
| ~3.9 KB | 0 / 10 |
| **~24 KB** | **6 / 10** |
| ~129 KB | **10 / 10** |
| ~1.3 MB | 10 / 10 |

FR-145's `grep -q` managed **8–13 in 400** on a 90 KB producer. Same fuzzy, data-dependent
boundary around the pipe buffer; a wholly different rate.

**And it fails in a third direction, which is neither of FR-145's.** Measured:

| position | status reaches | consequence |
|---|---|---|
| `X="$(p \| head -1)"` — assignment | `set -e` | **run ends, `X` never assigned, no summary line** |
| `p \| head -N >&2` — bare | `set -e` | **run ends, no summary line** |
| `if [ -n "$(p \| head -1)" ]` — value in a condition | discarded | survives, value correct |
| `… \|\| true` | discarded | immune |

FR-146 as filed had this partly inverted: it predicted condition position would invert an
assertion "same as FR-145", and it has no row at all for **assignment**, which is the dominant
idiom — ~18 of 37 sites, all `task create … | grep -oE UUID | head -1`. Those do not report the
wrong answer. They stop the gate before it reaches its summary line, which is §4.4 shape 7's
failure arriving through a completely ordinary-looking line of shell.

That also means the remedy is not a here-string. A here-string is the right advice for a reader
that was matching something; for a reader that was slicing, the fixes are:

```sh
out="$(producer)"; first="${out%%$'\n'*}"   # no pipe at all; bash 3.2 clean
producer | sed -n '1,Np'                     # reads to EOF
producer | awk 'NR<=N'                       # reads to EOF
```

All three measured against a 1.3 MB producer. The scanner's `fix:` text switches on the family
for exactly this reason.

**37 sites, not 38.** Two independent routes: `grep` over tracked shell gives 39, the lexer plus
the quote-aware splitter gives 37. The two differences are
`(get|post|put|delete|patch|head|options)` — a regex alternation inside a single-quoted string in
`extract_surface.sh`. FR-146 also scoped its count to files that enable `pipefail`, which this
FR's own closure had already made obsolete; widening the scope adds one and the lexer removes
two, so **two independent errors nearly cancelled**. That is the argument for naming a method
beside every number, made by the FR written about that trap.

The same trap recurred a third time during the fix: after the rewrite, `grep` still reported
matches in two files, and both were comments freshly written to explain the hazard. The scanner's
own parser is the authority here, and it reports zero.

**Two `|| true` workarounds died with the readers they were protecting.** Four sites carried
`| head -N … || true`; the `|| true` was absorbing head's SIGPIPE, and it was equally absorbing a
real `diff` or `jq` failure — FR-144's class, kept alive by a workaround for this one.

**One site was the dynamic-scope case, live.** `scripts/regression/lib/probe-runner-lib.sh` sets
no shell options; `run-cli-probes.sh` sets `-euo pipefail` and sources it. It is governed only
because FR-145's closure removed the pipefail exemption, and it carried two sites.

## Known limits

1. ~~`| head` is not covered.~~ **Closed by FR-146** — see "The second family"
   above. The estimate recorded here (38 sites / 29 files, eight of them
   diagnostics) was wrong in both the count and the consequence mapping; the
   corrections are in that section.
2. **Three scripts are executed by `ci.yml` and absent from
   `qa-gate-surface.json`**: `scripts/qa-doc-lint.sh`,
   `scripts/coverage-governance.sh` and `scripts/check-async-lock-governance.sh`.
   Every scanner that derives its scope from that manifest —
   `jq-status-observed.rb`, `fixture-target-drift.rb` — is blind to all three.
   The third was found only by the certification sweep's reconciliation, which
   derives the invocations from `workflow_model.rb run-commands` rather than
   reading the workflow; the fact-check that produced this FR's finding G read
   the workflow and found two. That is the argument for the derivation, made by
   the same error one level up. Adding them changes those scanners' governed
   sets, which is a separate change with its own certification. Filed as FR-147.
   The FR-145 scanner is unaffected: its scope is `git ls-files '*.sh'`, so all
   three are governed by it today.
3. **`<<< "$(f)"` discards the producer's exit status**, where the pipeline
   under `pipefail` observed it. Checked site by site: in these 63, producer
   failure and "no match" already reached the same branch, so the conversion is
   behaviour-preserving. Five sites got explicit status handling instead,
   because there "could not read" and "found nothing" are different facts that
   the pipeline reported identically — the two `cargo tree` probes in
   `test-persistence-extraction.sh`, the two `cargo test`/`cargo clippy` probes,
   and the three `sqlite3` dumps, whose empty result is now its own failure
   (§4.4 shape 5).
4. **Five of the nine manual-runbook gates were not executed.** They drive the
   ambient daemon and the runtime database; CLAUDE.md forbids operating on it
   and §4.7 forbids self-referential gates. Their evidence is `bash -n` over all
   106 tracked shell files, `bash32-compat.rb`, and this scanner. Named in
   QA-195 rather than folded into an aggregate.
