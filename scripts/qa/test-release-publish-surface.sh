#!/usr/bin/env bash
#
# FR-150: release publish/ship surface truth.
#
# The crates.io publish loop in release.yml is a hand-typed list, and it went
# stale the way every hand-typed list does (§4.4 shape 2): orchestrator-persistence
# and slack-gateway were extracted after the list was written, both are non-dev
# dependencies of crates the loop does publish, and the next tag would have
# failed on "no matching package" — after the GitHub Release and Homebrew push
# had already succeeded. The shipped-target surface had the same defect in the
# other direction: install.sh happily composed x86_64-apple-darwin, a triple
# release.yml never builds, and died in a bare curl 404.
#
# Three assertions, each derived from the repository rather than restated:
#
#   1. The publish loop names exactly the publishable workspace members
#      (cargo metadata, publish != false), in dependency-topological order.
#   2. The shipped-target set is identical across release.yml's build matrix,
#      install.sh's SUPPORTED_TARGETS, and the Homebrew formula's url stanzas.
#   3. install.sh actually refuses an unsupported platform: behavioral, run
#      under a stubbed uname, with a sentinel curl that fails loudly if the
#      real download path is ever reached (§4.4: a proxy may be an additional
#      condition, never the only one).
#
# Usage:
#   test-release-publish-surface.sh                 verify the real repository
#   test-release-publish-surface.sh --fixture-test  prove the checks fail on injected defects

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

for command in cargo jq; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

# shellcheck source=../lib/gate_jq.sh
. "$REPO_ROOT/scripts/lib/gate_jq.sh"
# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

if [[ "${1:-}" != "" && "${1:-}" != "--fixture-test" ]]; then
  echo "usage: $0 [--fixture-test]" >&2
  exit 2
fi

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

WORK="$(mktemp -d)"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

# ── The publishable set, computed once from the real workspace ────────────────
#
# The Cargo manifests are the fact source for both fixture and real mode; the
# fixtures mutate only the surfaces that are compared against them. `publish`
# renders as null when a crate is publishable and as [] when publish = false.
METADATA="$WORK/metadata.json"
if ! cargo metadata --manifest-path "$REPO_ROOT/Cargo.toml" \
    --format-version 1 --no-deps > "$METADATA" 2>"$WORK/metadata.err"; then
  echo "cargo metadata failed; nothing below can assert anything:" >&2
  cat "$WORK/metadata.err" >&2
  exit 1
fi

# One row per publishable package: "<repo-relative dir>\t<package name>".
PKG_ROWS="$(gate_jq_rows require-rows "$METADATA" \
  '.packages[] | select(.publish == null) | [.manifest_path, .name] | @tsv')" || {
  echo "could not derive the publishable set from cargo metadata" >&2
  exit 1
}
PUBLISHABLE_DIRS="$WORK/publishable_dirs"
printf '%s\n' "$PKG_ROWS" \
  | sed -e "s|^$REPO_ROOT/||" -e 's|/Cargo.toml\t|\t|' \
  | cut -f1 | LC_ALL=C sort > "$PUBLISHABLE_DIRS"

# One row per intra-workspace normal dependency of a publishable package:
# "<crate dir>\t<dependency dir>", both repo-relative. kind == null is a normal
# dependency; dev and build dependencies do not constrain crates.io publish
# order. .path is present exactly for path dependencies, i.e. workspace members.
DEP_ROWS="$(gate_jq_rows allow-empty "$METADATA" \
  '.packages[] | select(.publish == null) as $p
   | $p.dependencies[] | select(.kind == null and .path != null)
   | [$p.manifest_path, .path] | @tsv')" || {
  echo "could not derive workspace dependency edges from cargo metadata" >&2
  exit 1
}
DEP_EDGES="$WORK/dep_edges"
printf '%s\n' "$DEP_ROWS" \
  | sed -e "s|$REPO_ROOT/||g" -e 's|/Cargo.toml\t|\t|' > "$DEP_EDGES"

# ── Shared extraction helpers ─────────────────────────────────────────────────
#
# Extraction failing is a failed assertion, never a skip (§4.4 shape 7): a
# refactor that renames the loop or the variable must land here as a red gate
# whose diagnostic says what moved, not as a vacuous pass over zero rows.

# Prints the publish loop's crate paths in order, one per line.
extract_publish_list() {
  local file="$1"
  awk '
    /for crate in \\$/ { active = 1; next }
    active {
      line = $0
      gsub(/^[[:space:]]+/, "", line)
      if (line ~ /;[[:space:]]*do$/) {
        sub(/;[[:space:]]*do$/, "", line)
        if (line != "") print line
        active = 0
        next
      }
      sub(/[[:space:]]*\\$/, "", line)
      if (line != "") print line
    }
  ' "$file"
}

