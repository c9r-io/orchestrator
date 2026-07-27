---
lifecycle: active
related_fr: FR-141
---

# DD-151: The Persistence Connection Capability Boundary

**Module**: Architecture / Governance
**Status**: Implemented (FR-141)
**Related Plan**: FR-141
**Related QA**: `docs/qa/orchestrator/189-persistence-connection-capability-boundary.md`
**Related**: DD-147 (persistence dependency chokepoint), DD-148 (persistence crate extraction), DD-142 (core boundary freeze), DD-149 (governance aggregation completeness)
**Created**: 2026-07-27
**Last Updated**: 2026-07-27

## Background

DD-147 froze two facts about the SQLite driver: who may *declare* it, and how
much SQL each file holds. It also wrote down, in its own words, why those two
were not enough:

> `AsyncDatabase::writer()` and `reader()` return `&tokio_rusqlite::Connection`
> … so "how many times does a crate mention `rusqlite`" measures something
> adjacent to the problem.

A crate handed a connection runs `conn.execute(sql, [])` without the token
`rusqlite` appearing anywhere in its source. Condition 1 reports it clean.
Condition 2 counts what it did *once it had* a connection, not that it could get
one. The capability itself — obtaining a connection — was governed by nothing.

FR-141 closes that. This record covers the boundary decision, the seven places
where the FR's own account of the tree turned out to be wrong, the two designs
that came out of it, and the one hole that remains with the argument for why it
is not the hole that was closed.

## The three doors

The FR proposed removing `AsyncDatabase::writer()`/`reader()` from the public
API. Measurement showed that would not have closed anything, because there were
three ways to obtain a connection and the FR saw one.

| | Door | Who it served |
|---|---|---|
| 1 | `AsyncDatabase::writer()` / `reader()` | 54 production call sites outside the layer |
| 2 | `db::open_conn(path)` — a fresh connection by path, bypassing `AsyncDatabase` entirely | 27 production call sites; both forbidden crates hold `state.db_path` |
| 3 | `orchestrator-security`'s eleven `pub fn (conn: &Connection, …)` | forced `daemon/src/server/secret.rs` to open four connections purely to satisfy them |

The doors are in series, downward. Door 3 makes door 2 necessary; door 2 makes
door 1 pointless to close. The decision taken was to close all three, which puts
this FR's scope substantially beyond its own text — the reasoning for that, and
the alternatives rejected, are below.

Door 3 is the structurally interesting one. DD-147 classified
`orchestrator-security` as `exempt` on the ground that it sits below core and
opens its own connections. That is true about its *position* and false about its
*shape*: an exempt crate whose public API demands a connection pushes the driver
**upward**, past the layer, into the crate that is forbidden to hold it. The
exemption and the violation were the same fact seen from two directions.

## Boundary decision, and what was rejected

**Close all three doors.** The two alternatives, with the reason each fails:

- **Close door 1 only (the FR's literal scope).** The new gate would fail its
  own §4.4 question — *what other state would this assertion still pass on?* —
  with the answer **"today's"**. All 27 `open_conn` sites would be untouched, and
  both forbidden crates would go on obtaining a real driver connection on every
  call, having merely changed which function they call to get it. A gate that
  certifies an enforcement it cannot observe is worse than no gate: it converts
  an unknown into a false assurance.
- **Doors 1 and 2, leaving door 3 to a successor FR.** `daemon` would still have
  to construct a connection for `orchestrator-security`, so `open_conn` could not
  actually become `pub(crate)` and acceptance criterion 1 would remain false for
  the `secret.rs` path — a criterion reported as met on every path except the one
  that motivated it.

Both expansions are written departures from the FR's stated non-goals, recorded
here in the manner DD-145 established for FR-134: the FR said
`orchestrator-security` was "unaffected by this FR", and it was the direct
blocker of the FR's first acceptance criterion.

## Fact verification: seven corrections

The FR document is a proposal, not a description. Every count in it was rebuilt
from the tree with the repository's own scanner — `rust_files_under` +
`strip_test_modules` + `RustLexer.mask_literals`, the same three the sibling
ledgers use. Seven claims did not survive.

1. **Call sites: 54, not 165.** Two causes compounded: the original count
   predated FR-130 Phase B moving ~74 core sites into the layer, and it included
   `cfg(test)` code. Per crate: core 22 (not 126), daemon 21 (not 27), scheduler
   11 (not 12).
2. **The crate list omitted `crates/integration-tests`,** which holds five
   `.reader()` assertions in `tests/trigger_fire.rs`. The ledger gives it a
   `test-only` role — a blessed consumer, whose assertions would have failed to
   compile the moment `reader()` sank. This is requirement 1's own sentence
   ("an enumerated list guards only what was known when it was written")
   happening to the document that wrote it.
3. **Public leakage: 87 items, not 5** — and requirements 1 and 4 were not the
   same scope. Five items *yield* a connection; 82 *demand* one
   (`pub fn foo(conn: &Connection, …)`). Requirement 4's assertion, read
   literally, reddens on all 87, so no single change could have satisfied both
   acceptance criteria as written. This is the category conflation the
   governance procedure names.
4. **Changing `AsyncDatabase` could not close the door** — door 2 above. The FR
   listed `open_conn` as "two items the first draft missed" rather than as the
   load-bearing path for the two crates the FR exists to constrain.
5. **`orchestrator-security` was a blocker, not a bystander** — door 3 above.
   The FR counted nine such functions; there are eleven.
6. **`fn other` was duplicated 4 times, not 6, and 2 of them do not disappear.**
   Two are in core and migrate; two are *inside* the layer
   (`source_automation_routes.rs`, `trigger_state.rs`) and are internal helpers
   of the crate that legitimately owns the driver. The acceptance criterion "six
   duplicate `fn other` gone with the migration" was wrong twice over.
7. **This FR is the payer of DD-147's frozen residual, and the FR never said
   so.** DD-147 recorded daemon 22 / scheduler 17 as `residualDeclaration: true`
   and booked the debt to FR-130 Phase B, which closed without paying it. This
   was the largest ledger movement in the FR and went unmentioned in it.

Correction 6 also carried an expired parenthetical: requirement 4 warned that
nothing enforced registration in the `governance` job's `OUTCOMES` list. FR-137
closed, and `check_continue_on_error_aggregated` now fails on omission.

## Two designs

### `SecretStoreSession` — an opaque handle, not an accessor

`orchestrator-security`'s eleven connection-demanding functions became methods on
a handle that owns its connection and exposes operations rather than the
connection. There is deliberately no accessor returning the inner `Connection`;
a getter would reopen door 3 with an extra step.

Key rotation is the reason the handle owns the connection rather than each call
opening one. `begin_rotation`, `re_encrypt_all_secrets` and `complete_rotation`
are three steps of one operation, and binding them to a single handle keeps the
sequence expressible. The test that holds this is
`a_rotation_interrupted_after_begin_is_finished_by_a_later_session`: it asserts
that an interrupted rotation leaves the outgoing key `decrypt_only` and that a
*later* session finishes it. That is resumability, and it is the true invariant.
Asserting atomicity instead would have been asserting something false — the
rotation is deliberately not atomic, which is why `resume_rotation` exists.

### `ConfigTx` — a transaction scope without a transaction handle

`core/src/persistence/repository/config.rs` ran sixteen statements inside a
caller-controlled transaction, so moving them required moving the transaction
boundary without exporting `Transaction`. `ConfigStore::write(|tx| …)` takes a
closure receiving `&ConfigTx`, which exposes named operations only. `Transaction`
derefs to `Connection`, which is how `DeletionGuardQueries` had been satisfied by
`&*tx`; that is now `ConfigTx::deletion_guards()`.

DD-147's `transaction-boundary` category had zero files. These are the first two.

## What the gate asserts

`scripts/qa/persistence-api-boundary.rb`, frozen against
`config/governance/persistence-api-boundary-ledger.json`. Three facts, reported
independently because no two substitute:

1. **YIELDS** — a public item whose *return* position names a driver connection.
2. **DEMANDS** — a public item whose *parameter* position names a driver type.
3. **HOLDS** — production source outside the layer calling something the gate
   itself classified as yielding.

Fact 3's scanned names are **derived from fact 1's output**, not listed. A name
that stops yielding a connection stops being searched for, and a new one is
searched for on the same run that discovers it. This is the direct answer to
correction 2: an enumerated list would have guarded exactly the tree that existed
when it was typed.

Signatures are taken by bracket matching over lexically masked source, never by
line and never by grep. Two consequences that a token matcher does not get:
`use rusqlite::Connection as Db;` followed by a signature naming `Db` is caught,
because driver types are resolved through each file's own `use` statements; and a
signature split across lines is caught, because the parse ends at the first `{`
or `;` outside brackets. Angle brackets are deliberately not counted as depth —
`->` puts a `>` in every returning signature.

Public API is resolved through the **module tree and its re-exports**, not per
file. `task_repository/mod.rs` declares `mod items;` and `mod write_ops;`
privately and re-exports four of the seventeen public functions they define; a
file-level heuristic reports all seventeen and invents thirteen items for a
migration to move.

The gate was built **before** the migration, as the FR's own implementation order
required, and it immediately found two leaks no hand-written list had:
`struct Migration`'s `pub up: fn(&Connection)` field — a leak through a *field*
rather than a signature — and the scheduler's own public
`create_dynamic_task_items`, which re-exported the capability out of the crate
least entitled to hold it.

## The test-only door, and why it is not the door that was closed

Three groups of consumers could not follow their statements into the layer:

1. **Core's own tests** build fixtures from `TestState` and `create_task_impl` —
   domain machinery *above* the layer — run core logic, then open the database to
   assert on the rows it wrote. Moving them inverts the dependency. Rewriting
   them to read back through the repository they are testing would make them
   assert against the code path they exist to be evidence for.
2. **`orchestrator-persistence/tests/round_trip.rs`** is compiled as a separate
   crate, so it is outside this crate's privacy boundary by the language's
   design, not by anyone's choice.
3. **`crates/integration-tests`**, already blessed `test-only` by the dependency
   ledger, asserts against the database directly because that is what makes it
   end-to-end evidence rather than a restatement of the layer's API.

They reach a connection through `src/test_support.rs`, compiled only under the
`test-support` feature. What separates this from the hole FR-141 closed is that
two things stop it being opened in production, and both are asserted rather than
assumed:

- `cargo build` does not enable the feature, so a production call to anything in
  that module is a **compile error in the shipped artifact**. Verified by writing
  such a caller and reading the `E0433`, not by inspection.
- Every consumer enables the feature from `[dev-dependencies]`. Under resolver 2
  those features are not unified into a normal build, and the gate **fails** if
  any crate enables it from `[dependencies]`.

Critically, the gate does **not skip** the gated module. Skipping it would
certify an exemption the gate cannot observe — the §4.4 failure. It inventories
what the module exposes in `testOnlyYields` and `testOnlyDemands`, keeps scanning
production source for those names in fact 3, and asserts the edge condition
separately. Case 12 of the QA suite adds an item behind the feature and requires
the gate to name it; cases 13 and 14 pair a `[dependencies]` edge that must fail
with a `[dev-dependencies]` edge that must pass, so the case tests the *table*
and not the feature's name.

## Result

| | Before | After |
|---|---|---|
| Public items yielding a driver connection | 6 | **0** |
| Public items demanding a driver type | 79 | **0** |
| Connection acquisitions outside the layer | 81 across 25 files | **0** |
| `core` driver references (`core-boundary-ledger`) | 9 | **0** |
| DD-147 residual: daemon | 22 refs / 19 SQL | **0** |
| DD-147 residual: scheduler | 17 refs / 16 SQL | **0** |
| Behind the `test-support` feature | — | 3 yield, 25 demand |

Ledger role changes, all in `persistence-dependency-ledger.json`:

- `crates/daemon`: `forbidden` → **`none`**. It no longer names the driver in any
  section of its manifest, production or dev. `none` rather than `forbidden`
  because `forbidden` permits a `[dev-dependencies]` declaration and there is
  nothing left here to declare one for.
- `core`: `persistence` → **`forbidden`**. Core stopped being the persistence
  layer at FR-130 Phase A; this entry went on saying it *was* the layer for four
  FRs after that stopped being true. Its driver declaration moved to
  `[dev-dependencies]`.
- `crates/orchestrator-scheduler`: stays `forbidden`. Zero production references,
  but its `cfg(test)` modules still name the driver, and `forbidden` is exactly
  the role that permits that and nothing more.

`config/governance/schema-snapshot.sql` is byte-identical throughout: every SQL
statement was relocated verbatim, never rewritten. The two genuine
deduplications found while relocating — 19→18 statements in the daemon, 16→15 in
the scheduler — are recorded as arithmetic rather than allowed to read as dropped
reads.

## Known limits

- **The `test-support` feature is a real conditional hole.** Nothing prevents a
  future author enabling it from `[dev-dependencies]` in a new crate and reaching
  a connection from test code there. That is the intended shape; what is
  asserted is that the hole cannot exist in a production build. A reader who
  wants the stronger property — that no test anywhere touches the database
  directly — will not find it here, and should not conclude from the gate's PASS
  line that it holds.
- **`nested_public_signatures` accepted `pub(…)` where the item scan rejected
  it.** `struct Migration` went on reporting as a leak after its `up` field
  became `pub(crate)`. Fixed, and pinned by QA cases 15 and 16. It was found by
  disbelieving a count, not by review — worth recording because the same shape
  (two visibility tests in one gate, agreeing by accident) can recur.
- ~~**Inherited FR-138 defect.**~~ **Resolved by FR-138.** `scripts/qa/bash32-compat.rb`
  silently swallowed a file's tail when here-document state was misread, and this
  FR did not fix it; `test-persistence-api-boundary.sh` was unaffected only
  because of its length. Quoting is now tracked across lines and a file ending
  inside a here-document is a finding, so the length of a gate script no longer
  determines whether it is scanned. See DD-152.
- **The `governance` job's runtime.** FR-140 recorded it at 45 minutes; this FR
  adds five cases to an existing gate rather than a new job.
