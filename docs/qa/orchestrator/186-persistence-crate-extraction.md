---
lifecycle: active
related_fr: FR-130
self_referential_safe: true
---

# Orchestrator - Persistence Crate Extraction (FR-130 Phase A)

**Module**: Governance / Persistence
**Scope**: `crates/orchestrator-persistence` extraction — linkage, migration-chain resume extent, write/read round trip, layer direction, driver-error removal, schema baseline
**Scenarios**: 5
**Priority**: High

## Background

FR-130 Phase A moved the persistence layer out of `core` into
`crates/orchestrator-persistence`, in four independently revertible commits. `core` went from
143 to 129 scanned files, 52 to 50 top-level `pub mod`, 924 to 665 public items, and 200 to 86
`rusqlite` references across 20 files.

Everything in that paragraph is structural, and every one of those numbers would be equally
true of a crate that persists nothing and of a `core` that declares the dependency without
using it. [QA 180](180-core-boundary-freeze.md) and
[QA 185](185-persistence-dependency-chokepoint.md) already hold the structural half through
`core-boundary.rb` and `persistence-dependency.rb`. This document covers what those gates
cannot see.

All scenarios are read-only against the working tree except for a `git archive` copy under
`$TMPDIR`. None starts a daemon, invokes a provider, or opens
`~/.orchestratord/agent_orchestrator.db`. The only databases created are temporary SQLite files
inside `tempfile::tempdir()`.

Primary entry points:

```bash
./scripts/qa/test-persistence-extraction.sh            # all five cases
cargo test -p orchestrator-persistence --test round_trip
cargo test -p agent-orchestrator schema_snapshot
```

---

## Scenario 1: The Boundary Is Real, And One-Directional

### Preconditions

- Clean worktree at the commit under test.
- `cargo` available; the workspace has been fetched.

### Steps

1. `cargo tree -p agent-orchestrator --depth 1 | grep 'orchestrator-persistence v'` — the edge
   exists.
2. Inspect the fixture the gate builds: a `git archive HEAD` copy under `$TMPDIR` whose
   `core/Cargo.toml` has the `orchestrator-persistence = ...` line **commented out**, then
   `cargo check -p agent-orchestrator` inside it.
3. `cargo tree -p orchestrator-persistence | grep 'agent-orchestrator v'` — expect no match.
4. `grep -rn 'agent_orchestrator::' crates/orchestrator-persistence/src` — expect no match.
5. `bash scripts/qa/test-persistence-extraction.sh` and read Cases 1 and 4.

### Expected result

- Step 1 prints the edge; step 2 exits non-zero with `orchestrator_persistence` in the log;
  steps 3 and 4 are empty.
- Cases 1 and 4 report two passes each.

Steps 1 and 2 are the "core is built on this crate" half. The second carries it: a declared
dependency that no source names still satisfies `cargo tree`, so the tree query alone cannot
distinguish "built on" from "listed". Compiling without it can. The mutation is a comment, not a
deletion, deliberately — deletion is the case an author has in mind while writing the check, and
a manifest reader that strips comments would pass a deletion test while accepting a commented-out
dependency as present.

Steps 3 and 4 are the direction half, and it is the invariant neither ledger gate checks:
`core-boundary.rb` counts what is in `core/src`, `persistence-dependency.rb` checks which crates
name the driver, and a `persistence -> core` edge would leave both green while making the
extraction a directory rearrangement. Step 3 reads cargo's own resolution, so a transitive path
through a third member fails it as well as a direct one. Step 4 is the weaker check, kept as a
second condition rather than the only one: a core path in those sources would not compile today,
but it is the edit that would change step 3's answer.

The crate's only workspace edges are `orchestrator-config` and `orchestrator-collab`, both leaf
data crates, reached from two fields of two `dto` structs.

---

## Scenario 2: The Resume Sweep Covers Every Migration The Chain Applied

### Steps

1. `cargo test -p agent-orchestrator schema_snapshot::tests::an_interrupted_chain_resumes_to_the_same_schema`
2. Run the same test against a `git archive` copy whose loop header has been rewritten from
   `for stop_after in 1..=total` to `(1..=total).step_by(5)`. The gate does this as Case 2's
   second assertion.

### Expected result

- The test passes.
- After the loop it compares the number of interrupt points it exercised against
  `SELECT COUNT(*) FROM schema_migrations` on a one-shot bootstrapped database, and separately
  against `registered_migrations().len()`. All three are 37 today.

  Four other documents said 74. That is `grep -c m00` over `migration.rs`, which matches each
  entry twice — once in `name:` and once in `up:` — and it had been repeated since FR-130's
  requirement 3 closed without anyone re-deriving it. The sweep always ran the right number of
  points; only the prose was wrong. The assertion below is what surfaced it, on its first
  mutation test.

