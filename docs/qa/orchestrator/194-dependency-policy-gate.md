---
lifecycle: active
related_fr: FR-133
---

# Orchestrator - A Dependency Policy Must Still Bind

**Module**: CI / Governance / Dependencies
**Scope**: `deny.toml`, `.cargo/audit.toml`, the `cargo-deny` job in
`security.yml`, and `scripts/qa/dependency-policy.rb` with its fixtures
**Scenarios**: 5
**Priority**: Medium

## Background

`cargo audit` answered *is anything here known-bad*. Nothing answered *what
shape is this graph allowed to have* — 48 crates resolve to more than one
version, under 34 distinct licence expressions, and none of it had ever been
decided.

`deny.toml` decides it: 70 accepted extra copies, each naming the dependency
that introduces it. But a policy file is only as good as the invocation that
runs it, and this repository has found that gap four times — FR-127 (wired is
not running), FR-137 (an aggregation nobody guarded), FR-144 (PASS over
unreadable input), FR-143 (a fixture that mutated nothing). So there are two
subjects here, not one.

Design record: `docs/design_doc/orchestrator/156-dependency-policy-gate.md`.

**Safety**: read-only against the working tree. Every case builds a scratch tree
under `$TMPDIR`; no daemon starts, no database is touched, no provider is
invoked. The `--tool-fixtures` half runs `cargo deny check bans licenses
sources` against the repository's real manifest with a mutated *config*, so
nothing in the tree changes; none of the three fetches the advisory database,
which is the half this policy leaves to `cargo audit`.

## Why the assertions are shaped the way they are

**All seven rules are guards with zero violations today.** `sources` was already
clean, no `skip-tree` has ever existed, every severity is already `deny`. That
makes the fixtures the *only* evidence any of them works — the same position
FR-143's `exit-code-only` and `restated-expectation` were in — so every "must
fire" case is paired with a "must not fire" one on the same probe. A rule that
fires on correct input gets switched off long before it catches anything.

**The mutations go through `scripts/lib/gate_fixture.sh`.** Several of them name
a specific line of `deny.toml`, which is a file this gate exists to let people
edit. A fixture whose anchor has moved fails loudly rather than proving nothing.

**Two halves, because they need different things.** The default half needs Ruby
and runs in `ci.yml`'s governance job. `--tool-fixtures` needs the `cargo-deny`
binary and runs in `security.yml`'s job, where it exists. "The flag is present
in the YAML" and "the ratchet ratchets" are different claims, and only the
second one is worth trusting.

---

## Scenario 1: The committed policy passes, three ways

**Steps**

```bash
cargo deny --workspace --all-features check --deny unmatched-skip bans licenses sources
cargo audit --deny unsound
ruby scripts/qa/dependency-policy.rb
```

**Expected result**

- `bans ok, licenses ok, sources ok`, exit 0. The 70 acceptances silence every
  duplicate; `unused-allowed-license = "deny"` means the nine-entry allow list
  and the one exception contain nothing dead.
- `cargo audit` exits 0 with `17 allowed warnings found` — one fewer than before
  this FR, because RUSTSEC-2024-0429 is now an explicit acceptance rather than a
  warning nobody read.
- `Dependency policy: PASS (70 accepted duplicate(s), 0 finding(s))`.

*What this would still pass on*: a policy nobody runs. Scenario 3 is what rules
that out.

---

## Scenario 2: The acceptance list cannot rot

**Steps**

Read cases 15 and 15b of `scripts/qa/test-dependency-policy.sh`, then run

```bash
bash scripts/qa/test-dependency-policy.sh --tool-fixtures
```

**Expected result**

- Case 15: a skip naming `serde@1.0.999` — a version the graph does not have —
  makes `cargo deny check --deny unmatched-skip bans` fail and name it.
- Case 15b: a skip naming `serde@1.0.229` — present in the lock, but not
  duplicated — **passes `cargo deny` cleanly**, and is caught by
  `dependency-policy.rb`'s `skip-is-live` rule and nothing else.
- Case 16: deleting one of the 70 acceptances makes `cargo deny` fail naming
  `base64`. This is the licence-to-fail on a *new* duplicate, exercised without
  waiting for one to appear.
