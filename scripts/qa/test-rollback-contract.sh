#!/usr/bin/env bash
# Negative fixtures for both guards of the forward-only rollback contract.
#
# The contract is stated in crates/orchestrator-persistence/src/migration.rs and
# guarded from two sides:
#
#   previous_release_schema_is_a_subset_of_current   clause 2, mechanically
#     (core/src/persistence/schema_snapshot.rs)
#   rollback-contract-single-source.rb               the prose points at the code
#
# Both are new, and a guard nobody has made fail is a guard nobody has checked.
# Every case below asserts the *diagnostic*, never the exit code: an exit code
# cannot say which branch a gate failed through, and §4.4 shape 7 records nine
# fixture-target drifts of which eight stayed green because nothing distinguished
# them. Each case is preceded by a before-run so that a case failing for an
# unrelated reason cannot read as a case that caught its mutation.
#
# Mutations are chosen to be the ones the implementations are least likely to
# catch. Where a line is removed, it is commented out rather than deleted:
# deletion is the shape an author has in mind, comment-out is the one that slips
# past text presence checks.
#
# One deliberate difference from FR-165's acceptance wording. The criterion asks
# that the prose gate "stay silent" about class B, C and D instances. This gate
# is stricter: *any* unclassified mention fails, whatever its class, because a
# gate that decides an unbooked mention's class by matching its text would be
# §4.4 shape 4 — a text pattern standing in for a semantic property, which is the
# exact defect the four-way overload creates. So the B/C/D cases below are two
# steps: the new mention fails as *unclassified* rather than as *uncited*, and
# once booked in its class the gate goes green without ever asking it to cite the
# single source. That is the property the criterion is after, proved rather than
# assumed.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

GATE="scripts/qa/rollback-contract-single-source.rb"
LEDGER="config/governance/rollback-contract-sites.json"
SNAPSHOT="config/governance/schema-snapshot.sql"
PREVIOUS="config/governance/schema-snapshot-previous-release.sql"
SUBSET_TEST="previous_release_schema_is_a_subset_of_current"
CANON="crates/orchestrator-persistence/src/migration.rs"
THREAT="docs/security/slack-gateway-threat-model.md"

PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); echo "PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL: $1" >&2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr165-rollback-contract.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

for command in ruby cargo; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

# fixture_premise / fixture_mutate, shared rather than re-implemented. Every
# mutation below goes through fixture_mutate, which proves the file actually
# changed: a substitution that silently stops matching is how eight of the nine
# recorded fixture-target drifts stayed green. scripts/qa/fixture-target-drift.rb
# enforces this mechanically and reported this file before it was true.
# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

summary() {
  echo
  echo "FR-165 rollback contract fixtures: $PASS passed, $FAIL failed"
}

# ══ Part 1: the schema additivity guard ═══════════════════════════════════════
#
# The test reads both snapshots through overridable paths, so every case here
# points it at doctored copies under $WORK. Nothing writes to the working tree,
# and no rebuild is needed between cases.

CUR="$WORK/current.sql"
PREV="$WORK/previous.sql"
cp "$REPO_ROOT/$SNAPSHOT" "$CUR"
cp "$REPO_ROOT/$PREVIOUS" "$PREV"

# Runs the subset test against whatever $CUR and $PREV currently hold. The status
# is captured directly rather than through a pipe: a pipe would hand back the
# reader's status, which is the FR-145/FR-146 defect operating on the fixture.
run_subset() {
  local log="$1"
  set +e
  (cd "$REPO_ROOT" &&
    SCHEMA_SNAPSHOT_PATH="$CUR" \
      PREVIOUS_RELEASE_SCHEMA_SNAPSHOT_PATH="$PREV" \
      cargo test -p agent-orchestrator "$SUBSET_TEST" >"$log" 2>&1)
  local status=$?
  set -e
  return "$status"
}

restore_snapshots() {
  cp "$REPO_ROOT/$SNAPSHOT" "$CUR"
  cp "$REPO_ROOT/$PREVIOUS" "$PREV"
}

