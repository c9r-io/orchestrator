---
lifecycle: active
related_fr: FR-130
self_referential_safe: true
---

# Orchestrator - Persistence Crate Extraction (FR-130 Phase A)

**Module**: Governance / Persistence
**Scope**: `crates/orchestrator-persistence` extraction — linkage, migration-chain resume extent, write/read round trip, layer direction, schema baseline
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

## Scenario 1: Core Links The Crate, And Cannot Build Without It

### Preconditions

- Clean worktree at the commit under test.
- `cargo` available; the workspace has been fetched.

### Steps

1. `cargo tree -p agent-orchestrator --depth 1 | grep 'orchestrator-persistence v'`
2. `bash scripts/qa/test-persistence-extraction.sh` and read Case 1.
3. Inspect the fixture the gate builds: a `git archive HEAD` copy under `$TMPDIR` whose
   `core/Cargo.toml` has the `orchestrator-persistence = ...` line **commented out**, then
   `cargo check -p agent-orchestrator` inside it.

### Expected result

- Step 1 prints the edge.
- Step 3 exits non-zero and the log names `orchestrator_persistence`.
- Case 1 reports two passes.

The second assertion is the one that carries the scenario. A declared dependency that no source
names still satisfies `cargo tree`, so the tree query alone cannot distinguish "core is built on
this crate" from "core lists it". Compiling without it can.

The mutation is a comment, not a deletion, deliberately. Deletion is the case an author has in
mind while writing the check; a manifest reader that strips comments would pass a deletion test
and still accept a commented-out dependency as present.

---

## Scenario 2: The Resume Sweep Covers Every Migration The Chain Applied

### Steps

1. `cargo test -p agent-orchestrator schema_snapshot::tests::an_interrupted_chain_resumes_to_the_same_schema`
2. Read the assertions at the end of that test in `core/src/persistence/schema_snapshot.rs`.

### Expected result

- The test passes.
- After the loop it compares the number of interrupt points it exercised against
  `SELECT COUNT(*) FROM schema_migrations` on a one-shot bootstrapped database, and separately
  against `registered_migrations().len()`. All three are 74 today.

`for stop_after in 1..=total` reads as exhaustive, which is exactly why it needs an assertion:
a `.step_by(5)` or a `.take(10)` inserted to make the test faster leaves it passing while
covering a seventh of the chain, and nothing in the output would say so. Comparing the extent
against what the database records as applied is the difference between a sweep and a claim of
one.

A shortened *chain* is caught by `full_chain_reproduces_the_reviewed_snapshot`; a shortened
*sweep* is caught only here.

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

## Scenario 4: The Persistence Layer Does Not Depend On Core

### Steps

1. `cargo tree -p orchestrator-persistence | grep 'agent-orchestrator v'` — expect no match.
2. `grep -rn 'agent_orchestrator::' crates/orchestrator-persistence/src` — expect no match.

### Expected result

Both are empty; Case 4 reports two passes.

This is the invariant Phase A exists to establish, and it is the one neither ledger gate checks.
`core-boundary.rb` counts what is in `core/src`; `persistence-dependency.rb` checks which crates
name the driver. A `persistence -> core` edge would leave both green while making the extraction
a directory rearrangement.

Step 1 reads cargo's own resolution, so a transitive path through a third member fails it as
well as a direct one. Step 2 is the weaker check and is kept as a second condition rather than
the only one: a core path in those sources would not compile today, but it is the edit that
would change step 1's answer, so it is worth naming where it appears.

Today the crate's only workspace edges are `orchestrator-config` and `orchestrator-collab`,
both leaf data crates, reached from two fields of two `dto` structs.

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

## Checklist

- [ ] Case 1: the edge is declared, and core fails to compile with it commented out
- [ ] Case 2: the resume sweep passes and asserts its extent against the applied rows
- [ ] Case 3: the round trip and its unmigrated-database negative both pass
- [ ] Case 4: no dependency and no source path from the layer back to core
- [ ] Case 5: the snapshot is unchanged and predates the first extraction commit
- [ ] `cargo test --workspace` green
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `ruby scripts/qa/core-boundary.rb` and `ruby scripts/qa/persistence-dependency.rb` pass
