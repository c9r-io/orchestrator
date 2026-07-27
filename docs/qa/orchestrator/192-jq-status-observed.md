---
lifecycle: active
related_fr: FR-144
---

# Orchestrator - JSON Reads Observe Their Exit Status

**Module**: CI / Governance
**Scope**: `scripts/lib/gate_jq.sh`, the scanner `scripts/qa/jq-status-observed.rb`, and the
conversion of 39 jq-reachable loop feeds across the five gates that read JSON
**Scenarios**: 5
**Priority**: Medium

## Background

`test-qa-gate-surface.sh` printed `13 passed, 0 failed` over a manifest jq could
not parse. One entry read `"providerIsolation": "no-provider"` where the schema
requires `{"mode": "no-provider"}`; jq exited 5, the loop reading it saw zero
rows, and the check returned success.

Design record: `docs/design_doc/orchestrator/154-jq-status-observed.md`.
Sibling: `docs/qa/orchestrator/191-governance-execution-cost.md` covers the FR
during which this defect was introduced and found.

**Safety**: read-only against the working tree. Scenarios build scratch git
clones under `$TMPDIR`; no daemon starts, no database is touched, no provider is
invoked, and nothing contacts the network. Safe to run against this repository.

## Why the assertions are shaped the way they are

**The judge is the real gate, not the fixture suite.** This defect's entire
signature is that the two disagreed: the fixture suite failed six cases while
the gate reported `13 passed, 0 failed` on the same tree. An assertion that only
exercised the fixture harness would reproduce the original mistake exactly.
Scenario 3 therefore runs `test-qa-gate-surface.sh` itself.

**The mutation is a type error, not a deletion.** Deleting an entry is the case
the author has in mind, and it does not make jq exit non-zero — it yields fewer
rows, which every check already handles. Retyping an object to a string is the
mutation the implementation is least likely to catch, and it is the one that
actually happened.

**Every "must fail" case is paired with a "must still pass" case.** A reader that
rejects everything satisfies half of these on its own.

---

## Scenario 1: The reader fails loudly, and says which file and why

**Steps**

```bash
bash scripts/qa/test-jq-status-observed.sh
```

Read the control and cases 1–4.

**Expected result**

- Control: a well-formed read returns its rows and succeeds.
- Case 1: a type error fails, and the diagnostic contains **both** the file path
  and jq's own text (`Cannot index string with string`).
- Case 2: `require-rows` fails on an empty result.
- Case 3: `allow-empty` **passes** on an empty result and iterates zero times.
- Case 4: a read that declares no emptiness at all is an error.

**Mutation targeted**: the exit code alone is a proxy — a crashed interpreter
produces a non-zero status too. Asserting jq's own diagnostic is what
distinguishes "this file is malformed" from "something went wrong".

*What case 2 would still pass on*: a reader that rejects every result. Case 3 is
the condition that rules it out, and it is not decoration —
`staleClaimExemptions` is legitimately empty in this repository today, so a
reader without `allow-empty` would force somebody to keep an exemption alive
purely to keep a gate quiet.

---

## Scenario 2: A failure inside a process substitution still leaves a record

**Steps**

Read case 5 of the same script.

**Expected result**

A `gate_jq_rows` call that fails inside `< <(…)` — where a non-zero return has
nowhere to go — still increments `gate_jq_failure_count` in the parent.

**Mutation targeted**: this is the mechanism the per-call-site fix cannot
provide, and the case that proves it works. It also found a real defect during
implementation: `gate_jq_rows` originally read jq's status as
`rows="$(jq …)"; status=$?`, which only works where `set -e` is already
suppressed. Called where it is live, the assignment tripped ERR and the shell
left before the record was written — the FR's own defect reproduced inside its
fix. Nothing else in the suite would have caught it.

---

## Scenario 3: The real gate rejects a manifest it cannot parse

**Steps**

Read case 6, then reproduce it directly:

```bash
git clone -q . /tmp/fr144 && cd /tmp/fr144
# retype one providerIsolation object to a string
ruby -rjson -e 'd=JSON.parse(File.read(ARGV[0])); \
  d["scripts"].find{|s| s["providerIsolation"].is_a?(Hash)}["providerIsolation"]="no-provider"; \
  File.write(ARGV[0], JSON.pretty_generate(d)+"\n")' config/governance/qa-gate-surface.json
bash scripts/qa/test-qa-gate-surface.sh; echo $?
```

**Expected result**

The gate exits non-zero and its output names the manifest and quotes jq's
diagnostic. At `cedbef41` the same tree produced `13 passed, 0 failed` and
exit 0.

**Why not run through `expect_fail`**: the FR-127 convention requires a fixture
to isolate to exactly one check. It cannot here — `providerIsolation` is read by
both `check_provider_isolation` and `check_provider_stub_coverage`, so the type
error trips two. Asserting isolation would be asserting something false, so the
claim made is the one that holds: the gate as a whole rejects the tree.

---

## Scenario 4: The scanner parses, and is not a grep

**Steps**