# Each schema case starts from a fresh copy of the real snapshot and edits it in
# place, so fixture_mutate has a before-state to digest: a pattern that has
# stopped matching leaves the file unchanged and fails the case by name rather
# than silently testing an unmutated artifact. fixture_mutate is called directly
# at each site rather than through a local helper, because
# scripts/qa/fixture-target-drift.rb recognises the wrapper by name at the head
# of the statement and a helper of ours would hide the mutation from it — the
# scanner is right to insist, so the call sites are shaped to be visible.
fresh_current() { cp "$REPO_ROOT/$SNAPSHOT" "$CUR"; }

if run_subset "$WORK/subset-base.log"; then
  pass "schema before-run: the subset guard is green on the real snapshots"
else
  fail "schema before-run: the subset guard is already failing before any mutation:"
  sed 's/^/    /' "$WORK/subset-base.log" >&2
  summary
  exit 1
fi

# ── 1. A table removed, the way a regenerated snapshot would show it ──────────
# Both the CREATE TABLE and its indexes go, because that is what regenerating
# schema-snapshot.sql after a DROP TABLE actually produces. Removing only the
# table leaves orphaned indexes and fails through a different branch — case 5
# covers that separately, and conflating them is how a fixture comes to assert a
# branch it never reaches.
fresh_current
if fixture_mutate "table removal" "$CUR" \
  sed -i.bak -e '/^CREATE TABLE attention_actions /d' -e '/ ON attention_actions(/d' "$CUR"; then
  rm -f "$CUR.bak"
  if run_subset "$WORK/subset-table.log"; then
    fail "a table the previous release knows about was removed and the guard passed"
  elif grep -qF "table attention_actions existed in the previous release and is gone" \
    "$WORK/subset-table.log"; then
    pass "a removed table is named, with the table"
  else
    fail "the guard failed, but not with the removed-table diagnostic:"
    sed 's/^/    /' "$WORK/subset-table.log" >&2
  fi
fi
restore_snapshots

# ── 2. One column removed from a table that stays ────────────────────────────
# The case whole-object comparison cannot see. Nothing about the table's
# existence changes; a set comparison over CREATE TABLE names passes cleanly.
fresh_current
if fixture_mutate "column removal" "$CUR" \
  sed -i.bak 's/, writer_fencing_token INTEGER NOT NULL DEFAULT 0//' "$CUR"; then
  rm -f "$CUR.bak"
  if run_subset "$WORK/subset-column.log"; then
    fail "a column the previous release reads was removed and the guard passed"
  elif grep -qF "column agent_sessions.writer_fencing_token existed in the previous release and is gone" \
    "$WORK/subset-column.log"; then
    pass "a removed column is named, with both the table and the column"
  else
    fail "the guard failed, but not with the removed-column diagnostic:"
    sed 's/^/    /' "$WORK/subset-column.log" >&2
  fi
fi
restore_snapshots

# ── 3. An index removed ──────────────────────────────────────────────────────
# Commented out rather than deleted: a guard that read the file as text rather
# than executing it would still see the name on the line.
fresh_current
if fixture_mutate "index removal" "$CUR" \
  sed -i.bak 's|^CREATE INDEX idx_attention_task |-- CREATE INDEX idx_attention_task |' "$CUR"; then
  rm -f "$CUR.bak"
  if run_subset "$WORK/subset-index.log"; then
    fail "an index the previous release depends on was commented out and the guard passed"
  elif grep -qF "index idx_attention_task existed in the previous release and is gone" \
    "$WORK/subset-index.log"; then
    pass "a commented-out index is named, not read as still present"
  else
    fail "the guard failed, but not with the removed-index diagnostic:"
    sed 's/^/    /' "$WORK/subset-index.log" >&2
  fi
fi
restore_snapshots

# ── 4. A pure addition must PASS ─────────────────────────────────────────────
# The direction this guard must never block. A subset check that also failed on
# additions would stop every forward migration, which is the opposite of the
# contract: forward-only means forward is allowed.
fresh_current
if fixture_mutate "pure addition" "$CUR" \
  tee -a "$CUR" <<<'CREATE TABLE fixture_added_table ( id TEXT PRIMARY KEY, note TEXT );
CREATE INDEX idx_fixture_added ON fixture_added_table(note);'; then
  if run_subset "$WORK/subset-add.log"; then
    pass "a new table and index are allowed; the guard does not block forward motion"
  else
    fail "adding a table failed the subset guard, which would block every migration:"
    sed 's/^/    /' "$WORK/subset-add.log" >&2
  fi
