---
lifecycle: active
related_fr: FR-145
---

# Orchestrator - A Reader That Leaves Early Kills The Producer

**Module**: CI / Governance / Shell gates
**Scope**: `scripts/qa/pipefail-short-circuit.rb` with its fixtures, and the 61
rewritten sites across 20 tracked shell files
**Scenarios**: 5
**Priority**: Medium

## Background

`set -o pipefail` makes a pipeline's status "any stage that was non-zero".
`grep -q` leaves on the first match. Where the producer still has more than a
pipe buffer to write, it is killed by EPIPE — and the reader's **successful
match** is reported to the caller as a **failed** one.

FR-133's certification sweep hit it: a gate announced that `CHANGELOG`'s
`[Unreleased]` section did not name `RunnerExecutorKind`, which it does, at byte
59273 of a 90047-byte section. Ten isolated re-runs passed.

Design record: `docs/design_doc/orchestrator/157-pipefail-short-circuit.md`.

**Safety**: read-only against the working tree. Every case builds a scratch git
checkout under `$TMPDIR`; no daemon starts, no database is touched, no provider
is invoked, nothing reaches the network.

## Why the assertions are shaped the way they are

**The rule is a guard with zero violations once the rewrite lands**, so the
fixtures are the only evidence it works. Every "must fire" case is paired with a
"must not fire" one on the same probe: a rule that fires on correct input is
switched off long before it catches anything.

**Three of the silent cases are the FR's own errors, turned into assertions.**
FR-145 counted 42 sites where there were 35, because `grep -c` counts lines
including prose — four of the matched lines are comments *describing* the
pattern, one of them written by FR-133 to explain the first fix — and it claimed
every site sat under `pipefail` when two tracked files do not enable it. A
scanner that repeats those errors reports findings on its own documentation.

**The mechanism is asserted deterministically, not statistically.** The
probabilistic form measures 8–13 in 400 here and **0 in 200** on a 1 MB producer
with the match at byte 0. Committed as a gate, that is a coin flip on someone
else's runner.

---

## Scenario 1: The tree is clean, and the scanner says what it governs

**Steps**

```bash
ruby scripts/qa/pipefail-short-circuit.rb; echo $?
ruby scripts/qa/pipefail-short-circuit.rb --list-files | wc -l
```

**Expected result**

- `pipefail short-circuit: PASS (106 tracked shell file(s), 92 under pipefail, 0 finding(s))`,
  exit 0.
- `--list-files` prints the governed subset — tracked `*.sh` that enable
  `pipefail` — and the second number on the summary line equals its length.

*What this would still pass on*: a scanner that governs nothing. Separating
**tracked** from **under pipefail** is what makes "0 findings" and "read
nothing" different sentences, and case 14 of the fixture asserts it on a
two-file scratch tree where both numbers are checkable by hand.

---

## Scenario 2: The rule fires five ways, and stays quiet on six

**Steps**

```bash
bash scripts/qa/test-pipefail-short-circuit.sh
```

**Expected result** — cases 2 through 6 each mutate exactly one line of a
correct probe and assert the finding names **that line**:

| case | mutation | why this one |
|---|---|---|
| 2 | a here-string rewritten back into `printf … \| grep -q` | reintroduction is what happens; deletion is the case the author had in mind |
| 3 | `rg --quiet` as a downstream stage | the long form a short-flag regex misses |
| 4 | `grep -qxF` | `q` in the middle of a cluster |
| 5 | `cat f \| grep -q x` inside `"$( … )"` | a command substitution opens a fresh quoting context; the first version of this scanner read that `\|` as quoted and missed the stage |
| 6 | a pipeline broken after the `\|` | the reader is the first word on its line and is still a downstream stage |

**The expected line is derived, never written down.** Each case locates its
marker in the mutated probe and asserts on that line number. A fixture that
restates a number stops working the moment the probe gains a line, and it stops
working *by passing on the wrong finding* (§4.4 shape 7).

**And it stays quiet on six shapes that only look like it.** Cases 7 through 12
assert silence on:

- the shape written in a **comment**
- the shape written inside a **here-document body**
- the same file with `pipefail` **removed**
- `grep -q "a|b|c|d"` — the `|` is inside double quotes
- `grep -F -- -q -quiet --silent` — `--` ends the option list
- `[[ "$(… | grep -c .)" -eq 3 ]]` — `grep -c` counts, and therefore reads to EOF

**Case 12 is a false positive this gate produced on its first run over this
repository**, three times. `-eq` is a short-flag cluster containing `q`, and the
flag scan had run past the end of the command into the enclosing `[[ ]]`. It is
kept because it is the case a reader would not think to write.

---

## Scenario 3: The mechanism, without a race

**Steps**

```bash
bash scripts/qa/test-pipefail-short-circuit.sh   # case 16
```

**Expected result**

```
PASS: pipefail reports a matched pattern as unmatched when the producer
      outlives the reader (10/10)
PASS: the here-string form reports the match every time (10/10)
```

The producer emits the match, sleeps, then writes again, so it is still writing
*by construction* when `grep -q` leaves. **10/10 and 0/10** on any machine,
under any load, with any `grep`, independent of the pipe buffer.

The field measurement that found the defect is recorded here rather than
committed, because it is not reproducible enough to gate on. At `f105ce66`,
`CHANGELOG`'s `[Unreleased]` section (90047 bytes, match at byte 59273), 400
iterations per row:

