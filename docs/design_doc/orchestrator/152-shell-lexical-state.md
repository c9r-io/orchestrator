---
lifecycle: active
related_fr: FR-138
---

# DD-152: Shell Lexical State And The Scanner's Coverage Surface

**Module**: CI / Governance
**Status**: Implemented (FR-138)
**Related Plan**: FR-138
**Related QA**: `docs/qa/orchestrator/190-bash32-scanner-lexical-state.md`
**Related**: DD-146 (bash 3.2 compatibility, the gate this corrects), DD-134-era
`scripts/lib/rust_lexer.rb` (the same failure on the Rust side), DD-139 (QA gate
enforcement surface)

## The defect

`scripts/qa/bash32-compat.rb` decided quoting one line at a time: `strip_comment`
reset `in_single` / `in_double` at the start of every line. A quoted region
opened on one line and closed on another was therefore read as code, and if that
region contained anything shaped like `<< WORD`, the scanner believed a
here-document had begun. It then dropped every subsequent line until one matched
the terminator — which never arrived — so **the rest of the file left the scan
and no diagnostic was produced**.

Two files were escaping when FR-138 was filed:

| File | Opener | Lines dropped | Trigger |
|---|---|---|---|
| `scripts/qa/test-qa-gate-surface.sh` | 993 | 360 | `<<EOF` inside a `perl -e` replacement string |
| `scripts/qa/test-bash32-compat.sh` | 369 | 16 | `hosting << job_name` inside a `ruby -e` program |

The second is worth remembering. That `ruby -e` block is case 9 of the bash 3.2
gate's own wrapper — the assertion that some CI job runs the gate on a macOS
host, without which a green ubuntu run would mean the executed half had been
skipped everywhere and nobody would know. It is written in ruby, and ruby's
array-append operator is `<<`. **The line proving the gate has a bash 3.2 host is
the line that hid the gate's own last 16 lines from it.** Embedding an
interpreter in a shell wrapper is this repository's ordinary idiom, so this is a
path that recurs rather than a coincidence.

This is the same failure `scripts/lib/rust_lexer.rb` was written for one FR
earlier: `strip_test_modules` counted braces per line, a `{` inside a string
literal left a `cfg(test)` block open forever, and the production code after it
disappeared from the scan. FR-135 reintroduced the per-line approximation on the
shell side seven commits later. DD-146's Known Limits recorded that the comment
scanner tracked quoting per line, but followed the consequence only as far as
comment detection — which over-reports, and is therefore visible. The
here-document consequence under-reports, and is not.

## What the FR got wrong, and why it matters

FR-138 attributed both escapes to missing cross-line quote state. Implementing
exactly that — carrying `in_single` / `in_double` across lines — fixes
`test-qa-gate-surface.sh` and leaves `test-bash32-compat.sh` **still truncated**.
This was measured, not reasoned about.

The chain at the second site begins at line 359:

```bash
MACOS_JOBS="$(ruby -ryaml -e '        # opens a DOUBLE-quoted region
  ...
  next unless "#{runners} #{matrix}".include?("macos")   # line 366
  ...
  hosting << job_name                 # line 369
' "$REPO_ROOT/.github/workflows/ci.yml" ...)"
```

Quoting resets inside `$( )`, so that `'` genuinely opens a single-quoted region.
A flat two-boolean tracker sees an apostrophe inside double quotes, calls it an
ordinary character, and never enters the single-quoted state at all. It then
derails completely at line 366: the second `#` follows a space with `in_double`
already flipped false by the string's opening quote, so the remainder of the line
is discarded as a comment and quote parity is lost outright. By line 369 the
tracker believes it is at top level, and `hosting << job_name` reads as a
here-document opener.

Modelling command substitution as a nested quoting context is therefore not an
embellishment — FR-138's own acceptance criterion, that every line of
`test-bash32-compat.sh` enters the scan, is unreachable without it.

## Design

### `scripts/lib/shell_lexer.rb`

Cross-line lexical state, at the same magnitude as `rust_lexer.rb`. Not a parser:
it models quoting, comments and here-document extent, and nothing else needs
more.

