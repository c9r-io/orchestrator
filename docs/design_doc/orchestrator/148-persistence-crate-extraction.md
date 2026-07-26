---
lifecycle: active
related_fr: FR-130
---

# DD-148: Persistence Crate Extraction (FR-130 Phase A)

**Status**: Implemented (FR-130 Phases A, B and C; all 18 Phase B files disposed)
**Related**: DD-142 (core boundary freeze), DD-147 (persistence dependency chokepoint), QA 186, FR-047, FR-048

## Background

`core` (`agent-orchestrator`) was 45% of the workspace with 52 top-level `pub mod` and 924
public items, and its highest-churn cluster was persistence. FR-047 and FR-048 extracted
`orchestrator-config` and `orchestrator-scheduler`; DD-142 froze what remained; DD-147 decided
who may reach the driver. This is the extraction those three were prerequisites for.

Phase A moved the modules whose boundaries were already clear. All three phases are now closed:
each of Phase B's eighteen files has a written conclusion, and core is at 9 references across 3
files, every one of them the driver connection type on the layer's own public API — FR-141's
subject, not this FR's. FR-130's per-file disposition table records the conclusion for each of the
eighteen,
including the reference shape and blocking reason for the twelve still open.

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

## Phase B and Phase C

### What the reference count actually measures

Classifying all 83 Phase B references by shape was the finding that shaped the phase:

| Shape | Count | Example |
|---|---|---|
| error-adapter | 28 | `tokio_rusqlite::Error::Other(e.into())`, `-> tokio_rusqlite::Error` |
| sql-params | 21 | `rusqlite::params![…]`, `params_from_iter`, `ToSql` |
| import | 12 | `use rusqlite::{Connection, OptionalExtension, params};` |
| error-construction | 11 | `rusqlite::Error::FromSqlConversionFailure`, `rusqlite::types::Type` |
| connection-type | 6 | `conn: &rusqlite::Connection` |
| row-mapping | 4 | `row: &rusqlite::Row`, `rusqlite::Result<T>` |

About 61% is driver-error plumbing, and SQL text — in string literals — is never counted at all.
Six files carried an identical two-line `fn other(…) -> tokio_rusqlite::Error`, so "18
independent design judgements" overstated the variety.

**The shortcut this makes available, and why it was refused.** Giving `AsyncDatabase` closure
methods that take `anyhow::Result` and deleting those six helpers converges roughly 39 of the 83
**without moving one SQL statement**. Every mixed function would stay exactly as mixed. It is the
same trade Phase A refused for `core/src/migration.rs`, twenty times larger. Phase B's goal is
the per-file disposition; the ledger is evidence of it, not the target.

It was also checked against a second possible justification — DD-147's `forbidden` residual for
`daemon` and `orchestrator-scheduler` — and does not earn its keep there either: their 39
references are 27 `sql-params` and only 9 error-adapter, so the API would not clear the residual.

### The dispositions taken

Six files, one commit each, ledger re-frozen in the same commit as the code:

| File | Disposition |
|---|---|
| `lib.rs` | Five module doc comments described re-export shims as implementations. One held the ledger's only non-code reference. |
| `service/resource/delete.rs` | `DELETE FROM resources` moved to `db::delete_project_resources`. Unblocks Phase C. |
| `config_load/persist.rs` | Production code had zero references; a `#[cfg(test)] use` at file scope was counted as production. |
| `task_cleanup.rs` | Split. Retention query moved to `queries::list_terminal_tasks_older_than`; the cascade delete was a hand-rolled duplicate of an existing async repository method; filesystem cleanup stayed. |
| `config_load/build.rs` | Split via a port. `db::DeletionGuardQueries` replaced `&rusqlite::Connection` in the deletion guards. |
| `events.rs` | Split at the seam already in the code: rows below, payload interpretation above. The event-type filter moved *up* into core as a constant and is passed down. |

Two of them are worth separating from the rest, because they are not refactors: `lib.rs` and
`config_load/persist.rs` were files whose ledger entries were artefacts of what the scanner
counts rather than of code that touches the database. Counting them as Phase B progress without
saying so would misreport nine converged references as nine moved statements.

A second round took five more files and the Phase A residual, same discipline — one file, one
commit, ledgers re-frozen in it:

| File | Disposition |
|---|---|
| `service/bootstrap.rs` | Split. Six blank-scope backfill statements and the SecretStore key probe moved to `db`; rendering `workspace_root`, serializing `qa_targets`, and the `unwrap_or(false)` that keeps the probe advisory stayed. |
| `action_audit.rs` | Split. The `control_action_audit` table and its seven statements moved; the field bounds, the canonical hash, the lifecycle allowlists and the idempotency-conflict rule stayed. |
| `task_ops.rs` | Split. Two duplicated creation paths collapsed into one transaction in `task_repository::creation`; FR-094's diagnostic events became a *builder* rather than a writer. |
| `event_cleanup.rs` | Split. Retention statements moved; JSONL grouping and file writing stayed. |
| `source.rs` | Split. Four tables, 24 statements moved to `source_events`; validation, deterministic id derivation, the retry backoff and the state allowlists stayed. |
| `migration.rs` (Phase A residual) | Retired. Three wrappers, zero production callers workspace-wide. |

### Async at the boundary, not a closure API on `AsyncDatabase`

`action_audit.rs` was the first file whose references were mostly `tokio_rusqlite::Error::Other`
adapters inside `writer().call` closures, and it forced the question this FR had already answered
once: the refused shortcut was to give `AsyncDatabase` closure methods taking `anyhow::Result` and
delete six duplicated `fn other` helpers, converging ~39 references without moving one statement.

The distinction that makes the moves here legitimate rather than the same trade under a new name
is that the closure and its error adapter go **with the SQL**. `orchestrator_persistence::
control_action_audit::reserve` takes `&AsyncDatabase`; the `writer().call` and the
`Error::Other` mapping are inside it, next to the `INSERT` they exist to serve. Core does not
name the driver because it no longer holds the statement, not because a generic helper hid the
name. `AsyncDatabase` gained no methods. FR-141 still owns the API-boundary question.

### The store names its case; the caller says what it means

Three of these files had the same shape: a write, a conditional read, and a rule about what the
read means. Splitting them at the statement boundary would have put the rule below the layer;
splitting them at the operation boundary would have kept the statement above it. Both moves
return a *named case* instead:

- `Reservation::{Claimed, PriorByRetryIdentity, PriorByRequestId}` — `INSERT OR IGNORE` plus one
  of two reads, atomic; the caller compares hashes and produces the two different diagnostics.
- `CommandActionStart::{Started, Restarted, AlreadySucceeded, RequestMismatch}` — the read and
  the write that follows it must be one operation on the writer, so the string comparison stays
  below and only its meaning goes up.
- `bool` from `complete_routing` and `defer_to_automation` — "was it still in a state you could
  close", with the caller deciding that `false` is an error and what to call it.

### What B11's mutations found

Every batch mutates the statements it moved. `source.rs` is the one where that turned up
something: five guards were moved, and all five were confirmed to be pinned by **no test at
all** — each was mutated in place and core's 96 `source::` tests stayed green.

| Guard | What its absence does |
|---|---|
| `complete_routing`'s `AND routing_state='routing'` | A late worker overwrites a routing decision another worker already committed. |
| The same guard on `defer_to_automation` — a separate statement with its own copy | A delivery nobody is routing is handed to the automation worker, and two workers own it. |
| `routing_attempts < 5` on the claim | A poison message is retried forever. |
| `CommandActionStart::RequestMismatch` | A retry key reused under a different request is quietly *restarted* — running a command nobody asked for, under an approval given for another one. |
| `INSERT OR IGNORE … == 1` | A duplicate delivery reports itself as newly inserted and is routed twice. |

The batch's product is not that 24 statements moved. It is that five safety guards were carried
by nothing, and are now carried by
`source_routing_guards_hold_the_line_they_are_there_for`. Moving code is what made anyone look.

### Two error shapes the moves removed

Both were the same defect wearing different clothes: work that is not a database operation being
performed inside a callback that can only return a driver error.

- `event_cleanup.rs` wrote its JSONL archive inside the writer's closure, so `create_dir_all`,
  `open` and `writeln!` failures were reported as `ToSqlConversionFailure` — a full disk
  presented to the operator as a type-conversion error. The file writing is now in core, with
  `anyhow` context naming the path.
