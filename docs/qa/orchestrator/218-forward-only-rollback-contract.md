---
lifecycle: active
related_fr: FR-165
self_referential_safe: true
---

# 218. The forward-only rollback contract

Verifies FR-165 requirement 2: the contract has one definition, its second clause
is enforced mechanically, and every live restatement of it points at that
definition.

Every scenario is read-only or operates on copies under `$TMPDIR`. Nothing starts
a daemon, writes the runtime database, or mutates a checked-in artifact. The
schema scenarios drive the guard through
`SCHEMA_SNAPSHOT_PATH`/`PREVIOUS_RELEASE_SCHEMA_SNAPSHOT_PATH`, so no doctored
snapshot ever reaches the working tree; the prose scenarios copy the tree into a
scratch git repository and mutate that.

The contract itself is in `crates/orchestrator-persistence/src/migration.rs`.
Design record: `docs/design_doc/orchestrator/180-forward-only-rollback-contract.md`.

## Scenario 1 — the contract has exactly one definition, and the prose agrees

### Steps

```bash
ruby scripts/qa/rollback-contract-single-source.rb
```

### Expected

Exit 0, and the report names every class with its counts:

```
source   3 site(s) /  1 file(s)
A       15 site(s) / 14 file(s)
record   3 site(s) /  1 file(s)
B        6 site(s) /  4 file(s)
C        1 site(s) /  1 file(s)
D        1 site(s) /  1 file(s)
index    2 site(s) /  2 file(s)
15 class-A statement(s) cite the single source, each by its own line
scanned 1592 tracked text file(s), 58 non-regular path(s) skipped
```

The three `record` sites are CHANGELOG's released `[0.4.0]` section and carry no
citation by design. The single `D` site is threat-model row T8 — monotonic
connection-state change, four rows above the A-class T12 in the same table.

Re-derive the site total independently, which must agree with the gate:

```bash
git ls-files -z | xargs -0 grep -ilE 'forward[- ]only' | wc -l   # files
```

## Scenario 2 — clause 2 is enforced, and only in the removing direction

### Steps

```bash
cargo test -p agent-orchestrator schema_snapshot
```

### Expected

Five tests pass, including `previous_release_schema_is_a_subset_of_current`.

Then confirm the direction, which is the half that matters most: the guard must
fail on removal and stay silent on addition. Both are covered by scenario 4's
cases 1-4 rather than by hand here, because doing it by hand means editing a
checked-in snapshot.

Spot-check the artifact's own premises:

```bash
grep -c 'CREATE TABLE' config/governance/schema-snapshot-previous-release.sql   # 46
grep -c 'CREATE TABLE' config/governance/schema-snapshot.sql                    # 47
git rev-parse 'v0.5.0^{commit}'                                                 # 58166a9f...
grep 'revision:' config/governance/schema-snapshot-previous-release.sql         # the same sha
```

The one added table is migration 38's `attention_projection_gaps`. The revision in
the artifact header and the release tag must be the same commit; that commit is
also `FR113_PREVIOUS_REF` in `scripts/qa/test-slack-skill-automation-vertical.sh`,
so the repository has one "previous release" and not two.

## Scenario 3 — the behavioural half still runs

### Steps

```bash
bash scripts/qa/test-slack-skill-automation-vertical.sh
```

### Expected

Exit 0. This is the end-to-end assertion behind clause 2: it builds the 0.5.0
binary in an isolated worktree and runs it against a schema-34 database. It is
the gate that was red from 2026-08-11 until `77cc351a`, and the reason the
mechanical guard in scenario 2 exists — nothing in CI could see that failure.

Recorded run: exit 0 at `e131c069` on a clean worktree, 2026-08-12, as part of
`scripts/qa/test-slack-skill-automation-release.sh` (359s).

## Scenario 4 — both guards fail on a broken implementation

### Steps

```bash
bash scripts/qa/test-rollback-contract.sh
```

### Expected

`24 passed, 0 failed`, and the summary line must be present — its absence means
the run ended early regardless of the exit code.

The cases, and what each one is for:

| # | Mutation | Must |
|---|---|---|
| 0 | none (before-run, both halves) | green, so a case failing for an unrelated reason cannot read as a catch |
| 1 | a table and its indexes removed | fail naming the table |
| 2 | one column removed from a table that stays | fail naming table and column |
| 3 | an index commented out | fail naming the index |
| 4 | a table and an index **added** | **pass** — the guard must not block forward motion |
| 5 | a table commented out, indexes orphaned | fail through the unapplicable-statement branch, not the removal branch |
| 6 | the previous-release snapshot truncated | fail closed on the empty read, not pass vacuously |
| 7 | a new unclassified mention | fail naming the file and its digest |
| 8-10 | a class B, C and D mention added, then booked | fail as **unclassified** and specifically not as uncited; once booked, silent |
| 11 | a class-D site added to the threat model **and** T12's citation removed | fail — proves a D site does not stop the A site in the same file being checked |
| 12 | a booked class-A statement commented out | fail through the mirror condition |
| 13a | a self-citing statement's citation edited away | fail through the mirror condition, because the statement's own digest moved |
| 13b | a separated citation commented out | fail naming the missing `citedBy` |
| 13c | `citedBy` repointed at a real line that names nothing | fail on the citation's content, not its presence |
| 14 | the single source moved out of the tree | fail closed rather than making every citation vacuous |
| 15 | the scope derivation returns nothing | fail closed with the read-nothing diagnostic |
| 16 | after-run, both halves | green, so no case leaked state |

