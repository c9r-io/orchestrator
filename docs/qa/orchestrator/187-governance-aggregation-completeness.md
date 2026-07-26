---
lifecycle: active
related_fr: FR-137
self_referential_safe: true
---

# Orchestrator - Governance Aggregation Completeness

**Module**: Governance / CI
**Scope**: the completeness of the `governance` job's outcome aggregation, derived from the parsed
workflow rather than declared; the three ways a swallowed step failure disappears; and the
aggregate script proven to convert those outcomes into the job's result
**Scenarios**: 4
**Priority**: High

## Background

FR-134 made the governance gate steps `continue-on-error: true` so one run reports every problem,
and added a final step that reads each outcome and fails the job. The list of outcomes it reads was
hand-written and unguarded. Inserting a `continue-on-error: true` step that runs `exit 1` and
leaving it out of `OUTCOMES` produced `12 passed, 0 failed` while that step failed on every run.

The list grew **19 → 20 → 21 → 22** across four FR cycles, and when FR-137 was governed the three
documents describing it had each stopped at a different number. The sets did in fact agree — the
defect was latent — but nothing could establish that, which is the point. See DD-149.

All scenarios are read-only against the working tree or operate on copies under `$TMPDIR`. Nothing
starts a daemon, touches the runtime database, or invokes a provider.

Primary entry points:

```bash
./scripts/qa/test-qa-gate-surface.sh                   # 13 checks
./scripts/qa/test-qa-gate-surface.sh --fixture-test    # 34 assertions: 24 negative fixtures, three
                                                       # positive controls, four behavioural cases,
                                                       # and three meta-assertions
ruby scripts/lib/workflow_model.rb outcome-facts .     # the facts the check reasons over
```

---

## Scenario 1: A Gate Left Out Of The Aggregate Is Rejected

The reproduction FR-137 was filed on.

### Preconditions

- Clean worktree; repository root is the working directory.
- `jq`, `rg`, `git`, `ruby` on PATH.

### Steps

```bash
# 1. The resident fixtures: the defect rejected, and the same step accepted
#    once its OUTCOMES line is added.
./scripts/qa/test-qa-gate-surface.sh --fixture-test 2>&1 | grep 'fixture 22'

# 2. End to end on a throwaway copy: insert a gate that fails on every run and
#    is not aggregated, exactly as an author adding a gate would leave it.
WORK="$(mktemp -d)"; git archive HEAD | (cd "$WORK" && tar xf -)
cd "$WORK" && git init -q . && git add -A >/dev/null &&
  git -c user.email=q@l -c user.name=q commit -qm base >/dev/null
perl -0pi -e 's{^(      - name: Governance result$)}{      - name: unaggregated gate\n        id: unaggregated\n        continue-on-error: true\n        run: exit 1\n\n$1}m' \
  .github/workflows/ci.yml
./scripts/qa/test-qa-gate-surface.sh > surface.log 2>&1; echo "exit=$?"
tail -3 surface.log
```

### Expected Result

- Step 1 prints `PASS: fixture 22: ... (isolated to check_continue_on_error_aggregated)`. The
  `isolated to` clause is the assertion that the fixture fails the check it names **and** that the
  other twelve checks still pass on the same tree — so it is not passing by tripping something
  else.
- `PASS: fixture 22b: the same step with its OUTCOMES line added passes ...`. This is the half of
  the criterion that is easy to skip, and skipping it is the difference between a rule and a
  tripwire: fixtures 22, 23 and 24 only ever ask the check to say *no*, so all three are satisfied
  by a check that rejects any edited `ci.yml` — or that returns 1 unconditionally. 22b applies the
  same edit, aggregates it correctly, and requires a pass. The unmodified-repository positive
  control does not cover this, because it never leaves the pristine tree.
- Step 2 exits non-zero with `FAIL: a job swallows a step's failure without aggregating it, ...`
  and a diagnostic naming `.github/workflows/ci.yml job 'governance'` and the id `unaggregated`.
- The same tree passes the classification, wiring and dependency checks. That is the scenario: the
  three checks that fire when a gate is added all go green, and only this one does not.

---

## Scenario 2: The Two Directions The FR Under-Specified

### Preconditions

Same as Scenario 1.

### Steps

