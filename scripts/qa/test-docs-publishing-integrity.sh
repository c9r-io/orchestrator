#!/usr/bin/env bash
#
# FR-131: Documentation publishing single source.
#
# config/governance/docs-publishing.json declares what gets published. This script
# proves the repository, the generator, and the site navigation still agree with it.
#
# The defect that motivated it: site/{en,zh}/guide was generated and gitignored while
# site/{en,zh}/showcases was 36 hand-maintained tracked files with no generator. One
# source — docs/showcases/streaming-mark-done-convergence.md — had no published page
# at all, and the EN/ZH CEL guides sent readers to it. No check existed on either
# side, so the repository was green while the site was wrong.
#
# The publish set is proven by *running* the sync into a temp directory and diffing
# trees, never by comparing filenames alone. The expected set is derived here from
# the policy independently of the generator, so a generator that silently stops
# emitting a source cannot satisfy this script by agreeing with itself.
#
# Usage:
#   test-docs-publishing-integrity.sh                 verify the real repository
#   test-docs-publishing-integrity.sh --fixture-test  prove each check fails on an injected defect

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
POLICY_REL="config/governance/docs-publishing.json"
SYNC_REL="scripts/sync-docs.mjs"

for required in jq git node; do
  command -v "$required" >/dev/null 2>&1 || {
    echo "missing required command: $required" >&2
    exit 1
  }
done

if [[ "${1:-}" != "" && "${1:-}" != "--fixture-test" ]]; then
  echo "usage: $0 [--fixture-test]" >&2
  exit 2
fi

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# ── Policy accessors, all reading from $root so fixtures run on a copy ──────────

policy()          { jq -r "$2" "$1/$POLICY_REL"; }
policy_site_root(){ policy "$1" '.siteRoot'; }
policy_nav()      { policy "$1" '.navConfig'; }
collection_names(){ policy "$1" '.collections[].name'; }

collection_field() {
  jq -r --arg name "$2" --arg field "$3" \
    '.collections[] | select(.name == $name) | .[$field] // "" | tostring' "$1/$POLICY_REL"
}
collection_langs() {
  jq -r --arg name "$2" '.collections[] | select(.name == $name) | .sources | keys[]' "$1/$POLICY_REL"
}
collection_source() {
  jq -r --arg name "$2" --arg lang "$3" \
    '.collections[] | select(.name == $name) | .sources[$lang]' "$1/$POLICY_REL"
}

site_dir() { echo "$(policy_site_root "$1")/$3/$2"; }

# Slugs authored directly in one locale's source directory. Subdirectories are other
# locales (docs/guide/zh), never part of this one.
authored_slugs() {
  local root="$1" name="$2" lang="$3" src strip
  src="$(collection_source "$root" "$name" "$lang")"
  strip="$(collection_field "$root" "$name" stripNumericPrefix)"
  [[ -d "$root/$src" ]] || return 0
  local f base
  while IFS= read -r f; do
    base="$(basename "$f")"
    [[ "$base" == "README.md" || "$base" == .* ]] && continue
    [[ "$strip" == "true" ]] && base="${base#[0-9][0-9]-}"
    printf '%s\n' "${base%.md}"
  done < <(find "$root/$src" -mindepth 1 -maxdepth 1 -type f -name '*.md') | LC_ALL=C sort
}

# Declared gaps for a collection, as "slug<TAB>absentSource".
declared_gaps() {
  jq -r --arg name "$2" \
    '.translationGaps[] | select(.collection == $name) | "\(.slug)\t\(.absentSource)"' \
    "$1/$POLICY_REL"
}

# What a locale must publish: everything authored in it, plus every declared gap whose
# absent locale is this one and whose slug is authored in some other locale. Derived
# from the policy, not from the generator — that independence is the point.
expected_slugs() {
  local root="$1" name="$2" lang="$3" line slug absent other
  authored_slugs "$root" "$name" "$lang"
  if [[ "$(collection_field "$root" "$name" fallback)" == "declared-gaps" ]]; then
    while IFS=$'\t' read -r slug absent; do
      [[ -z "$slug" || "$absent" != "$lang" ]] && continue
      while IFS= read -r other; do
        [[ -z "$other" || "$other" == "$lang" ]] && continue
        if authored_slugs "$root" "$name" "$other" | grep -qxF "$slug"; then
          printf '%s\n' "$slug"
          break
        fi
      done < <(collection_langs "$root" "$name")
    done < <(declared_gaps "$root" "$name")
  fi
}
expected_sorted() { expected_slugs "$@" | LC_ALL=C sort -u; }

