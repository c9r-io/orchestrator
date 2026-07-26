---
lifecycle: active
related_fr: FR-136, FR-139
self_referential_safe: true
---

# Orchestrator - Persistence Dependency Chokepoint

**Module**: Architecture / Governance
**Scope**: the FR-136 chokepoint decision, its machine-readable classification of every driver reference outside core, the two-condition gate that holds it, the FR-139 corrections to that gate's SQL verb set, scan surface and assertion validity, and the assertion that neither FR introduced production code
**Scenarios**: 5
**Priority**: High

## Background

FR-130 froze core's boundary and found that core is not the persistence
chokepoint: six crates take the SQLite driver directly. Extracting
`orchestrator-persistence` before deciding who may depend on it produces a new
crate that five crates depend on instead of core — the same reach, one more
directory level. FR-136 makes that decision and fixes it in a gate.

The decision is a layered chokepoint scoped to `agent_orchestrator.db`, because
two of the six driver-holding crates are not above the persistence layer at all:
`orchestrator-security` sits below core, and `slack-gateway` owns a different
database. See DD-147.

The gate asserts two independent things because the obvious one is a proxy. A
manifest check says who may *declare* the driver; it says nothing about a crate
already on the list adding SQL, and nothing at all about a crate handed
`&tokio_rusqlite::Connection` by core — `conn.execute(sql, [])` names no
`rusqlite` path. That is not hypothetical:
`crates/orchestrator-security/src/secret_store_crypto.rs` runs four production
SQL statements with zero driver references today.

All scenarios below are read-only against the working tree or operate inside
`$TMPDIR`. None starts a daemon, opens the runtime database, compiles anything,
or invokes a provider.

---

## Scenario 1: The Classification Covers The Scan, And The Assertion Can Fail

### Preconditions

- A clean working tree.

### Steps

1. Run `ruby scripts/qa/persistence-dependency.rb` and read the summary.
2. Confirm the reported totals are 13 members, 16 files, 55 driver references.
3. Confirm every entry under `references` in
   `config/governance/persistence-dependency-ledger.json` carries a `category`
   that is not `unclassified`.
4. Read case 16 of `scripts/qa/test-persistence-dependency.sh`: one ledger entry
   has its `category` **key deleted**, counts left untouched.
5. Confirm `grep -n "do not sum to the scanned" scripts/qa/persistence-dependency.rb`
   returns nothing.

### Expected Result

- Step 2 reports `55 driver reference(s) and 114 SQL statement(s) across 16
  file(s) outside core`.
- Step 3 holds for all 16 entries.
- Step 4 fails with `1 file(s) touch persistence with no reviewed category`,
  naming the file, **and** the run does not also report `persistence touch
  points differ` — the case exercises the classification branch alone. A file
  added to the tree arrives as `unclassified` and fails; it cannot be absorbed
  as already reviewed.
- Step 5 returns nothing, and that absence is the point of this scenario.

### Notes

- FR-136 shipped a second branch here that summed the categorised references and
  required the total to equal the scan. It could not fail: `totals["rusqlite"]`
  is *defined* as that sum over the same hash, so the comparison was the scan
  against itself, and a file with no category was counted into the total and
  then found equal to it. This document previously stated it as a live
  guarantee, as did DD-147. FR-139 deleted the branch and case 16 is what the
  surviving branch owes in its place — an input that actually makes it fail.
- The mutation deletes the key rather than writing `"unclassified"` into it.
  Writing the sentinel is the case the author had in mind; the absent key is
  what a real edit produces, and it reaches the branch through the default
  rather than through the literal.
- The 55/16 figures are not FR-136's. The FR reported 75 references across 23
  files, counted by a `grep` over `src/` that includes test code — the method
  DD-142's ledger explicitly rejects. Core reproduces at exactly 200/37 under the
  ledger's own scanner, which is what establishes that the method here is the
  right one and the FR's was not.
- The SQL total is 114 rather than FR-136's 112 because FR-139 added `PRAGMA` to
  the verb set. See scenario 3.

---

## Scenario 2: The Rule Discriminates Between Crates

### Preconditions

- `bash scripts/qa/test-persistence-dependency.sh` available; nothing else
  writing to the repository.

### Steps