fi
restore_snapshots

# ── 5. A malformed artifact must fail loudly, not resolve silently ───────────
# The table is commented out and its indexes are left behind, so no execution
# order can apply them. This is the branch that catches a hand-edited snapshot,
# and it must be distinguishable from case 1: both are failures, and they mean
# different things.
fresh_current
if fixture_mutate "orphaned indexes" "$CUR" \
  sed -i.bak 's|^CREATE TABLE attention_actions |-- CREATE TABLE attention_actions |' "$CUR"; then
  rm -f "$CUR.bak"
  if run_subset "$WORK/subset-orphan.log"; then
    fail "a snapshot whose indexes reference a missing table was accepted"
  elif grep -qF "cannot be applied in any order" "$WORK/subset-orphan.log"; then
    pass "an unapplicable snapshot fails naming the statements, through its own branch"
  else
    fail "the guard failed, but not through the unapplicable-statement branch:"
    sed 's/^/    /' "$WORK/subset-orphan.log" >&2
  fi
fi
restore_snapshots

# ── 6. An empty previous side must fail closed ───────────────────────────────
# Zero tables and no removals are indistinguishable in a subset comparison: the
# difference of an empty set is empty, so every check passes vacuously. §4.4
# shape 5, on the artifact rather than on a loop.
: >"$PREV"
if run_subset "$WORK/subset-empty.log"; then
  fail "the guard passed with an empty previous-release snapshot; every check was vacuous"
elif grep -qF "yielded no tables" "$WORK/subset-empty.log"; then
  pass "an empty previous-release snapshot fails closed and says the read was empty"
else
  fail "the guard failed on an empty artifact, but not with the read-nothing diagnostic:"
  sed 's/^/    /' "$WORK/subset-empty.log" >&2
fi
restore_snapshots

if run_subset "$WORK/subset-after.log"; then
  pass "schema after-run: the snapshots are back where they started"
else
  fail "schema after-run: a mutation was left behind:"
  sed 's/^/    /' "$WORK/subset-after.log" >&2
fi

# ══ Part 2: the prose single-source gate ══════════════════════════════════════
#
# A scratch copy of the working tree, so no mutation touches the real one.
# Copied from the working tree rather than from HEAD: a fixture that tests HEAD
# cannot see the change being made, so it would pass on an unmodified gate and
# tell its author nothing until after the commit.

TREE="$WORK/tree"
mkdir -p "$TREE"
git -C "$REPO_ROOT" ls-files -co --exclude-standard -z |
  tar -C "$REPO_ROOT" --null -T - -cf - |
  tar -x -C "$TREE"

# The gate derives its scope from `git ls-files`, so the scratch tree has to be a
# repository or the gate reads nothing and every case below passes for the wrong
# reason. Initialising one keeps the fixture on the production code path rather
# than giving the gate a second way to enumerate files: a scope predicate that
# only fixtures exercise is a scope predicate nothing checks.
git -C "$TREE" init -q
git -C "$TREE" add -A >/dev/null 2>&1

run_gate() {
  local log="$1"
  set +e
  (cd "$TREE" && ruby "$GATE" >"$log" 2>&1)
  local status=$?
  set -e
  return "$status"
}

restore() {
  cp "$REPO_ROOT/$1" "$TREE/$1"
}

# Booking a site in the scratch ledger, so a case can prove not just that an
# unclassified mention fails but that the classification is what clears it.
#
# This is a real in-place rewrite of a fixture input, so it goes through
# fixture_mutate like every other mutation here: a book_site whose JSON edit
# silently matched nothing would leave the following assertion testing an
# unbooked site while claiming to test a booked one.
book_site() {
  local path="$1" digest="$2" klass="$3"
  fixture_mutate "booking $path as class $klass" "$TREE/$LEDGER" \
    ruby -rjson -e '
      ledger = JSON.parse(File.read(ARGV[0]))
      ledger["sites"] << { "path" => ARGV[1], "digest" => ARGV[2], "class" => ARGV[3],
                           "note" => "fixture" }
      File.write(ARGV[0], JSON.pretty_generate(ledger))
    ' "$TREE/$LEDGER" "$path" "$digest" "$klass"
}

