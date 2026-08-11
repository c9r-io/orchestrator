---
lifecycle: active
related_fr: FR-165
---

# 180. The forward-only rollback contract, in one place

**Status**: Released

The contract is three sentences: migrations are forward-only, the previous release
binary must be able to serve the current schema, and restoring a backup is a
disaster action rather than a rollback. Before FR-165 requirement 2 it was stated
in fifteen documents and asserted in none.

That is not a hypothetical cost. `c1060338` centralised daemon readiness in
`gate_daemon.sh` on a `--wait-ready` flag the 0.5.0 binary cannot accept, and
`test-slack-skill-automation-vertical.sh` pins `FR113_PREVIOUS_REF` to the 0.5.0
cut precisely so it can exercise clause 2. From 2026-08-11 until `77cc351a` the
behavioural half of the contract was dead. Four manual gates went red; nothing in
CI said anything, because nothing in CI was watching. Fifteen paraphrases did not
survive one flag.

## What the FR proposed, and why neither candidate worked

FR-165 named two candidate guards: a structural assertion on the migration
registry, or contract-naming the existing populated-upgrade tests. Both were
checked against the tree and both assert nothing new.

`Migration` (`crates/orchestrator-persistence/src/migration.rs`) carries `up` and
nothing else. There is no down migration and no mechanism for one, anywhere in the
crate. "Migrations are forward-only" is therefore a tautology over the type's own
shape, and a test asserting it would be a test of Rust. The chain's well-formedness
was already covered: `registered_versions_are_unique_and_ascending` and
`full_chain_reproduces_the_reviewed_snapshot` predate this work by two FRs.

So clause 1 gets a doc comment saying it is structural, and explicitly saying that
no test was written for it and why. Naming what is deliberately unguarded is worth
more than a green assertion nobody would trust.

## Clause 2 is a superset property, and nothing was checking it

The gap was in the second clause, and it took a different shape than the FR
expected. `config/governance/schema-snapshot.sql` is a reviewed artifact compared
byte-for-byte on every push. What no test asked is whether the new schema still
*contains* the old one. A migration that drops a column regenerates the snapshot,
arrives in a diff, and passes every test in the file. The diff was the only guard,
and a diff is a guard only if someone reads it already knowing what to look for.

"The previous release binary can serve the current schema" is, mechanically,
"every table, column and index the previous release knew about still exists". That
is checkable, so it is now checked:

- `config/governance/schema-snapshot-previous-release.sql` — the schema the
  previous release shipped, with the release, revision and date it was taken at in
  its header. Not a second baseline tracking the chain; a frozen record of what
  the older binary reads.
- `previous_release_schema_is_a_subset_of_current` in
  `core/src/persistence/schema_snapshot.rs` — executes both snapshots into
  in-memory `rusqlite` connections and compares tables, columns and indexes
  through `sqlite_master` and `PRAGMA table_info`.

Two decisions inside that are worth recording.

**Executing the SQL rather than parsing it.** The cheap version is a per-line
regex over `CREATE TABLE x ( ... )`. The first draft was exactly that and it
silently parsed **zero tables**, because it required no space before the opening
parenthesis while the renderer emits one. It reported "no removed columns" over an
empty comparison. That is §4.4 shape 3 — counting or matching standing in for
parsing — caught by luck rather than by review, since a scan that finds nothing
and a scan that finds nothing wrong print the same thing. `rusqlite` is already a
dependency of this crate; SQLite parses SQL correctly and for free.

**Applying statements to a fixed point rather than ordering them.** The snapshot is
sorted by object type then name, so every `CREATE INDEX` precedes every
`CREATE TABLE` and executing the file in order fails on line 1. Partitioning by
text prefix would work today and is a lexical guess about SQL. Instead the loop
applies what it can and retries the remainder until a pass makes no progress,
failing with the leftover statements named. Nothing depends on recognising a
statement, and a malformed artifact fails loudly through its own branch — which
turned out to matter: a fixture that comments out a table without its indexes
reaches *that* branch, not the removal branch, and conflating the two would have
been a case asserting something it never reached.

**What it deliberately permits.** Data may be remapped in place. Migration 29
relabelled the terminal Session state `exited` to `closed`; migration 34 remapped
route statuses. Both keep every column, so both satisfy the contract while
changing what the rows say — the older binary opens the database and reads the new
spelling. A subset check that also failed on additions would block every forward
migration, which is the opposite of what "forward-only" means, so a fixture
asserts that a new table and index **pass**.