```bash
# The no-id direction: a swallowed failure that nothing can ever reference.
# FR-137 specified the check over steps "with an id and continue-on-error",
# which does not see this at all.
./scripts/qa/test-qa-gate-surface.sh --fixture-test 2>&1 | grep 'fixture 24'

# The dangling direction: a record naming a step the job does not define,
# which is what a rename leaves behind.
./scripts/qa/test-qa-gate-surface.sh --fixture-test 2>&1 | grep 'fixture 23'

# Both directions read off the facts, on a tree carrying both defects.
WORK="$(mktemp -d)"; git archive HEAD | (cd "$WORK" && tar xf -)
perl -0pi -e 's{^(      - name: Governance result$)}{      - name: anonymous\n        continue-on-error: true\n        run: exit 1\n\n$1}m' \
  "$WORK/.github/workflows/ci.yml"
perl -pi -e 's{^(            execution-migration=\$\{\{ steps\.execution-migration\.outcome \}\})$}{$1\n            ghost=\${{ steps.ghost.outcome }}}' \
  "$WORK/.github/workflows/ci.yml"
ruby scripts/lib/workflow_model.rb outcome-facts "$WORK" > facts.txt
awk -F'\t' '$1=="coe" && $4==""{print "no-id: " $5}' facts.txt
comm -23 <(awk -F'\t' '$1=="ref"{print $4}' facts.txt | LC_ALL=C sort -u) \
         <(awk -F'\t' '$1=="step"{print $4}' facts.txt | LC_ALL=C sort -u) | sed 's/^/dangling: /'
```

### Expected Result

- Fixture 24 passes, isolated to `check_continue_on_error_aggregated`, with the diagnostic
  *"a continue-on-error step has no id, so no step can read its outcome"*.
- Fixture 23 passes, isolated to the same check. It touches only the `OUTCOMES` block, so it
  reports the dangling direction and **not** the omission direction — the two rules are tested
  separately and neither can be deleted without a fixture failing.
- The facts pass prints `no-id: anonymous` and `dangling: ghost`.

---

## Scenario 3: The Aggregated Outcomes Really Decide The Job

The structural check proves each outcome is *referenced*. This proves referenced is load-bearing.
An aggregate that printed the table and exited 0 would pass every assertion in Scenario 1 and 2
while turning every gate in the job into decoration.

### Preconditions

Same as Scenario 1.

### Steps

```bash
# The three behavioural cases, run against the real script lifted out of ci.yml.
./scripts/qa/test-qa-gate-surface.sh --fixture-test 2>&1 | grep 'behavioural.*aggregate\|behavioural.*outcome'

# The same thing by hand, to see the exit codes.
ruby -r"$PWD/scripts/lib/workflow_model" -e '
  step = WorkflowModel.steps(".github/workflows/ci.yml", "governance")
    .find { |s| s["name"] == "Governance result" }
  File.write("agg.sh", step["run"])'
OUTCOMES="$(printf 'liveness=success\nsurface=skipped')"  bash agg.sh; echo "all-pass    exit=$?"
OUTCOMES="$(printf 'liveness=success\nsurface=failure')"  bash agg.sh; echo "one-failure exit=$?"
OUTCOMES="$(printf 'liveness=success\nghost=')"           bash agg.sh; echo "empty       exit=$?"
```

### Expected Result

- `all-pass exit=0`, `one-failure exit=1`, `empty exit=1`.
- The one-failure run prints `surface                failure` — the aggregate names which gate, so a
  reader can act on the summary without opening the step list.
- The empty case is the one FR-137 got backwards. It argued a dangling reference "resolves to empty
  forever, with the same effect as the omission". It exits 1: the job goes red, permanently, naming
  a gate that no longer exists. Loud, not silent — the opposite direction. The rule was kept and
  the reason rewritten; this assertion is what holds the corrected fact in place.

---

## Scenario 4: Coverage Is Discovered, And The Repository Is Currently Consistent

### Preconditions

Same as Scenario 1.

### Steps

```bash
# 1. No workflow path, job name or step id is written into the check.
rg -n "governance|ci\.yml" scripts/qa/test-qa-gate-surface.sh \
  | rg -v '^\s*[0-9]+:\s*#' | rg 'check_continue_on_error_aggregated' || echo "no literals in the check"

# 2. The scan covers every workflow found on disk, not a list.
ruby scripts/lib/workflow_model.rb workflows .

# 3. The latent-not-active record: the two sets currently agree, both ways.
comm -3 <(ruby scripts/lib/workflow_model.rb continue-on-error-steps .github/workflows/ci.yml governance \
            | cut -f1 | LC_ALL=C sort) \
        <(ruby scripts/lib/workflow_model.rb outcome-references .github/workflows/ci.yml governance \
            | LC_ALL=C sort)

# 4. The registry meta-assertions pick the new check up automatically.
./scripts/qa/test-qa-gate-surface.sh --fixture-test 2>&1 | grep '^  PASS: meta'
```