digest_of() {
  ruby -rdigest -e 'print Digest::SHA256.hexdigest(ARGV[0].strip)[0, 12]' "$1"
}

if run_gate "$WORK/prose-base.log"; then
  pass "prose before-run: the gate is green on the unmutated tree"
else
  fail "prose before-run: the gate is already failing before any mutation:"
  sed 's/^/    /' "$WORK/prose-base.log" >&2
  summary
  exit 1
fi

# ── 7. An unclassified mention fails ─────────────────────────────────────────
VICTIM="docs/architecture.md"
printf '\nA new sentence claiming migrations are forward-only, booked nowhere.\n' >>"$TREE/$VICTIM"
if run_gate "$WORK/prose-new.log"; then
  fail "a new unclassified mention did not trip the gate"
elif grep -qF "is not classified in $LEDGER" "$WORK/prose-new.log" &&
  grep -qF "$VICTIM" "$WORK/prose-new.log"; then
  pass "an unclassified mention is named, with its file and its digest"
else
  fail "the gate failed, but not with the unclassified diagnostic naming $VICTIM:"
  sed 's/^/    /' "$WORK/prose-new.log" >&2
fi
restore "$VICTIM"

# ── 8-10. B, C and D each fail as *unclassified*, then clear without citing ──
# Two steps per class, and the first step matters as much as the second: the
# diagnostic must be "not classified" and must NOT be "must cite the single
# source", because the second would mean the gate had decided the class from the
# text.
check_non_citing_class() {
  local label="$1" file="$2" sentence="$3" klass="$4"
  printf '\n%s\n' "$sentence" >>"$TREE/$file"
  local digest
  digest="$(digest_of "$sentence")"

  if run_gate "$WORK/prose-$klass-unbooked.log"; then
    fail "$label: an unbooked class-$klass mention passed without being classified"
  elif grep -qF "is not classified" "$WORK/prose-$klass-unbooked.log" &&
    ! grep -qF "must name the line that cites" "$WORK/prose-$klass-unbooked.log"; then
    pass "$label: fails as unclassified, not as uncited — the gate does not guess the class"
  else
    fail "$label: the gate failed, but not through the unclassified branch:"
    sed 's/^/    /' "$WORK/prose-$klass-unbooked.log" >&2
  fi

  book_site "$file" "$digest" "$klass"
  if run_gate "$WORK/prose-$klass-booked.log"; then
    pass "$label: booked as class $klass, the gate is silent and never asks it to cite $CANON"
  else
    fail "$label: booked as class $klass and the gate still failed:"
    sed 's/^/    /' "$WORK/prose-$klass-booked.log" >&2
  fi
  restore "$file"
  restore "$LEDGER"
}

check_non_citing_class "class B" "docs/architecture.md" \
  'The Gateway applies its own additive forward-only schema in its own database.' "B"

check_non_citing_class "class C" "crates/orchestrator-collab/src/dag.rs" \
  '// Forward only the newest sibling artifact, unrelated to any schema.' "C"

# The case this ledger's whole shape exists for: a fourth-sense mention placed in
# the threat model, the one file that already holds both an A-class row (T12) and
# a D-class row (T8) four rows apart.
check_non_citing_class "class D beside T8" "$THREAT" \
  '| T14 | Fixture row | Fixture | Monotonic forward-only state transitions | none |' "D"

# ── 11. A D-class site in that file must not disable A's check there ─────────
# The per-site keying claim, stated as behaviour. A path-keyed ledger would read
# the threat model as one classified file and stop checking T12's citation; if
# that were happening, breaking T12 would go unnoticed. Both mutations are
# applied at once so the assertion is specifically that A is still enforced in a
# file that also contains D.
sentence='| T15 | Fixture row | Fixture | Monotonic forward-only state transitions | none |'
printf '\n%s\n' "$sentence" >>"$TREE/$THREAT"
book_site "$THREAT" "$(digest_of "$sentence")" "D"
if fixture_mutate "per-site keying" "$TREE/$THREAT" \
  sed -i.bak "s|Independent forward-only migrations per the contract in \`$CANON\`|Independent forward-only migrations|" \
  "$TREE/$THREAT"; then
  rm -f "$TREE/$THREAT.bak"
  if run_gate "$WORK/prose-persite.log"; then
    fail "T12's citation was removed from a file containing a class-D site and the gate passed; the ledger is behaving as if keyed by path"
  elif grep -qF "$THREAT" "$WORK/prose-persite.log"; then
    pass "a class-D site in the same file does not stop the class-A site there being checked"
  else
    fail "the gate failed, but without naming $THREAT:"
    sed 's/^/    /' "$WORK/prose-persite.log" >&2
  fi