`for stop_after in 1..=total` reads as exhaustive, which is exactly why it needs an assertion:
a `.step_by(5)` or a `.take(10)` inserted to make the test faster leaves it passing while
covering a seventh of the chain, and nothing in the output would say so. Comparing the extent
against what the database records as applied is the difference between a sweep and a claim of
one.

A shortened *chain* is caught by `full_chain_reproduces_the_reviewed_snapshot`; a shortened
*sweep* is caught only here.

Step 2 is run, not read. The first version of this check grepped
`core/src/persistence/schema_snapshot.rs` for the assertion's message — which the assertion
satisfies by existing, including commented out, since a comment contains the same text. The
mutation is `step_by(5)` rather than a deleted line because shortening for speed is the
realistic edit and it leaves every iteration that does run correct, which is precisely the case
the schema comparison cannot distinguish from a full sweep.

Expected mutant output: `the resume sweep exercised 8 interrupt point(s) but the chain applied
37 migration(s)`.

---

## Scenario 3: A Task Written Through The Layer Reads Back Through The Layer

### Steps

1. `cargo test -p orchestrator-persistence --test round_trip`

### Expected result

Two tests pass.

`a_task_written_through_the_layer_reads_back_through_the_layer` bootstraps a real database
through the whole registered chain, then drives one task across every module the extraction
touched — `sqlite`, `schema`, `async_database`, `task_repository`, `db_write`, `session_store`,
`db` — reading each write back through a **different** module than wrote it: written
synchronously, read asynchronously; written through `db_write`, read through the repository;
written through `session_store` on one connection, read on another; and finally counted through
the admin facade.

`the_layer_fails_loudly_on_a_database_that_never_migrated` is the negative half, and it is
there because the positive half cannot stand alone. Every assertion in it runs against a
bootstrapped database, so all of them would also hold if `PersistenceBootstrap` were the only
thing still working and every reader returned empty. Against an unmigrated database the layer
must error, not return a plausible nothing.

The one row seeded with raw SQL is the initial `tasks` row: task creation is domain logic in
`core::task_ops`, above this layer. Core's own `task_repository` and `db_write` test modules
drive that path from the other side — they call `create_task_impl` against a `TestState`
fixture and assert on what was persisted — and they stayed in `core` for exactly that reason.

---

## Scenario 4: Core's Error Type No Longer Converts Driver Errors

### Preconditions

- **Clean worktree.** The gate refuses to run otherwise, and this scenario is why: its fixture is
  built with `git archive HEAD`, so on a dirty tree it answers a question about the previous
  commit. The first run of this case reported that the conversion still existed, because at HEAD
  it did.

### Steps

1. `grep -E '^impl From<rusqlite::Error> for OrchestratorError' core/src/error.rs` — expect no
   match.
2. In a `git archive HEAD` copy, append a documented function to `core/src/error.rs` that applies
   `?` to a `rusqlite::Result` in a function returning `Result<i64>` (the crate's
   `OrchestratorError` alias), then `cargo check -p agent-orchestrator`. The gate does this as
   Case 5's second assertion.

### Expected result

- Step 1 is empty.
- Step 2 exits non-zero with ``couldn't convert the error to `OrchestratorError` ``.

Step 1 is anchored to a line opening an `impl` block, not a substring search. The first version
searched for the text anywhere in the file and was satisfied by the doc comment explaining that
the impl had been removed — a gate its own explanatory prose can trip is measuring the prose.

Step 2 is what makes the scenario mean anything. Grepping one file cannot distinguish "removed"
from "moved to another module and still compiling", and the capability is what matters, not the
location of a line. The probe carries a doc comment deliberately: core denies `missing_docs`, so
without one the build stops on the lint and the case passes on an error unrelated to the
conversion — which is why the assertion matches the specific diagnostic rather than a non-zero
exit.

The category the deleted impl guaranteed is asserted separately, in
`core::service::resource::delete`'s
`phase_c_preserves_the_external_dependency_category`: a real error from a real unmigrated
database, mapped through the production function, must be `ExternalDependency`. Mutating that
function to `classify_resource_error` makes it fail with `left: NotFound, right:
ExternalDependency`, because SQLite's phrase for a missing table is `no such table: resources`
and the message classifier reads `not found` anywhere in the text as `NotFound`.

---

## Scenario 5: The Reviewed Schema Baseline Is Unchanged, And Predates The Extraction

### Steps

1. `git diff --quiet -- config/governance/schema-snapshot.sql`
2. Confirm the commit that last touched the snapshot is an ancestor of the first extraction
   commit (`git log --grep='FR-130 A1'`).

### Expected result