- Case 17 and case 18: removing `MPL-2.0` from `allow` fails naming the five
  crates that carry it, and emptying the allowed-registry list makes every
  package in the graph a `source-not-allowed` finding. `licenses` and `sources`
  would report zero findings whether they were evaluating the real graph or
  nothing at all, and these two cases are the difference.

**Mutation targeted**: 15b is the important one and it is not hypothetical. The
first version of case 15 used `serde@1.0.229` on the assumption that
`unmatched-skip` means "this entry accepts nothing". It does not — it means "the
entry matched no crate in the graph" — and the case failed, which is how the
limit was found. **Neither observer closes the ratchet alone**, and this case is
what holds them apart. Written the other way round, the pair would have looked
redundant and one of them would eventually have been deleted as such.

---

## Scenario 3: The policy still binds

**Steps**

Read cases 2 to 5 and case 10 of the same script, then

```bash
bash scripts/qa/test-dependency-policy.sh
```

**Expected result**

- Case 2: dropping `--deny unmatched-skip` from the run line is a finding.
  Case 2b: the same flag written *after* the check names is not.
- Case 3: adding `advisories` is a finding; 3b: `all` is a finding; 3c:
  `sources bans licenses` is not.
- Case 4: an invocation that has been commented out is a finding. **A grep for
  `cargo deny` is satisfied by that line**; the gate reads the parsed step,
  where a comment is not a command. 4b: `continue-on-error: true` on the deny
  step is a finding.
- Case 5, 5b, 5c: any of the four severities weakened to `warn`/`allow` is a
  finding.
- Case 10: `cargo audit` without `--deny unsound` is a finding; 10b: an ignored
  advisory with no reason above it is a finding; 10c: a missing
  `.cargo/audit.toml` is a finding, **not an empty pass**.

**Mutation targeted**: case 2's control *moves* the flag rather than removing
it, because the drop that actually happens is someone reformatting a long
command, not someone deleting a step. Case 4 is the one that separates this gate
from a grep, and it is not decoration — FR-134 recorded three distinct ways a
text-presence check certified an enforcement that does not run.

---

## Scenario 4: No blanket, and no unreviewed acceptance

**Steps**

Read cases 6 to 9.

**Expected result**

- Case 6: a `skip-tree` entry is a finding. 6b: an empty `skip-tree` is not.
- Case 6c: the words `skip-tree` inside a `reason` string are not a
  `skip-tree` — and `deny.toml`'s own header discusses `skip-tree` at length, so
  the repository itself is the standing proof for the comment half.
- Case 7: an empty `reason` is a finding; 7b: a skip with no `reason` key is a
  finding; case 8: a licence exception whose explaining comment has been removed
  is a finding, because cargo-deny rejects a `reason` key there and the comment
  is the only place a justification can live.
- Case 9, 9b, 9c: a skip for a crate with one version, for a version the lock
  does not have, or for a crate absent from the lock, is a finding each.

**Mutation targeted**: `skip-tree` is the blanket, and a blanket is the mirror
of §4.4 shape 2. A hand-listed set guards only what was known, and its tell is
that it grows by one entry per audit round; a blanket guards nothing at all and
**never produces a line in any log**. One `skip-tree = [{ crate = "tauri" }]`
would absorb 28 of the 48 accepted duplicates and every future one beneath that
tree.

---

## Scenario 5: A scan that read nothing is not a clean scan

**Steps**

Read cases 11, 11b, 12 and 13.

**Expected result**

- A `security.yml` with no jobs **fails**. Every rule above is vacuously
  satisfied over an empty set — §4.4 shape 5, and the reason the FR immediately
  before this one added a case just like it.
- A `Cargo.lock` with no packages fails for the same reason.
- Registering a new gate is reflected without editing the fixture: case 12 reads
  the enforcement manifest rather than a list.
- Case 13: the gate passes against the working tree, so every case above is
  evidence of detection rather than of a broken gate.

---

## Recorded measurement

Derived at `1b5615e2`, macOS, `cargo 1.96.0`, system Ruby 2.6, `cargo-deny
0.20.2` (prebuilt `aarch64-apple-darwin`, sha256 `fe67d82a…`, matching the
published checksum).

