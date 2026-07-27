---
lifecycle: active
related_fr: FR-138
self_referential_safe: true
---

# Orchestrator - bash 3.2 Scanner Lexical State And Coverage Surface

**Module**: CI / Governance
**Scope**: cross-line quoting in `scripts/lib/shell_lexer.rb`, the `unclosed-heredoc` backstop, the per-file coverage census, negation as a command position, and the removal of per-file emptiness inference
**Scenarios**: 5
**Priority**: High

## Background

`scripts/qa/bash32-compat.rb` reset its quote state at every newline. A `<< WORD`
lookalike inside a region opened on an earlier line read as a here-document
opener, and every remaining line of the file left the scan with no diagnostic at
all. `test-qa-gate-surface.sh` was losing 360 lines to a `<<EOF` inside a
`perl -e` string; `test-bash32-compat.sh` was losing 16 to `hosting << job_name`
inside the `ruby -e` block that exists to prove CI runs that gate on a bash 3.2
host.

Design record: `docs/design_doc/orchestrator/152-shell-lexical-state.md`.
Sibling QA document: `docs/qa/orchestrator/184-bash32-compatibility.md`, which
covers the executed half — the seven constructs run under a real `/bin/bash` 3.2.
This document covers the lexical half, which is host-independent.

**Safety**: read-only against the working tree. Every scenario builds scratch git
repositories under `$TMPDIR`; no daemon is started, no database is touched, no
provider is invoked. Safe to run against this repository.

## Why the assertions are shaped the way they are

The defect this document covers occurred **while the gate was green**. Any
scenario whose evidence is "the gate passes" is satisfied by exactly the state
being tested for, so the coverage claim is asserted by per-file line accounting
instead (scenario 5), and each negative fixture is asserted by *rule name and
line*, never by exit code alone.

Each fixture also asserts that no other rule fired on the same tree (the FR-127
isolation convention). A fixture that trips an unrelated rule reports success for
the wrong reason and leaves the rule it names unverified.

The mutation each fixture is aimed at is recorded below, because a fixture that
also passes on the broken implementation is not evidence. Verified during
governance: fixtures 1a, 1b, 2, 3 and 4a are all **accepted** by the pre-FR-138
gate and rejected after, so each is live. Fixture 1c is the exception and says so.

---

## Scenario 1: The scan reaches end of file past every shape that used to end it early

Three fixtures, one claim. They are separate fixtures rather than one because they
exercise different lexer state, and a single tree carrying all three would pass
as soon as any one of them was caught.

### 1a - A here-document lookalike inside a cross-line single-quoted region

**Steps**

```bash
bash scripts/qa/test-bash32-lexer.sh
```

Read case 1. Its fixture places `<<EOF` inside a multi-line `perl -pi -e '...'`
program and a `mapfile` invocation on the **last line of the file**.

**Expected result**

The gate rejects the tree, reports `[mapfile]` at `subject.sh:7`, and no other
rule fires.

**Mutation targeted**: the original per-line quote reset. The pre-FR-138 gate
accepts this tree.

**Why the hazard is on the last line**: placed mid-file, a partial fix that
recovers only part of the swallowed tail still reaches it and the fixture passes
without full recovery. The assertion has to sit where only reaching end of file
gets to it.

---

### 1b - The same lookalike inside `$( ... ' ... ' )`

**Steps**

Read case 2 of the same script. Its fixture is
`JOBS="$(ruby -e '` … `hosting << job_name` … `' )"` followed by
`declare -A tail_of_file=()`.

**Expected result**

The gate rejects the tree, reports `[associative-array]` at `subject.sh:9`, and
no other rule fires.

**Mutation targeted**: a lexer that carries `in_single`/`in_double` across lines
but does not model command substitution — that is, the fix FR-138 literally asked
for. Quoting resets inside `$( )`, so the `'` after `-e` opens a single-quoted
region; a flat tracker reads it as an ordinary character inside double quotes and
never enters that state. **This fixture is separate from 1a on purpose**:
they exercise different state, and the shape here is the one that survives the
obvious fix.

