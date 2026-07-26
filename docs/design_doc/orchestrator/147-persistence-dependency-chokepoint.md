---
lifecycle: active
related_fr: FR-136, FR-139
---

# DD-147: The Persistence Dependency Chokepoint

**Module**: Architecture / Governance
**Status**: Implemented (FR-136), corrected (FR-139)
**Related Plan**: FR-136, FR-139
**Related QA**: `docs/qa/orchestrator/185-persistence-dependency-chokepoint.md`
**Related**: DD-142 (core boundary freeze), DD-139 (QA gate enforcement surface), DD-145 (gate surface execution truth)
**Created**: 2026-07-26
**Last Updated**: 2026-07-26

## Background

FR-130 froze `core`'s boundary and, in doing so, found a second axis it had never
budgeted for: core is not the persistence chokepoint. Six crates take the SQLite
driver directly. FR-130 stated the consequence itself:

> Defining a port trait in core does not prevent this — those crates would depend
> on the new crate instead, which is the opposite of the goal.

That is correct, and it means the extraction cannot answer the question. Extract
first and the likely outcome is `orchestrator-persistence` depended on by five
crates, `rusqlite`'s reach unchanged, one more directory level. A god crate
traded for a god dependency. So the decision has to precede the extraction, and
FR-130 Phase A is gated on this document.

This design record contains no production code. Its output is a decision, a
machine-readable classification, and an executable rule.

## What the inventory actually was

FR-136 was drafted from a `grep` over `src/` and reported 4 non-core production
crates, 23 files and 75 references. Rebuilt with `RustSource.scannable_source` —
the scanner DD-142's own ledger uses, which strips inline `#[cfg(test)]` modules
— core reproduces at exactly 200 across 37 files, confirming the method, and the
non-core figure is **15 files and 55 references**. The FR counted test code by a
method the ledger it cited explicitly rejects.

Three per-file claims did not survive either:

| Claim | Reality |
|---|---|
| `orchestrator-scheduler` 37 refs / 13 files | 17 / 6 in production |
| `service/task.rs` (4) is a production consumer | zero production refs — all four sit below `#[cfg(test)]` at line 462 |
| `task_state.rs` (9) is the decisive case | 8 in production, 1 in a test module; still the decisive case |
| `spawn.rs` (3) | two different files: `scheduler/spawn.rs` (2) and `phase_runner/spawn.rs` (1) |

But the counts were not the important error. Three structural facts, none of them
in the FR, decide the shape of the answer.

### `orchestrator-security` is below core, not above it

`core/Cargo.toml` depends on `orchestrator-security`; the reverse is false. It
opens the orchestrator database by path with its own synchronous
`rusqlite::Connection` (`crates/orchestrator-security/src/lib.rs:99`) precisely
*because* it cannot depend upward. The FR treats it as a peer that could migrate
to a persistence layer sitting above it. It cannot, without inverting that edge.

### `slack-gateway` owns a different database

It has no workspace dependencies at all. `GatewayStore::open`
(`crates/slack-gateway/src/store.rs:164`) opens the path from
`SLACK_GATEWAY_DATABASE`, which `config.rs:23` documents as the "Gateway-owned
SQLite database path". Its 56 SQL statements never touch
`agent_orchestrator.db`. Routing it through a shared persistence crate would
*create* coupling that does not exist — the exact outcome this FR exists to
avoid, arrived at from the other direction.

### The driver token is a proxy, and it is already wrong

`AsyncDatabase::writer()` and `reader()` return `&tokio_rusqlite::Connection`
(`core/src/async_database.rs:60,65`). The driver's connection type is in core's
public API, and `conn.execute(sql, [])` needs no `rusqlite::` path anywhere. So
"how many times does a crate name `rusqlite`" measures something adjacent to the
question:

| File | driver refs | SQL statements |
|---|---|---|
| `orchestrator-security/src/secret_store_crypto.rs` | **0** | 4 |
| `orchestrator-security/src/secret_key_lifecycle.rs` | 1 | 14 |

A manifest-only or token-only inventory reports the first row as clean. It runs
four production SQL statements against the orchestrator database.

## The decision

**A layered chokepoint, scoped to the database rather than to the crate graph.**

