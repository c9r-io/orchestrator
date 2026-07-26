---
lifecycle: active
related_fr: FR-136
self_referential_safe: true
---

# Orchestrator - Persistence Dependency Chokepoint

**Module**: Architecture / Governance
**Scope**: the FR-136 chokepoint decision, its machine-readable classification of every driver reference outside core, the two-condition gate that holds it, and the assertion that FR-136 introduced no production code
**Scenarios**: 6
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

## Scenario 1: The Classification Covers The Scan, Derived Rather Than Asserted

### Preconditions

- A clean working tree.

### Steps

1. Run `ruby scripts/qa/persistence-dependency.rb` and read the summary.
2. Confirm the reported totals are 13 members, 16 files, 55 driver references.
3. Confirm every entry under `references` in
   `config/governance/persistence-dependency-ledger.json` carries a `category`
   that is not `unclassified`.
4. Add a `category` of `"unclassified"` to one entry in a copy of the ledger,
   re-run the gate against that copy with `--ledger`, and revert.

### Expected Result

- Step 2 reports `55 driver reference(s) and 112 SQL statement(s) across 16
  file(s) outside core`.
- Step 3 holds for all 16 entries.
- Step 4 fails, naming the file. The coverage claim is an assertion the gate
  evaluates — classified references are summed and required to equal the scanned
  total — not a sentence in a document. A file added to the tree arrives as
  `unclassified` and fails; it cannot be absorbed as already reviewed.

### Notes

- The 55/16 figures are not FR-136's. The FR reported 75 references across 23
  files, counted by a `grep` over `src/` that includes test code — the method
  DD-142's ledger explicitly rejects. Core reproduces at exactly 200/37 under the
  ledger's own scanner, which is what establishes that the method here is the
  right one and the FR's was not.

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

## Scenario 3: The Gate Sees SQL That Names No Driver

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

### Notes

- Both mutations were chosen as the ones the implementation is least likely to
  catch. Case 7 adds SQL *without* the token the author was thinking about; case
  8 removes rather than adds. A fixture that deleted a `use rusqlite::` line
  would prove only that the check its author had in mind works.
- The wrapper aborts case 7 as a failure if its own fixture introduces a driver
  token, so the case cannot quietly stop isolating what it claims to isolate.

---

## Scenario 4: Coverage Comes From The Member List, Not A Glob

### Preconditions

- As scenario 2.

### Steps

1. Read case 6. It creates `tools/probe/` — a new workspace member declaring
   `rusqlite` and executing one SQL statement — and adds it to the `members`
   list in the root `Cargo.toml`.
2. Confirm the member is placed **outside** `crates/`.
3. Confirm the case asserts two separate diagnostics: the missing reviewed role,
   and the unledgered source file.

### Expected Result

- The gate fails on both counts.
- Step 2 is what makes the case worth writing. A probe under `crates/` would be
  found by a `crates/*` glob as well, and the case would pass without
  distinguishing discovery from enumeration. `core-boundary.rb` used exactly that
  glob.
- Step 3 matters because the two halves fail differently. Discovering the
  manifest and not the source would leave the new member's SQL unread, and that
  half produces no diagnostic at all — it is silent by construction, which is the
  shape that survives review.

### Notes

- `scripts/lib/rust_source.rb` gained `rust_files_under(repo_root, roots)` for
  this: the exclusion rules stay in one place while the discovery belongs to each
  caller. `rust_source_files` still hardcodes `core/src` plus `crates/*/src`,
  which is correct for the two ledgers that count core and wrong for a question
  about the workspace. Recorded in DD-147's known limits rather than left
  implicit.

---

## Scenario 5: The Gate Is Wired, And Wiring Is Read Rather Than Grepped

### Preconditions

- A clean working tree.

### Steps

1. Run `bash scripts/qa/test-qa-gate-surface.sh` and capture `$?`.
2. Confirm `scripts/qa/persistence-dependency.rb` and
   `scripts/qa/test-persistence-dependency.sh` are both classified
   `ci-required` in `config/governance/qa-gate-surface.json`.
3. Confirm the `governance` job in `.github/workflows/ci.yml` has a
   `persistence-dependency` step **and** a matching `persistence-dependency=`
   line in the `Governance result` step's `OUTCOMES`.
4. Run `ruby scripts/qa/ci-liveness.rb`.

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
- Step 4 passes with no `knownFailing` annotation.

---

## Scenario 6: FR-136 Introduced No Production Code

### Preconditions

- The FR-136 closure commits identified.

### Steps

1. Run `git diff --stat <base>..HEAD` across the FR-136 commits.
2. Confirm no path under `core/` or `crates/*/src/` appears.
3. Run `cargo test --workspace`.
4. Run `cargo clippy --workspace --all-targets -- -D warnings`.

### Expected Result

- Step 2 holds. The changed set is documentation, governance configuration and
  gate scripts only.
- Steps 3 and 4 pass. They confirm the tree is unmoved rather than exercising new
  behaviour; there is no new Rust code for them to cover.
- This is an acceptance criterion of FR-136 in its own right. The value of the FR
  is that the decision precedes the extraction — once implementation is mixed in,
  the decision stops being one that can be overturned cheaply.

---

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | The classification covers the scan, derived rather than asserted | ☑ PASS | 2026-07-26 | Claude |
| 2 | The rule discriminates between crates | ☑ PASS | 2026-07-26 | Claude |
| 3 | The gate sees SQL that names no driver | ☑ PASS | 2026-07-26 | Claude |
| 4 | Coverage comes from the member list, not a glob | ☑ PASS | 2026-07-26 | Claude |
| 5 | The gate is wired, and wiring is read rather than grepped | ☑ PASS | 2026-07-26 | Claude |
| 6 | FR-136 introduced no production code | ☑ PASS | 2026-07-26 | Claude |

## Certification Conditions

A run of these scenarios counts as closure evidence only when `git status
--porcelain` is empty at start and at end, `git rev-parse HEAD` matches across
the run, nothing else is writing to the repository, each script is invoked as
`bash <script> > log 2>&1` with `$?` captured directly rather than through a
pager, and each log ends with its own summary line.