produced_slugs() {
  local dest="$1" name="$2" lang="$3" root="$4" dir
  dir="$dest/$(site_dir "$root" "$name" "$lang")"
  [[ -d "$dir" ]] || return 0
  find "$dir" -mindepth 1 -maxdepth 1 -type f -name '*.md' -exec basename {} .md \; | LC_ALL=C sort
}

# Generate the whole site into a scratch directory. Never writes into $root.
sync_into() {
  local root="$1" dest="$2"
  mkdir -p "$dest"
  SYNC_DOCS_DEST="$dest" node "$root/$SYNC_REL" >/dev/null 2>&1
}

# Route links the VitePress config declares, as "/<lang>/<collection>/<slug>".
nav_routes() {
  local root="$1" nav
  nav="$(policy_nav "$root")"
  [[ -f "$root/$nav" ]] || return 0
  grep -oE 'link:[[:space:]]*"/[^"]*"' "$root/$nav" \
    | sed -E 's/.*"([^"]*)".*/\1/' \
    | grep -E '^/[a-z-]+/[a-z-]+/[^/]+$' \
    | LC_ALL=C sort -u
}

# Every route the publish set actually produces, in the same shape.
published_routes() {
  local root="$1" dest="$2" name lang slug
  while IFS= read -r name; do
    while IFS= read -r lang; do
      while IFS= read -r slug; do
        [[ -z "$slug" ]] && continue
        printf '/%s/%s/%s\n' "$lang" "$name" "$slug"
      done < <(produced_slugs "$dest" "$name" "$lang" "$root")
    done < <(collection_langs "$root" "$name")
  done < <(collection_names "$root")
  : # keep the pipeline's exit status out of the caller
}
published_sorted() { published_routes "$@" | LC_ALL=C sort -u; }

# ── The checks ─────────────────────────────────────────────────────────────────

# Check 1: the policy may not accumulate claims about things that no longer exist.
# A gap for a deleted showcase is a translation nobody owes, and a nav exemption for
# a deleted page is a reachability rule nobody has to justify again.
check_policy_fresh() {
  local root="$1" name lang src slug absent rc=0

  while IFS= read -r name; do
    while IFS= read -r lang; do
      src="$(collection_source "$root" "$name" "$lang")"
      if [[ ! -d "$root/$src" ]]; then
        echo "    collection '$name' declares source '$src' for '$lang', which does not exist" >&2
        rc=1
      fi
    done < <(collection_langs "$root" "$name")
    if [[ "$(collection_field "$root" "$name" requireBilingual)" != "true" ]] \
       && [[ "$(collection_field "$root" "$name" requireBilingualReason | wc -c | tr -d ' ')" -lt 20 ]]; then
      echo "    collection '$name' waives bilingual coverage without a substantive reason" >&2
      rc=1
    fi
  done < <(collection_names "$root")

  while IFS= read -r name; do
    while IFS=$'\t' read -r slug absent; do
      [[ -z "$slug" ]] && continue
      local found=0 lang2
      while IFS= read -r lang2; do
        authored_slugs "$root" "$name" "$lang2" | grep -qxF "$slug" && found=1
      done < <(collection_langs "$root" "$name")
      if [[ "$found" -eq 0 ]]; then
        echo "    translationGaps names '$name/$slug', which is authored in no locale" >&2
        rc=1
      fi
      if ! collection_langs "$root" "$name" | grep -qxF "$absent"; then
        echo "    translationGaps entry '$name/$slug' names absentSource '$absent', which is not a declared locale" >&2
        rc=1
      fi
    done < <(declared_gaps "$root" "$name")
  done < <(collection_names "$root")

  local unreasoned
  unreasoned="$(jq -r '.translationGaps[] | select((.reason // "") | length < 40) | "\(.collection)/\(.slug)"' "$root/$POLICY_REL")"
  if [[ -n "$unreasoned" ]]; then
    echo "    translation gap(s) with no substantive reason:" >&2
    printf '      %s\n' $unreasoned >&2
    rc=1
  fi

  # A nav exemption for a route the site no longer publishes is a reachability rule
  # nobody has to justify again; check_nav_complete would simply never consult it.
  local dest route
  dest="$WORK/policy-fresh.$$"
  rm -rf "$dest"
  if sync_into "$root" "$dest"; then
    local produced
    produced="$(published_sorted "$root" "$dest")"
    while IFS= read -r route; do
      [[ -z "$route" ]] && continue
      if ! printf '%s\n' "$produced" | grep -qxF "$route"; then
        echo "    navExemptions names '$route', which the publish set does not produce" >&2
        rc=1
      fi
    done < <(jq -r '.navExemptions[].route' "$root/$POLICY_REL")
  fi
  rm -rf "$dest"

  local nav
  nav="$(policy_nav "$root")"
  if [[ ! -f "$root/$nav" ]]; then
    echo "    navConfig names '$nav', which does not exist" >&2
    rc=1
  fi
  return $rc
}