FR-136 offered three forms: strict (A), controlled sharing (B), layered (C). The
trichotomy assumes all six crates are consumers of one persistence layer, and two
of them are not. The line is therefore drawn at `agent_orchestrator.db`:

| Crate | Role | Basis |
|---|---|---|
| `core`, later `orchestrator-persistence` | `persistence` | the layer itself |
| `crates/orchestrator-scheduler` | `forbidden` | above core, borrows its connection to run raw SQL in scheduling logic |
| `crates/daemon` | `forbidden` | above core, borrows its connection to run raw SQL in gRPC handlers |
| `crates/orchestrator-security` | `exempt` | below core; owns `secret_keys`, `secret_key_audit` and the encrypted `resources` rows |
| `crates/slack-gateway` | `separate-database` | different database, no workspace edges |
| `crates/integration-tests` | `test-only` | `[dev-dependencies]`; a test asserting against the database directly is legitimate |
| the remaining seven members | `none` | no relationship to persistence at all |

`crates/orchestrator-scheduler/src/scheduler/task_state.rs` is on the
**forbidden** side. FR-136 names that file as the test for whether a decision is
form C or form B wearing its badge, and this is the answer to it.

Strict form A was rejected on two counts, both structural rather than budgetary:
it requires `orchestrator-persistence` to sit below `orchestrator-security` or
the `core → security` edge to be inverted, and it requires `slack-gateway` to
adopt a shared crate for a database that shares nothing with this one. Form B was
rejected because it leaves `task_state.rs` in place.

### `exempt` is not `permitted`

`orchestrator-security` keeps its connection, and its residual is frozen at
6 driver references and 23 SQL statements across 4 files. Inverting a dependency
edge to recover six references is not proportionate; leaving the growth
unwatched is not acceptable either. `separate-database` is the same shape for a
different reason: slack-gateway is frozen so that it cannot quietly begin opening
the orchestrator database, which is the only way it could enter this scope.

## Why the gate has two conditions

A rule about who may depend on a driver invites a check on `Cargo.toml`. That
check is a proxy for the thing it claims to enforce, and the repository already
contains the state it passes on:

1. A crate already on the permitted list may add unlimited SQL. The manifest does
   not move.
2. A crate handed `&tokio_rusqlite::Connection` needs no declaration at all.
   `secret_store_crypto.rs` is that shape today.

So the gate asserts two independent things, and neither is alone:

- **Condition 1 — who may declare.** Members are discovered from the root
  `Cargo.toml` `members` list, and each manifest is parsed *by section*, so
  `[dependencies]` and `[dev-dependencies]` are different facts. Both
  `rusqlite` and `tokio-rusqlite` count; freezing only the former leaves the
  async wrapper as an unguarded second door.
- **Condition 2 — who may use.** For every non-core member file — its `src` tree
  and its Cargo build script, because condition 1 treats `[build-dependencies]`
  as a production declaration and the two halves have to mean the same thing by
  "production" — the per-file count of SQL statement literals and driver
  references is frozen by **exact equality in both directions**. The roots the
  walk visits are themselves frozen in the ledger as `scanRoots`.

Exact equality rather than a monotonic ratchet is FR-128's decision, inherited
deliberately. Under a monotonic rule a decrease passes silently, and here a
decrease is the migration finishing — the one event this ledger exists to record.

`forbidden` is the only role whose current and target states differ. Scheduler and
daemon declare the driver today; the ledger records `residualDeclaration: true`
and the per-file residual, so the declaration is tolerated while it is paid down
and condition 2 stops it growing. The flag comes off when the residual reaches
zero, and the declaration itself then starts failing.

FR-130 Phase A did **not** reach that point, and this paragraph used to say it
would ("when FR-130 Phase A finishes, the flag comes off"). Phase A moved the
layer; it did not migrate the callers above it. Scheduler and daemon still hold
17 and 22 driver references, because they borrow a connection and write SQL in
place — which is Phase B's work, not Phase A's. The trigger is the residual
reaching zero, not a phase completing.

### Requirement 1 stated as an assertion

Every file the scan finds carries a reviewed `category`, and a file without one
is `unclassified`, which fails. A file added to the tree cannot be absorbed as
"already reviewed": it arrives uncategorised and fails here, and it is absent
from the ledger's `references` and fails condition 2's exact equality as well.