The snapshot is byte-identical across all four extraction commits, and it was committed before
the first of them.

Step 2 is what makes step 1 mean something. A baseline recorded after the change it is supposed
to measure is a record of the outcome, not a comparison — FR-130's own requirement 3 exists
because that ordering has to be established first, and this asserts it from history rather than
from prose.

---

## Reverse-applicable removal patch

Recorded here rather than run in CI: it is a one-time property of a specific commit range, and a
gate that hard-codes commit hashes fails permanently after any history rewrite.

Executed on 2026-07-26 in a scratch `git worktree` pinned at `524ed26b`, the commit at which
Phase A finished:

```bash
W=$(mktemp -d); git worktree add -q --detach "$W" 524ed26b; cd "$W"
A1=$(git log --format=%H --grep='FR-130 A1' -1); A2=$(git log --format=%H --grep='FR-130 A2' -1)
A3=$(git log --format=%H --grep='FR-130 A3' -1); A4=$(git log --format=%H --grep='FR-130 A4' -1)
git revert --no-commit --no-edit "$A4" "$A3" "$A2" "$A1"   # newest first
cargo check --workspace
ruby scripts/qa/core-boundary.rb && ruby scripts/qa/persistence-dependency.rb
```

The four commits are named individually rather than as the range `A1^..A4`. An unrelated commit
(`c0730b78`, a liveness-ledger refresh) landed between A1 and A2, and a range revert takes it
too — which proves that *some set of commits* reverts, not that the extraction does. The first
run of this proof used the range and reverted 45 paths; the correct set is 44.

Result: the revert applied with no conflicts across 44 paths, `cargo check --workspace` finished
clean, and both gates reported the pre-extraction state exactly — `143 files, 52 pub mod, 924
public items`, `200 rusqlite references across 37 files`, `13 members`. The ledgers returning to
their original values is the part worth noting: it shows the revert restored the governance
record along with the code, rather than leaving a tree that compiles while its ledgers describe
a layout that no longer exists.

The worktree must be pinned at `524ed26b` rather than at `HEAD`. Reverting the extraction from a
later commit conflicts on `crates/orchestrator-persistence/src/db.rs`, because the closure
commit edits a file A4's revert deletes. That is expected and is not a defect in the revert: the
property being proved is that Phase A can be backed out of the tree Phase A produced.

---

## Certification conditions

A run of `scripts/qa/test-persistence-extraction.sh` counts as closure evidence only when:

1. `git status --porcelain` is empty at start and at end.
2. Nothing else is writing to the repository during the run.
3. `git rev-parse HEAD` is identical before and after.
4. The script is invoked as `bash scripts/qa/test-persistence-extraction.sh > log 2>&1` with
   `$?` captured directly — not piped into `tail` or `head`, which reports the pager's status.
5. The final summary line is present in the log. Its absence means the run terminated early
   regardless of the reported status.

## Phase B statement-level assertions

Phase B moves SQL out one statement at a time, and a ledger entry disappearing proves the
reference moved rather than that the statement still works. Each moved statement therefore gets
an assertion in `crates/orchestrator-persistence/tests/round_trip.rs`, and each is about the
contract rather than about `Ok`:

| Moved statement | What the assertion pins |
|---|---|
| `db::delete_project_resources` | Two projects seeded; only one loses its rows, the row count is the number deleted, and a second call returns 0. A `DELETE` with a non-matching predicate succeeds and affects nothing, so `Ok` proves nothing. |
| `queries::list_terminal_tasks_older_than` | Six tasks whose three exclusions each fail for a different reason — wrong status, too recent, both — plus `LIMIT`, plus a 365-day window selecting nothing. A query returning everything would satisfy an `Ok` check, and auto-cleanup would then delete running work. |
| `events::step_event_rows` | Only the requested event types come back, a two-type list returns both, and an **empty** list returns nothing rather than everything — the filter is the caller's policy, so the query must carry none of its own. |

