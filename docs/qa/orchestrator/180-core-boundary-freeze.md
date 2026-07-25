---
self_referential_safe: true
---

# Orchestrator - Core Crate Boundary Freeze And Schema Baseline

**Module**: Governance / Persistence
**Scope**: core crate public surface and rusqlite inventory, migration chain schema equivalence, idempotency and resume
**Scenarios**: 5
**Priority**: High

## Background

FR-130 proposes extracting `orchestrator-persistence` out of `core`. Its acceptance criterion is
that the migration chain produces the same schema before and after — a comparison with no
subject unless the "before" side is recorded first. FR-130 (requirements 1 and 3) records it.

`config/governance/core-boundary-ledger.json` freezes what the boundary is today: 52 top-level
`pub mod`, 924 public items across 143 files, 200 `rusqlite` references across 37 files, and
the six crates that take `rusqlite` directly. `config/governance/schema-snapshot.sql` records
what the 74 registered migrations produce: 46 tables and 92 indexes.

All scenarios here are read-only against the working tree, or operate on copies under
`$TMPDIR`. None starts a daemon, mutates the runtime database, or invokes a provider. The only
databases created are temporary SQLite files inside `tempfile::tempdir()`.

Primary entry points:

```bash
./scripts/qa/test-core-boundary.sh                          # all nine gate cases
ruby scripts/qa/core-boundary.rb                            # the ratchet itself
cargo test -p agent-orchestrator schema_snapshot            # the schema baseline
```

---

## Scenario 1: The Boundary Is Frozen In Both Directions

### Preconditions

- `ruby` and `cargo` are installed.

### Steps

1. `ruby scripts/qa/core-boundary.rb`
2. `ruby scripts/qa/core-boundary.rb --emit-baseline | diff - config/governance/core-boundary-ledger.json`
3. Confirm cases 3, 4 and 5 of `./scripts/qa/test-core-boundary.sh` pass.

### Expected result

- Step 1 exits 0 and reports `core/src files: 143, pub mod: 52, public items: 924` and
  `rusqlite: 200 reference(s) across 37 file(s) in core, 6 crate(s) depend on it directly`.
- Step 2 prints nothing. The recovery path and the compared value are the same expression, so
  a regenerated candidate cannot be one the gate then rejects.
- Step 3 covers the three directions a boundary can move: a new `pub mod` (case 3), a new
  `rusqlite` reference (case 4), and a *removed* one (case 5).
- Case 5 is the one that distinguishes this ratchet from the monotonic ratchet FR-130 asked
  for. A decrease that passes silently leaves the ledger asserting debt the repository no
  longer carries — FR-128 found `capturesOrJsonPath` at 54 against a reviewed 55 for exactly
  that reason. Here a decrease is the goal, which is why it must be blessed rather than
  absorbed.

## Scenario 2: Each Case Rejects Its Own Defect

This is the scenario that separates a gate that is enforced from one that merely looks
enforced. A gate observed only passing has not been observed doing anything.

### Steps

1. `./scripts/qa/test-core-boundary.sh`
2. `git status --porcelain`

### Expected result

- Step 1 exits 0 and prints `FR-130 core boundary: 9 passed, 0 failed`.
- Step 2 prints nothing. Every mutation happens inside a copy under `$TMPDIR`.
- Each case was confirmed against a targeted mutation of the implementation, recorded in
  DD-142:

  | Case | Assertion | Mutation that breaks it |
  |---|---|---|
  | 1 | the gate holds on the working tree | M3 scope change |
  | 2 | `--emit-baseline` is byte-identical to the ledger | M5 emit a candidate missing a field |
  | 3 | a new `pub mod` fails, naming the count | M8 drop `pubMod` from the comparison |
  | 4 | a new `rusqlite` reference fails, naming the file | M9 drop the added-file branch |
  | 5 | a *removed* reference also fails | M2 revert to the monotonic ratchet |
  | 6 | a `cfg(test)` module moves neither gate's baseline | M3 strip only modules named `tests` |
  | 7 | `--write` refuses under `CI` | M4 remove the `CI` guard |
  | 8 | a doctored schema snapshot fails, naming the table | M6 make the comparison always succeed |
  | 9 | neither gate runs without the shared scanner | M7 give a gate a private copy |

- Case 6 asserts the **emitted baseline** does not move, not that `strip_test_modules` exists.
  Testing the helper directly proves a function is present, not that the counting path calls
  it — the textual-presence-as-execution-fact error FR-134 documents.
- Case 9 asserts both gates **pass** with `scripts/lib/rust_source.rb` present and fail
  without it. Asserting only the failure would be satisfied by a gate that was already broken;
  DD-142 records that this is not hypothetical.

## Scenario 3: The Migration Chain Reproduces The Reviewed Schema

### Steps