---

### 1c - An apostrophe inside a double-quoted string is not a quote opener

**Steps**

Read case 3. Its fixture is `echo "one file's worth of output"` followed by
`wait -n`.

**Expected result**

The gate rejects the tree, reports `[wait-n]` at `subject.sh:4`, and no other
rule fires.

**Mutation targeted** — and this fixture is the exception to the pattern above.
Its target is **not** the original defect: the pre-FR-138 scanner had no
cross-line state to corrupt, so it rejects this tree too, and this fixture does
not discriminate against it. It targets the mistake the *replacement* is most
likely to make, and the one the first draft of `shell_lexer.rb` did make —
treating `'` as an opener even inside `"`. Verified during governance by patching
`shell_lexer.rb` to drop the `&& quote.nil?` guard: the mutated lexer **accepts**
this fixture, while fixtures 1a and 1b still pass against it.

**Why the fixture has exactly one apostrophe**: give it a partner anywhere later
in the file and the bogus single-quoted region closes, the hazard comes back into
view, and the fixture passes against the very bug it exists to catch.

---

## Scenario 2: A file that ends inside a here-document is a finding

**Steps**

Read case 4. Its fixture opens `<<NEVER_CLOSED` and ends without a terminator.

**Expected result**

Two assertions:

1. The gate reports `[unclosed-heredoc]` at `subject.sh:3` — the **opener's**
   line, not end of file — and no other rule fires.
2. The diagnostic text contains `NEVER_CLOSED`.

The second is not decoration. A finding that says "this file ends inside a
here-document" without naming the terminator leaves a reader unable to tell a
genuinely unterminated body from a lookalike inside quoting, which are different
repairs.

**Mutation targeted**: absence of the backstop. This is the assertion that does
not depend on the lexer being correct about anything, so it still fires on escape
shapes nobody has anticipated. Asserted by rule name rather than exit code: a
fixture carrying any hazard exits non-zero whichever rule caught it.

---

## Scenario 3: Negation is a command position, and mentions still are not

**Steps**

Read case 5. Two trees:

- `if ! mapfile -t xs < /dev/null; then :; fi`
- a tree of mentions: `CLASSES="… mapfile …"`, `"$WORK/hazard/mapfile.sh"` with a
  trailing `# mapfile` comment, `${entries[@]+"${!entries[@]}"}`, and
  `[[ ! -f mapfile ]]`

**Expected result**

The first is rejected under `[mapfile]` at `subject.sh:3`, with no other rule
firing. The second is **accepted**.

**Mutation targeted**: `COMMAND_POSITION` listing `not` — not a bash keyword —
while omitting `!`, which is. Both halves are needed: the first alone would be
satisfied by deleting the command-position restriction entirely, which is what
commit `3b5f9eb4` introduced it to prevent.

---

## Scenario 4: Emptiness is matched, not inferred — and the gate still accepts

### 4a - An array emptied in one file and expanded in another

**Steps**

Read case 6. Two files: `lib/shared.sh` containing `shared_args=()`, and
`consumer.sh` which sources it and runs `printf '%s\n' "${shared_args[@]}"`.

**Expected result**

The gate rejects the tree, reports `[empty-array-expansion]` at `consumer.sh:4`,
and no other rule fires.

**Mutation targeted**: per-file emptiness inference. FR-138 removed the inference
rather than extending it across the source graph, so this passes by construction
now — every unguarded value expansion is a finding regardless of where the array
was emptied. The fixture stays because it is what pins this direction shut if
anyone reintroduces inference; without it, a future "optimisation" that only
flags arrays emptied nearby would reopen the hole silently.

---

### 4b - The gate still has an accepting state

**Steps**

Read case 7. Its fixture uses the guarded form
`${args[@]+"${args[@]}"}`, the length expansion `${#args[@]}`, and a
here-document body containing `mapfile`.

**Expected result**

The gate accepts the tree.