| form | no artificial load | 8 busy loops |
|---|---|---|
| `printf '%s' "$U" \| rg -q P` | 8 / 400, re-measured 13 / 400 | 10 / 400 |
| `printf '%s' "$U" \| grep -q P` | 3 / 400 | 2 / 400 |
| `rg -q P <<< "$U"` | 0 / 400 | 0 / 400 |

and in the inverted shape, where the match feeds the failing branch, **2 / 200
real matches reported as "not found"** — each one a violation reported as clean.

**This corrects QA-194 and FR-145**, both of which recorded `0/400` idle and
attributed the trigger to CPU contention. It fires at 2–3% on a quiet machine.

---

## Scenario 4: The governed set follows git, not a list

**Steps**

```bash
bash scripts/qa/test-pipefail-short-circuit.sh   # case 13
```

**Expected result** — adding one tracked `.sh` that enables `pipefail` grows
`--list-files` by exactly one and the new path appears in it, **with no edit to
the scanner**. This is the FR-143 precedent: a scanner whose scope is a list is
a scanner that stops seeing.

---

## Scenario 5: The rewritten gates still assert what they asserted

**Steps**

Run each touched gate before and after its rewrite and compare the pass count. A
rewrite that silently stops a check from matching stays green while lowering the
count, and the count is the only thing that sees it.

**Expected result** — gates CI executes, measured at `596500e7` and again after:

| gate | before | after |
|---|---|---|
| `test-docs-publishing-integrity.sh` | 8 | 8 |
| `test-fixture-target-drift.sh` | 18 | 18 |
| `test-jq-status-observed.sh` | 18 | 18 |
| `test-markdown-link-integrity.sh` | 2 | 2 |
| `test-skill-mirror-integrity.sh` | 7 | 7 |
| `test-coverage-governance-mainpath.sh` | 10 | 10 |
| `test-persistence-extraction.sh` | 11 | 11 |
| `scripts/qa-doc-lint.sh` | PASS | PASS |

`test-agent-driver-production-parity.sh` is certified by the sweep rather than
in isolation; it needs `PROTOC` and a clean worktree.

**Six sites needed more than a mechanical rewrite**, and they are the ones a
careless one breaks *silently*: `printf '%s\n' $targets | grep -qxF "$check"`
leaves the expansion unquoted on purpose, so `<<< "$targets"` would collapse the
list onto one line and `-x` would stop matching — with no count moving.
They became `<<< "$(printf '%s\n' $targets)"`, at
`test-docs-publishing-integrity.sh:483,597`,
`test-markdown-link-integrity.sh:266,340` and
`test-skill-mirror-integrity.sh:378,527`.

**Manual-runbook gates.** Four of the nine use isolated data directories and
19xxx ports and were run:

| gate | result |
|---|---|
| `test-attention-inbox.sh` | 10 passed, 0 failed |
| `test-handoff-safe-resume.sh` | 8 passed, 0 failed |
| `test-slack-reaction-task-routing.sh` | 6 passed, 0 failed |
| `test-coordination-collapse.sh` | aborts before reaching this FR's change — see below |

`test-coordination-collapse.sh` ends at line 140 on
`Error: resource.apply: [legacy_coordination_removed] workflow 'coordination-legacy'
step 'legacy_test' uses behavior.captures`, which `set -e` turns into an exit.
The only line this FR changed in that file is 51 lines further down. A before-run
at `596500e7` — the revision before the rewrite — reproduces the same abort, so
this is pre-existing rot in a manual gate whose fixture the daemon has outgrown,
not a regression from this change. **The before-run is what establishes that**;
the line numbers alone would be the "already failing before the mutation"
residue §4.4 shape 7 names.

The remaining five drive the ambient daemon and the runtime database. CLAUDE.md
forbids operating on it and §4.7 forbids self-referential gates, so they were not
run: `test-health-policy-check.sh`, `test-per-trigger-webhook-auth.sh`,
`test-webhook-trigger.sh`, `test-wp05-integration.sh`,
`test-self-bootstrap-cycle2-regression.sh`. Their evidence is `bash -n` over all
106 tracked shell files, `bash32-compat.rb` (PASS, 106 scanned) and this scanner.
Named individually rather than folded into an aggregate, because "the suite
passes" is not per-object evidence.

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | The tree is clean, and the scanner says what it governs | ☑ PASS | 2026-07-29 | Claude |
| 2 | The rule fires five ways, and stays quiet on six | ☑ PASS | 2026-07-29 | Claude |
| 3 | The mechanism, without a race | ☑ PASS | 2026-07-29 | Claude |
| 4 | The governed set follows git, not a list | ☑ PASS | 2026-07-29 | Claude |
| 5 | The rewritten gates still assert what they asserted | ☑ PASS | 2026-07-29 | Claude |

## Related gates

- `scripts/qa/bash32-compat.rb` — same `git ls-files '*.sh'` scope and the same
  `scripts/lib/shell_lexer.rb`, so the two agree on what a comment is.
- `scripts/qa/jq-status-observed.rb` — the other half of "a gate must observe
  what it reads". Its scope is manifest-derived and therefore narrower; see
  DD-157's known limits.
- `scripts/qa/fixture-target-drift.rb` — the fixtures here are built on
  `gate_fixture.sh`, and this scanner is what keeps them that way.
- `scripts/qa/ci-cost.rb` — carries the two new governance steps as
  `pendingMeasurement` until CI measures them, which is why the budget currently
  reports `NOT ENFORCED`.
- `scripts/qa/ci-liveness.rb` — `ci.yml` changed, so DD-146's two-pass
  convergence applies.
