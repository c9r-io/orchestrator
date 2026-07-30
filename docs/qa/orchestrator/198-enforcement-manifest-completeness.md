---
lifecycle: active
related_fr: FR-147
---

# Orchestrator - The Enforcement Manifest Is Complete With Respect To What CI Executes

**Module**: CI / Governance
**Scope**: `check_workflow_execution_declared` in
`scripts/qa/test-qa-gate-surface.sh`, the `executed_scripts` and
`development_triggered?` reporters in `scripts/lib/workflow_model.rb`, the three
new `ci-required` entries and the `release-tooling` role in
`config/governance/qa-gate-surface.json`, and the provider-stub backstop on
`ci.yml`'s two coverage jobs
**Scenarios**: 5
**Priority**: Medium

## Background

Since FR-143, scanners in this repository derive their **scope** from
`config/governance/qa-gate-surface.json` instead of listing the files they guard.
That moves the enumeration into the manifest rather than removing it, and nothing
was comparing the manifest against what CI actually runs.

Three shell gates had been executed by `ci.yml` with no entry in the manifest, so
the two scanners that derive scope from it — `jq-status-observed.rb` and
`fixture-target-drift.rb` — had never read any of them. One of the three,
`scripts/qa-doc-lint.sh`, was named as the `invokedBy` of a gate that *was*
governed: the callee was checked and its caller was not.

Design record: `docs/design_doc/orchestrator/160-enforcement-manifest-completeness.md`.
Siblings: `docs/qa/orchestrator/183-gate-surface-execution-truth.md` asks whether a
declared gate really runs; this asks whether a running script is really declared.
The two directions are separate checks because either rule could be deleted
without the other noticing.

**Safety**: read-only against the working tree. Every case builds a scratch tree
under `$TMPDIR`; no daemon starts, no database is touched, no provider is invoked,
and nothing contacts the network. Safe to run against this repository.

## Why the assertions are shaped the way they are

**The difference set is derived twice and the routes must agree.** A single
derivation is how the third missing gate stayed hidden: a hand count found two.
Scenario 1 asserts the model-derived set and a raw grep agree, which also
establishes that no script here is *only* mentioned in a comment or a heredoc —
the two routes would disagree if one were.

**The scanned counts are the evidence, not "the suite passes".** Adding manifest
entries is supposed to widen two scanners. An aggregate green says nothing about
whether they widened, so scenario 2 reads the counts.

**Every exemption gets attacked, and by its cheapest bypass.** An exemption nobody
has tried to trip is an exemption whose reach is a guess. Deleting a
`release-tooling` entry is the obvious defect; relabelling it `library` is one
word and leaves it declared, so scenario 4 applies the obvious mutation, the cheap
one and the conditional one as three separate cases.

**A check that can pass having read nothing is asserted against, not reasoned
about.** Scenario 5.

---

## Scenario 1: The difference set is derived, by two routes that agree

**Steps**

```bash
ruby scripts/lib/workflow_model.rb executed-scripts . | cut -f1 | sort -u > /tmp/route-a.txt
grep -ohE '(\./)?scripts/[A-Za-z0-9._/-]*\.sh' .github/workflows/*.yml \
  | sed 's|^\./||' | sort -u > /tmp/route-b.txt

# every executed script is declared, as a gate or as a non-gate
jq -r '.scripts[].path, (.supportFiles // [])[].path' \
  config/governance/qa-gate-surface.json | sort -u > /tmp/declared.txt
comm -23 /tmp/route-a.txt /tmp/declared.txt
```

**Expected result**

- Route A reports 42 distinct paths (`.sh` and `.rb`); its `.sh` subset and route
  B are the same 36 paths.
- The final `comm` prints **nothing**: the forward difference set is empty.
- Reverse direction, for the record: 7 `ci-required` entries are not executed
  directly by any job, and all 7 carry `invokedBy`.

**Mutation targeted**: none — this is the measurement. It is a scenario rather
than a note because the number is the acceptance criterion, and a number nobody
re-derives is the thing FR-147 was filed about.

*What this would still pass on*: a manifest that declares everything and a
`release-tooling` role stretched over three governance gates. Scenario 4 is what
rules that out.

---

## Scenario 2: Both derived scanners widened, and neither lost a passing case

**Steps**

```bash
ruby scripts/qa/jq-status-observed.rb    | tail -1
ruby scripts/qa/fixture-target-drift.rb | tail -1
ruby scripts/qa/pipefail-short-circuit.rb | tail -1
```

**Expected result**

- `jq status observed: PASS (39 shell gate(s) scanned, 0 finding(s))` — 39, up
  from 36 before the three entries existed.
- `Fixture target drift: PASS (34 ci-required shell gates scanned)` — 34, up from
  31.
- `pipefail short-circuit: PASS (108 tracked shell file(s) scanned, ...)`,
  unchanged: it derives scope from `git ls-files '*.sh'` and is deliberately
  independent of the manifest.

**Mutation targeted**: the aggregate verdict is a proxy for "the scanners now see
the three gates". Two gates could pass while scanning exactly what they scanned
before. The counts are what observes the widening; each must be **exactly three
higher**, not merely non-decreasing, because a count that rose by one would mean
two gates are still invisible.

---

## Scenario 3: The new check passes on this repository, and is registered

**Steps**

```bash
bash scripts/qa/test-qa-gate-surface.sh
```

**Expected result**

- 14 checks pass, 0 fail, including `every script a workflow job executes is
  declared here, and no release-tooling exemption runs on a branch push or a pull
  request`.