**A stack of quoting contexts, not two booleans.** `$(` pushes a context whose
quote state starts fresh; the matching `)` pops it. `$((` is arithmetic and
balances inside the current context. This is the piece a naive port of
`rust_lexer.rb` does not have, and the one the second escape site requires.

**Single-quoted regions are blanked; double-quoted regions are not.** This is
where shell parts company with Rust, and reversing it breaks the gate outright:
the canonical hazard is `printf '%s\n' "${args[@]}"`, whose dangerous half lives
*inside* double quotes, because shell still expands there. In Rust every string
literal is inert and `rust_lexer.rb` can blank them all. Here only single-quoted
regions are inert. This is why the Rust lexer is referenced rather than reused.

**An apostrophe inside a double-quoted string is an ordinary character.** Stated
explicitly because the first draft of this lexer got it wrong, and the failure
mode is quiet: every quote after it is mispaired, the rest of the file is blanked
as one long single-quoted region, and the gate reports a clean tree. Case 3 of
the QA script exists for exactly this mutation.

### `unclosed-heredoc`, the backstop

A file that ends while still inside a here-document is now a finding in its own
right, reported at the opener and naming the terminator it waited for. Whether
the cause is a genuinely unterminated body — a broken script either way — or a
lookalike inside quoting the lexer misread, the honest report is the same:
everything after this line was dropped, so the rest of the file is unchecked.

This matters beyond the two known sites. It is the one assertion that does not
depend on the lexer being correct, so it still fires on the next escape shape
nobody has thought of.

### The coverage census

`--coverage-census` emits `file total scanned heredoc last` per file. Two
deliberate choices:

- **`heredoc` is counted by the lexer as it drops lines, never derived from
  `total - scanned`.** Derived, the invariant `scanned + heredoc == total` is an
  identity — true of a lexer that gives up at line one — and the check would
  certify an accounting it never performed. The first draft of the census made
  this mistake.
- **`last` is what actually catches truncation.** A scan that stops early leaves
  it below `total`. Under the defect the two named files read 993/1353 and
  369/385.

Measured, not argued: patching the lexer to stop after line 200 breaks the sum
for **35** files when `heredoc` is counted and for **0** when it is derived,
while `last == total` catches it either way. The derived spelling would have
certified a scanner that dropped 35 files' tails.

`last == total` is asserted for the two named files rather than for all of them,
because a file legitimately ending with a here-document terminator has
`last < total` — `check-linux-x86-rlimit.sh` does. The cost is that appending a
here-document to the very end of either named file would fail this assertion
spuriously. That is a loud failure with an obvious cause, and preferable to
weakening the check to something both spellings satisfy.

The census exists because the FR-138 defect happened *while the gate was green*.
"The gate passes" cannot be evidence that the gate reads whole files, because a
truncated scan is precisely the state it was passing in. Exit codes are satisfied
by the broken state; line accounting is not.

### Emptiness: inference replaced by matching

`emptyable_arrays` inferred which arrays could be empty by looking for `name=()`
or `name=("$@")` **in the same file**. That rule was wrong in both directions and
only one direction was visible:

- it over-reported where a guard had already proved the array non-empty
  (recorded in DD-146), and
- it silently missed arrays emptied in a `source`d library and expanded in the
  caller — `scripts/lib/` holds exactly such libraries.

Both come from one rule, so FR-138 removed the rule rather than extending it
across the source graph. Every value expansion not written in the canonical
guarded form is now a finding: 43 sites across 16 files, rewritten mechanically.
The rule drops from inference to match, which leaves no inference surface to
route around, and DD-146 already recorded the repository's position — a gate
subject was rewritten into the guarded form despite qualifying for an exemption,
"since the guarded form costs nothing".

### Negation is a command position

