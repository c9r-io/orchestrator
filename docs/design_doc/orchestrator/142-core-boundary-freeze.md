---
lifecycle: active
---

# DD-142: Core Crate Boundary Freeze And Migration Schema Baseline

**Status**: Implemented (FR-130, requirements 1 and 3; requirements 2 and 4 remain open)
**Related**: DD-140 (governance ledger regeneration), DD-139 (QA gate enforcement surface), FR-047, FR-048, QA 180

## Background

`core` (`agent-orchestrator`) is 79,659 lines across 143 scanned files — 45% of the
workspace — with 52 top-level `pub mod` and 924 public items. FR-047 and FR-048 extracted
`orchestrator-config` and `orchestrator-scheduler`; core is still the god crate underneath
them, and its highest-churn cluster is persistence: `core/src/lib.rs` has been edited 64
times in 1,022 commits, `migration.rs` 48, `persistence/migration.rs` 28,
`persistence/migration_steps.rs` 27.

FR-130 proposed extracting `orchestrator-persistence` and freezing a boundary ratchet behind
it. Verification against the codebase split the proposal in two: the freeze is sound and
buildable now, and the extraction as specified cannot reach its own acceptance criterion.

### A corrected premise

FR-130's requirement 2 named the files to move: `persistence/**`, `db.rs`, `db_write.rs`,
`async_database.rs`, `migration.rs`, `migration_steps.rs`, `task_repository/**` — 11,049
lines. Its acceptance criterion was that core would no longer depend on `rusqlite` directly.

Those two statements are incompatible. **37 core files carry 200 `rusqlite` references** once
inline `cfg(test)` modules are excluded, and roughly 22 of them are not in the list:
`trigger_engine.rs` (18 references), `action_audit.rs` (9), `service/bootstrap.rs` (7),
`source_automation.rs` (7), plus `event_cleanup.rs`, `attention.rs`, `source.rs`,
`task_ops.rs`, `session_store.rs`, `handoff.rs`, `process_metrics.rs`, `config_load/**`.
These are not persistence modules that were overlooked; they interleave SQL with domain logic
inside the same functions, which is why they were never in a persistence directory to begin
with. `error.rs` additionally holds `impl From<rusqlite::Error> for OrchestratorError`, so
core's error type is coupled to the driver regardless of which files move.

Two further claims did not survive:

- **`core` is not the persistence chokepoint.** Six crates declare `rusqlite` directly —
  `core`, `daemon`, `orchestrator-scheduler`, `orchestrator-security`, `slack-gateway`,
  `integration-tests` — across 19 non-core files. A port trait defined in core does not stop
  that; those crates would take a direct dependency on the new crate instead, which inverts
  the goal. The extraction has a second axis the FR never budgeted for.
- **The `#[allow(clippy::too_many_arguments)]` count is workspace-wide, not core's.** 43 is
  the workspace total; core holds **3**. The concentration is `orchestrator-scheduler` (21),
  `gui` (7), `daemon` (7). FR-130 read the number as evidence of coupling inside core and
  derived a requirement from it.