- `source.rs` and `handoff.rs` parse a stored JSON payload inside the row mapper, so a
  `serde_json` failure had to become `FromSqlConversionFailure` with an invented column index.
  `source.rs`'s payload now crosses the boundary as text and is parsed in core. `handoff.rs`
  still has this shape; see the remaining work below.

### A third round: `source_connection.rs` and `handoff.rs`

| File | Disposition |
|---|---|
| `source_connection.rs` | Split. Five tables, 23 statements moved to `source_connections`; field bounds, the mode and terminal-status allowlists, what each refused fence means to an operator, and the enum and JSON parsing stayed. |
| `handoff.rs` | Split. Three tables, 18 statements moved to `handoff_store`; the briefing projection, the workspace digest, the state-version hash and the plan rules stayed. |

`handoff.rs` is the file whose reason to move was recorded here before it moved, and it is worth
restating because the reference count never showed it: `task_state_version` runs three `git`
subprocesses and reads every untracked file in the workspace, and **every caller invoked it
inside a `writer().call` closure**. `reserve_execution` did it inside its transaction. On a
database with one writer, that is the write lock held for the duration of an external process
tree. The store now returns *inputs* — `snapshot_inputs`, `boundary_inputs`,
`state_version_inputs`, each one reader closure — and core projects, digests and hashes between
calls.

That move required one deliberate behaviour change and paid for it in the same commit. With the
digest computed before the reservation rather than inside it, `reserve_execution` re-fences
inside its transaction on both `status='planned'` and `expected_state_version`, and writes
nothing if either moved. This is strictly stronger than what it replaced: the old code read the
status, inserted the execution row, then ran `UPDATE … WHERE status='planned'` **without checking
how many rows changed**, so two operators racing could both come away believing they owned the
execution. Restoring the old shape is one of the mutations, and it fails.

### Counting a guard's copies before mutating it

B11 mutated the first textual occurrence of a routing-state guard, hit a different statement than
intended, and passed. From B13 the copy count is established by grep before any mutation, and
recorded:

| File | Guard | Copies |
|---|---|---|
| `source_connections.rs` | `version=?3` | 3 |
| `source_connections.rs` | `state='active'` | 4 |
| `source_connections.rs` | `owner_daemon_id=?3` | 2 |
| `handoff_store.rs` | each of its four | 1 |

Sixteen mutations for `source_connections.rs`, eight for `handoff_store.rs`. Four of those
twenty-four passed on the first attempt and named a real gap in the assertion rather than in the
code:

- `record_delivery` carries two fences. Asserting that a backward cursor is refused pins only the
  monotonic one; the `state='active'` fence needed a *forward* cursor on a suspended connection.
- `last_acked_cursor=MAX(last_acked_cursor,?16)` was asserted when offered and stored were both
  the same value, so removing `MAX` changed nothing. It has to run after the cursor has advanced.
- `update_dedicated_lifecycle` had no fixture at all; it needed its own `managed_dedicated`
  connection.
- The snapshot identity's `task_id=?1` needed a second task. Two tasks can reach the same cursor
  with the same briefing hash — an empty task at cursor 0 is the ordinary case — and without that
  column task B is handed the briefing recorded for task A.

The pattern across all four: an assertion that exercises a statement is not an assertion that
exercises its guard. The guard needs the input that makes it say no.

### One more unreachable fixture, recorded in place

`daemon_id`'s read-back after `INSERT OR IGNORE` cannot be covered. Replacing it with
`Ok(candidate)` passes: the ignore only fires when another writer inserted between this call's
check and its insert, and one `AsyncDatabase` serializes its writer, so the race needs two
processes on one file. The read-back stays, because two daemons can share a database. This is the
second such case — the first was task creation's transaction in B9 — and both are written into
the test rather than left as green assertions that read like coverage.

### The fourth round: `source_automation.rs` and `trigger_engine.rs`

The two largest remaining files, split by the pattern the previous rounds established.

`source_automation.rs` (B15) gave up four tables and 33 statements. It carried the last two
`FromSqlConversionFailure` constructions in `core`: `read_execution_snapshot` parsed two JSON
snapshots inside a row mapper, so a malformed snapshot had to be reported as a column-conversion
failure against an index computed from the string's own length. The snapshots now cross as text
and parse above with the route named. Its row types were all flat columns, so they sank whole and
are re-exported — the shape `ActionAuditRecord` took in B8.