1. Run the wrapper as `bash scripts/qa/test-persistence-dependency.sh > log 2>&1`
   and capture `$?` directly.
2. Read case 3: `crates/cli` (role `none`) gains a `rusqlite` entry under
   `[dependencies]`.
3. Read case 4: `crates/orchestrator-security` (role `exempt`) gains a
   `tokio-rusqlite` entry.
4. Read case 5: `crates/integration-tests` has its `rusqlite` line moved from
   `[dev-dependencies]` to `[dependencies]`.

### Expected Result

- Case 3 fails the gate and names both the crate and its role.
- **Case 4 passes.** This is the case that separates a policy from a ratchet: a
  gate that failed on any manifest change would have exactly the same green
  record on this repository as the real rule, and case 3 alone cannot tell them
  apart.
- Case 5 fails. The two sections are different facts. `core-boundary.rb`'s
  whole-file `match?` could not tell them apart, which is why
  `crates/integration-tests` sat in its frozen list beside four production crates
  although its declaration is a dev-dependency.

---

## Scenario 3: The Gate Sees SQL That Names No Driver, And Reads It Correctly

### Preconditions

- As scenario 2.

### Steps

1. Read case 7 of the wrapper run. It appends a function to
   `crates/daemon/src/protection.rs` containing one SQL statement, written the
   way this repository writes a parameterless statement, and containing no
   `rusqlite` substring anywhere — not even inside `tokio_rusqlite`.
2. Confirm the wrapper asserts the probe file names no driver both before the
   mutation and after it.
3. Read case 8: one SQL literal in
   `crates/orchestrator-scheduler/src/scheduler/task_state.rs` is neutralised
   without touching the `rusqlite::params!` beside it.
4. Read cases 12 and 13: `crates/daemon/src/server/attention.rs` — a file already
   in the ledger at `sql: 1` — gains a `PRAGMA` statement, and separately a
   literal opening `"\n            SELECT …"`.
5. Read case 14, which is two runs against one case directory: four log/prose
   strings containing `VACUUM`, `BEGIN`, `update`, `create`, `delete` and
   `Created index`, plus a comment naming `SELECT`, `INSERT`, `DELETE` and
   `PRAGMA` uppercase with no quote before them, are appended and the gate must
   stay green; then one real `PRAGMA` is appended to the **same file** and it
   must fail.
6. Regenerate the ledger with `--emit-baseline` on a tree where `PRAGMA` has just
   been added to the verb set and confirm the diff is `+1` on
   `crates/orchestrator-security/src/lib.rs`, `+1` on
   `crates/slack-gateway/src/store.rs`, and nothing else.

### Expected Result

- Case 7 fails the gate, naming the file and its SQL count. This is the state a
  manifest-only or driver-token gate reports as clean: `crates/daemon` already
  declares the driver, so condition 1 has nothing to say, and the statement names
  no driver, so a token inventory does not see it either.
- Case 8 fails with `~ … sql 8 -> 7`. A decrease is the migration finishing —
  the one event this ledger exists to record — and under a monotonic ratchet it
  would pass silently while the ledger asserted debt the repository no longer
  carries. FR-128 found `capturesOrJsonPath` at 54 against a reviewed 55 for
  exactly that reason.
- Cases 12 and 13 each fail with `~ crates/daemon/src/server/attention.rs sql 1
  -> 2` and nothing else. Mutating a file already in the ledger is deliberate: a
  new file would trip the reference freeze and the classification branch at once,
  and the case could not say which assertion it exercised.
- Case 14's first run passes and its second fails with `sql 1 -> 2`. The two
  halves are one case because "the gate stayed green after I added prose" is
  also satisfied by the file never being read — a state this suite considers
  broken. The control statement in the same file is what rules that out.
- Step 6 shows exactly `+2`. This, not a green gate, is the evidence that the
  verb set was **corrected** rather than **relaxed**: any other number means the
  match now reads something that is not SQL.

### Notes

- Every mutation was chosen as the one the implementation is least likely to
  catch. Case 7 adds SQL *without* the token the author was thinking about; case
  8 removes rather than adds; case 13's literal exists nowhere on this tree, so
  the anchor was widened before the shape appeared rather than after. A fixture
  that deleted a `use rusqlite::` line would prove only that the check its author
  had in mind works.
