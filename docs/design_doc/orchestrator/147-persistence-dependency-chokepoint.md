---
lifecycle: active
related_fr: FR-136
---

# DD-147: The Persistence Dependency Chokepoint

**Module**: Architecture / Governance
**Status**: Implemented (FR-136)
**Related Plan**: FR-136
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
SQLite database path". Its 55 SQL statements never touch
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
6 driver references and 22 SQL statements across 4 files. Inverting a dependency
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
- **Condition 2 — who may use.** For every non-core member file, the per-file
  count of SQL statement literals and driver references is frozen by **exact
  equality in both directions**.

Exact equality rather than a monotonic ratchet is FR-128's decision, inherited
deliberately. Under a monotonic rule a decrease passes silently, and here a
decrease is the migration finishing — the one event this ledger exists to record.

`forbidden` is the only role whose current and target states differ. Scheduler and
daemon declare the driver today; the ledger records `residualDeclaration: true`
and the per-file residual, so the declaration is tolerated while it is paid down
and condition 2 stops it growing. When FR-130 Phase A finishes, the flag comes
off and the declaration itself starts failing.

### Requirement 1 stated as an assertion

Every file the scan finds carries a reviewed `category`, and a file without one
is `unclassified`, which fails. The classified driver references are then summed
and required to equal the scanned total. A file added to the tree cannot be
absorbed as "already reviewed", and the coverage claim is derived rather than
asserted in prose.

The four categories are what the code shows, not the four FR-136 proposed. "Pure
data access" and "type penetration" are not disjoint in this codebase, and "test
assertion" is outside the production scan by construction:

| Category | Files | Meaning |
|---|---|---|
| `borrowed-connection-raw-sql` | 9 | takes core's `AsyncDatabase` connection and writes SQL in place |
| `driver-error-type` | 2 | names only `tokio_rusqlite::Error::Other` |
| `owned-connection` | 5 | opens its own `Connection` |
| `transaction-boundary` | **0** | explicit caller-controlled transaction scope |

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
only the counting is shared.

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