Two smaller shapes changed with it. `adopt_generation` used to read the row and check three rules
in Rust before writing; the three are now fences on the write itself, so a rejection is one
statement rather than a read followed by a write that could disagree with it. And the audit
request id of a new generation is derived in `core` beside the other three id derivations instead
of being formatted inside a transaction.

`trigger_engine.rs` (B16) gave up the `trigger_state` table and the task reads around it — seven
statements. Two decisions that had no name acquired one. `ACTIVE_TASK_STATUSES` was a `matches!`
arm inside a connection closure, and it is what decides whether a `Skip` trigger stays shut.
`trigger_task_name` was `format!("trigger-{}")` written twice, once by the fire path and once by
the cleanup query; if those two ever disagreed, history cleanup would match nothing and the limit
would stop applying with no error anywhere.

### Guards that no fixture can reach

B15 and B16 ran 41 mutations between them. Thirty-five were caught. The six that were not are
recorded here and in the tests, because a guard with no failing mutation and no note beside it
reads as covered:

| Guard | Why nothing reaches it |
|---|---|
| The claim `UPDATE`'s re-check of `attempt_count`, `next_attempt_at` and `lease_expires_at` | The candidate `SELECT` in the same transaction already applied them, and a single-writer database gives the row no chance to move in between. |
| `if changed != 1 { continue }` after that `UPDATE` | Same reason: the `UPDATE` cannot fail to move a row the `SELECT` just admitted. |
| The in-memory installation set in `claim_due` | The SQL occupancy probe catches the same case — a route claimed earlier in the loop already holds an unexpired lease by the time the next candidate is read. The set is a saved query. |
| `status NOT IN ('routed','ignored')` in `transition_leased` | Every transition that reaches a terminal state also releases the lease, so `lease_token=?2` refuses first and the terminal check is never the reason. |
| `delete_tasks`'s empty-list early return | SQLite accepts `IN ()` and matches nothing, so skipping the statement changes no answer. Measured, not assumed. |

Four of these are defence against a second writer that does not exist yet. That is a reasonable
thing to keep and an unreasonable thing to claim a test protects. The fifth and sixth are saved
work rather than guarantees, and both now say so in the code.

Four assertions also passed their first mutation, each naming a gap in itself rather than in the
code — the same shape the third round found. The attempt ceiling and the live-lease filter each
needed a *candidate-window starvation* fixture to reach their `SELECT` copy: because the window is
a fixed multiple of the batch size, a route that is still selected but not claimable costs a slot,
and enough of them ahead of a live route starve it. Asserting "the exhausted route was not
claimed" reaches neither copy, because both refuse it.

### Moving a statement is when someone finally reads it

B16's `DELETE FROM tasks` is the fourth round's version of B11's five unguarded fences. Trigger
history limits have never applied to a task that actually ran: the delete clears no child rows,
`task_items` does not cascade, and every task a trigger creates has items. The error is caught and
logged by the caller, so the only symptom is that the history never shrinks. It is recorded in
Known limits below and pinned by an assertion in the round-trip test, and it is not fixed here —
fixing it decides whether a history limit may delete a task's items, events and command runs,
which is not a question a statement-moving batch should answer.

### `config.rs` was not blocked on `crd`

Phase A recorded `persistence/repository/config.rs` as blocked, with the unblock condition that
`crd` must sink into its own crate first, because the file holds 17 references to `crate::crd` and
`crd/plugins.rs` calls back into `db.rs`. That was the right answer to Phase A's question, which
was whether the *whole file* could move.

It is the wrong answer to Phase B's question, and re-deriving it at closure showed why. Phase B
asks whether the file's *statements* can move. They can: they are over
`orchestrator_config_versions`, `config_heal_log`, `resources`, `resource_versions` and
`sqlite_master`, all flat columns with `spec_json` and `metadata_json` as text. Not one of them
names a `crd` type — the `crd` types appear only in the callers that build a `ResourceStore` out
of the rows. There is no cycle, and nothing for a `crd`-sinking FR to own.