- The wrapper aborts case 7 as a failure if its own fixture introduces a driver
  token, so the case cannot quietly stop isolating what it claims to isolate.
- The tempting repair for a missing verb is a looser match. Measured on this
  tree, case-insensitivity reads 20 help strings in
  `crates/cli/src/commands/guide.rs` as SQL, and every `VACUUM` hit outside core
  is a log message. Case 14 therefore asserts the *non*-counting direction at the
  same strength as case 12 asserts the counting one.
- Case 14's fixture covers both ways the match can loosen, because neither
  implies the other. The four prose strings answer "was the verb set widened, or
  the uppercase requirement dropped"; the trailing comment — in-set verbs,
  uppercase, with no opening quote before them — answers "was the quote anchor
  dropped". That line scores 0 under the anchored expression and 4 without it.

---

## Scenario 4: The Scan Reaches Every Member, And Every File Of One

### Preconditions

- As scenario 2.

### Steps

1. Read case 6. It creates `tools/probe/` — a new workspace member declaring
   `rusqlite` and executing one SQL statement — and adds it to the `members`
   list in the root `Cargo.toml`.
2. Confirm the member is placed **outside** `crates/`.
3. Confirm the case asserts two separate diagnostics: the missing reviewed role,
   and the unledgered source file.
4. Read case 15: `crates/daemon/build.rs` — a `forbidden` crate's build script —
   gains one SQL statement written with **no** `rusqlite` token anywhere.
5. Read case 17: `crates/daemon/build.rs` is removed from the ledger's
   `scanRoots`.
6. Confirm `config/governance/persistence-dependency-ledger.json` lists all five
   member build scripts under `scanRoots`, and that `crates/daemon` and
   `crates/orchestrator-scheduler` — the two `forbidden` crates — are among them.
7. Read case 18. Its first half appends a `[package.metadata.fr139-probe]` table
   carrying `build = "nowhere.rs"` to `crates/cli/Cargo.toml`; its second half
   renames `crates/daemon/build.rs` and declares the new name in `[package]`.

### Expected Result

- Case 6 fails on both counts.
- Step 2 is what makes the case worth writing. A probe under `crates/` would be
  found by a `crates/*` glob as well, and the case would pass without
  distinguishing discovery from enumeration. `core-boundary.rb` used exactly that
  glob.
- Step 3 matters because the two halves fail differently. Discovering the
  manifest and not the source would leave the new member's SQL unread, and that
  half produces no diagnostic at all — it is silent by construction, which is the
  shape that survives review.
- Case 15 fails with `+ crates/daemon/build.rs has 0 driver reference(s) and 1
  SQL statement(s)`. The wrapper's `new_case` copies build scripts for this; if
  it did not, the case would exercise a root the gate reports as missing and
  pass for the wrong reason.
- Case 17 fails naming the root. This is the assertion the scope check could not
  make: `expected["scope"] != SCOPE` compares the ledger's copy of the prose to
  the constant, prose against prose, and it agreed for all of FR-136 while the
  constant said "its non-test Rust source" and the walk read only
  `<member>/src`.
- Case 18's first half **passes** and its second half fails naming both ends of
  the move. Both halves are needed: FR-139 read the `build` key with a whole-file
  regex, so any table carrying that key moved the walk off the real script, and a
  fix that simply stopped honouring `build` would satisfy the first half while
  silently dropping renamed scripts from the scan.

### Notes

- `scripts/lib/rust_source.rb` gained `rust_files_under(repo_root, roots)` for
  this: the exclusion rules stay in one place while the discovery belongs to each
  caller. `rust_source_files` still hardcodes `core/src` plus `crates/*/src`,
  which is correct for the two ledgers that count core and wrong for a question
  about the workspace. Recorded in DD-147's known limits rather than left
  implicit. FR-139 taught the walk to accept a single file as a root, because a
  build script is one file and the alternative was restating the exclusion rules
  in the gate.
- Build scripts are in scope because condition 1 already classifies
  `[build-dependencies]` as a **production** declaration. Reading the manifest
  half of build-time driver use while never opening the source half governs a
  usage the gate cannot see.