Read cases 7–11b.

**Expected result**

- Case 7: the repository as it stands passes.
- Case 8: a reintroduced `done < <(jq …)` is a finding naming file and line.
- Case 9: **the same line inside a comment is not a finding.**
- Case 10: **the same line inside a here-document body is not a finding.**
- Case 11: `$(jq … | …)` is a finding.
- Case 11b: a reader captured without testing its status is a finding, **and** a
  correctly written multi-line reader whose `||` sits past a backslash
  continuation is not.

**Mutation targeted**: cases 9 and 10 are the ones that separate a parse from a
`grep`, and they are not hypothetical — DD-154 and this document both quote the
forbidden pattern, and so does the fixture script. A grep-based scanner passes
case 8 and fails case 9, and the natural way to silence it would be to stop
writing the rule down.

Case 11b is paired for the same reason in the other direction. The rule it
guards is the one the fix invites against itself — copy a call, drop the
`|| return 1` — but almost every correct call in this repository spans lines
with the `||` past a continuation. A rule judging the opening line alone passes
the first half of 11b and fails the second, and a rule that flags correct code
gets switched off long before it catches anything.

*What the scanner would still pass on*: a double-quoted **message** containing
the pattern is flagged, because `shell_lexer.rb` scans double-quoted regions as
code by design (shell expands there). This is a known false positive, recorded
in DD-154; it hit this FR's own fixture script and was fixed by rewording.

---

## Scenario 5: Coverage follows the manifest, not a list

**Steps**

Read case 12.

**Expected result**

Registering a new `ci-required` shell gate grows the scanned set by exactly one,
and the new path appears in `--list-files`, with no edit to the scanner.

**Mutation targeted**: §4.4 shape 2 — a hand-listed scope guards exactly what was
known the day it was written. The tell is a list that grows by one entry per
audit round.

---

## Recorded measurement

Taken during governance at `905909ff`, macOS, system Ruby 2.6.

**The defect, before and after** — same tree, same mutation:

| | at `cedbef41` | after |
|---|---|---|
| `test-qa-gate-surface.sh` on a malformed manifest | `13 passed, 0 failed`, **exit 0** | `11 passed, 2 failed`, **exit 1** |
| diagnostic | none | names the manifest, quotes jq |

**The run-level record earning its place** — a type error injected into
`.sources` of `docs-publishing.json`:

```
PASS: check_policy_fresh          <- reports PASS on a policy it could not read
...
FAIL: 2 JSON read(s) failed during this run
```

**Scope, measured rather than counted from the FR:**

| | FR-144 as filed | measured |
|---|---|---|
| jq-reachable loop feeds | 17 | **39** |
| worst single gate | `test-qa-gate-surface.sh` (13) | `test-docs-publishing-integrity.sh` (**22**) |
| failing fixtures on the malformed manifest | 3 | **6** |

**Assertion counts, all five gates** — the FR's own criterion that nothing
regresses:

| suite | before | after |
|---|---|---|
| `test-qa-gate-surface.sh` | 13 | 13 |
| `test-qa-gate-surface.sh --fixture-test` | 34 | 34 |
| `test-docs-publishing-integrity.sh --fixture-test` | 20 | 20 |
| `test-docs-publishing-integrity.sh` | 7 | **8** |
| `test-markdown-link-integrity.sh` | 2 | 2 |
| `test-ci-environment-parity.sh` | 1 | 1 |
| `test-slack-live-certification.sh` | 13 | 13 |

The one increase is the new run-level assertion. No count fell.

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | The reader fails loudly, and says which file and why | ☑ PASS | 2026-07-28 | Claude |
| 2 | A failure inside a process substitution still leaves a record | ☑ PASS | 2026-07-28 | Claude |
| 3 | The real gate rejects a manifest it cannot parse | ☑ PASS | 2026-07-28 | Claude |
| 4 | The scanner parses, and is not a grep | ☑ PASS | 2026-07-28 | Claude |
| 5 | Coverage follows the manifest, not a list | ☑ PASS | 2026-07-28 | Claude |

## Certification Conditions

A run counts as closure evidence only when `git status --porcelain` is empty at
start and at end, `git rev-parse HEAD` matches across the run, nothing else is
writing to the repository while it runs, and each script's final summary line is
present in its log. Invoke as `bash script > log 2>&1` and read `$?` directly;
piping into a pager reports the pager's status and masks a failed script.

Scenario 3 clones the working tree and overlays the working-tree copies of the
gate and its library. A run that omits the overlay certifies the previous commit
rather than the change under test.

## Related gates

- `scripts/qa/test-qa-gate-surface.sh` — the gate this defect silenced; asserts
  the new scripts are registered and wired into a CI job.
- `scripts/qa/bash32-compat.rb` — `gate_jq.sh` must stay bash 3.2 clean; the
  scanned set is `git ls-files '*.sh'`, which includes `scripts/lib`.
- `scripts/qa/ci-cost.rb` — carries the two new steps as `pendingMeasurement`
  until CI measures them.