# Check 2: a collection that requires bilingual coverage may not silently acquire a
# monolingual page, and no two sources may collapse onto the same published slug.
check_source_inventory() {
  local root="$1" name lang other slug rc=0

  while IFS= read -r name; do
    # Slug collision: 01-quickstart.md and quickstart.md both publish quickstart.md,
    # and whichever readdir returns last wins silently.
    while IFS= read -r lang; do
      local dupes src
      dupes="$(authored_slugs "$root" "$name" "$lang" | uniq -d)"
      if [[ -n "$dupes" ]]; then
        src="$(collection_source "$root" "$name" "$lang")"
        echo "    $src has sources that publish to the same slug:" >&2
        printf '      %s\n' $dupes >&2
        rc=1
      fi
    done < <(collection_langs "$root" "$name")

    [[ "$(collection_field "$root" "$name" requireBilingual)" == "true" ]] || continue

    while IFS= read -r lang; do
      while IFS= read -r slug; do
        [[ -z "$slug" ]] && continue
        local present_everywhere=1
        while IFS= read -r other; do
          [[ "$other" == "$lang" ]] && continue
          authored_slugs "$root" "$name" "$other" | grep -qxF "$slug" || present_everywhere=0
        done < <(collection_langs "$root" "$name")
        [[ "$present_everywhere" -eq 1 ]] && continue
        if ! declared_gaps "$root" "$name" | cut -f1 | grep -qxF "$slug"; then
          echo "    $name/$slug exists in '$lang' only and is not a declared translation gap" >&2
          rc=1
        fi
      done < <(authored_slugs "$root" "$name" "$lang")
    done < <(collection_langs "$root" "$name")
  done < <(collection_names "$root")
  return $rc
}

# Check 3: every published page is generated output. Guide pages have been gitignored
# for as long as the sync has existed; showcase pages were tracked, which is how they
# drifted from their sources without anything failing.
check_generated_not_tracked() {
  local root="$1" name lang dir tracked rc=0
  while IFS= read -r name; do
    while IFS= read -r lang; do
      dir="$(site_dir "$root" "$name" "$lang")"
      tracked="$(git -C "$root" ls-files "$dir")"
      if [[ -n "$tracked" ]]; then
        echo "    $dir has tracked files; published pages are generated by $SYNC_REL" >&2
        printf '      %s\n' $(printf '%s\n' "$tracked" | head -5) >&2
        rc=1
      fi
    done < <(collection_langs "$root" "$name")
  done < <(collection_names "$root")
  return $rc
}

# Check 4: run the generator and compare what it produced against what the policy
# says must exist, in both directions and per locale. A source the generator stops
# emitting fails here; so does a page it emits that nothing declares.
check_publish_bijection() {
  local root="$1" dest name lang want got missing extra rc=0
  dest="$WORK/bijection.$$"
  rm -rf "$dest"
  if ! sync_into "$root" "$dest"; then
    echo "    $SYNC_REL failed to run" >&2
    return 1
  fi

  while IFS= read -r name; do
    while IFS= read -r lang; do
      want="$(expected_sorted "$root" "$name" "$lang")"
      got="$(produced_slugs "$dest" "$name" "$lang" "$root")"
      missing="$(comm -23 <(printf '%s\n' "$want") <(printf '%s\n' "$got"))"
      extra="$(comm -13 <(printf '%s\n' "$want") <(printf '%s\n' "$got"))"
      if [[ -n "$missing" ]]; then
        echo "    $name/$lang: declared but not published:" >&2
        printf '      %s\n' $missing >&2
        rc=1
      fi
      if [[ -n "$extra" ]]; then
        echo "    $name/$lang: published but not declared:" >&2
        printf '      %s\n' $extra >&2
        rc=1
      fi
    done < <(collection_langs "$root" "$name")
  done < <(collection_names "$root")
  rm -rf "$dest"
  return $rc
}

