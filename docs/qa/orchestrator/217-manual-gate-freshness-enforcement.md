---
lifecycle: active
related_fr: FR-165
self_referential_safe: true
---

# 217. Manual-gate freshness enforcement

Verifies the FR-165 criterion — a gate is fresh only when its last recorded run
succeeded on a clean worktree — and the release edge that enforces it.

Every scenario is read-only or operates on a scratch tree under `$TMPDIR`.
Nothing starts a daemon, writes the runtime database, or mutates
`config/governance/manual-gate-freshness.json`. That matters more than usual
here, because the subject *is* that ledger and a scenario writing to it would be
recording evidence of itself.

## Scenario 1 — the criterion rejects every kind of non-run

**Steps**

```bash
bash scripts/qa/test-manual-gate-freshness.sh
```

**Expected**: exit 0, final line
`FR-165 manual-gate freshness fixtures: 12 passed, 0 failed`.

Cases 1–4 each mutate one field of an otherwise-green record and require
`--strict` to fail *and* name the mutated gate:

| Case | Mutation | State |
|---|---|---|
| 1 | `exitStatus` 0 → 1, dated today | `FAILED` |
| 2 | `worktreeDirty` false → true, exit still 0 | `dirty` |
| 3 | date backdated past `staleAfterDays` | `aged` |
| 4 | `lastRun` null | `never` |

Cases 1 and 2 are the FR-165 defect: before it, both read `ok` and `--strict`
passed them. The mutations change one field only, so nothing but the field under
test distinguishes them from the green before-run. Case 3 is the pre-existing
recency rule, which must survive the two new ones.

## Scenario 2 — exemptions bind to one gate and cannot be silent

Cases 5–8 of the same script.

**Expected**: an exempt never-run gate leaves `--strict` green *and* prints the
exemption with its reason (case 5); an exemption on one gate does **not** excuse
a stale neighbour (case 6 — this is the only case that catches an exemption
applied set-wide rather than per gate); `releaseBlocking: false` with no reason
fails even without `--strict`, since it is a fact about a committed file (case
7); and a `releaseBlockingReason` left behind after its exemption was removed
also fails (case 8).

Case 8 is the inverted form of case 7 — §4.4 shape 9. A stale reason reads to
the next person as an exemption still in force.

## Scenario 3 — counts are derived, and the two files agree

Case 9 empties the manifest's manual-runbook set and reads the expected number
out of the fixture ledger instead of restating it.

**Expected**: fails closed with `the freshness ledger records 2`. Before FR-165
this diagnostic said "35 are expected" while the manifest declared 38.

Then, against the real repository:

```bash
ruby scripts/qa/manual-gate-freshness.rb; echo $?
```

**Expected**: exit 0. Fails with a named diff if a gate is `manual-runbook` in
`config/governance/qa-gate-surface.json` but absent from the ledger, or the
reverse (the FR-158 check, unchanged). Capture the status directly — `| tail`
reports the pager's status (§4.6 condition 4).

The report's last line must name the four states separately, e.g.
`17 of 38 gate(s) not fresh (6 never, 6 failed, 5 dirty)`. A single `STALE`
marker for all four would hide which, and an operator's response to a broken
gate is not their response to an unrun one.

## Scenario 4 — the release edge is real, not merely spelled

Case 10 parses `.github/workflows/release.yml` with a YAML parser and requires:
the `manual-gate-freshness` job exists; a step runs
`manual-gate-freshness.rb --strict`; that step carries neither
`continue-on-error` nor a pipe; and **`build` and `gui-build` both name the job
in `needs:`**.

Verify by hand:

```bash
ruby -ryaml -e 'y=YAML.load_file(".github/workflows/release.yml");
  j=y["jobs"]; puts j.select{|_,s| Array(s["needs"]).include?("manual-gate-freshness")}.keys.inspect'
```

**Expected**: `["build", "gui-build"]`.

A grep would be satisfied by a `needs:` inside a comment, by the job name in a
`name:` field, or by a job nothing depends on — which runs, goes red, and
publishes the release anyway.

## Scenario 5 — the fixtures fail on a broken implementation

Fixtures are worth their runtime only if they go red when what they guard
regresses. Apply each mutation to a copy, run, revert.

| Mutation | Expected |
|---|---|
| criterion reverted to `age.nil? \|\| age > stale_after` | cases 1 and 2 fail, no others |
| `manual-gate-freshness` removed from `build`'s `needs:` | case 10: "build does not need manual-gate-freshness" |
| `continue-on-error: true` added to the strict step | case 10: names `continue-on-error` |
| derived count restated as a literal `38` | case 9: "does not name the derived count 2" |

**Expected**: each mutation caught by the named case and no other, with a
diagnostic identifying the branch. Recorded at FR-165: 10, 11, 11 and 11 passing
respectively, against 12 unmutated.

## Regression Checklist

- [ ] `bash scripts/qa/test-manual-gate-freshness.sh` — 12 passed, 0 failed, exit 0
- [ ] `ruby scripts/qa/manual-gate-freshness.rb` — exit 0, summary names the four states
- [ ] `ruby scripts/qa/manual-gate-freshness.rb --strict` — exit reflects release-blocking gates only; exempt gates listed with reasons
- [ ] `build` and `gui-build` both `needs: manual-gate-freshness` in `release.yml`
- [ ] The strict step carries no `continue-on-error` and no pipe
- [ ] Every `releaseBlocking: false` carries a non-empty `releaseBlockingReason`
- [ ] All four Scenario 5 mutations still fail their own case and no other
- [ ] `bash scripts/qa/test-qa-gate-surface.sh` — the fixture gate is declared and ci-required