Case 11 is the one the ledger's shape exists for, and cases 5, 13a and 13b are
the ones whose branch was established by running them rather than by design.

## Scenario 5 — the gate is registered and costed

### Steps

```bash
bash scripts/qa/test-qa-gate-surface.sh
ruby scripts/qa/ci-cost.rb
ruby scripts/qa/fixture-target-drift.rb
```

### Expected

All exit 0. Specifically:

- both new paths are `ci-required` in `config/governance/qa-gate-surface.json`
  with a `shape` field naming the §4.4 failure shape each catches — the surface
  gate rejects a new ci-required gate that cannot answer that;
- both are booked in `config/governance/ci-step-cost.json` under
  `pendingMeasurement`, since neither has run in CI yet;
- the drift scanner passes, which it did not before this work: see the regression
  checklist.

## Regression Checklist

- [x] `cargo test --workspace` — 2894 passed, exit 0
- [x] `cargo clippy --workspace --all-targets -- -D warnings` — exit 0
- [x] `cargo fmt --all -- --check` — exit 0, status captured directly
- [x] `ruby scripts/qa/rollback-contract-single-source.rb` — exit 0
- [x] `bash scripts/qa/test-rollback-contract.sh` — 24 passed, 0 failed
- [x] `bash scripts/qa/test-qa-gate-surface.sh` — 17 passed; `--fixture-test` 52 passed
- [x] `ruby scripts/qa/ci-cost.rb` — exit 0, 2024s of 2700s
- [x] `ruby scripts/qa/fixture-target-drift.rb` — exit 0, 45 gates scanned
- [x] `bash scripts/qa/test-manual-gate-freshness.sh` — 12 passed, 0 failed
- [x] `bash scripts/qa/test-governance-ledger-tooling.sh` — 14 passed, 0 failed
- [x] Derived ci-required sweep: 60 gates from the manifest, 64 invocations from
      `workflow_model.rb run-commands`, reconciled — 50 matched directly, 7 via
      their own harness, 3 via `scripts/qa-doc-lint.sh --fixture-test`

### Two gates that were red before this work, and one that still is

`ruby scripts/qa/fixture-target-drift.rb` and
`bash scripts/qa/test-governance-ledger-tooling.sh` both failed at `e131c069`,
before any of this FR's requirement-2 changes:

- the drift scanner on `scripts/qa/test-manual-gate-freshness.sh`, the harness
  requirement 1 shipped at `55c0d766` — three unproven mutations and one aborting
  premise. **Fixed here.**
- the ledger tooling on a hand-edited row inside the generated FR registry block
  in `docs/feature_request/README.md`, also from requirement 1. **Fixed here** by
  regenerating with `ruby scripts/lib/fr_registry.rb write`; the status note moved
  to the prose section below the generated block, which is where every other
  closure note lives.

`ruby scripts/qa/ci-liveness.rb` remains red and **cannot be fixed locally**. It
reports 12 stale job records because `config/governance/ci-job-liveness.json` was
last refreshed on 2026-08-03 at `9f94a892` and `ci.yml` has changed since. Verified
red with the identical 12 records at `bd0e2389`, `77cc351a` and `e131c069`, so this
is pre-existing from requirement 1's own `ci.yml` edit and is not made worse here.
Refreshing it requires a real CI run to record run IDs against the current sha.

All three are instances of the same mechanism, §4.6 condition 6: requirement 1's
certification ran a hand-listed sweep, these gates were not on the list, and the
log was all green while saying nothing about what it did not run.

### One local-only failure, ticketed

`bash scripts/qa/test-markdown-link-integrity.sh` exits 134 (`Abort trap: 6`, a
bash 3.2 malloc abort) in the primary working directory, and passes at the same
commit in a fresh `git worktree` and in a tracked-files-only copy. The trigger is
ignored build output in that directory rather than anything in the tree; macOS
bash 3.2 only, and CI runs ubuntu bash 5. Recorded in
`docs/ticket/20260812-markdown-link-gate-aborts-under-bash32.md` rather than left
as a green claim, because the gate never printed a verdict and a run that aborted
before its summary is not evidence either way.
