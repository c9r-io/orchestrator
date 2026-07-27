---
lifecycle: active
related_fr: FR-144
---

# DD-154: A gate that cannot read its input must not report PASS

**Status**: Released
**FR**: FR-144
**QA**: `docs/qa/orchestrator/192-jq-status-observed.md`

## The problem

`test-qa-gate-surface.sh` is the enforcement surface for every governance gate
FR-127 onwards. On 2026-07-27 it reported

```
FR-127 gate surface: 13 passed, 0 failed
```

over a manifest it could not parse. One entry had been written
`"providerIsolation": "no-provider"` where the schema requires
`{"mode": "no-provider"}`. jq exited 5 with `Cannot index string with string
"mode"`, the loop reading it saw zero rows, its body never ran, and the check
returned success.

The typo was mine, made while adding two entries during FR-140. It was not found
by review or by an audit. It was found because the gate's own negative fixtures
failed — six of them — while the gate itself passed. **When a gate and its
fixtures disagree, the fixtures are right.**

## Why nobody saw it

```bash
while IFS=$'\t' read -r path mode evidence; do
  ...
done < <(jq -r '<query>' "$manifest")
```

A process substitution's exit status is not the loop's, and the loop's is not
the function's. `set -euo pipefail` does not reach inside `< <(…)`. Running zero
times and running thirty-four times and passing are indistinguishable from the
outside.

### The second channel, which the FR did not mention

Blaming process substitution is not enough. Every check is invoked as

```bash
"$check" "$root" || return 1
```

— *condition position*, which disables `set -e` for the entire call tree beneath
it. Measured directly:

| how the same function is called | what happens when jq fails |
|---|---|
| condition position (`check … \|\| return 1`) | jq errors, `declared=''`, the check continues, returns **0** |
| bare, with `set -e` live | jq errors, the script exits **5** |

So `declared="$(jq -r … | LC_ALL=C sort)"` in `check_surface_complete` was
equally silent — twice over, since the pipe already replaced jq's status with
`sort`'s.

### Direction decides the consequence, which is why emptiness must be declared

The two silences look identical in code and behave in opposite ways:

| where | empty result means | falls |
|---|---|---|
| `check_surface_complete` | every file on disk looks unclassified | **closed** — loud, safe, but blames the repository for a broken manifest |
| `check_provider_isolation` | the loop body never runs | **open** — the gate certifies an isolation it never checked |

Nothing at the call site distinguishes them. That is the entire argument for
requirement 2, and it is why `gate_jq_rows` has **no default**: the caller
writes `require-rows` or `allow-empty`, or it is an error. A default is a way to
forget, and forgetting is the defect.

Both meanings are live in one manifest today — `staleClaimExemptions` is present
and empty, which is the best state it can be in; `enforcement == "ci-required"`
selects 34, and zero would be impossible.

## What FR-144 itself got wrong

The FR was written on the spot by whoever hit the bug, and never rechecked. It
counted the text `done < <(jq`.

| gate | FR implied | measured |
|---|---|---|
| `test-docs-publishing-integrity.sh` | 1 | **22** |
| `test-qa-gate-surface.sh` | 13 | 13 |
| `test-ci-environment-parity.sh` | 1 | 2 |
| `test-markdown-link-integrity.sh` | 1 | 1 |
| `test-slack-live-certification.sh` | 1 | 1 |
| **total** | **17** | **39** |

The defect is not textual — it is whether the feed can *reach* jq. Six functions
(`collection_langs`, `collection_names`, `declared_gaps`, `published_sorted`,
`authored_slugs`, `in_scope_gates`) run jq one call deeper. The FR named
`test-qa-gate-surface.sh` as the epicentre and listed the actual worst gate as
though it had a single site.

It also named three failing fixtures; six fail, and one of them belongs to a
second check that reads the same field.

This is the `§4.4` shape-2 error — a cheap proxy standing in for the fact under
test — committed inside the report whose subject is exactly that. Worth stating
plainly rather than quietly correcting: a reproduction written at the moment of
discovery is evidence that the defect exists, not a measurement of its extent.

## Design

### `scripts/lib/gate_jq.sh`

```sh
rows="$(gate_jq_rows require-rows "$manifest" '<query>')" || return 1
while IFS=$'\t' read -r a b; do … done <<< "$rows"
```

Assignment from a command substitution carries the status, which is what makes
this observable in condition position where `set -e` is not. A here-string
rather than a pipe, so the loop body stays in the current shell and the check's
`rc=1` accumulation still works.

jq writing to stderr on an otherwise successful run is also treated as a
failure, rather than merged into the rows where a warning would be
indistinguishable from data.