What the file's three references actually are is the driver connection type: an import,
`open_conn(&self) -> Result<rusqlite::Connection>` (which is `orchestrator_persistence::db`'s
`open_conn`, re-exported), and a `&rusqlite::Connection` parameter handed on to
`orchestrator-security`'s key-audit writer. Moving the statements down would not clear those,
because they exist so that `core` can obtain a connection and pass it — which is FR-141
requirement 4 word for word: the persistence layer's public API must not hand out driver types.
So `config.rs` is disposed the same way `attention.rs` and `process_metrics.rs` are: kept, with
the reason recorded, pointing at FR-141.

This correction matters beyond one file. A blocker written for one question and inherited by
another is invisible: it is already written down, already has a reason, and reads as settled. The
only thing that catches it is asking again at closure whether the reason still answers the
question being asked.

### Reverting a batch is not the same as undoing it

Every batch here carries a proof that its commit reverts mechanically. That proof is about form,
not about safety, and one batch makes the difference concrete.

B14 took a `git` subprocess tree out of the SQLite writer's transaction. Doing so meant the
caller verifies the state version before the write, so the store had to re-fence on
`status='planned' AND expected_state_version`. The code it replaced inserted the execution row and
*then* ran `UPDATE … WHERE status='planned'` without checking how many rows changed — two
operators racing could both believe they owned the same resume execution.

Reverting B14 puts that race back. Anyone who reverts it on the grounds that "the batch is proved
revertible" will have reintroduced a live defect while citing evidence that says nothing about it.
The revert proofs establish that the commits are independent, and nothing more.

### The guard audit has a blind spot, and it is the three files that stayed

The most valuable output of Phase B was not the sixteen files that moved. It was the SQL
invariants that turned out to have no test at all — B11's five routing fences, B14's resume race,
and the four assertions across B13–B16 that passed their first mutation. Every one of those was
found because a statement was being moved and somebody had to read it.

Three files were not moved: `attention.rs`, `process_metrics.rs` and
`persistence/repository/config.rs`. **Their SQL guards have not been audited, and this mechanism
will never reach them**, because the mechanism is migration. Nothing in this design record or in
FR-130 should be read as evidence about them; they were judged on their reference shape, not on
their invariants.

All three are in FR-141's scope — the 165 call sites it migrates include theirs — so the audit has
an owner and an occasion. Whoever takes FR-141 should treat those three as a guard audit as well
as an API migration, because it will be the first time anyone reads their statements with a reason
to ask what each one is holding shut.

### What is left, and why each is left

| File | Refs | Disposition |
|---|---|---|
| `attention.rs` | 3 | Kept. Import 1 + the two-line `fn other` adapter. Its SQL is entirely inside `writer().call` closures; clearing the count means either moving the file whole or changing the pipe, and the pipe change is FR-141's. |
| `process_metrics.rs` | 3 | Kept, same shape and same reason. |
| `persistence/repository/config.rs` | 3 | Kept. Connection-type 2 + import 1, all of them the driver connection the layer hands out. See above — not blocked, and not blocked on `crd`. |

Nine references across three files, each named in `core-boundary-ledger.json` and frozen by exact
equality in both directions. Each has the same successor, and that successor exists: FR-141, whose
own non-goals require it to start only after Phase B closes.

### Ports where they fit, not where they were proposed

FR-130 proposed a port-layer error type for Phase C. That was the wrong tool there — `error.rs`
had one dead consumer — and the right one in `config_load/build.rs`, where the deletion guards
diff two `OrchestratorConfig` snapshots and need exactly four counts and samples from the
database. `DeletionGuardQueries` is four methods with one impl for `Connection`; a caller holding
a `Transaction` passes `&*tx`, so the guard still runs inside the caller's transaction, which is
the property that matters — a count taken outside it could be stale before the delete lands.

The port's justification was that guard logic becomes testable without a database, so that is
demonstrated rather than asserted: a stub drives both branches with no SQLite, and pins two
things a database-backed test tends to leave alone — that the refusal names the resource and its
project, and that a zero count does not go on to query for a sample of blockers.

### Phase C was answered by measurement

`impl From<rusqlite::Error> for OrchestratorError` is deleted. FR-130 framed the choice as
port-layer conversion versus accepting the coupling; both presume consumers. Deleting it in a
scratch copy failed with exactly three errors, all in the SQL block B1 moved out.