| | FR-133 as filed | measured |
|---|---|---|
| dependencies | 443 "transitive" | **443** on the host tree *including* the 14 workspace members → **429**; **653** external in the lock, which is what cargo-deny reads |
| duplicated crates | 37 | **48** as cargo-deny counts them; **25** by name@version on the host; **37** by `cargo tree -d`, twelve of which resolve to one version |
| accepted extra copies | not counted | **70** |
| unifiable by us | implied non-empty | **0** |
| duplicates the GUI alone introduces | not identified | **28** of 48 (`--exclude orchestrator-gui` gives 20) |
| licence expressions | not counted | **34**, zero missing, **1** needing an exception |
| `sources` findings | implied outstanding | **0** — a guard, not a repair |
| advisories | "重叠" | cargo audit **18**, cargo deny **17**, neither set containing the other |

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | The committed policy passes, three ways | ☑ PASS | 2026-07-28 | Claude |
| 2 | The acceptance list cannot rot | ☑ PASS | 2026-07-28 | Claude |
| 3 | The policy still binds | ☑ PASS | 2026-07-28 | Claude |
| 4 | No blanket, and no unreviewed acceptance | ☑ PASS | 2026-07-28 | Claude |
| 5 | A scan that read nothing is not a clean scan | ☑ PASS | 2026-07-28 | Claude |

## Certification Conditions

A run counts as closure evidence only when `git status --porcelain` is empty at
start and at end, `git rev-parse HEAD` matches across the run, nothing else is
writing to the repository while it runs, and each script's final summary line is
present in its log. Invoke as `bash script > log 2>&1` and read `$?` directly.

**The gate set must be derived, not listed** (§4.6.6):

```bash
jq -r '.scripts[] | select(.enforcement == "ci-required") | .path' \
  config/governance/qa-gate-surface.json
```

**And a derived path is not a derived invocation.** This FR's sweep ran all 41
derived paths and one of them, `scripts/qa/certify-slack-managed-live.sh`,
exited 2 having printed its usage: CI runs it as `certify-slack-managed-live.sh
status`, and the manifest records the path, not the argument. Run bare it
asserts nothing, and a sweep that took its exit code at face value would have
reported a failure that is not there — the mirror of the omission §4.6.6 was
written for. The manifest's `note` field is where that fact lives; read it
before concluding anything from a gate's bare exit code.

## What the sweep found that this FR did not cause

Certified at `1272d6c9`: clean worktree at both ends, revision pinned across the
run, gate set derived from the manifest — **46 of 47 green**, the one non-zero
being `ci-liveness.rb`, which is DD-146's known first pass rather than a defect.

An earlier sweep at the same revision reported **45 of 47**, and the extra
failure was real but not in this FR's code: `scripts/qa-doc-lint.sh` said the
CHANGELOG did not name `RunnerExecutorKind`, which it does, at line 74. Ten
isolated re-runs passed. The cause is `printf '%s' "$UNRELEASED" | rg -q P`
under `set -o pipefail`: `rg -q` exits on first match, the 90 KB section is past
the pipe buffer, `printf` dies of EPIPE, and pipefail turns that into a failed
assertion. Measured 10/400 under CPU load and 0/400 idle; a here-string is
0/400 under the same load. The four sites were converted and re-measured.

Recorded here rather than dropped, for two reasons. It is the second time this
FR's own certification produced a result that looked like the thing being
verified and was not — the first was the sweep voided by documents authored
while it ran. And a certification that quietly discards its one red line is
precisely the shape these gates exist to prevent. The systemic case is
`FR-145`.

## Related gates

- `scripts/qa/test-qa-gate-surface.sh` — asserts both new scripts are registered
  and executed by the job they declare. Its check 8 also constrains this gate's
  own preamble; see DD-156's known limits.
- `scripts/qa/fixture-target-drift.rb` — the fixtures here are built on
  `gate_fixture.sh`, and this scanner is what keeps them that way.
- `scripts/qa/ci-cost.rb` — carries the two new governance steps as
  `pendingMeasurement` until CI measures them.
- `scripts/qa/ci-liveness.rb` — records the new `cargo-deny` job; a workflow
  change stales every record at once, which is DD-146's two-pass convergence.