Smaller corrections, all in the direction of "nobody had measured this": 741 public items
became 924 once `pub async fn` was counted (the FR's regex omitted it) and 710 once
`cfg(test)` items were excluded; 51 tables became **46 tables and 92 indexes**; the file
`core/src/migration_steps.rs` named by requirement 2 does not exist.

The order of work follows from all this. Recording the schema baseline *after* the extraction
would prove nothing — there would be nothing to compare against. So the baseline is committed
first, and the extraction is checked against it later.

## Design

### One scanner, two ledgers

`scripts/lib/rust_source.rb` holds `rust_source_files`, `strip_test_modules`,
`scannable_source`, `relative_path` and `ledger_json`. Both
`scripts/qa/coordination-governance.rb` and `scripts/qa/core-boundary.rb` require it.

This is not tidiness. Stripping inline `cfg(test)` modules moves the core `rusqlite` count
from 237 to 200 and the file count from 43 to 37, so the scan *is* the number. Two
implementations that drift produce two reviewed states, both of which look correct in
isolation — which is the shape of defect FR-128 spent its budget on.

The library lives under `scripts/lib/` rather than `scripts/qa/lib/` deliberately. The FR-127
enforcement surface enumerates `ls scripts/qa/*.sh scripts/qa/*.rb` non-recursively
(`test-qa-gate-surface.sh:44`), so a ruby file under `scripts/qa/lib/` would be a gate-shaped
file the surface manifest structurally cannot see. Exploiting that blind spot to avoid
classification would be a governance evasion inside the governance tooling. A library outside
the governed directory says what it is.

### The boundary ledger

`config/governance/core-boundary-ledger.json` records:

| Section | Content |
|---|---|
| `scope` | prose defining exactly what is counted, checked against the gate's own constant |
| `coreSurface` | `files` 143, `pubMod` 52, `publicItems` 924 |
| `rusqlite` | `total` 200 and a per-file map of 37 entries |
| `rusqliteDependentCrates` | the six crates taking `rusqlite` directly |

The per-file map is the ratchet and the extraction work-list at once. FR-130's remaining half
inherits a machine-readable inventory rather than the prose list that was wrong.

`scope` is compared, not merely stored. FR-128's lesson was that scope prose and scan
behaviour drift silently — the coordination ledger claimed to exclude `cfg(test)` modules
while the scanner stripped only a trailing `mod tests`. Comparing the string means the ledger
cannot describe a measurement the gate does not perform.

### Exact equality, not the monotonic ratchet

FR-130 requirement 4 asked for counts that are 单调不增 — monotonically non-increasing. The
gate compares by exact equality instead.

Under a monotonic rule a decrease passes silently, and the ledger goes on asserting debt the
repository no longer carries: green, and saying something false. FR-128 found
`capturesOrJsonPath` sitting at 54 against a reviewed 55 for exactly that reason, undetected
for a full FR cycle. Here the asymmetry is sharper still, because a decrease is the *goal* —
the extraction's entire purpose is to drive these counts down. Under the monotonic rule, the
one event the ledger exists to record would be the one event it ignores.

The cost of tightening is a regeneration rather than an argument: `--emit-baseline` prints the
candidate, `--emit-baseline --write` applies it, and `--write` refuses under `CI` for the
reason DD-140 gives — a regenerated ledger is a proposal for a human to read in a diff, and in
CI there is no human.

### The schema baseline

`config/governance/schema-snapshot.sql` is the normalised `sqlite_master` output after the 74
registered migrations run against an empty database: 46 tables and 92 indexes, one statement
per line, sorted. Runs of whitespace are collapsed so that reindenting a migration's DDL is
not a schema change, while a column, type, constraint or index change still is. `sqlite_%`
objects are excluded — SQLite names its own autoindexes, so they are an engine artifact rather
than a reviewed decision.

`core/src/persistence/schema_snapshot.rs` holds the chain to it, as `cargo test` rather than a
shell gate because the workspace test job is already `ci-required` and QA §4.6 prefers unit
tests:

| Test | What would falsify it |
|---|---|
| `full_chain_reproduces_the_reviewed_snapshot` | any migration that changes the schema without updating the snapshot |
| `registered_versions_are_unique_and_ascending` | a duplicated or out-of-order version |
| `a_second_bootstrap_applies_nothing_and_changes_nothing` | a non-idempotent step; the snapshot is compared as well as the applied count, because "applied zero" alone would pass for a chain that re-ran its DDL while altering the schema |
| `an_interrupted_chain_resumes_to_the_same_schema` | a step whose effect depends on being applied in the same pass as its neighbours |

The resume test runs all 74 interruption points rather than a sample. A resume defect lives in
one specific migration, and sampling is precisely how you miss it.

Two environment overrides: `UPDATE_SCHEMA_SNAPSHOT=1` rewrites the fixture, and
`SCHEMA_SNAPSHOT_PATH` redirects the comparison. The second is what makes the negative fixture
cheap — the QA gate points the comparison at a doctored copy under `$TMPDIR`, with no rebuild
and no write to the working tree.

Independently of the extraction, this closes a gap that existed today: adding a migration
changed the schema of 46 tables with no reviewable artifact. Every migration now arrives with
its schema delta in the same diff.

### Verification by mutation

`scripts/qa/test-core-boundary.sh` runs nine cases. Each was confirmed to fail against a
mutation that breaks the specific thing it targets:

| Mutation | Cases that failed |
|---|---|
| M1 drop the rusqlite comparison | 4, 5 |
| M2 revert to the monotonic ratchet | 5 |
| M3 strip only modules literally named `tests` | 1, 2, 6, 9 |
| M4 remove the `CI` guard from `--write` | 7 |
| M5 emit a candidate missing a field | 2 |
| M6 make the schema comparison always succeed | 8 |
| M7 give the gate a private copy of the scanner | 3, 4, 5, 6, 7, 9 |
| M8 drop `pubMod` from the surface comparison | 3 |
| M9 drop the added-file branch of the rusqlite report | 4 |

M3 moves cases 1 and 2 as well as 6, and that is honest rather than sloppy: a change to what
the scan means changes the ledger, so the whole gate should notice. Case 6's distinct
contribution is that it names the cause and covers *both* gates with a single probe.

The run also caught a defect in the gate's own case 9, which is why it is written the way it
is. Case 9 originally asserted only that both gates fail once `scripts/lib/rust_source.rb` is
removed. Under M7 it passed — while the mutation it was written to catch was in place. Both
gates did fail, but the boundary gate failed because its private copy was not in the case
directory, not because the shared library was gone. "Both fail after the removal" is satisfied
by a gate that was already broken. It now asserts both gates **pass** with the library present
and fail without it, which is what makes the failure attributable to the removal.

## Review workflow

When core grows a module or a rusqlite reference:

```bash
ruby scripts/qa/core-boundary.rb                       # names the file and the count that moved
ruby scripts/qa/core-boundary.rb --emit-baseline       # print the candidate and read it
ruby scripts/qa/core-boundary.rb --emit-baseline --write   # apply it locally
```

When a migration changes the schema:

```bash
UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p agent-orchestrator schema_snapshot
```

Commit the regenerated ledger or snapshot **in the same commit as the change that caused it**.
For the snapshot this is a review property: the diff is the only place the schema change is
legible. For the boundary ledger it is the same constraint DD-140 states for the coordination
ledger, and for the same reason — an intermediate revision that fails the gate is a revision
nobody can bisect through.

## Consequences

### What the freeze established

Three numbers in FR-130 were wrong in the direction of "nobody had counted": public items
(741 → 924), tables (51 → 46 plus 92 indexes), and the rusqlite inventory (14 named files → 37
actual). None of these were discoverable by reading the FR; all three came out of building the
measurement. That is the argument for freezing before extracting, restated as evidence.

### Accepted costs

- `test-core-boundary.sh` invokes `cargo test -p agent-orchestrator schema_snapshot` twice for
  case 8. The `governance` job already builds the workspace, so this is cache-warm, but it is
  a real dependency on the Rust toolchain in a job that is otherwise ruby and jq.
- The `publicItems` count matches FR-130's item kinds plus `pub async fn`. `pub use`
  re-exports are not counted, so a crate split that only re-exports would not move the number.
  Recorded here rather than silently assumed.
- `rusqlite` is counted by textual occurrence, not by resolved type. A comment mentioning
  rusqlite counts. This over-counts slightly and never under-counts, which is the safe
  direction for a ratchet.

### Known limits

- The boundary is frozen, not enforced by the compiler. Nothing stops a new `rusqlite`
  reference from being written; the gate only stops it from being written *unreviewed*.
- The schema snapshot certifies DDL, not data. A migration that backfills rows incorrectly
  produces an identical snapshot. `m0002_backfill_historical_defaults` and its kin are covered
  by their own tests, not by this one.
- FR-130 requirements 2 and 4 remain open, and the FR is updated in place rather than closed.
  The extraction now has a corrected inventory, a schema baseline to be checked against, and a
  ratchet that will notice if core grows while it waits.