FR-136 also summed the categorised references and required the total to equal
the scanned total. That branch could not fail — see [Corrections](#corrections-fr-139)
— and FR-139 removed it.

The categories are what the code shows, not the four FR-136 proposed. "Pure
data access" and "type penetration" are not disjoint in this codebase, and "test
assertion" is outside the production scan by construction:

| Category | Files | Meaning |
|---|---|---|
| `persistence-layer` | 18 | is the layer, rather than a consumer of it |
| `borrowed-connection-raw-sql` | 9 | takes core's `AsyncDatabase` connection and writes SQL in place |
| `owned-connection` | 5 | opens its own `Connection` |
| `driver-error-type` | 2 | names only `tokio_rusqlite::Error::Other` |
| `transaction-boundary` | **0** | explicit caller-controlled transaction scope |

`persistence-layer` is FR-130's, added when Phase A made `crates/orchestrator-persistence` a
scanned member. Every other category describes a relationship *to* the layer; a fifth was
needed because the layer itself now appears in a scan whose scope is "outside core", and
classifying it as any of the four would have described the decision as a residual.

## Requirement 4: there were no transactions to draft for

FR-136 calls the cross-crate transaction interface the hardest part of forms A
and C, and requires a draft proving `task_state.rs` and daemon's existing
transaction usage can be expressed in it.

There is no such usage. Scheduler and daemon contain **zero** explicit
transactions. All eleven explicit transaction sites outside core belong to
slack-gateway (ten, its own database) and `orchestrator-security` (one, exempt).
The premise is false, and its falsity is the finding: the forbidden side is far
cheaper to migrate than the FR assumed.

What the forbidden side does have is multi-statement units of work — seven in
`task_state.rs`, eleven in `session.rs` — and they are already closures. This is
`persist_task_execution_metric` (`task_state.rs:20-53`) as it stands:

```rust
state.async_database.writer().call(move |conn| {
    let command_runs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN \
         (SELECT id FROM task_items WHERE task_id = ?1)",
        rusqlite::params![task_id_owned],
        |row| row.get(0),
    )?;
    conn.execute("INSERT INTO task_execution_metrics (…) VALUES (…)", rusqlite::params![…])?;
    Ok(())
}).await
```

and under the draft interface, with the closure body owned by the repository
rather than by the scheduler:

```rust
state.task_repo.record_execution_metric(TaskExecutionMetric { … }).await
```

The mapping is mechanical because the unit of work is already delimited: the
`call(move |conn| …)` boundary *is* the boundary, and moving it into a repository
method changes who owns the SQL, not what is atomic. A `with_write(|tx| …)`
escape hatch on the repository covers any site whose statements do not factor
into one named operation.

One form does not express under any of this: slack-gateway's ten
`connection.transaction()` sites, which hold a transaction open across
application logic. They are out of scope — a different database — and this is
recorded rather than solved.

## Division of labour with FR-133

FR-133 introduces `cargo-deny`. Its `bans.deny.wrappers` field can express a
per-crate allowlist for a banned crate, so the overlap is real and has to be
decided rather than discovered later.

`cargo-deny` governs the **external** dependency graph: duplicate versions,
licences, sources. It evaluates the *resolved* graph, which means it cannot
distinguish `[dependencies]` from `[dev-dependencies]` by section, cannot see
condition 2 at all, and needs a network-capable full resolve to run. This gate
governs **intra-workspace persistence siting**, offline, from manifests and
source text.

Neither expresses the other's rule, and the `rusqlite` siting rule is stated in
exactly one place: here. `core-boundary.rb` used to freeze the same crate list as
`rusqliteDependentCrates`; FR-136 removed it (see below).

## What moved out of `core-boundary-ledger.json`

`rusqliteDependentCrates` is gone from DD-142's ledger. Three reasons, and the
first is the weakest:

1. It is a fact about the workspace, not about core's boundary.
2. It was computed from `Dir["crates/*/Cargo.toml"]` plus core — enumeration by
   glob. A member declared anywhere else was invisible to it, which is the
   coverage shape FR-134 eliminated six times elsewhere.
3. It matched the whole manifest, so `crates/integration-tests` sat in the frozen
   list beside four production crates although its declaration is a
   `[dev-dependency]` — the category conflation this FR exists to resolve.

It also had no negative fixture in `test-core-boundary.sh`: it was frozen, and
nothing ever demonstrated that the freeze could fail. Carrying it forward into a
shared expression would have multiplied a known-defective check rather than
retiring it.

`scripts/lib/rust_source.rb` gained `rust_files_under(repo_root, roots)`, which
is the exclusion walk on its own. `rust_source_files` still hardcodes `core/src`
plus `crates/*/src` — the right scope for the two ledgers that count core, and
the wrong one for a question about the workspace. This gate derives its roots
from the member list and calls the shared walk, so the discovery is its own and
only the counting is shared. FR-139 taught that walk to accept a single file as
a root as well as a directory, which is what a build script is; the alternative
was for this gate to restate the exclusion rules locally, and two statements of
one scope is the condition the library exists to prevent.

## Corrections (FR-139)

A post-closure audit of this gate found three defects. All three are fixed; the
first two had falsified statements in this document, which is why they are
recorded here rather than only in the changelog.

### The sum branch was the scan compared to itself

`classification_errors` summed `references`' driver counts and required the
result to equal `totals["rusqlite"]`. But `totals["rusqlite"]` *is* that sum:
same reduction, same hash, no rewrite in between. No input could make it fail —
a file with no category was counted into the total and then found equal to it.
It was confirmed empirically on a `git archive` copy, with the `unclassified`
branch and the reference freeze disabled so the sum branch spoke alone: a
file carrying one driver reference and no category reported `PASS`.

This document had stated it as a live guarantee ("the categorised references
sum to the scan"), and so had QA-185. Coverage was never actually missing —
`reference_errors` reports an unledgered file by exact equality — but the
document claimed an enforcement the code did not provide, which is worse than
claiming nothing. The branch is deleted, and `test-persistence-dependency.sh`
case 16 is what the surviving `unclassified` branch now owes: an input that
makes it fail, isolated so no other assertion fires.

### `PRAGMA` was not a SQL verb

`SQL_STATEMENT` matched nine verbs and not `PRAGMA`. The narrow, uppercase,
quote-anchored shape is correct and was kept — remeasured for FR-139, a
case-insensitive match reads 20 help strings in `crates/cli/src/commands/guide.rs`
as SQL. Only a real verb was missing. Adding `PRAGMA` and nothing else moves the
ledger from 112 to 114 statements, with the delta being exactly
`orchestrator-security/src/lib.rs` +1 and `slack-gateway/src/store.rs` +1 and no
other file moving at all. That two-sided figure is the evidence, not the gate
turning green: any other number would mean the match was relaxed rather than
repaired.

`orchestrator-security/src/lib.rs:104` is the pointed case. It is an `exempt`
crate running `PRAGMA foreign_keys = ON` on a connection, precisely the shape
condition 2 exists to see, and its ledger entry recorded one statement where
there are two. It matters for FR-130 Phase B, whose per-file disposition is
"SQL migrated out / domain logic stays / kept with a reason" read off these
counts: a file judged migrated while still holding a `PRAGMA` read as clean.

`VACUUM`, `BEGIN`, `COMMIT` and `WITH` were measured the same way and rejected.
Every hit on this tree is prose or a log message — `daemon/src/server/system.rs:140`
and `integration-tests/src/lib.rs:1600` both log `"VACUUM"` — so they would buy
false positives and no statements. Case 14 asserts the non-counting direction at
the same strength as case 12 asserts the counting one.

The anchor also now steps over a leading escape sequence, so
`"\n            SELECT …"` counts. There are zero such literals on this tree;
this closes a free bypass before it is used rather than repairing an undercount,
and the total is 114 with or without it.

### The scan was narrower than the scope prose

`SCOPE` said "its non-test Rust source" and the walk read only `<member>/src`.
Five members ship a Cargo build script — `cli`, `daemon`, `gui`,
`orchestrator-scheduler`, `proto` — and `daemon` and `orchestrator-scheduler`
are the two `forbidden` crates. Meanwhile condition 1 classifies
`[build-dependencies]` as a **production** declaration. So the gate governed who
may declare a build-time driver dependency while refusing to open the only file
that could consume one.

Both directions were available: widen the scan, or narrow the prose and accept
the gap. Widening is the one that leaves the two conditions agreeing about what
"production" means, and it costs nothing measurable — all five build scripts hold
zero driver references and zero SQL, so the `references` section is unchanged and
FR-139's `+2` is entirely `PRAGMA`'s. The build script's path is read from the
manifest's `build` key rather than assumed, so a member that renames it does not
drop out of the scan silently.

That key is read from `[package]` and nowhere else. FR-139 first read it with a
whole-file regex, so any `build = "…"` in any table redirected the walk away from
the real script — a dependency named `build`, a `[package.metadata.*]` table a
tool defined for itself. `scanRoots` caught all three forms tried against it and
named both ends of the move, which is what an outer freeze is for; the reading
itself was still the mistake `driver_declarations` exists to avoid, two functions
above it. Case 18 fixes both halves in place: a decoy outside `[package]` must
leave the scan alone, and a genuine rename must still be followed — a fix that
merely stopped honouring `build` would pass the first half and silently drop
renamed scripts.

The scope check itself was the third layer of the same problem: `expected["scope"]
!= SCOPE` compares the ledger's copy of the prose to the constant — prose against
prose. It agreed throughout, because both said the same wrong thing. The ledger
now also carries `scanRoots`, the roots the walk actually visited, frozen and
compared in both directions. A reviewer reads `crates/daemon/build.rs` in the
reviewed state, and narrowing the walk produces a diff rather than a quietly
smaller number. Case 17 is its negative fixture.

## Known limits

- Condition 2 counts SQL statement literals lexically: an opening quote followed
  by an uppercase SQL keyword. A statement assembled at runtime from fragments
  would not be counted. Nothing in the forbidden crates does this today, and the
  exact-equality freeze means an attempt to hide behind it still has to move some
  other count to be useful.
- `scannable_source` strips `#[cfg(test)]` modules but not comments, so a doc
  comment quoting SQL would be counted. This is stable rather than correct: the
  freeze turns any such edit into a review event rather than a false pass.
- The `exempt` and `separate-database` roles are frozen at their current
  residual, not driven to zero. That is the decision, not an omission.
- `category` is a reviewed judgement and nothing verifies it. The gate asserts
  that every scanned file *has* one, so a file cannot arrive unclassified — but a
  file miscategorised as `borrowed-connection-raw-sql` when it opens its own
  connection would pass. Nothing depends on the category: the rule is driven by
  `role`, which is per crate. The categories are the extraction work-list, and
  their correctness rests on review, as `role` and the decision prose do.
- **A production module named `test*.rs` is invisible to condition 2.** The
  shared walk (`scripts/lib/rust_source.rb:56`) excludes any file whose basename
  matches `test*.rs`, by filename rather than by `cfg(test)`. That rule is right
  for the tree it was written against — the files it drops in `core` are
  `task_repository/tests/*` and similar real test code — but it decides on the
  name, so a production module can be hidden behind one.
  `crates/orchestrator-runner/src/test_env.rs` is the live instance: `lib.rs:23`
  declares it `pub(crate) mod test_env;` with no `cfg`, so it compiles into
  production and the scan never opens it. It holds zero driver references and
  zero SQL today. FR-139 recorded this rather than fixing it because the
  exclusion is shared with `core-boundary.rb`, whose reviewed `200 / 37` is
  derived from it; changing the rule moves that ledger too, and that is a
  separate reviewed change, not a side effect of this one.
- The `scope` string is compared against the ledger's copy of itself. That
  catches a ledger left behind by an edit to the constant, and nothing more —
  both sides are prose, so a constant that has stopped describing the scan reads
  as agreement, which is exactly what it did until FR-139. `scanRoots` is the
  check with a real subject; the prose check is retained for what it does, not
  as evidence about the scan.
- The build script is found at the path the `[package]` table declares
  (`build = "…"`, defaulting to `build.rs`). Cargo's own autodiscovery has
  further forms; a member using one of them would be scanned for its `src` tree
  only, and the `scanRoots` diff is what would show it.
- `OUTCOMES` in the `governance` job is a hand-written enumeration that nothing
  guards, so this gate's ability to fail the build rests on a line a future
  author has to remember to add for their own gate. That defect is FR-137's, not
  this one's; the 21 step ids and 21 `OUTCOMES` entries were confirmed to have an
  empty difference when this gate was wired.
