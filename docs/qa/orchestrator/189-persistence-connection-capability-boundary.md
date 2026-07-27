---
lifecycle: active
related_fr: FR-141
self_referential_safe: true
---

# Orchestrator - Persistence Connection Capability Boundary

**Module**: Architecture / Governance
**Scope**: the FR-141 decision that `orchestrator-persistence` hands out no driver connection through its unconditional public API, the three-fact gate that holds it, the compiler-level guarantee behind the test-only door, and the payment of DD-147's frozen residual
**Scenarios**: 5
**Priority**: High

## Background

DD-147 froze who may declare the SQLite driver and how much SQL each file holds,
and said in its own text why neither observes the capability underneath both: a
crate handed a connection runs arbitrary SQL with no `rusqlite` token in its
source. FR-141 governs the capability itself. See DD-151.

Nothing here starts a daemon, writes to `~/.orchestratord/`, or opens the
runtime database. Every mutation happens in a temporary copy under `$TMPDIR`.

## Scenario 1: the layer's public API yields no connection

**Steps**

```bash
ruby scripts/qa/persistence-api-boundary.rb
ruby -rjson -e 'puts JSON.parse(File.read("config/governance/persistence-api-boundary-ledger.json"))["totals"].inspect'
```

**Expected result**

Exit 0, `Persistence API boundary: PASS`. Totals report `yields` 0 and `demands`
0 — the unconditional public API names a driver connection in neither return nor
parameter position — and `acquisitions` 0 across 0 files outside the layer.

Read both numbers, not just the first. `yields` 0 alone would still permit
`pub fn f(conn: &Connection)`, which forces every caller to obtain a connection
some other way; that is how `orchestrator-security` pushed the driver up past the
layer into `crates/daemon` before this FR.

## Scenario 2: the gate rejects each way of reopening the door

**Steps**

```bash
bash scripts/qa/test-persistence-api-boundary.sh
```

**Expected result**

Exit 0, 16 cases pass. Eleven apply a mutation the gate must reject; five apply
one it must **not** reject, because a check that fails on every edit is a ratchet
and not a boundary. See the mutation evidence table below for what each pins.

## Scenario 3: a production caller of the test-only door does not compile

The `test-support` feature is the one remaining path to a connection from outside
the layer. This scenario asserts the property that makes it safe, at the level
where it is actually enforced — the compiler, not the gate.

**Steps**

```bash
cp core/src/source.rs /tmp/fr141-source.bak
cat >> core/src/source.rs <<'RS'
pub fn fr141_probe(p: &std::path::Path) -> anyhow::Result<()> {
    let _c = orchestrator_persistence::test_support::open_conn(p)?;
    Ok(())
}
RS
cargo build -p agent-orchestrator 2>&1 | grep E0433
cp /tmp/fr141-source.bak core/src/source.rs
```

**Expected result**

`error[E0433]: cannot find `test_support` in `orchestrator_persistence``. The
module does not exist in a build that does not enable the feature, so production
code cannot reach a connection through it — a compile error in the shipped
artifact rather than a lint or a gate finding.

Restore `core/src/source.rs` before continuing; leaving the probe in place makes
every later scenario invalid.

## Scenario 4: DD-147's frozen residual is paid, and the ledgers agree

**Steps**

```bash
ruby scripts/qa/persistence-dependency.rb
ruby scripts/qa/core-boundary.rb
grep -c rusqlite crates/daemon/Cargo.toml core/Cargo.toml || true
git diff --stat config/governance/schema-snapshot.sql
```

**Expected result**

Both gates exit 0. `core-boundary` reports `rusqlite: 0 reference(s) across 0
file(s) in core`. `crates/daemon/Cargo.toml` contains no `rusqlite` line at all
(role `none`); `core/Cargo.toml` contains them only under `[dev-dependencies]`
(role `forbidden`, changed from the stale `persistence`).

`git diff --stat` on `schema-snapshot.sql` prints nothing. Every SQL statement
was relocated verbatim; a change here means one was rewritten, which the FR
listed as a non-goal.

## Scenario 5: the migrated paths still work end to end

Structural evidence proves the old path is gone; it does not prove the new one
works. This scenario is the behavioural half.

**Steps**