# Check 5: the same sources must produce the same site twice. A generator that varies
# run to run makes every drift comparison meaningless, including this script's own.
check_sync_idempotent() {
  local root="$1" a b rc=0
  a="$WORK/idem-a.$$"
  b="$WORK/idem-b.$$"
  rm -rf "$a" "$b"
  sync_into "$root" "$a" || { echo "    first sync failed" >&2; return 1; }
  sync_into "$root" "$b" || { echo "    second sync failed" >&2; return 1; }
  if ! diff -r "$a" "$b" >/dev/null 2>&1; then
    echo "    two consecutive syncs of unchanged sources produced different output" >&2
    diff -rq "$a" "$b" 2>&1 | head -5 >&2
    rc=1
  fi
  rm -rf "$a" "$b"
  return $rc
}

# Check 6: every collection route the navigation declares resolves to a page the
# publish set actually produces. A sidebar entry pointing at nothing is a 404 that
# no repository-side check has ever been able to see.
check_nav_reachable() {
  local root="$1" dest route rc=0
  dest="$WORK/nav-fwd.$$"
  rm -rf "$dest"
  sync_into "$root" "$dest" || { echo "    $SYNC_REL failed to run" >&2; return 1; }

  local produced
  produced="$(published_sorted "$root" "$dest")"
  while IFS= read -r route; do
    [[ -z "$route" ]] && continue
    if ! printf '%s\n' "$produced" | grep -qxF "$route"; then
      echo "    $(policy_nav "$root") links '$route', which the publish set does not produce" >&2
      rc=1
    fi
  done < <(nav_routes "$root")
  rm -rf "$dest"
  return $rc
}

# Check 7: every produced page is reachable from the navigation. Publishing a page and
# linking it are independent acts here, and only the first was ever checked — which is
# how eight guide chapters came to exist on the site with nothing pointing at them.
check_nav_complete() {
  local root="$1" dest route rc=0
  dest="$WORK/nav-rev.$$"
  rm -rf "$dest"
  sync_into "$root" "$dest" || { echo "    $SYNC_REL failed to run" >&2; return 1; }

  local routes exempt
  routes="$(nav_routes "$root")"
  exempt="$(jq -r '.navExemptions[].route' "$root/$POLICY_REL")"
  while IFS= read -r route; do
    [[ -z "$route" ]] && continue
    printf '%s\n' "$exempt" | grep -qxF "$route" && continue
    if ! printf '%s\n' "$routes" | grep -qxF "$route"; then
      echo "    '$route' is published but nothing in $(policy_nav "$root") links it" >&2
      rc=1
    fi
  done < <(published_sorted "$root" "$dest")
  rm -rf "$dest"
  return $rc
}

ALL_CHECKS=(check_policy_fresh check_source_inventory check_generated_not_tracked
            check_publish_bijection check_sync_idempotent check_nav_reachable
            check_nav_complete)

run_all_checks() {
  local root="$1" check rc=0
  for check in "${ALL_CHECKS[@]}"; do
    if "$check" "$root"; then
      pass "$check"
    else
      fail "$check"
      rc=1
    fi
  done
  return $rc
}

# ── Fixture mode ───────────────────────────────────────────────────────────────