fi
rm -f "$TREE/$THREAT.bak"
restore "$THREAT"
restore "$LEDGER"

# ── 12. An A-class statement going away must trip the mirror condition ───────
# The branch that catches the gate going blind rather than the tree going wrong.
# Commented out rather than deleted, and in a Rust-free markdown file the comment
# marker is HTML, so the line still contains every word it did before.
VICTIM="docs/guide/agent-process-console-v1-operations.md"
if fixture_mutate "mirror condition" "$TREE/$VICTIM" \
  sed -i.bak 's|^- Console migrations are additive and forward-only\.|<!-- - Console migrations are additive and forward-only.|' \
  "$TREE/$VICTIM"; then
  rm -f "$TREE/$VICTIM.bak"
  if run_gate "$WORK/prose-blind.log"; then
    fail "a booked class-A statement was commented out and the gate reported success"
  elif grep -qF "is booked but no line in the tree matches it" "$WORK/prose-blind.log" &&
    grep -qF "$VICTIM" "$WORK/prose-blind.log"; then
    pass "the mirror condition catches a booked statement going away, and names the file"
  else
    fail "the gate failed, but not through the mirror condition:"
    sed 's/^/    /' "$WORK/prose-blind.log" >&2
  fi
fi
rm -f "$TREE/$VICTIM.bak"
restore "$VICTIM"

# ── 13. Losing the citation, in all three ways it can be lost ────────────────
#
# There are two kinds of class-A site and they fail differently, which is worth
# stating because the difference was found by writing the fixture rather than by
# designing it. Eleven sites are *self-citing*: the statement names the single
# source on its own line. Four have a *separated* citation, because the statement
# is a heading or a recorded table row that cannot carry a path.
#
# For a self-citing site the citation cannot be removed without changing the
# statement's own digest, so tampering is caught by the mirror condition rather
# than by the citation check. That is a property of the design and not a gap, but
# only a fixture can tell you which branch actually reports.

# 13a. Self-citing: editing the line takes the statement with it.
VICTIM="docs/qa/orchestrator/161-slack-reaction-skill-automation-release.md"
if fixture_mutate "self-citing statement" "$TREE/$VICTIM" \
  sed -i.bak "s|, per the contract in \`$CANON\`\.|, per the contract documented elsewhere.|" \
  "$TREE/$VICTIM"; then
  rm -f "$TREE/$VICTIM.bak"
  if run_gate "$WORK/prose-selfcite.log"; then
    fail "a self-citing class-A statement dropped its citation and the gate passed"
  elif grep -qF "$VICTIM 85290bbfc90c is booked but no line in the tree matches it" \
    "$WORK/prose-selfcite.log"; then
    pass "a self-citing statement cannot shed its citation without the mirror condition firing"
  else
    fail "the gate failed, but not through the mirror condition naming that site:"
    sed 's/^/    /' "$WORK/prose-selfcite.log" >&2
  fi
fi
rm -f "$TREE/$VICTIM.bak"
restore "$VICTIM"

# 13b. Separated citation removed. Here the statement is untouched and only the
# citing line goes, so this reaches the citation branch proper — the case a
# self-citing site cannot produce.
VICTIM="docs/design_doc/orchestrator/111-control-plane-action-audit-envelope.md"
if fixture_mutate "separated citation" "$TREE/$VICTIM" \
  sed -i.bak "s|^The contract those two sentences depend on|<!-- The contract those two sentences depend on|" \
  "$TREE/$VICTIM"; then
  rm -f "$TREE/$VICTIM.bak"
  if run_gate "$WORK/prose-nocite.log"; then
    fail "a class-A statement lost its separated citation and the gate passed"
  elif grep -qF "citedBy 73e1103974e8 matches no line in that file" "$WORK/prose-nocite.log" &&
    grep -qF "$VICTIM" "$WORK/prose-nocite.log"; then
    pass "a separated citation going away is caught while its statement stands untouched"
  else
    fail "the gate failed, but not with the missing-citation diagnostic naming $VICTIM:"
    sed 's/^/    /' "$WORK/prose-nocite.log" >&2
  fi