```bash
cargo test --workspace
cargo test -p orchestrator-security secret_store_session
cargo test -p orchestrator-persistence --test round_trip
```

**Expected result**

All pass; 39 binaries, 2726 tests, 0 failures. Specifically
`a_rotation_interrupted_after_begin_is_finished_by_a_later_session` passes: an
interrupted key rotation leaves the outgoing key `decrypt_only` and a *later*
`SecretStoreSession` finishes it. That asserts resumability, which is the true
invariant — the rotation is deliberately not atomic, which is why
`resume_rotation` exists, so an atomicity assertion here would have been
asserting something false.

## Mutation Evidence

Each case names the mutation applied and the shape it defeats. Cases marked
*must pass* are the ones that keep the failing cases meaningful.

| # | Mutation | Expect | Defeats |
|---|---|---|---|
| 1 | none — the working tree | pass | a gate that fails on everything |
| 2 | none — compare `--emit-baseline` to the ledger | pass | a recovery path that emits what the gate then rejects |
| 3 | add `pub fn f(..) -> &rusqlite::Connection` | **fail** | the base case |
| 4 | name the driver in a **doc comment** only | pass | `grep rusqlite` |
| 5 | name it in a **string literal** carrying `){` | pass | an unmasked scanner, which mis-terminates the signature and swallows the next item |
| 6 | `use rusqlite::Connection as Fr141Db;` then name `Fr141Db` | **fail** | matching the token `Connection` |
| 7 | split the signature across lines | **fail** | per-line matching |
| 8 | `pub fn` inside a **privately declared** module | pass | file-level visibility, which invents 13 items in `task_repository` |
| 9 | the same function, re-exported by name | **fail** | case 8 passing because the file was skipped |
| 10 | a **new** connection-yielding function plus a call site | **fail**, naming both | an enumerated list of names to scan for |
| 11 | acquire a connection inside a `cfg(test)` module | pass | a ledger that moves when a test is added |
| 12 | add an item behind the `test-support` feature | **fail**, naming it | skipping the gated module — certifying an exemption the gate cannot observe |
| 13 | enable `test-support` from `[dependencies]` on a crate that **already** declares the dep under `[dev-dependencies]` | **fail** | searching the manifest for the feature's name, which finds it either way |
| 14 | enable it from `[dev-dependencies]` | pass | case 13 passing because any manifest edit is rejected |
| 15 | `pub(crate)` field of driver type | pass | the actual defect: the field regex accepted `pub(…)` where the item regex rejected it |
| 16 | bare `pub` field of driver type | **fail** | case 15 passing because the struct body was never read |

Case 13 is the one the implementation was least likely to catch: the mutation is
not a new dependency but the *same* crate gaining the feature on its production
edge, so the feature's name appears in both tables and only the table it appears
in decides.

## Certification validity conditions

A run counts as closure evidence only when all five hold. If any fails the run is
void — fix the condition and re-run rather than reporting the result.

1. `git status --porcelain` is empty at start **and** at end.
2. Nothing else writes to the repository during the run. A shell script rewritten
   while `bash` is executing it produces garbled commands at shifted byte offsets.
3. `git rev-parse HEAD` before and after must match.
4. Invoke as `bash script > log 2>&1` and read `$?` directly. Piping into `tail`
   reports the pager's status and masks a failed script.
5. The script's final summary line must be present in the log. Its absence means
   the run terminated early, whatever status was reported.

## Checklist

- [ ] `ruby scripts/qa/persistence-api-boundary.rb` exits 0 with `yields` 0 and `demands` 0
- [ ] `bash scripts/qa/test-persistence-api-boundary.sh` exits 0 with 16 cases passing
- [ ] A production caller of `test_support` fails to compile with `E0433`
- [ ] `ruby scripts/qa/persistence-dependency.rb` and `core-boundary.rb` both exit 0
- [ ] `core-boundary` reports 0 rusqlite references in core
- [ ] `crates/daemon/Cargo.toml` names the driver in no section
- [ ] `git diff --stat config/governance/schema-snapshot.sql` prints nothing
- [ ] `cargo test --workspace` passes with 0 failures
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0
- [ ] `cargo fmt --all --check` exits 0
- [ ] The five certification validity conditions above all held for the run
