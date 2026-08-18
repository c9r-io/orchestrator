---
lifecycle: active
related_fr: FR-174
self_referential_safe: true
---

# QA: CI Governance Meta-Verification Tiering (FR-174)

Verifies that the 19 meta-verification steps in `ci.yml`'s `governance` job are
deferred only when the changeset cannot have affected them, that a deferral is
asserted rather than assumed, and that the deferred steps have a home that runs.

All scenarios are read-only against the working tree or build throwaway trees
under `$TMPDIR`. No daemon starts, no database is touched, no provider is
invoked. `self_referential_safe: true`.

Design: [DD-192](../../design_doc/orchestrator/192-ci-governance-tiering.md).

---

## Scenario 1: the tier predicate returns work to the PR path, and fails closed

### Steps

```bash
bash scripts/qa/test-ci-tier.sh
```

To exercise one failure path by hand:

```bash
cd "$(mktemp -d)" && git init -q && git config user.email a@b.c && git config user.name a
mkdir -p src && echo x > src/a.rs && git add -A && git commit -qm base
git update-ref refs/remotes/origin/main HEAD
echo y >> src/a.rs && git add -A && git commit -qm change

GITHUB_EVENT_NAME=pull_request GITHUB_BASE_REF=nope \
  bash "$OLDPWD/scripts/qa/ci-tier.sh"
```

### Expected

- Exit 0, summary `FR-174 meta-verification tier: 23 passed, 0 failed`.
- Cases 1–4 pass individually, one per tiered root — a single case covering all
  four would pass while three patterns were wrong.
- Case 0 (the control) passes: a changeset touching only `src/` **defers**.
  Without it every `full` verdict could be a predicate returning `full`
  unconditionally.
- Cases 6–9 fail closed: a `push`, an empty `GITHUB_BASE_REF`, an unresolvable
  base ref, and an **empty diff** all yield `full`. The hand-run above prints
  `full` with its reason on stderr.
- The empty-diff case is the one a reasonable author gets wrong. "No files
  changed" is the question going unanswered, not evidence that no gate changed;
  a predicate that deferred there would defer every run whose diff computation
  silently returned nothing.

---

## Scenario 2: a deferral is asserted, not assumed

The scenario that matters. A predicate wrongly returning `deferred` skips all 19
gates; if the aggregator merely tolerates `skipped`, the job is green and no
meta-verification ran anywhere.

### Steps

```bash
# a meta gate that RAN when the tier said it would not
TIER=deferred META=$'a-fixtures\nb-fixtures' \
  OUTCOMES=$'a-fixtures=success\nb-fixtures=skipped\nreal=success' \
  bash scripts/qa/governance-result.sh; echo "exit=$?"

# a meta gate skipped when the tier said it would run
TIER=full META=$'a-fixtures\nb-fixtures' \
  OUTCOMES=$'a-fixtures=skipped\nb-fixtures=success\nreal=success' \
  bash scripts/qa/governance-result.sh; echo "exit=$?"

# nothing to read at all
TIER=full META='' OUTCOMES='' bash scripts/qa/governance-result.sh; echo "exit=$?"
```

### Expected

- All three exit 1.
- The first names the offending gate and says the condition did not hold. **A
  `success` here is a violation**, not a bonus: the tier is a claim about what
  executed, so an unexpected success falsifies it exactly as a failure would.
- The third reports that OUTCOMES named no gates. Reading nothing is not passing.
- Note the invocation shape: this script takes its input from the environment, so
  running it bare exits 1 by design. A sweep that invokes derived *paths* rather
  than derived *invocations* will read that as a failure.

---

## Scenario 3: the rosters cannot drift, and coverage did not shrink

Five places name the tiered set: the `if:` conditions in `ci.yml`, `ci.yml`'s
`META`, the nightly's steps, the nightly's `META`, and `tieredBy` in
`config/governance/qa-gate-surface.json`.

### Steps

Cases 19–22 of `test-ci-tier.sh`, plus the count:

```bash
ruby -rjson -e 'puts JSON.parse(File.read("config/governance/qa-gate-surface.json"))["scripts"]
  .count { |s| s["enforcement"] == "ci-required" }'
```

To confirm the roster cases bite, mutate and re-run — comment a line out rather
than deleting it, deletion being the case the author already had in mind:

```bash
cp .github/workflows/ci.yml /tmp/ci.bak
# comment out one id in ci.yml's META block, then:
bash scripts/qa/test-ci-tier.sh; echo "exit=$?"
cp /tmp/ci.bak .github/workflows/ci.yml
```

### Expected

- Unmutated: all three cases pass, reporting 19 gates with identical commands.
- Mutated: a **named diagnostic** — `PROBLEM ci gated steps != ci META: [...]` —
  not merely a non-zero exit. An exit code cannot say which comparison fired.
- A step made tier-conditional that also belongs to the tiering mechanism fails
  case 20. A gate that can defer its own verification is the deadlock this FR
  must not build.
- Case 22 ties the three names that must be one name: the tier step's `id`, the
  `steps.<id>.outputs.<key>` the conditions read, and the `<key>=` line
  `ci-tier.sh` writes. Renaming either end leaves every condition resolving to an
  empty string and skipping all 19 gates; the aggregator's unset-tier check is a
  backstop, not a diagnosis. Verified against both drift directions.
- The count is **61**, against 58 before FR-174 (`ci-tier.sh`,
  `governance-result.sh`, `test-ci-tier.sh`), and must never fall — FR-174's
  negative acceptance criterion. 19 entries carry `tieredBy`; without it the
  manifest would claim those 19 run on every push, which is what `ci-required`
  means and is no longer true of them.

---

## Scenario 4: the critical path is recomputed, not trusted

### Steps

```bash
ruby scripts/qa/ci-cost.rb
bash scripts/qa/test-ci-cost.sh
```

### Expected

- `critical path: 1335s full / 774s deferred (19 tiered step(s), 561s)` plus the
  longest chain.
- `test-ci-cost.sh` reports `12 passed, 0 failed`, including the drift case,
  which mutates the **`needs` graph** rather than the recorded number: adding
  `parity needs governance` makes the chain 400s and the recorded 300s stale
  while every per-job second stays correct, so no other check here has an
  opinion.
- Compare the deferred figure against the longest **product** job (`test`, 324s),
  never against the product jobs' sum — parallel jobs' seconds do not add into a
  latency. The next bound is `ci-environment-parity` at 577s, which this FR does
  not tier.

---

## Scenario 5: the deferred gates have a home that runs

The requirement that decides whether this FR removed work or removed checking.
DD-159's precedent: a gate dead since 2026-03-26 because it exited before its
first scenario, and nothing looked.

### Steps

```bash
ruby scripts/qa/ci-liveness.rb
```

### Expected

- `.github/workflows/nightly-governance.yml` has a liveness record whose
  conclusion is a **real run**, whose `headSha` is an ancestor of HEAD, and which
  does not predate the workflow's last change.

### Known state

**Not yet satisfied.** The workflow has never run, so no honest record exists —
every entry in that ledger is a real run, and a fabricated `headSha` is the entry
the ledger exists to prevent. Until this passes, the 19 deferred gates have a
declared home and no evidence of arriving there, and FR-174 stays In Progress.

Sequence to satisfy it: push the branch, dispatch `nightly-governance.yml`, let
it conclude, then `ci-liveness.rb --refresh --write` and
`ci-cost.rb --refresh --write`. The second also measures the two steps under
`pendingMeasurement` and re-arms the cost ceiling.

Expect `ci-liveness.rb` to be red on any commit touching `ci.yml` until that
refresh: a record taken before the workflow last changed describes a pipeline
that no longer exists, so all 12 records expire at once. Local runs mislead while
the change is uncommitted — `ci.yml`'s last-change sha is still the old one, so
the gate passes here and fails in CI.

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | S1 predicate returns work, and fails closed | ☑ | 23/23; one case per root; four fail-closed paths |
| 2 | S2 deferral asserted in both directions | ☑ | 11 aggregator states exercised on bash 3.2 |
| 3 | S3 rosters cannot drift; coverage did not shrink | ☑ | 3 mutations, each named; 58 → 61 ci-required |
| 4 | S4 critical path recomputed | ☑ | 12/12; 1335s full / 774s deferred |
| 5 | S5 deferred gates have a running home | ☐ | **outstanding** — the nightly has never run |