# Prints the build matrix target triples, one per line, sorted.
extract_matrix_targets() {
  local file="$1"
  grep -E '^[[:space:]]+target: [a-z0-9_]+-[a-z0-9_-]+$' "$file" \
    | awk '{print $2}' | LC_ALL=C sort -u
}

# Prints install.sh's SUPPORTED_TARGETS entries, one per line, sorted.
extract_install_targets() {
  local file="$1"
  sed -n 's/^SUPPORTED_TARGETS="\(.*\)"$/\1/p' "$file" | tr ' ' '\n' \
    | sed '/^$/d' | LC_ALL=C sort -u
}

# Prints the target triples named by the formula's url stanzas, sorted.
# Anchored so a commented-out stanza does not count as shipped — a bare
# substring grep here would be satisfied by exactly the mutation fixture 3
# injects (§4.4 shape 1: text presence standing in for effect).
extract_formula_targets() {
  local file="$1"
  grep -E '^[[:space:]]*url "' "$file" \
    | grep -oE '[a-z0-9_]+-(apple-darwin|unknown-linux-(gnu|musl))' \
    | LC_ALL=C sort -u
}

# ── Check 1: publish loop completeness and dependency order ──────────────────
#
# Completeness and order live in one check deliberately: a crate missing from
# the loop also breaks the order premise of everything that depends on it, so
# two separate checks could never be failed in isolation by one fixture.
check_publish_loop() {
  local release_yml="$1"
  local listed missing extra rc=0

  listed="$(extract_publish_list "$release_yml")"
  if [[ -z "$listed" ]]; then
    echo "    no 'for crate in \\ ... ; do' publish loop found in $release_yml" >&2
    echo "    if the loop was refactored, teach extract_publish_list its new shape" >&2
    return 1
  fi
  printf '%s\n' "$listed" > "$WORK/listed"
  LC_ALL=C sort "$WORK/listed" > "$WORK/listed_sorted"

  missing="$(comm -23 "$PUBLISHABLE_DIRS" "$WORK/listed_sorted")"
  if [[ -n "$missing" ]]; then
    echo "    release.yml publish loop omits publishable crate(s):" >&2
    printf '      %s\n' $missing >&2
    echo "    a tag would fail mid-publish on 'no matching package' after the" >&2
    echo "    GitHub Release already went out" >&2
    rc=1
  fi
  extra="$(comm -13 "$PUBLISHABLE_DIRS" "$WORK/listed_sorted")"
  if [[ -n "$extra" ]]; then
    echo "    release.yml publish loop names path(s) that are not publishable workspace members:" >&2
    printf '      %s\n' $extra >&2
    rc=1
  fi
  [[ "$rc" -ne 0 ]] && return 1

  # Order: every workspace dependency of a listed crate must be listed earlier.
  local crate dep crate_pos dep_pos
  while IFS=$'\t' read -r crate dep; do
    [[ -z "$crate" ]] && continue
    grep -qx "$dep" "$PUBLISHABLE_DIRS" || continue
    # sed -n '1p' rather than head -1: every head short-circuits and under
    # pipefail an EPIPE'd producer becomes the pipeline's status (FR-146).
    crate_pos="$(grep -nx "$crate" "$WORK/listed" | cut -d: -f1 | sed -n '1p')"
    dep_pos="$(grep -nx "$dep" "$WORK/listed" | cut -d: -f1 | sed -n '1p')"
    if [[ -z "$crate_pos" || -z "$dep_pos" ]]; then
      echo "    internal: $crate or $dep vanished between the set and order passes" >&2
      return 1
    fi
    if [[ "$dep_pos" -ge "$crate_pos" ]]; then
      echo "    publish order violation: $crate (position $crate_pos) is published" >&2
      echo "    before its dependency $dep (position $dep_pos)" >&2
      rc=1
    fi
  done < "$DEP_EDGES"
  return "$rc"
}