1. `cargo test -p agent-orchestrator schema_snapshot`
2. `grep -c '^CREATE TABLE' config/governance/schema-snapshot.sql`
3. `grep -cE '^CREATE (UNIQUE )?INDEX' config/governance/schema-snapshot.sql`
4. Confirm case 8 of `./scripts/qa/test-core-boundary.sh` passes.

### Expected result

- Step 1 runs four tests and all pass.
- Step 2 reports **46**; step 3 reports **92**. FR-130 claimed 51 tables; the snapshot is the
  first artifact in the repository from which the real figure is readable.
- Step 4 removes `CREATE TABLE tasks` from a copy of the snapshot, points the comparison at it
  through `SCHEMA_SNAPSHOT_PATH`, and requires the test to fail naming that table — then
  requires the real snapshot to pass in the same run. A baseline that cannot fail is not a
  baseline.

## Scenario 4: Migrations Are Idempotent And Resume To The Same Schema

This is the behavioural evidence FR-130's QA plan asked for. Structural checks — "the symbol
moved", "the count is equal" — cannot show that the chain still works.

### Steps

1. `cargo test -p agent-orchestrator schema_snapshot::tests::a_second_bootstrap_applies_nothing_and_changes_nothing`
2. `cargo test -p agent-orchestrator schema_snapshot::tests::an_interrupted_chain_resumes_to_the_same_schema`
3. `ruby -e 'puts File.read("core/src/persistence/migration.rs").scan(/version: \d+/).length'`

### Expected result

- Step 1 passes: a second bootstrap applies zero migrations *and* leaves the schema unchanged.
  The second half matters — "applied zero" alone would pass for a chain that re-ran its DDL
  idempotently while altering the schema.
- Step 2 passes. It interrupts the chain after every one of the 74 steps and requires the
  resumed database to match the one-shot database exactly. A resume defect lives in one
  specific migration; sampling a few interruption points is how you miss it.
- Step 3 reports 74, matching the number of interruption points step 2 exercises.

## Scenario 5: The Two Governance Gates Share One Scanner

`strip_test_modules` moves the core `rusqlite` count from 237 to 200 and the file count from
43 to 37. The scan is therefore not incidental to the number, and two implementations that
drift would produce two reviewed states, each looking correct on its own.

### Steps

1. Confirm case 6 of `./scripts/qa/test-core-boundary.sh` passes — one `#[cfg(test)]` probe
   containing `rusqlite`, `captures`, `json_path` and `PipelineVariables` moves neither
   `core-boundary.rb --emit-baseline` nor `coordination-governance.rb --emit-baseline`.
2. Confirm case 9 passes.
3. `./scripts/qa/test-coordination-governance.sh` and
   `./scripts/qa/test-governance-ledger-tooling.sh`

### Expected result

- Step 1 shows the two gates agree on what `cfg(test)` means, tested through their outputs
  rather than through the helper.
- Step 2 shows they agree *because they are the same code*: removing
  `scripts/lib/rust_source.rb` stops both, and neither carries a private copy.
- Step 3 exits 0 with `FR-128 governance ledger tooling: 8 passed, 0 failed`, confirming the
  extraction of the scanner into a library was behaviour-neutral for the gate that owned it.

---

## Certification conditions

A run of this document counts as closure evidence only when all of the following hold:

1. `git status --porcelain` is empty at start and at end.
2. `git rev-parse HEAD` matches before and after.
3. Each script is invoked as `bash <script> > log 2>&1` with `$?` captured directly, never
   through a pipe.
4. Each log ends with the script's own summary line.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | The boundary is frozen in both directions | ☑ PASS | 2026-07-25 | Claude | Gate reports 143 files / 52 pub mod / 924 public items / 200 rusqlite refs across 37 files / 6 dependent crates; emitted candidate byte-identical to the ledger. |
| 2 | Each case rejects its own defect | ☑ PASS | 2026-07-25 | Claude | `9/9`. Nine mutations run; every case failed against at least one. M7 exposed case 9 passing for the wrong reason, and it was rewritten before shipping. Working tree byte-identical afterwards. |
| 3 | The migration chain reproduces the reviewed schema | ☑ PASS | 2026-07-25 | Claude | 46 tables, 92 indexes. The doctored-snapshot fixture failed naming `CREATE TABLE tasks` while the real snapshot passed in the same run. |
| 4 | Migrations are idempotent and resume to the same schema | ☑ PASS | 2026-07-25 | Claude | All 74 interruption points reach the one-shot schema; a second bootstrap applies 0 and changes nothing. |
| 5 | The two governance gates share one scanner | ☑ PASS | 2026-07-25 | Claude | Case 6 and case 9 both pass; FR-128's gate stayed `8/8` across the extraction, so the move was behaviour-neutral. |