**What it cannot see.** A column kept but retyped, or losing a constraint. That is
covered by `full_chain_reproduces_the_reviewed_snapshot`, which compares whole
normalised statements. The two are complements and neither subsumes the other.

## The prose half, and why the ledger is keyed per statement

`scripts/qa/rollback-contract-single-source.rb` with
`config/governance/rollback-contract-sites.json`, following
`connectivity-path-single-source.rb` — derived scope, a bidirectional allowlist,
empty input failing closed, and the known limit re-derived on every run rather
than asserted in a comment.

Two things about the shape are new, and both came out of measuring rather than
designing.

### `forward-only` is four concepts, and two of them share a table

FR-165 recorded three senses. There are four:

| Class | Meaning | Sites |
|---|---|---|
| A | the daemon migration rollback contract | 15 |
| B | the Slack Gateway's own schema, another database | 6 |
| C | artifact forwarding in `orchestrator-collab` | 1 |
| D | monotonic *state* change — connection generation/version CAS | 1 |

The fourth is `docs/security/slack-gateway-threat-model.md` row T8, and its
position is the whole point: it sits **four rows above the A-class T12 in the same
markdown table**. There is no file-level or section-level scope predicate that
separates them. A ledger keyed by path would classify that file as A, satisfy the
citation requirement file-wide, and bless T8 in silence — §4.4 shape 3, a
whole-file total standing in for the per-object association the contract claims.

So sites are keyed by a digest of the matched line. Digest rather than line
number, because line numbers move and the governed prose moving is exactly what
this gate exists to permit: a stale line number points at innocent text or past
the end of the file, while a stale digest simply stops matching, which is a fact
the gate can report. A fixture adds a class-D row to that file *and* removes
T12's citation in the same run, and asserts T12 is still checked.

T8's own wording now says so in words as well — "forward-only (monotonic)
connection-state changes — a different property from T12's schema migrations,
sharing only the adjective". The ledger keeps a machine honest; the sentence
keeps a reader honest, and neither replaces the other.

### No scope predicate at all

The obvious scope is tracked `*.md` and `*.rs`. It is also §4.4 shape 9's third
premise: a scope sufficient for today's tree is a fact about the tree, not about
the check. So the gate reads **every tracked file whose bytes contain no NUL** —
1592 of them. Measured before choosing: the extension-restricted scan and the
extension-free scan find the same 38 sites, so the widest scope costs nothing and
cannot be wrong later.

The 58 tracked paths that are not regular files are all `.agents/skills/` and
`.cursor/skills/` symlinks into `.claude/skills/`, whose 90 files are tracked and
read directly. The gate re-derives that on every run and fails if a non-regular
path appears outside those two roots, rather than trusting this paragraph.

### Three CHANGELOG mentions are history, not statements

FR-165's criterion read "all 17 A-class sites cite the single source". Three of
them are in CHANGELOG's released `[0.4.0]` section, under a repository that
declares Keep a Changelog. Satisfying the criterion literally would have meant
editing published release notes. They are booked as class `record` — true when
written, never rewritten, no citation required — and the live surface is 15
statements across 14 documents. This is the one Step-0 correction that changed
what was *possible* rather than only what was counted.

### Classification is required, not inferred

The gate is stricter than the FR's wording, which asked it to "stay silent" about
B, C and D. Any unclassified mention fails, whatever its class. A gate that
decided an unbooked mention's class by matching its text would be §4.4 shape 4 —
a text pattern standing in for a semantic property — which is the exact defect a
four-way overload creates.

The fixtures prove the property the criterion was after rather than the wording:
for each of B, C and D, a new mention fails as **unclassified** and specifically
*not* as uncited, and once booked in its class the gate is silent and never asks
it to cite anything.

Whole-file entries exist for files whose subject *is* the contract — the gate
itself, its fixtures, this ledger, the frozen snapshot, the FR, the ticket, DD-179
and the guard in `schema_snapshot.rs`. Each carries a mandatory reason and each is
one named path, never a subtree: §4.4 shape 8 is explicit that a recursive
exemption goes on absorbing instances that do not exist yet and never produces a
line in any log. Entries in classes that are deleted at closure (the FR, the
ticket) are marked `ephemeral`, pruned with a printed notice instead of failing
the mirror condition — and a *new* mention in a *new* FR still fails until
somebody books it.

