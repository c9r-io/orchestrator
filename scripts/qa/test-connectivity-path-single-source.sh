#!/usr/bin/env bash
# Negative fixtures for scripts/qa/connectivity-path-single-source.rb.
#
# The ledger gate reports a count, and a count that has never been made to move
# is a count nobody has checked. Each case below reintroduces one shape of the
# FR-163 defect into a scratch copy of the tree and asserts the gate names it —
# the diagnostic, not the exit code, because an exit code cannot say which of
# the gate's branches fired (§4.4 shape 7).
#
# The mutations are chosen to be the ones the implementation is least likely to
# catch, not the ones it obviously would:
#   1. a second spelling added in a *comment* — must NOT trip the gate, because
#      a gate that counts prose is the DD-142 defect and would be worked around
#      by people rewording comments;
#   2. a second spelling added as real code — must trip it;
#   3. the canonical file moved out of scan range — must trip the *mirror*
#      condition, the one that catches the gate going blind rather than the code
#      going wrong. A gate without this reports success having read nothing.
set -euo pipefail

. "$(git rev-parse --show-toplevel)/scripts/lib/gate_runlog.sh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="scripts/qa/connectivity-path-single-source.rb"
CANON="crates/orchestrator-config/src/paths.rs"
VICTIM="crates/daemon/src/lifecycle.rs"

PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); echo "PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL: $1" >&2; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr163-ledger-fixture.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

gate_runlog_arm "scripts/qa/test-connectivity-path-single-source.sh"

# A scratch copy of the tree, so no mutation can touch the real one. Copied from
# the *working tree* rather than from HEAD: a fixture that tests HEAD cannot see
# the change being made, so it would pass on an unmodified gate and tell its
# author nothing until after the commit. `-co --exclude-standard` is tracked
# files plus untracked ones that are not ignored, which is what a reviewer would
# call "the tree" — and it keeps target/ out.
TREE="$WORK/tree"
mkdir -p "$TREE"
git -C "$REPO_ROOT" ls-files -co --exclude-standard -z |
  tar -C "$REPO_ROOT" --null -T - -cf - |
  tar -x -C "$TREE"

# Fixture 0 establishes the before-run. Without it a case that fails for an
# unrelated reason — a syntax error, a missing dependency — reads exactly like a
# case that caught its mutation (§4.4 shape 7).
if (cd "$TREE" && ruby "$GATE" >"$WORK/base.log" 2>&1); then
  pass "before-run: the gate is green on the unmutated tree"
else
  fail "before-run: the gate is already failing before any mutation was applied:"
  sed 's/^/    /' "$WORK/base.log" >&2
  echo
  echo "FR-163 path-ledger fixtures: $PASS passed, $FAIL failed"
  exit 1
fi

run_gate() {
  local log="$1"
  (cd "$TREE" && ruby "$GATE" >"$log" 2>&1)
}

# Restore from the working tree, the same source the scratch copy came from.
# Restoring from HEAD instead would silently undo the author's own edits along
# with the mutation, and the later cases would then run against a tree nobody
# was looking at.
restore() {
  cp "$REPO_ROOT/$1" "$TREE/$1"
}

# ── 1. A spelling in a comment must not count ────────────────────────────────
# The mutation the gate is most likely to get wrong in the *false positive*
# direction. If this trips, the gate is counting prose and its numbers mean
# nothing.
printf '\n// A comment naming "orchestrator.sock" and "agent_orchestrator.db".\n' \
  >>"$TREE/$VICTIM"
if run_gate "$WORK/comment.log"; then
  pass "a layout name inside a comment does not count as a spelling"
else
  fail "a comment naming layout files tripped the gate — it is counting prose:"
  sed 's/^/    /' "$WORK/comment.log" >&2
fi
restore "$VICTIM"

# ── 2. A second spelling in real code must be named ──────────────────────────
# Appended as a live function rather than editing an existing one: the subject
# is the presence of a second spelling anywhere, so position must not matter.
cat >>"$TREE/$VICTIM" <<'RUST'

pub fn fixture_second_spelling(data_dir: &Path) -> PathBuf {
    data_dir.join("orchestrator.sock")
}
RUST
if run_gate "$WORK/code.log"; then
  fail "a second spelling of orchestrator.sock in live code did not trip the gate"
elif grep -qF "$VICTIM" "$WORK/code.log" && grep -qF "socket file name" "$WORK/code.log"; then
  pass "a second spelling in live code is named, with the file and the fact"
else
  fail "the gate failed, but not with the socket-file diagnostic naming $VICTIM:"
  sed 's/^/    /' "$WORK/code.log" >&2
fi
restore "$VICTIM"

# ── 3. The canonical definition going out of scope must trip the mirror ──────
# The failure mode a presence-only gate cannot see: nothing spells the name
# twice, because nothing spells it at all any more. Moving the file out of the
# scanned roots is the mechanical form of a crate rename or a module move.
mkdir -p "$TREE/out-of-scope"
mv "$TREE/$CANON" "$TREE/out-of-scope/paths.rs"
if run_gate "$WORK/blind.log"; then
  fail "the canonical definition left the scan and the gate still reported success"
elif grep -qF "allowlisted but the scan found no spelling there" "$WORK/blind.log"; then
  pass "the mirror condition catches the canonical file leaving the scan"
else
  fail "the gate failed, but not through the mirror condition:"
  sed 's/^/    /' "$WORK/blind.log" >&2
fi
mkdir -p "$(dirname "$TREE/$CANON")"
mv "$TREE/out-of-scope/paths.rs" "$TREE/$CANON"

# ── 4. An empty scan must fail closed ────────────────────────────────────────
# Zero scanned files and a clean scan are different facts, and only one is
# evidence (§4.4 shape 5).
mv "$TREE/core" "$WORK/core-parked"
mv "$TREE/crates" "$WORK/crates-parked"
if run_gate "$WORK/empty.log"; then
  fail "the gate reported success with no source files to scan"
elif grep -qF "the scan read nothing" "$WORK/empty.log"; then
  pass "an empty scan fails closed and says so"
else
  fail "the gate failed on an empty tree, but not with the read-nothing diagnostic:"
  sed 's/^/    /' "$WORK/empty.log" >&2
fi
mv "$WORK/core-parked" "$TREE/core"
mv "$WORK/crates-parked" "$TREE/crates"

# The tree must be back where it started, or a later reader of this fixture
# would be reasoning about a mutation nobody removed.
if run_gate "$WORK/after.log"; then
  pass "after-run: every mutation was reverted and the gate is green again"
else
  fail "after-run: the scratch tree did not return to its starting state:"
  sed 's/^/    /' "$WORK/after.log" >&2
fi

echo
echo "FR-163 path-ledger fixtures: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