| `db::backfill_blank_default_scope` | Per-column counts, and the already-scoped task keeps its workspace. Commenting out one `WHERE column = ''` reports `tasks_workspace_id: 2` against an expected `1` — every task in the database silently rewritten to the default workspace, which a "the blank row was filled" assertion would not see. |
| `db::secret_store_resources_reference_key` | A key named only outside a `SecretStore`, and a key-id *prefix*, both answer no. Dropping `kind='SecretStore'` makes the first answer yes and a rotation never finishes. |
| `control_action_audit::{reserve, complete, get, list}` | Which prior row a reservation found, not merely that it found one; a terminal envelope is not rewritten by a second completion; reads do not cross a project. Four mutations, one per guard. |
| `task_repository::creation::{insert_task_with_items, reset_task_item}` | Item order is one-based and follows the given paths; the creation event lands; a duplicate task id is a no-op; a reset clears `last_error` and drops the stale command runs that would otherwise let compensation re-finalize the item. |
| `event_retention::*` | No new assertions — the file arrived with 17 tests already pinned on exactly these statements. Verified they are the reason and not a coincidence: commenting out `AND tasks.status IN (…)` fails three of them, and dropping the archival delete fails three more. |
| `source_events::*` | Five guards, each mutated and each caught. See below. |
| `source_connections::*` | Sixteen fences, each mutated. Three of them exist in more than one statement — `version=?3` in three, `state='active'` in four, `owner_daemon_id=?3` in two — and every copy got its own mutation. Credential release is asserted in all three refusing directions (wrong project, wrong owner, not active), because any one of them alone leaking the pairing secret is a credential leak. |
| `handoff_store::*` | Four fences, each in exactly one statement (verified by grep, not assumed): the snapshot identity, the reservation's retry identity, `status='planned' AND expected_state_version`, and `status='executing'`. Plus a mutation that restores the *old* shape — an unchecked `UPDATE … WHERE status='planned'` — which fails. |

The two halves that stayed in core are tested without a database at all, which is what the splits
bought: `config_load::build`'s deletion guards against a stub implementation of
`db::DeletionGuardQueries`, and `events::step_events_from_rows` against struct literals.

### Two things the mutations established that the assertions alone would not have

**Five `source.rs` guards were pinned by nothing.** Each was mutated in place before the assertion
was written, and core's 96 `source::` tests stayed green for all five: `complete_routing`'s
`AND routing_state='routing'`, the identical guard in `defer_to_automation` (a separate statement
with its own copy), the claim's `routing_attempts < 5`, `CommandActionStart::RequestMismatch`, and
`INSERT OR IGNORE … == 1`. The first of these was itself a near miss — the initial mutation
replaced the *first* textual occurrence of the guard, which is `defer_to_automation`'s, and passed;
targeting `complete_routing` specifically is what made it fail. Two statements carrying the same
guard need two mutations, not one.

**Four assertions passed their first mutation, and each named a gap in itself rather than in the
code.** They are listed because the shape repeats: an assertion that exercises a statement is not
an assertion that exercises its guard, and the guard needs the input that makes it say no.
`record_delivery` carries two fences and only the monotonic one was pinned, so the
`state='active'` fence needed a *forward* cursor on a suspended connection.
`last_acked_cursor=MAX(…)` was asserted when offered and stored were equal, so it had to move
after the cursor advanced. `update_dedicated_lifecycle` had no fixture at all. And the snapshot
identity's `task_id=?1` needed a second task, since two tasks can reach the same cursor with the
same briefing hash.

**Task creation's transaction has no reachable fixture.** The assertion for
`insert_task_with_items` covers the rows, the ordering and the duplicate-id no-op, but *not*
atomicity, and this is recorded in the test rather than left implied. `INSERT INTO tasks` is the
first statement and the only one a well-formed call can make fail — no later table in that
transaction carries a constraint a caller can violate. Both confirming mutations pass: reordering
the events ahead of the task insert, and deleting the transaction outright. With the only failure
at statement one, rollback has nothing to undo. A green assertion here is not evidence of
atomicity, and the test says so.

**`daemon_id`'s read-back has no reachable fixture either.** Replacing it with `Ok(candidate)`
passes. `INSERT OR IGNORE` is only ignored when another writer inserted between this call's check
and its insert, and one `AsyncDatabase` serializes its writer, so the race needs two processes on
one file. The read-back stays because two daemons can share a database; the test records that it
is defensive code with no coverage rather than letting the assertion above it imply otherwise.

## Checklist

- [ ] Cases 1 and 4: the edge is declared, core fails to compile with it commented out, and
      neither cargo's tree nor the sources hold a path from the layer back to core
- [ ] Case 2: the resume sweep passes, and a `step_by`-shortened copy fails
- [ ] Case 3: every declared round-trip test passes, including the unmigrated-database negative
- [ ] Case 5: no `From<rusqlite::Error>` impl, and a `?` on a `rusqlite::Result` no longer compiles
- [ ] Case 6: the snapshot is unchanged and predates the first extraction commit
- [ ] The gate refuses to run on a dirty worktree (exit 2)
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] Every Phase B batch: `config/governance/schema-snapshot.sql` byte-identical, both ledgers
      re-frozen in the batch's own commit, `phase_c_preserves_the_external_dependency_category`
      passing and unmodified, and the batch revertible when named as a single commit
- [ ] `ruby scripts/qa/core-boundary.rb` and `ruby scripts/qa/persistence-dependency.rb` pass