## What the fixtures found that the design did not

Two of the 24 cases exist because writing them contradicted an assumption.

**A self-citing statement cannot shed its citation.** Eleven of the fifteen
class-A sites name the path on their own line; four have a separated citation
because the statement is a heading or a recorded table row that cannot carry a
path. Editing a self-citing line changes the statement's own digest, so tampering
is caught by the mirror condition and never reaches the citation check. The first
attempt at that fixture asserted the citation diagnostic and failed — the design
was sound and the assertion was aimed at the wrong branch. Reaching the
citation-content branch takes a third mutation: repointing `citedBy` in the ledger
at a line that really exists and really does not name the path, which is the
careless-ledger-edit shape a presence check cannot see.

**A commented-out table is not a removed table.** Removing a `CREATE TABLE`
without its indexes leaves them orphaned, and no execution order can apply them,
so the case fails through the unapplicable-statement branch. The removal branch
needs the table *and* its indexes gone, which is what regenerating the snapshot
after a `DROP TABLE` actually produces. Both cases are kept and both assert their
own diagnostic; an exit code could not tell them apart.

## A ci-required gate that had been red since requirement 1 shipped

`scripts/qa/fixture-target-drift.rb` is ci-required, and it fails at `e131c069` on
`scripts/qa/test-manual-gate-freshness.sh` — the fixture harness FR-165
requirement 1 shipped at `55c0d766`. Three ledger rewrites never proved they
landed, and one block's premise aborted rather than failing its case.

The mechanism is §4.6 condition 6, operating on the FR that wrote condition 6's
own fixtures: requirement 1's certification ran a hand-listed sweep, the drift
scanner was not on the list, and a gate the FR's own new file breaks was red from
the moment it shipped. The log was all green and said nothing about what it did
not run. Fixed here rather than recorded as a known limit, because it is the same
FR and a certification cannot run on a red tree.

Two details of the repair are worth keeping:

- The drift scanner recognises `fixture_mutate` by name at the head of a
  statement. A local `doctor()` helper wrapping it was rejected, correctly: the
  scanner cannot see through an indirection, and a mutation it cannot see is one
  nothing keeps honest. The call sites are shaped to be visible.
- The release-edge reader's `abort` calls did become failed assertions in that one
  caller, via `|| edge_status=$?`. The scanner refuses the shape anyway, and it is
  right to: the reader cannot know its caller checks, and the same block in a
  context without that guard takes the run down before the summary prints. It now
  collects every objection and prints `OK` or `BROKEN`, which is also strictly
  more useful than reporting the first. All four `BROKEN` branches were verified
  to fire with distinct diagnostics.

## Known limits

- **The gate sees only tracked files.** A new document stating the contract is
  invisible until it is staged. CI always works on a commit so enforcement is
  unaffected, but locally an author can be green until they `git add`. This was
  found the honest way: the gate reported the tree clean while its own ledger and
  script were untracked.
- **Clause 3 has no code to point at.** "Restore is for disaster only" constrains
  a runbook, not the chain. The prose gate keeps the runbooks that state it
  pointing at the single source, and that is the whole of its enforcement.
- **The frozen snapshot must be refreshed at a release cut**, and the header says
  so, but nothing yet asserts that its recorded revision is the newest release
  tag. A refresh that is skipped leaves the guard comparing against an older
  release, which is stricter rather than weaker, so it fails safe — but it means
  the artifact can silently describe a release two cuts back. Deriving the
  expected tag is the obvious next ratchet and was not in scope here.
- **Rewriting the frozen snapshot is the way to defeat clause 2's guard.** The
  header names this and nothing enforces it. A removal is a breaking change that
  needs a release boundary, not a snapshot refresh, and that judgement is left to
  review.
- **The class of a site is a human decision**, permanently. The gate enforces that
  every mention has one and that class A cites the single source; it cannot tell
  whether a statement was classified honestly. Misclassifying an A-class statement
  as `meta` would pass. This is the residue that cannot be closed by a gate, and
  it is why the ledger requires a per-entry note rather than only a class.
- **The budgeted pair records 2024s against the 2700s ceiling**, not the 1793s
  DD-172 states. Six entries including this FR's four have never run and are
  excluded from the total, so the ceiling binds again on the refresh that measures
  them.