What the impl guaranteed had to be preserved explicitly, and nearly was not. It categorised every
driver failure as `ExternalDependency`, which is on the wire. B1's call site first used
`classify_resource_error`, which classifies by message — and SQLite's phrase for a missing table
is `no such table: resources`, which that classifier's `not found` branch reads as `NotFound`.
Same failure, different category, no compile error. The call site now uses a named function that
is explicitly `external_dependency`, pinned by a test that takes a real error from a real
unmigrated database and asserts through the production mapping. Mutating it back produces
`left: NotFound, right: ExternalDependency`.

`error.rs`'s `from_rusqlite_error` test was replaced rather than deleted: removing a test with its
subject leaves no record of whether the guarantee moved or lapsed.

### The gate's own defects, found by running it

Adding the Phase C case surfaced four problems in `test-persistence-extraction.sh`, all authored
here:

- The round-trip case asserted `"2 passed"` as a literal and went red when Phase B added a third
  test. It now derives the count from the file, which also fails if a test disappears.
- The Phase C proxy grepped for `From<rusqlite::Error>` anywhere in `error.rs` and was satisfied
  by the doc comment explaining that the impl had been removed. A gate its own explanatory prose
  can trip is measuring the prose. It is anchored to `^impl` now.
- The negative fixture appended an undocumented probe to a crate that denies `missing_docs`, so
  the build stopped on the lint and the case would have passed on an unrelated error.
- Three cases build fixtures from `git archive HEAD`, so on a dirty worktree they answer a
  question about the previous commit while printing identical PASS lines — which is how the Phase
  C case first reported that the conversion still existed. The gate now **refuses to run** on a
  dirty tree. QA 186 already required a clean one; this makes it a refusal rather than a
  condition a reader is trusted to check.

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

- **Phase A converged 114 of its 115 references, not 115** — resolved in Phase B (B12). The one
  left was `core/src/migration.rs`'s `use rusqlite::Connection`, guarding three wrappers over
  `persistence::migration` kept "for compatibility" with nobody. Phase B took the first of the
  two options named here: retire the dead public API. Deleting the wrappers immediately exposed
  what they had been hiding — `crate::migration::run_pending` returned a bare count while the
  real API returns an `AppliedMigrationSummary`, so six assertions had to gain `.count()`. That
  is the cost of a shim nothing is compatible with: it is a second name for the real API, free
  to drift.
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
- **The ratchet counts the driver's name in prose.** Recorded in DD-142's known limits; found
  twice during Phase B, the second time in a doc comment written to explain that the driver
  conversion had been removed. The workaround — naming the driver's error type without spelling
  its path — trades precision for a metric, and is stated as a cost rather than presented as
  neutral.
- The `schema_snapshot` test sits in `core` and exercises a crate below it. Deliberate and
  temporary, as above; it is the one place where the test and the code it tests are in different
  crates on purpose.
- **Trigger history limits do not delete anything that ran.** `trigger_state::delete_tasks` is a
  bare `DELETE FROM tasks`, moved unchanged from `trigger_engine::cleanup_history`. It clears no
  child rows, and `task_items` references `tasks(id)` without `ON DELETE CASCADE`
  (`migration_steps.rs:71`), while some other child tables do cascade. So the delete is refused
  with `FOREIGN KEY constraint failed` for any task that has items — which is every task a trigger
  fire creates. `cleanup_history` propagates the error and its caller logs it, so the trigger
  keeps firing and the history simply never shrinks. Found in B16 by asking what each moved
  statement would do if it were wrong; recorded rather than fixed, because the fix decides whether
  a history limit may delete a task's items, events and command runs, and nobody has answered that
  yet. `task_cleanup.rs` already deletes through the repository's cascade, which is probably where
  this should route. Pinned by `trigger_history_retention_keeps_the_newest_and_selects_nothing_else`
  so the behaviour is a known state rather than a surprise.
- **The guard audit covers only the files that moved.** Recorded in full above. `attention.rs`,
  `process_metrics.rs` and `config.rs` had their reference shape judged, not their invariants, and
  the mechanism that read every other file's guards cannot reach them.