`COMMAND_POSITION` listed `not`, which is not a bash keyword, and omitted `!`,
which is (`compgen -k` confirms both). `if ! mapfile -t xs < f` was missed while
the impossible `not mapfile` was caught — a candidate that could never match,
reading to anyone who checked as though negation were covered. `!` is now in the
punctuation class and `not` is gone. Measured repo-wide: zero new false
positives, and the mention forms the rule was introduced to suppress
(`$WORK/hazard/mapfile.sh`, word lists, comments, `${!entries[@]}`,
`[[ ! -f mapfile ]]`) are still not reported.

## The evidence that this is a fix, not a changed yardstick

Three measurements over the 98 tracked shell files as they stood before this FR
added its own QA script, differing only in which lexer and which emptiness rule
is in play:

| | lines scanned | findings |
|---|---|---|
| old lexer, inference rule | 18275 | 0 |
| **new lexer**, inference rule | **18629** | **1** |
| new lexer, **match rule** | 18629 | 43 |

The middle row is the load-bearing one. Both numbers move together, which is
FR-130's "ruler before measurement" discriminator: a scanner that merely changed
its definition of a finding would move the second number alone. The one finding
it exposed is real — `test-qa-gate-surface.sh:1307` expands `"${TARGETED[@]}"`
bare, with `TARGETED=()` assigned at line 876.

That also corrects FR-138's own claim that the escapes were latent. They were
when the FR was written; by the time it was governed the swallowed tail had grown
by 108 lines and acquired a violation of the gate's own policy. It did not break
CI only because `test-qa-gate-surface.sh` runs in the ubuntu-only `governance`
job — under `/bin/bash` on macOS it is precisely the failure FR-135 existed to
eliminate.

## Accepted costs

- The guarded form is noisier, and now applied everywhere rather than where an
  array is provably emptyable. Unchanged in spirit from DD-146; only the scope
  grew, from inferred sites to all sites.
- `$( )` context tracking is more machinery than a two-boolean tracker, and its
  paren matching is approximate for pathological input. The `unclosed-heredoc`
  backstop is what makes that acceptable: when the lexer is wrong about extent,
  the file still fails rather than passing quietly.

## Known limits

- **A builtin name in command position inside an ordinary double-quoted string
  is a finding.** Double-quoted text is live code to the scanner — it has to be,
  since `"${a[@]}"` is the canonical hazard — so `echo "if ! mapfile"` reports.
  The class predates FR-138 (`"x; mapfile"` matched via the `;` in the existing
  punctuation set); adding `!` widened it slightly. This is why both bash 3.2 QA
  wrappers write their fixtures as here-document bodies and spell hazards in prose
  with a separator, and `test-bash32-lexer.sh` hit it during governance and says
  so at the line. Distinguishing mention from invocation here needs per-character
  quote annotation rather than per-line text, which is more than the seven
  construct rules are worth.
- **`unclosed-heredoc` is an eighth rule, not an eighth class.** DD-146's count
  of seven refers to bash 3.2 constructs and is unchanged; this one reports that
  the scan of a file was incomplete, which is a fact about the scanner rather
  than about bash. `test-bash32-compat.sh` still executes exactly seven classes
  under the real interpreter.
- **Single-quoted regions are treated as inert.** A hazard inside `bash -c
  '...'` or `eval '...'` is not reported. This was true before FR-138 as well —
  the old `COMMAND_POSITION` did not match after an opening quote either — but it
  is now a deliberate consequence of blanking rather than an accident of pattern
  shape.
- **The lexer models `$( )`, not backticks.** ``` `...` ``` command substitution
  does not push a context. No tracked file uses it in a shape that matters, and
  the here-document backstop covers the case where one arrives.
- **Paren matching inside command substitution is positional.** An unbalanced `)`
  inside an unquoted region can pop a context early. The census would show the
  resulting truncation and `unclosed-heredoc` would fire if it reached end of
  file inside a here-document.
- **Ruby 2.6 is the floor.** macOS system ruby is 2.6 and the `coverage-policy-fixtures`
  macOS leg is the only host in CI where bash 3.2 semantics are real, so endless
  method definitions and later syntax are unavailable to anything on this path.
- The bullets DD-146 recorded about `.github/workflows/**` `run:` blocks being
  outside the scanned set, and about the seven classes not being exhaustive, are
  unchanged by this FR.
