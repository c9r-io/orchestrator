---
lifecycle: active
related_fr: FR-130
---

# DD-148: Persistence Crate Extraction (FR-130 Phase A)

**Status**: Implemented (FR-130 Phase A; Phases B and C remain open)
**Related**: DD-142 (core boundary freeze), DD-147 (persistence dependency chokepoint), QA 186, FR-047, FR-048

## Background

`core` (`agent-orchestrator`) was 45% of the workspace with 52 top-level `pub mod` and 924
public items, and its highest-churn cluster was persistence. FR-047 and FR-048 extracted
`orchestrator-config` and `orchestrator-scheduler`; DD-142 froze what remained; DD-147 decided
who may reach the driver. This is the extraction those three were prerequisites for.

Phase A moved the modules whose boundaries were already clear. Phase B (18 files that interleave
SQL with domain logic, one commit each) and Phase C (`error.rs`'s `From<rusqlite::Error>`)
remain open.

### What the extraction moved

| Commit | Moved | core `rusqlite` after |
|---|---|---|
| A1 | `persistence/{sqlite,migration,migration_steps,schema}.rs` | 194 |
| A2 | `async_database.rs`, `task_repository/**`, `db_write.rs`, `dto.rs`, `now_ts` | 118 |
| A3 | `persistence/repository/{session,scheduler,workflow_store,daemon_meta}.rs`, `session_store.rs` | 88 |
| A4 | `db.rs`, `db_maintenance.rs` | 86 |

`core`: 143 → 129 scanned files, 52 → 50 top-level `pub mod`, 924 → 665 public items,
200 → 86 `rusqlite` references across 37 → 20 files.

## Design

### The crate sits below core and above the leaf data crates

`orchestrator-persistence` owns `agent_orchestrator.db`: the connections that open it, the
migration chain that shapes it, the repositories that read and write it, and the admin facade
over it. Its only workspace edges are `orchestrator-config` and `orchestrator-collab`.

Those two edges are narrower than they look, and the distinction matters. They are reached from
exactly two fields of two `dto` structs — `ConfigOverview::config` and `RunResult::output`.
Nothing in the migration chain, the connection helpers or the repositories names them. The
chain's output is frozen byte-for-byte in `schema-snapshot.sql`, and an edge *from the chain* to
a crate that can change domain types is an edge along which that schema can move without the
diff saying so. An edge from two DTO fields is not.

QA 186 Scenario 4 asserts the direction rather than assuming it, because neither ledger gate
would notice a `persistence -> core` edge: `core-boundary.rb` counts what is in `core/src` and
`persistence-dependency.rb` checks which crates name the driver. Both stay green while the
extraction becomes a directory rearrangement.

### `dto.rs` moved whole

Seven of its fifteen structs are the row shapes the repositories return. The other eight —
`LogChunk`, `RunResult`, `TicketPreviewData`, `CreateTaskPayload` and the rest — are passengers.

The alternative was splitting the file so only the row shapes sank, which is better layering and
worse in every practical respect: a 371-line cohesive module in two crates, two places to look,
and a boundary that has to be re-litigated every time a DTO gains a field. Moving it whole costs
the two edges above. `core` re-exports it as `pub use orchestrator_persistence::dto`, so all 104
external `agent_orchestrator::dto::*` references are untouched.

### `now_ts` moved with the rows

`now_ts()` is `chrono::Utc::now().to_rfc3339()` — one line, and the obvious thing is to leave it
in `core::config_load` and let the persistence crate define its own. That is wrong here. The
format is a database contract: every caller in this repository is writing a `created_at` or
`updated_at` column, and a second definition beside the first is how two rows in one table come
to disagree about what a timestamp looks like. It lives in the persistence crate;
`core::config_load::now_ts` is a re-export, so its other call sites did not move.

### The tests stayed in core, and the shim modules exist for them

`core/src/{async_database,db,db_write,session_store}.rs`,
`core/src/task_repository/mod.rs` and `core/src/persistence/repository/{scheduler,workflow_store,daemon_meta}.rs`
are re-export shims holding `#[cfg(test)] mod tests`.

Those tests are not unit tests of the repositories. They create a real task through
`task_ops::create_task_impl` against a `test_utils::TestState` fixture and then assert on what
was persisted, which makes them the behavioural evidence that Phase A was a move and not a
rewrite — and simultaneously makes them unmovable, because the domain machinery they drive sits
above this layer.

Keeping them cost two things, both recorded rather than hidden:

- `task_repository::{queries, state, types, trait_def, command_run}` became public modules,
  because 32 test call sites reach their connection-level free functions directly. Twenty of
  those functions gained the doc comments `#![deny(missing_docs)]` then required. That is the
  honest outcome: they were already used from outside their module, so they were already API,
  and the lint turned an implicit fact into documentation.
- `#[cfg(test)] use rusqlite::params;` at file scope had to move inside `mod tests`. At file
  scope it survives the ledger scanner's `cfg(test)` stripping — the scanner excludes
  `#[cfg(test)] mod X { … }` blocks, not attributed items — and held `db_write.rs` on the ledger
  at one reference for a line no production build compiles.

The alternative was rewriting those 32 call sites onto the public repository surface. It is
better design and it was rejected, because a rewrite cannot claim behaviour is unchanged: it
moved nothing to compare.

### `persistence/repository/config.rs` did not move, against FR-130's Phase A list

It holds 17 production references to `crate::crd` plus `crate::resource`, and
`crd/plugins.rs:328` calls `crate::db::insert_plugin_audit` — `db.rs` being an A4 file. Sinking
both while `crd` stays in `core` closes a `persistence -> crd -> persistence` cycle.

The file lives under `persistence/` and is a domain repository. Directory position and
structural category are different facts, and FR-130's Phase A list was built from the first.
It moves in Phase B alongside `crd`.

`session_store.rs` moved the other way, out of Phase B and into Phase A, for the mirror reason:
its whole import set is `async_database`, `now_ts`, `persistence::repository` and `db`, and
Phase A's `repository/session.rs` delegates every operation to it. Phase A could not compile
without it. Both files carry three references, so the phases kept their totals.

### `schema_snapshot.rs` stayed in core

The chain it exercises moved; the test did not. `cargo test -p agent-orchestrator
schema_snapshot` is Phase A's only behavioural comparator, and a test that travels with the code
it tests is not the same test — it has to mean in the "after" what it meant in the "before". It
follows the chain in Phase C, when core stops re-exporting it.

Keeping it also left `CONTRIBUTING.md`, DD-142 and QA 180 true without edits, which is a
consequence rather than the reason.

### The resume sweep now asserts its own extent

`an_interrupted_chain_resumes_to_the_same_schema` iterates `1..=registered_migrations().len()`.
That reads as exhaustive, and reading as exhaustive is the problem: a `.step_by(5)` inserted to
make the test faster leaves it passing over a fifth of the chain with nothing in the output
saying so.

It now counts its iterations and compares them against
`SELECT COUNT(*) FROM schema_migrations` on a one-shot bootstrapped database. A shortened
*chain* was already caught by the snapshot comparison; a shortened *sweep* was caught by
nothing.

### `db.rs`'s two daemon-state entry points

`reset_db` and `reset_project_data` took `&InnerState`. A persistence layer that has to name
core's daemon state is a layer only in the directory listing. Both were already reading one
field, so `reset_project_data_by_path` joins the existing `reset_db_by_path` and `core` keeps
two three-line wrappers with the signatures its callers use.

## Verification

QA 186 covers five scenarios. The shape each one is guarding against:

| Scenario | Proxy | What observes the fact |
|---|---|---|
| 1 | `cargo tree` shows the edge | core compiled with the dependency **commented out** must fail |
| 2 | the sweep passes | the sweep's extent compared against the applied migration rows |
| 3 | the round trip returns data | the same calls against an unmigrated database must error |
| 4 | no `agent_orchestrator::` in the sources | cargo's resolved tree has no path to core |
| 5 | the snapshot file is unchanged | its commit is an ancestor of the first extraction commit |

The reverse-applicable removal patch was executed once against `524ed26b`, the commit at which
Phase A finished: `git revert --no-commit` over the four extraction commits named individually,
newest first, applied with no conflicts across 44 paths, `cargo check --workspace` finished
clean, and both gates reported `143 / 52 / 924`, `200 / 37`, `13 members` — the ledgers returned
to their pre-extraction values along with the code.

Named individually rather than as the range `A1^..A4`, because an unrelated commit landed
between A1 and A2 while Phase A was in progress. A range revert takes it too and proves that
some set of commits reverts, not that the extraction does; the first run of this proof made that
mistake and reverted 45 paths instead of 44.

Recorded in QA 186 rather than run in CI: a gate that hard-codes commit hashes fails permanently
after any history rewrite.

### Two of DD-142's own fixtures were pinned to numbers this change moved

`test-core-boundary.sh` case 3 asserted the gate's report said `coreSurface.pubMod 52 -> 53`,
and case 5 stripped `rusqlite` from `core/src/db.rs` and expected the removal to be reported.
Phase A broke both, in the two different ways a pinned fixture breaks:

- Case 3 **failed loudly**. The gate did its job and said `50 -> 51`; only the fixture's literal
  was stale.
- Case 5 **passed vacuously**, which is worse. `core/src/db.rs` still exists as a re-export shim
  whose only `rusqlite` token sits inside `mod tests`, where the scanner does not count it.
  Stripping the file changed nothing, so the gate reported success and the case reported that
  the gate had failed to notice a removal — a negative fixture that had stopped applying a
  mutation at all.

Both now read the ledger: case 3 takes `coreSurface.pubMod` and adds one, case 5 takes
`rusqlite.files.keys.min`, which is by construction a file with at least one production
reference. A gate whose entire subject is a set of numbers that are supposed to move cannot have
fixtures that only work while they do not.

## Consequences

### What Phase A established

- One crate owns the orchestrator database. `persistence-dependency.rb` records it as the second
  `persistence` role; `core` keeps the role until Phase C, because it still holds `config.rs`
  and the eighteen Phase B files.
- The layer cannot reach back into `core`, asserted rather than assumed.
- Both governance ledgers moved in the same commit as the code that moved them. That was not
  cosmetic: both compare by exact equality in both directions, so a commit that moves code
  without moving the ledger leaves the gate red until a later commit rescues it, and reverting
  that later commit then breaks the earlier one. Per-commit ledger updates are what make the
  four commits independently revertible.

### Accepted costs

- Eight modules in `core` are re-export shims that exist to host tests. They read as
  implementations in a directory listing and are not.
- `orchestrator-persistence` takes `libc`, unix-gated. `session_store` classifies a session row
  by whether its PID is still live, so the liveness check is part of what the row means rather
  than a process-control facility that wandered in. Splitting it out would have split the
  classification.
- The persistence crate's public surface grew by 20 documented functions and one constant
  (`HISTORICAL_AGENT_PLACEHOLDER`) that were previously `pub` inside private modules or
  `pub(crate)`. Net across the workspace the surface still fell sharply — core alone lost 259
  public items.

### Known limits

- **Phase A converged 114 of its 115 references, not 115.** The one left is
  `core/src/migration.rs`'s `use rusqlite::Connection`. That file is three wrappers over
  `persistence::migration` kept "for compatibility" with nobody: no crate outside `core` names
  `agent_orchestrator::migration`, and its only caller inside `core` is `action_audit.rs`'s test
  module. Converging it means either retiring dead public API — a decision, not a move — or
  adding a `run_pending_count` to the persistence crate that exists only to drive a count to
  zero. The second is worse than the residual it removes. It belongs to Phase B's per-file
  judgement.
- **The ledger counts the token `rusqlite`, not SQL statements.** `db_write.rs` is 1,441 lines
  of SQL and counted 1; `db.rs` is 1,104 lines and counted 1. Phase A's "115 references" was
  ~12,100 lines and Phase B's "83" is 17,963 — the smaller number is the larger phase. The count
  is a sound ratchet, because a new coupling adds a token wherever it lands, and it is not a
  size estimate. Anyone planning Phase B from the number will plan it wrong. (This limit belongs
  to DD-142's ruler and is recorded there too.)
- **The chain has 37 registered migrations, not 74.** Five documents said 74 — DD-142, QA 180,
  the CHANGELOG, FR-130 and this repository's two README indexes — since FR-130's requirement 3
  closed. 74 is what `grep -c m00` over `migration.rs` returns, because each entry names its
  step twice, in `name:` and in `up:`. The sweep always ran the right number of interrupt points
  and the schema baseline was always right; only the prose was wrong. It was found by the first
  mutation test of the new extent assertion, whose failure message reports the real count. All
  five documents are corrected.
- The `schema_snapshot` test sits in `core` and exercises a crate below it. Deliberate and
  temporary, as above; it is the one place where the test and the code it tests are in different
  crates on purpose.