### The failure record, and why capture-and-test was not sufficient

Converting call sites fixes the sites converted. It cannot fix a read that
happens *inside* a process substitution several loops deep, because the
subshell's status has nowhere to go — the original defect wearing a different
hat. `test-docs-publishing-integrity.sh` reads its policy four loops deep.

So a failed read also appends to a file. A subshell cannot return a status to
its parent, but it can write, and the gate asks once at the end. Measured on a
policy with a type error injected into `.sources`:

```
PASS: check_policy_fresh
...
FAIL: 2 JSON read(s) failed during this run
```

**`check_policy_fresh` reports PASS on a policy it could not read.** Its own
status is 0 because the failure happened where a status cannot travel. The
run-level record is the only thing between that and a green gate. This settles
whether the mechanism earns its place, and it is the strongest single piece of
evidence in the FR.

The record also covers reads nobody converted, including ones written after this
document, which a per-call-site fix does not.

### `scripts/qa/jq-status-observed.rb`

Three rules over every ci-required shell gate and the shared libraries, with the
scanned set derived from `qa-gate-surface.json` rather than listed:

| rule | what it rejects |
|---|---|
| `unobserved-feed` | `done < <(jq …)` — always convertible, so always a finding |
| `unrecorded-feed` | `done < <(fn …)` reaching jq, in a file that keeps no failure record |
| `status-dropped-by-pipe` | `$(jq … \| …)` — unobservable however carefully the caller tests it |

It **parses** with `scripts/lib/shell_lexer.rb` rather than grepping. This is not
fastidiousness: the documents describing this FR quote the forbidden pattern by
necessity, and so do the fixtures. A grep would flag them and the natural fix
would be to stop writing the pattern down, which teaches nothing and hides
the rule.

## What the fixtures caught that review did not

Three defects in this change were found by running it, not by reading it. They
are recorded because the pattern — the author's own reasoning being the thing
under test — is the reason the fixtures are shaped as they are.

1. **`gate_jq_rows` did not observe jq's status where `set -e` was live.** It was
   written `rows="$(jq …)"; status=$?`, which reads correctly only where `set -e`
   is already suppressed. In a process substitution the assignment trips ERR and
   the shell leaves before `status` is consulted — so the failure record was
   never written. The FR's own defect, reproduced inside its fix, caught by the
   fixture for the record.
2. **The scanner's reachability map treated "asked and answered no" as yes**,
   flagging `extract_links` (awk) and `bundle_providers` (ruby). A scanner
   reporting defects that are not there is worse than the silence it replaces.
3. **The fixture script tripped two of the repository's own rules** — its
   pass/fail messages quoted the forbidden pattern inside double quotes, which
   the lexer scans as code by design, and it used `python3`, which the governance
   job does not provide. Both were reported by the real gates.

## Accepted costs

- **Verbosity at the call site.** `rows="$(gate_jq_rows require-rows …)" ||
  return 1` plus `<<< "$rows"` is three lines where one used to do. The
  alternative is a reader that guesses what an empty result means.
- **A temporary file per failed read**, for jq's stderr. Removed on every path.
- **Two more CI steps.** Recorded in `ci-step-cost.json` as `pendingMeasurement`
  with reasons, so the FR-140 budget suspends itself and says so until the next
  refresh measures them.

## Known limits

- **Double-quoted strings are scanned as code.** `shell_lexer.rb` blanks
  single-quoted regions and not double-quoted ones, because shell still expands
  in the latter — the distinction FR-138 exists to respect. A *message* that
  quotes the forbidden pattern in double quotes is therefore a finding. That is
  a real false positive, it hit this FR's own fixture script, and the workaround
  is to reword the message. Narrowing the rule to exclude `pass`/`fail`
  arguments would reintroduce a text heuristic in the middle of a parse.
- **Function-body extraction is brace-at-column-zero.** Every shell file here is
  written that way. A missed body can only cause a false negative in the
  reachability map, never a false positive, because `done < <(jq …)` is matched
  directly.
- **Scope is ci-required shell gates and `scripts/lib`.** Roughly 85 further
  `$(jq …)` captures live in daemon and integration QA scripts. They were
  examined and left: they feed an assertion on the captured value, so an empty
  result fails the assertion rather than skipping it — they fail closed. If that
  stops being true the scope should widen, and the rule for widening it is the
  fail-open/fail-closed distinction above, not the file's directory.
- **The record is per process, not per check.** It reports that *some* read
  failed during the run and names the file and query, not which check was
  examining nothing at the time. Attributing it per check would mean threading
  state through four levels of subshell, which is the problem, not the solution.