### Expected Result

- Step 1 prints `no literals in the check`: the rule names no workflow and no job.
- Step 2 lists all four workflows in `.github/workflows/`, discovered by glob.
- Step 3 prints nothing. 22 swallowed steps, 22 records, difference empty in both directions — the
  record that FR-137 closed a latent defect rather than repairing a failure that had fired.
- Step 4 prints all three meta-assertions passing. `check_continue_on_error_aggregated` is in
  `ALL_CHECKS`, has a description, and is targeted by at least one negative fixture, without any of
  those three being asserted by hand.

---

## Mutation Evidence

Extends the table in [QA-183](183-gate-surface-execution-truth.md). The check was neutered by
inserting `return 0` after its opening brace and the fixture suite re-run. A check whose fixtures
still pass when the check does nothing is not tested by them.

| Check | Fixture failures when neutered |
|---|---|
| `check_continue_on_error_aggregated` | 3 |

Fixtures 22, 23 and 24 each report *"accepted the injected defect"*, one per direction. The
behavioural assertions are unaffected by the mutation, which is the correct result — they test the
aggregate script, not the check, and are the reason the check is not the only thing standing
between this job and a silently dead gate.

Two mutations the check is specifically built to catch, and which a text-matching implementation
would not:

| Mutation | What a `grep` of the job block would conclude | What the check concludes |
|---|---|---|
| a swallowed step added with no `id` | nothing to look for; the block still contains every `OUTCOMES` line | direction 1: unaggregatable by construction |
| `OUTCOMES` still lists a step that was renamed | the id is present in the block, so it looks aggregated | direction 3: the reference resolves to nothing |

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | A gate left out of the aggregate is rejected | ☑ PASS | 2026-07-27 | Claude |
| 2 | The two directions the FR under-specified | ☑ PASS | 2026-07-27 | Claude |
| 3 | The aggregated outcomes really decide the job | ☑ PASS | 2026-07-27 | Claude |
| 4 | Coverage is discovered, and the repository is currently consistent | ☑ PASS | 2026-07-27 | Claude |

## Certification Conditions

A run of these scenarios counts as closure evidence only when `git status --porcelain` is empty at
start and at end, `git rev-parse HEAD` matches across the run, nothing else is writing to the
repository, each script is invoked as `bash <script> > log 2>&1` with `$?` captured directly rather
than through a pager, and each log ends with its own summary line.

One condition specific to this document: **the bash 3.2 gate does not see the new fixtures.**
FR-138 (open) reports that `bash32-compat.rb` ends its scan of `test-qa-gate-surface.sh` at line
900, and fixtures 22-24 land after that. The compensating observation is running the gate under the
real `/bin/bash` 3.2 that ships with macOS, which is an execution rather than a scan. Recorded
below.

## Recorded Runs

| Command | Result | Notes |
|---|---|---|
| `bash scripts/qa/test-qa-gate-surface.sh` | `13 passed, 0 failed` | verification mode |
| `bash scripts/qa/test-qa-gate-surface.sh --fixture-test` | `34 passed, 0 failed` | 24 negative fixtures, 3 positive controls, 4 behavioural, 3 meta |
| `/bin/bash scripts/qa/test-qa-gate-surface.sh` | `13 passed, 0 failed` | GNU bash 3.2.57, compensating for the FR-138 scan gap |
| `ruby scripts/qa/bash32-compat.rb` | `PASS (97 files, 0 findings)` | see the scan-gap caveat above |
| `ruby scripts/qa/ci-liveness.rb` | `PASS (14 jobs, 3 workflows)` | the other consumer of `workflow_model.rb` |
| `bash scripts/qa/test-ci-liveness.sh` | `9 passed, 0 failed` | |
| `ruby scripts/lib/workflow_model.rb outcome-facts .` | 22 `coe`, 22 `ref`, 24 `step` | 0.33s for the whole checkout |