# ── Check 2: one shipped-target set across all three surfaces ────────────────
check_ship_surface() {
  local release_yml="$1" install_sh="$2" formula_rb="$3"
  local matrix install formula rc=0

  matrix="$(extract_matrix_targets "$release_yml")"
  if [[ -z "$matrix" ]]; then
    echo "    no literal 'target:' entries found in $release_yml build matrix" >&2
    return 1
  fi
  install="$(extract_install_targets "$install_sh")"
  if [[ -z "$install" ]]; then
    echo "    no SUPPORTED_TARGETS=\"...\" line found in $install_sh" >&2
    return 1
  fi
  formula="$(extract_formula_targets "$formula_rb")"
  if [[ -z "$formula" ]]; then
    echo "    no url stanzas naming target triples found in $formula_rb" >&2
    return 1
  fi

  if [[ "$matrix" != "$install" ]]; then
    echo "    release.yml matrix and install.sh SUPPORTED_TARGETS disagree:" >&2
    diff <(printf '%s\n' "$matrix") <(printf '%s\n' "$install") | sed 's/^/      /' >&2 || true
    rc=1
  fi
  if [[ "$matrix" != "$formula" ]]; then
    echo "    release.yml matrix and the Homebrew formula disagree:" >&2
    diff <(printf '%s\n' "$matrix") <(printf '%s\n' "$formula") | sed 's/^/      /' >&2 || true
    rc=1
  fi
  return "$rc"
}

# ── Check 3: install.sh refuses an unsupported platform, behaviorally ─────────
#
# Runs the real script under a stubbed uname reporting Darwin/x86_64 — the
# platform that used to die in a bare curl 404. The stub PATH also carries a
# sentinel curl: if the refusal ever regresses, the run does not silently reach
# the network, it trips the sentinel and the diagnostic names the regression.
check_install_refusal() {
  local install_sh="$1"
  local stub="$WORK/stub-$$" out rc=0
  mkdir -p "$stub"
  cat > "$stub/uname" <<'EOF'
#!/bin/sh
case "${1:-}" in
  -m) echo x86_64 ;;
  *) echo Darwin ;;
esac
EOF
  cat > "$stub/curl" <<EOF
#!/bin/sh
touch "$stub/curl-was-reached"
echo "sentinel curl invoked: install.sh reached the download path on an unsupported platform" >&2
exit 86
EOF
  chmod +x "$stub/uname" "$stub/curl"
  rm -f "$stub/curl-was-reached"

  out="$WORK/install-refusal.log"
  if PATH="$stub:$PATH" INSTALL_ORCHESTRATOR_VERSION="v0.0.0-fixture" \
      sh "$install_sh" > "$out" 2>&1; then
    echo "    install.sh exited 0 on x86_64 macOS, a platform with no artifact" >&2
    return 1
  fi
  if [[ -e "$stub/curl-was-reached" ]]; then
    echo "    install.sh reached curl on an unsupported platform instead of refusing first" >&2
    rc=1
  fi
  if ! grep -q "no prebuilt binaries for x86_64-apple-darwin" "$out"; then
    echo "    the refusal did not name the unsupported triple; actual output:" >&2
    sed 's/^/      /' "$out" >&2
    rc=1
  fi
  if ! grep -q "cargo install" "$out"; then
    echo "    the refusal offers no build-from-source alternative" >&2
    rc=1
  fi
  return "$rc"
}

# ── Real repository mode ──────────────────────────────────────────────────────
if [[ "${1:-}" != "--fixture-test" ]]; then
  echo "=== FR-150: release publish/ship surface ==="
  echo ""

  if check_publish_loop "$REPO_ROOT/.github/workflows/release.yml"; then
    pass "publish loop matches the publishable workspace set, in dependency order"
  else
    fail "publish loop disagrees with cargo metadata"
  fi

  if check_ship_surface "$REPO_ROOT/.github/workflows/release.yml" \
      "$REPO_ROOT/install.sh" "$REPO_ROOT/homebrew/orchestrator.rb"; then
    pass "release matrix, install.sh and the formula ship one target set"
  else
    fail "shipped-target surfaces disagree"
  fi

  if check_install_refusal "$REPO_ROOT/install.sh"; then
    pass "install.sh refuses an unsupported platform before touching the network"
  else
    fail "install.sh unsupported-platform refusal is broken"
  fi

  echo ""
  echo "$PASS passed, $FAIL failed"
  [[ "$FAIL" -eq 0 ]] || exit 1
  exit 0
fi

# ── Fixture mode ──────────────────────────────────────────────────────────────
#
# Each fixture mutates a private copy of exactly one governed file, proves the
# mutation landed (gate_fixture contracts), asserts the targeted check rejects
# it with a diagnostic that names the injected object, and asserts the other
# checks still pass against the unmodified base copies — manual isolation,
# because check 3's behavior depends on install.sh's own support list and a
# generic all-checks sweep would couple it to fixture 2's mutation.
echo "=== FR-150: release publish/ship surface (negative fixtures) ==="
echo ""

BASE="$WORK/base"
mkdir -p "$BASE"
cp "$REPO_ROOT/.github/workflows/release.yml" "$BASE/release.yml"
cp "$REPO_ROOT/install.sh" "$BASE/install.sh"
cp "$REPO_ROOT/homebrew/orchestrator.rb" "$BASE/orchestrator.rb"