if [[ "${1:-}" == "--fixture-test" ]]; then
  echo "=== FR-131: docs publishing integrity (negative fixtures) ==="
  echo ""

  FIXTURE_ROOT="$(mktemp -d)"
  cleanup_fixtures() { chmod -R u+w "$FIXTURE_ROOT" 2>/dev/null || true; rm -rf "$FIXTURE_ROOT"; }
  trap 'cleanup_fixtures; rm -rf "$WORK"' EXIT

  BASE="$FIXTURE_ROOT/base"
  mkdir -p "$BASE"
  (cd "$REPO_ROOT" && tar cf - \
    "$POLICY_REL" \
    "$SYNC_REL" \
    docs/guide \
    docs/showcases \
    site/.vitepress/config.ts) | (cd "$BASE" && tar xf -)

  # check_generated_not_tracked asks git what is tracked, so the fixture tree has to be
  # a repository. Throwaway, under $TMPDIR, never pushed — the index alone answers.
  # The site output must stay ignored here exactly as it is in the repository, or the
  # check would pass for the wrong reason.
  printf 'site/en/\nsite/zh/\n' > "$BASE/.gitignore"
  git -C "$BASE" init -q
  git -C "$BASE" add -A

  new_case() {
    local dir="$FIXTURE_ROOT/$1"
    mkdir -p "$dir"
    (cd "$BASE" && tar cf - .) | (cd "$dir" && tar xf -)
    echo "$dir"
  }

  # Drop every navigation line that points at a route, so a fixture that changes the
  # publish set can keep the navigation consistent and isolate its own check.
  drop_nav_route() {
    local dir="$1" route="$2" nav
    nav="$(policy_nav "$dir")"
    grep -v "\"$route\"" "$dir/$nav" > "$dir/$nav.tmp"
    mv "$dir/$nav.tmp" "$dir/$nav"
  }

  TARGETED=""

  expect_fail() {
    local name="$1" dir="$2" targets="$3" why="$4" check
    TARGETED="$TARGETED $targets"
    for check in $targets; do
      if "$check" "$dir" >/dev/null 2>&1; then
        fail "$name: $check accepted the injected defect ($why)"
        return
      fi
    done
    for check in "${ALL_CHECKS[@]}"; do
      printf '%s\n' $targets | grep -qxF "$check" && continue
      if ! "$check" "$dir" >/dev/null 2>&1; then
        fail "$name: defect also tripped $check, so it does not isolate [$targets]"
        return
      fi
    done
    pass "$name: $why (isolated to [$targets])"
  }

  # Positive control: the copy must be clean before any defect means anything.
  if run_all_checks "$BASE" > "$FIXTURE_ROOT/base.log" 2>&1; then
    pass "positive control: unmodified repository passes all ${#ALL_CHECKS[@]} checks"
  else
    fail "positive control: unmodified repository does not pass"
    cat "$FIXTURE_ROOT/base.log" >&2
  fi

  # 1. A translation removed, navigation updated, policy not. The monolingual page is
  #    now undeclared: nobody owes the translation because nobody recorded the debt.
  d="$(new_case f1)"
  rm "$d/docs/showcases/zh/plan-execute.md"
  drop_nav_route "$d" "/zh/showcases/plan-execute"
  expect_fail "fixture 1" "$d" "check_source_inventory" \
    "a page that exists in one locale only, with no declared translation gap, fails the inventory"

  # 2. The generator silently stops emitting a source, and the navigation no longer
  #    links it either — so only the declared-versus-produced comparison can see it.
  d="$(new_case f2)"
  perl -0pi -e 's/\.filter\(\(f\) => f\.endsWith\("\.md"\)/.filter((f) => !f.includes("scheduled-scan")).filter((f) => f.endsWith(".md")/' \
    "$d/$SYNC_REL"
  drop_nav_route "$d" "/en/showcases/scheduled-scan"
  drop_nav_route "$d" "/zh/showcases/scheduled-scan"
  expect_fail "fixture 2" "$d" "check_publish_bijection" \
    "a source the generator quietly stops emitting fails the declared-versus-produced comparison"

  # 3. A generated page committed to version control — the shape the showcases were in
  #    for their entire life, and the reason they drifted from their sources.
  d="$(new_case f3)"
  mkdir -p "$d/site/en/showcases"
  printf '# hand-edited\n' > "$d/site/en/showcases/hello-world.md"
  git -C "$d" add -f site/en/showcases/hello-world.md
  expect_fail "fixture 3" "$d" "check_generated_not_tracked" \
    "a tracked file under a published site directory fails, because published pages are generated"

  # 4. A translation gap for a page that no longer exists in any locale.
  d="$(new_case f4)"
  jq '.translationGaps += [{"collection":"showcases","slug":"no-such-showcase","absentSource":"zh","owedTranslation":"zh","reason":"a reason long enough to look like a deliberate decision was made here"}]' \
    "$BASE/$POLICY_REL" > "$d/$POLICY_REL"
  expect_fail "fixture 4" "$d" "check_policy_fresh" \
    "a translation gap naming a page authored in no locale fails the freshness check"

  # 5. A translation gap that records the debt without explaining it.
  d="$(new_case f5)"
  jq '.translationGaps += [{"collection":"showcases","slug":"qa-loop","absentSource":"zh","owedTranslation":"zh","reason":"todo"}]' \
    "$BASE/$POLICY_REL" > "$d/$POLICY_REL"
  expect_fail "fixture 5" "$d" "check_policy_fresh" \
    "a translation gap without a substantive reason is rejected"

  # 6. A generator whose output varies between runs. Every drift comparison in this
  #    script, and every review of a site diff, silently stops meaning anything.
  d="$(new_case f6)"
  perl -0pi -e 's/^    transformed,$/    transformed + String(Math.random()),/m' "$d/$SYNC_REL"
  expect_fail "fixture 6" "$d" "check_sync_idempotent" \
    "a generator that produces different output from unchanged sources fails idempotency"

  # 7. A sidebar entry pointing at a page that is not published — the 404 the
  #    repository could never see, because nothing compared nav against output.
  d="$(new_case f7)"
  nav="$(policy_nav "$d")"
  perl -0pi -e 's|(\{ text: "Hello World", link: "/en/showcases/hello-world" \},)|$1\n                { text: "Ghost", link: "/en/showcases/ghost-page" },|' \
    "$d/$nav"
  expect_fail "fixture 7" "$d" "check_nav_reachable" \
    "a navigation link to a page the publish set does not produce fails"

  # 8. A published page nothing links to. It renders, it is indexed, and no reader
  #    can get to it — the state eight guide chapters were already in.
  d="$(new_case f8)"
  drop_nav_route "$d" "/en/showcases/streaming-mark-done-convergence"
  expect_fail "fixture 8" "$d" "check_nav_complete" \
    "a page that is published with nothing linking it fails the reachability check"

  # 9. Two sources collapsing onto one published slug. The loser is overwritten with
  #    no diagnostic, and which one loses depends on directory order.
  d="$(new_case f9)"
  cp "$d/docs/guide/01-quickstart.md" "$d/docs/guide/09-quickstart.md"
  cp "$d/docs/guide/zh/01-quickstart.md" "$d/docs/guide/zh/09-quickstart.md"
  expect_fail "fixture 9" "$d" "check_source_inventory" \
    "two sources that publish to the same slug fail the inventory"

  # FR-134's lesson applied to this script itself: a check dropped from ALL_CHECKS
  # stops running in verification mode, and nothing above would notice, because the
  # fixtures call their targets by name. Assert both directions.
  defined="$(grep -oE '^check_[a-z_]+\(\)' "${BASH_SOURCE[0]}" | sed 's/()//' | LC_ALL=C sort)"
  registered="$(printf '%s\n' "${ALL_CHECKS[@]}" | LC_ALL=C sort)"
  if [[ "$defined" == "$registered" ]]; then
    pass "meta: ALL_CHECKS registers every check function defined in this script"
  else
    fail "meta: ALL_CHECKS drifted from the defined check functions"
    diff <(printf '%s\n' "$defined") <(printf '%s\n' "$registered") >&2 || true
  fi

  untargeted=""
  for check in "${ALL_CHECKS[@]}"; do
    printf '%s\n' $TARGETED | grep -qxF "$check" || untargeted="$untargeted $check"
  done
  if [[ -z "$untargeted" ]]; then
    pass "meta: every registered check is proven by at least one negative fixture"
  else
    fail "meta: check(s) with no negative fixture:$untargeted"
  fi

  echo ""
  echo "=== fixtures: $PASS passed, $FAIL failed ==="
  [[ "$FAIL" -eq 0 ]] || exit 1
  exit 0
fi

# ── Verification mode ──────────────────────────────────────────────────────────

echo "=== FR-131: docs publishing integrity ==="
echo ""
echo "policy:      $POLICY_REL"
echo "collections: $(collection_names "$REPO_ROOT" | tr '\n' ' ')"
echo "gaps:        $(jq -r '.translationGaps | length' "$REPO_ROOT/$POLICY_REL") declared"
echo ""

run_all_checks "$REPO_ROOT" || true

echo ""
echo "=== docs publishing integrity: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]] || exit 1