fi
rm -f "$TREE/$VICTIM.bak"
restore "$VICTIM"

# 13c. The §4.4 shape 1 case, and the one a presence check cannot see. The
# citation is repointed at a line that really exists in the same file and really
# is matched by digest — it simply does not name the single source. This is the
# careless-ledger-edit shape: a citation moves, someone re-points citedBy at
# whatever is nearby, and a gate that only checked the line's existence would go
# green. The decoy is derived from the tree (the statement's own line, which does
# not name the path) rather than typed, so it cannot go stale.
DECOY="188979330c52"
if fixture_mutate "citation repointed at a decoy line" "$TREE/$LEDGER" \
  ruby -rjson -e '
    ledger = JSON.parse(File.read(ARGV[0]))
    ledger["sites"].each do |site|
      next unless site["path"] == ARGV[1] && site["digest"] == ARGV[2]
      site["citedBy"] = ARGV[3]
    end
    File.write(ARGV[0], JSON.pretty_generate(ledger))
  ' "$TREE/$LEDGER" "$VICTIM" "$DECOY" "$DECOY"; then
  if run_gate "$WORK/prose-decoy.log"; then
    fail "citedBy was repointed at a line that does not name the single source and the gate passed"
  elif grep -qF "the citing line does not name $CANON" "$WORK/prose-decoy.log"; then
    pass "a citation pointing at a real line that names nothing is caught; presence is not enough"
  else
    fail "the gate failed, but not through the citation-content branch:"
    sed 's/^/    /' "$WORK/prose-decoy.log" >&2
  fi
fi
restore "$LEDGER"

# ── 14. The single source itself going away ──────────────────────────────────
# Every class-A citation points at one path. If that path stops existing, the
# citations are all vacuously satisfiable and the gate has nothing left to mean.
mkdir -p "$WORK/parked"
mv "$TREE/$CANON" "$WORK/parked/migration.rs"
if run_gate "$WORK/prose-nosource.log"; then
  fail "the single source was removed and the gate still reported success"
elif grep -qF "does not name a file that exists" "$WORK/prose-nosource.log"; then
  pass "a missing single source fails closed rather than making every citation vacuous"
else
  fail "the gate failed, but not with the missing-single-source diagnostic:"
  sed 's/^/    /' "$WORK/prose-nosource.log" >&2
fi
mkdir -p "$(dirname "$TREE/$CANON")"
mv "$WORK/parked/migration.rs" "$TREE/$CANON"

# ── 15. An empty scan must fail closed ───────────────────────────────────────
# Zero scanned files and a clean scan are different facts and only one of them is
# evidence (§4.4 shape 5). Removing the index is the mechanical form of the scope
# derivation failing for any reason — a git that is not on PATH, a checkout that
# is not a repository, a permission error — and all of them produce a gate that
# has read nothing and would otherwise print a clean result.
mv "$TREE/.git" "$WORK/git-parked"
if run_gate "$WORK/prose-empty.log"; then
  fail "the gate reported success having enumerated no files at all"
elif grep -qF "the scan read nothing" "$WORK/prose-empty.log"; then
  pass "a scope derivation that returns nothing fails closed and says so"
else
  fail "the gate failed on an empty scan, but not with the read-nothing diagnostic:"
  sed 's/^/    /' "$WORK/prose-empty.log" >&2
fi
mv "$WORK/git-parked" "$TREE/.git"

if run_gate "$WORK/prose-after.log"; then
  pass "prose after-run: every mutation was reverted and the gate is green again"
else
  fail "prose after-run: the scratch tree did not return to its starting state:"
  sed 's/^/    /' "$WORK/prose-after.log" >&2
fi

summary
[[ "$FAIL" -eq 0 ]]