- The walk still excludes any file named `test*.rs` by filename rather than by
  `cfg(test)`. `crates/orchestrator-runner/src/test_env.rs` is production —
  `lib.rs:23` declares it `pub(crate) mod test_env;` with no `cfg` — and is not
  scanned. It holds no driver references and no SQL today. Recorded in DD-147's
  known limits; not fixed here, because the rule is shared with
  `core-boundary.rb` and changing it moves that ledger's reviewed `200 / 37`.

---

## Scenario 5: The Gate Is Enforced, And Neither FR Changed Anything Else

Two assertions about the change rather than about the mechanism: that the gate
can actually fail the build, and that FR-136 and FR-139 delivered a decision and
a repair to it, with no production code.

### Preconditions

- A clean working tree; the FR-136 and FR-139 closure commits identified.

### Steps

1. Run `bash scripts/qa/test-qa-gate-surface.sh` and capture `$?`.
2. Confirm `scripts/qa/persistence-dependency.rb` and
   `scripts/qa/test-persistence-dependency.sh` are both classified
   `ci-required` in `config/governance/qa-gate-surface.json`.
3. Confirm the `governance` job in `.github/workflows/ci.yml` has a
   `persistence-dependency` step **and** a matching `persistence-dependency=`
   line in the `Governance result` step's `OUTCOMES`.
4. Run `git diff --stat <base>..HEAD` across the FR-136 and FR-139 commits and
   confirm no path under `core/` or `crates/*/src/` appears.
5. Run `cargo test --workspace` and
   `cargo clippy --workspace --all-targets -- -D warnings`.
6. Confirm the invariants FR-139 must not move: `ruby scripts/qa/core-boundary.rb`
   still reports `52 pub mod, 924 public items, 143 files` and `200 references
   across 37 files`, and `ruby scripts/qa/coordination-governance.rb` still
   exits 0.

### Expected Result

- Step 1 passes 12/12, including `every ci-required gate is executed by a live
  step of the workflow job it declares`. That check parses the workflow step
  rather than grepping the job block, so a step commented out, behind
  `if: false`, or named only in a `name:` field does not count as enforcement.
- Step 3's second half is not decoration. FR-137 documents that `OUTCOMES` is a
  hand-written enumeration nothing guards: a step with an `id:` and
  `continue-on-error: true` that is absent from that list fails silently while
  the job reports all gates green. Omitting the line would have left this gate
  wired and unable to fail the build.
- Step 4 holds. The changed set is documentation, governance configuration and
  gate scripts only. This is an acceptance criterion of FR-136 in its own right:
  the value of the FR is that the decision precedes the extraction, and once
  implementation is mixed in, the decision stops being one that can be overturned
  cheaply.
- Step 5 passes. These confirm the tree is unmoved rather than exercising new
  behaviour; there is no new Rust code for them to cover.
- Step 6 holds. FR-139 edited `scripts/lib/rust_source.rb`, which both other
  ledgers count with; the two reviewed states are what proves the edit was
  additive. A directory root behaves exactly as before — only a file root, which
  no other caller passes, is new.

### Notes

- `ruby scripts/qa/ci-liveness.rb` fails immediately after a change to
  `.github/workflows/ci.yml` and is expected to: editing the workflow invalidates
  every recorded job conclusion, because the record describes a pipeline that no
  longer exists. It is refreshed from `gh run` after CI has run on the new
  pipeline, not before. FR-139 does not touch the workflow.

---

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | The classification covers the scan, and the assertion can fail | ☑ PASS | 2026-07-26 | Claude |
| 2 | The rule discriminates between crates | ☑ PASS | 2026-07-26 | Claude |
| 3 | The gate sees SQL that names no driver, and reads it correctly | ☑ PASS | 2026-07-26 | Claude |
| 4 | The scan reaches every member, and every file of one | ☑ PASS | 2026-07-26 | Claude |
| 5 | The gate is enforced, and neither FR changed anything else | ☑ PASS | 2026-07-26 | Claude |

## Certification Conditions

A run of these scenarios counts as closure evidence only when `git status
--porcelain` is empty at start and at end, `git rev-parse HEAD` matches across
the run, nothing else is writing to the repository, each script is invoked as
`bash <script> > log 2>&1` with `$?` captured directly rather than through a
pager, and each log ends with its own summary line.