- `Enforcement surface: 46 of 79 gates are ci-required`.
- The gate's own registry assertions (run in `--fixture-test`) confirm the new
  check is named in `ALL_CHECKS`, has a `describe_check` entry, and has at least
  one negative fixture. A check that exists but is unregistered runs nowhere while
  still looking like enforcement.

---

## Scenario 4: The three negative fixtures, and the mutation each applies

**Steps**

```bash
bash scripts/qa/test-qa-gate-surface.sh --fixture-test
```

Read fixtures 25, 26 and 27, and the summary line.

**Expected result**

`37 passed, 0 failed`, with the summary line present — its absence means the run
terminated early regardless of the reported status. Each of the three cases fails
`check_workflow_execution_declared` and **only** that check, which the shared
`expect_fail` harness asserts by running every other check on the same tree.

### 4a. A gate CI still runs, with its manifest entry deleted

The case deletes the first `ci-required` entry whose path is **outside**
`scripts/qa`, and the check fails naming that path and the `workflow:job` that
runs it.

**Mutation targeted**: entry deletion, and specifically an entry outside
`scripts/qa`. Deleting one *inside* `scripts/qa` is caught first by
`check_surface_complete`'s disk compare, which would make the fixture prove
nothing about this check — and that is not incidental, it is the exact shape of
the original defect: `scripts/qa` is the only tree check 1 can see, which is why
three gates in `scripts/` were invisible.

The target is derived from the manifest, never named. This check's subject is a
set that is meant to grow; a fixture naming a path works only until the next gate
is classified, and eight of nine recorded target-drift incidents stayed green.

---

### 4b. The exemption is conditional, attacked two ways

- **Fixture 26** leaves the path declared and changes its role from
  `release-tooling` to `library`. The check fails: `library` states the file is
  never invoked as a gate itself, and a workflow job executes this one directly.
- **Fixture 27** leaves the entry untouched and adds a `ci.yml` step that runs the
  release script. The check fails: `ci.yml` triggers on branch pushes and pull
  requests, so the script now runs on every change, which is what the role denies.
**Mutation targeted**: fixture 26 is the mutation the author is least likely to
have in mind. Deleting the entry is the obvious defect and fixture 25 already
covers that shape; the cheap way to silence this check is one word — relabel, stay
declared, and the trigger rule no longer applies because it only examines
`release-tooling`. Fixture 27 attacks the *condition* rather than the
*declaration*, which is the only way to show the exemption is not a permanent
amnesty.

*What fixture 27 alone would still pass on*: a check that rejects any edit to
`ci.yml`. Scenario 3's clean run on the unmodified repository, plus the isolation
assertion inside `expect_fail`, is what rules that out.

**Trigger classification is derived, not listed.** `development_triggered?` reads
each workflow's parsed trigger map, so `release.yml` (`push: {tags: [v*]}` plus
dispatch) is release-only while `ci.yml`, `docs.yml` and `security.yml` are not. A
list of workflow names would be the enumeration this whole check exists to avoid.

---

## Scenario 5: The check fails closed when it reads nothing

**Steps**

```bash
# the model reports the executed set; make it report an empty one
d="$(mktemp -d)" && mkdir -p "$d/.github/workflows" "$d/config/governance"
cp config/governance/qa-gate-surface.json "$d/config/governance/"
ruby scripts/lib/workflow_model.rb executed-scripts "$d"; echo "rows=$?"
```

Then read the `[[ -z "$records" ]]` branch of `check_workflow_execution_declared`,
and confirm the Ruby call's status is observed rather than taken on trust.

**Expected result**

- Against a tree with no workflows the reporter emits nothing and the check
  **fails**, saying the model reported no executed scripts at all.
- A reporter that dies — a malformed workflow, a missing library — fails the check
  with the interpreter's own stderr quoted, rather than yielding an empty set.

**Mutation targeted**: this is the §4.4 shape 5 case, and it is asserted rather
than argued because a sibling gate has already printed `13 passed, 0 failed` over
a manifest it could not parse. Zero rows and N passing rows are indistinguishable
in an exit code, and an empty executed set reads exactly like "every executed
script is declared". The Ruby invocation is deliberately not left in condition
position: that disables `set -e` for its whole call tree, which is how the empty
read would have gone unnoticed.

*What an exit-code-only assertion would still pass on*: the check failing for any
other reason. The diagnostic is asserted, not just the status.

---

## Checklist

- [ ] `ruby scripts/lib/workflow_model.rb executed-scripts .` and a raw grep over
      `.github/workflows/*.yml` agree on the executed set; the forward difference
      against `scripts[].path ∪ supportFiles[].path` is empty
- [ ] `jq-status-observed.rb` reports 39 scanned, `fixture-target-drift.rb` 34 —
      each exactly three higher than before the three gates were classified
- [ ] `pipefail-short-circuit.rb`'s count is unchanged, confirming it is
      independent of the manifest
- [ ] `test-qa-gate-surface.sh` passes 14 checks, and `--fixture-test` reports
      `37 passed, 0 failed` **with the summary line present**
- [ ] Fixtures 25, 26 and 27 each fail `check_workflow_execution_declared` and no
      other check
- [ ] The check fails, rather than passing, when the executed set is empty
- [ ] No `release-tooling` entry is executed by a workflow that triggers on a
      branch push or a pull request
- [ ] `boundary-coverage` and `coverage-policy-fixtures` both install
      `./.github/actions/provider-stubs`