**Why**: a gate that rejects everything passes every fixture above and is useless. This
also re-asserts that `${#a[@]}` stays exempt (measured safe under bash 3.2 in
FR-135) and that here-document bodies are still treated as data.

---

## Scenario 5: Every line of every tracked file is accounted for

**Steps**

```bash
ruby scripts/qa/bash32-compat.rb --coverage-census
```

Output is one record per file: `file total scanned heredoc last`. Case 8 of the
QA script asserts three things over it.

**Expected result**

1. The census covers every file `--list-files` reports — 99 at closure, and
   whatever `git ls-files '*.sh' | wc -l` says on any later run. Derived from git,
   not from a roster, and deliberately not pinned here: QA 184 pinned `95` and it
   was wrong three FRs later while every other expectation still held.
2. For every file, `scanned + heredoc == total`.
3. For `test-qa-gate-surface.sh` and `test-bash32-compat.sh`, `last == total`.

**On assertion 2 being real rather than an identity**: `heredoc` is counted by
the lexer as it drops lines, never computed as `total - scanned`. Derived, this
invariant would hold for a lexer that gave up at line one, and the check would
certify an accounting it never performed. The first draft of the census made
exactly that mistake and was corrected during governance.

Measured rather than argued. Patching `shell_lexer.rb` to stop scanning after
line 200 — the FR-138 defect's exact shape, applied to every file at once:

| census `heredoc` | files failing `scanned + heredoc == total` | `last == total` catches it |
|---|---|---|
| counted by the lexer | **35** | yes |
| derived as `total - scanned` | **0** | yes |

Derived, the invariant certifies a scanner that dropped 35 files' tails. That is
why both assertions are here: 2 is the one that would have been a proxy, and 3 is
the one that observes truncation directly under either spelling.

**On assertion 3 rather than `heredoc == 0`**: both named files are QA wrappers
that write their fixtures from here-documents, so dropping lines is correct
behaviour for them. The question is whether the scan came back out again. Under
the defect these read `993` of `1353` and `369` of `385`.

**Why the two files are named explicitly**: assertion 2 holds for a lexer that
truncates and miscounts consistently. These two are the actual regression
targets.

---

## Recorded measurement

Taken over all 98 tracked shell files during governance, differing only in which
lexer and which emptiness rule is in play:

| | lines scanned | findings |
|---|---|---|
| old lexer, inference rule | 18275 | 0 |
| new lexer, inference rule | 18629 | 1 |
| new lexer, match rule | 18629 | 43 |

Both numbers moving in the middle row is what distinguishes a fixed defect from a
changed definition of "finding". The single finding exposed is
`scripts/qa/test-qa-gate-surface.sh:1307`, a bare `"${TARGETED[@]}"` with
`TARGETED=()` assigned at line 876 — so the swallowed tail was not empty, and
FR-138's own "currently latent" assessment had expired by the time it was
governed.

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | The scan reaches end of file past every shape that used to end it early | ☑ PASS | 2026-07-27 | Claude |
| 2 | A file that ends inside a here-document is a finding naming its terminator | ☑ PASS | 2026-07-27 | Claude |
| 3 | Negation is a command position, and mentions still are not | ☑ PASS | 2026-07-27 | Claude |
| 4 | Emptiness is matched not inferred, and the gate still accepts | ☑ PASS | 2026-07-27 | Claude |
| 5 | Every line of every tracked file is accounted for | ☑ PASS | 2026-07-27 | Claude |

## Certification Conditions

A run counts as closure evidence only when `git status --porcelain` is empty at
start and at end, `git rev-parse HEAD` matches across the run, nothing else is
writing to the repository while it runs, and each script's final summary line is
present in its log. Invoke as `bash script > log 2>&1` and read `$?` directly;
piping into a pager reports the pager's status and masks a failed script.

## Related gates

- `scripts/qa/test-bash32-compat.sh` — the executed half; must report
  `0 skipped` on a macOS host.
- `scripts/qa/test-qa-gate-surface.sh` — asserts this script is registered in
  `config/governance/qa-gate-surface.json` and wired into a CI job.