# Positive control: the unmodified copies must pass, or every "rejected the
# defect" below is the fixture failing for its own reason.
control_rc=0
check_publish_loop "$BASE/release.yml" >/dev/null 2>&1 || control_rc=1
check_ship_surface "$BASE/release.yml" "$BASE/install.sh" "$BASE/orchestrator.rb" >/dev/null 2>&1 || control_rc=1
check_install_refusal "$BASE/install.sh" >/dev/null 2>&1 || control_rc=1
if [[ "$control_rc" -eq 0 ]]; then
  pass "positive control: unmodified copies pass all three checks"
else
  fail "positive control: unmodified copies do not pass; fixtures below are void"
fi

# Fixture 1: comment out (not delete — the mutation the implementation is least
# likely to catch) orchestrator-persistence in the publish loop.
F1="$WORK/f1"; mkdir -p "$F1"; cp "$BASE/release.yml" "$F1/release.yml"
if fixture_mutate "fixture 1" "$F1/release.yml" \
    sh -c 'sed "s|^\( *\)crates/orchestrator-persistence |\1# crates/orchestrator-persistence |" "$1" > "$1.tmp" && mv "$1.tmp" "$1"' _ "$F1/release.yml"; then
  f1_out="$WORK/f1.log"
  if check_publish_loop "$F1/release.yml" > "$f1_out" 2>&1; then
    fail "fixture 1: a commented-out publishable crate was accepted"
  elif ! grep -q "crates/orchestrator-persistence" "$f1_out"; then
    fail "fixture 1: rejected, but the diagnostic does not name the missing crate"
  elif ! check_ship_surface "$F1/release.yml" "$BASE/install.sh" "$BASE/orchestrator.rb" >/dev/null 2>&1; then
    fail "fixture 1: defect also tripped the ship-surface check; not isolated"
  else
    pass "fixture 1: commented-out crate rejected, diagnostic names it, isolated"
  fi
fi

# Fixture 2: add a triple to install.sh that release.yml does not build — the
# exact historical defect, injected in the opposite direction.
F2="$WORK/f2"; mkdir -p "$F2"; cp "$BASE/install.sh" "$F2/install.sh"
if fixture_mutate "fixture 2" "$F2/install.sh" \
    sh -c 'sed "s|^SUPPORTED_TARGETS=\"\(.*\)\"$|SUPPORTED_TARGETS=\"\1 x86_64-apple-darwin\"|" "$1" > "$1.tmp" && mv "$1.tmp" "$1"' _ "$F2/install.sh"; then
  f2_out="$WORK/f2.log"
  if check_ship_surface "$BASE/release.yml" "$F2/install.sh" "$BASE/orchestrator.rb" > "$f2_out" 2>&1; then
    fail "fixture 2: install.sh advertising an unbuilt triple was accepted"
  elif ! grep -q "x86_64-apple-darwin" "$f2_out"; then
    fail "fixture 2: rejected, but the diagnostic does not name the extra triple"
  elif ! check_publish_loop "$BASE/release.yml" >/dev/null 2>&1; then
    fail "fixture 2: defect also tripped the publish-loop check; not isolated"
  else
    pass "fixture 2: unbuilt triple rejected, diagnostic names it, isolated"
  fi
fi

# Fixture 3: comment out one url stanza in the formula, so the formula ships
# fewer targets than the matrix — guards the url-line extraction from becoming
# a vacuous grep (§4.4 shape 3).
F3="$WORK/f3"; mkdir -p "$F3"; cp "$BASE/orchestrator.rb" "$F3/orchestrator.rb"
if fixture_mutate "fixture 3" "$F3/orchestrator.rb" \
    sh -c 'sed "s|^\( *\)url \".*aarch64-unknown-linux-gnu.tar.gz\"$|\1# &|" "$1" > "$1.tmp" && mv "$1.tmp" "$1"' _ "$F3/orchestrator.rb"; then
  f3_out="$WORK/f3.log"
  if check_ship_surface "$BASE/release.yml" "$BASE/install.sh" "$F3/orchestrator.rb" > "$f3_out" 2>&1; then
    fail "fixture 3: a formula missing a shipped target was accepted"
  elif ! grep -q "aarch64-unknown-linux-gnu" "$f3_out"; then
    fail "fixture 3: rejected, but the diagnostic does not name the dropped triple"
  else
    pass "fixture 3: formula/matrix divergence rejected, diagnostic names the triple"
  fi
fi

echo ""
echo "$PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]] || exit 1
exit 0
