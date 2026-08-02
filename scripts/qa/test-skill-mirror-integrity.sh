#!/usr/bin/env bash
#
# FR-129: Skill single source and mirror integrity.
#
# `.claude/skills` is the authoritative skill source. Other runtimes see the same
# skills through symlink mirrors declared in config/governance/skill-mirrors.json.
# This script proves the mirrors are complete, correctly shaped, actually readable,
# documented in the generated registry, and free of unscoped filesystem claims.
#
# The defect that motivated it: `.agents/skills/fr-governance/SKILL.md` was a symlink
# to a *directory*. Every entry existed, every symlink resolved, and the FR governance
# skill was unusable in that mirror (EISDIR) for its entire lifetime, because nothing
# ever opened it. check_skill_md_readable opens it.
#
# Usage:
#   test-skill-mirror-integrity.sh                 verify the real repository
#   test-skill-mirror-integrity.sh --fixture-test  prove each check fails on an injected defect

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
POLICY_REL="config/governance/skill-mirrors.json"
PATH_SCOPE_REL="config/governance/skill-path-scopes.json"
SKILL_HELPER_REL="scripts/lib/skill_docs.rb"

for required in jq git ruby; do
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

# ── Policy accessors, all reading from $root so fixtures run on a copy ──────────

policy_source()  { jq -r '.source'            "$1/$POLICY_REL"; }
policy_roots()   { jq -r '.mirrorRoots[]'     "$1/$POLICY_REL"; }
policy_notskill(){ jq -r '.notSkills[].path'  "$1/$POLICY_REL"; }

# Skills the policy allows to be absent from a specific mirror root.
policy_exempt_for_root() {
  jq -r --arg root "$2" '.exemptions[] | select(.mirrorRoot == $root) | .skill' "$1/$POLICY_REL"
}

# Directory entries under the source tree, one per line.
source_entries() {
  local root="$1" src
  src="$(policy_source "$root")"
  (cd "$root/$src" && find . -mindepth 1 -maxdepth 1 -type d | sed 's|^\./||' | LC_ALL=C sort)
}

# The set a mirror must carry: source entries that declare a SKILL.md and are not
# marked notSkills. Existence, not regular-file-ness — a SKILL.md that has degraded
# into a directory is still a skill, and is check_skill_md_readable's business, not
# this set's. Keeping the two apart is what lets each negative fixture isolate.
source_skills() {
  local root="$1" src entry
  src="$(policy_source "$root")"
  local not_skills
  not_skills="$(policy_notskill "$root")"
  while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue
    grep -qxF "$entry" <<< "$not_skills" && continue
    [[ -e "$root/$src/$entry/SKILL.md" ]] || continue
    printf '%s\n' "$entry"
  done <<< "$(source_entries "$root")"
}

# Entries present under a mirror root, one per line. Symlinks included, which plain
# `find -type d` would miss.
mirror_entries() {
  local root="$1" mirror="$2"
  [[ -d "$root/$mirror" ]] || return 0
  (cd "$root/$mirror" && find . -mindepth 1 -maxdepth 1 | sed 's|^\./||' | LC_ALL=C sort)
}

# ── Integrity checks ───────────────────────────────────────────────────────────

# Check 1: every source entry is either a skill (declares SKILL.md) or is declared
# notSkills. A helper directory that quietly appears next to real skills, or a skill
# that loses its SKILL.md, must be a decision rather than an omission.
check_source_inventory() {
  local root="$1" src entry not_skills rc=0
  src="$(policy_source "$root")"
  not_skills="$(policy_notskill "$root")"

  while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue
    grep -qxF "$entry" <<< "$not_skills" && continue
    if [[ ! -e "$root/$src/$entry/SKILL.md" ]]; then
      echo "    $src/$entry has no SKILL.md and is not declared in notSkills" >&2
      rc=1
    fi
  done <<< "$(source_entries "$root")"
  return $rc
}

# Check 2: bidirectional coverage per mirror root. Every skill is mirrored unless
# exempted for that root, and no mirror entry survives its source skill.
check_mirror_coverage() {
  local root="$1" mirror skills present exempt missing stale rc=0
  skills="$(source_skills "$root")"

  while IFS= read -r mirror; do
    [[ -z "$mirror" ]] && continue
    present="$(mirror_entries "$root" "$mirror")"
    exempt="$(policy_exempt_for_root "$root" "$mirror" | LC_ALL=C sort)"

    missing="$(comm -23 <(printf '%s\n' "$skills") <(printf '%s\n' "$present") \
             | comm -23 - <(printf '%s\n' "$exempt"))"
    if [[ -n "$missing" ]]; then
      echo "    $mirror is missing skill(s) with no exemption:" >&2
      printf '      %s\n' $missing >&2
      rc=1
    fi

    stale="$(comm -13 <(printf '%s\n' "$skills") <(printf '%s\n' "$present"))"
    if [[ -n "$stale" ]]; then
      echo "    $mirror has entries with no corresponding skill:" >&2
      printf '      %s\n' $stale >&2
      rc=1
    fi
  done <<< "$(policy_roots "$root")"
  return $rc
}

# Check 3: canonical shape. Every mirror entry is a symlink — not a directory, not a
# regular file, not a copy — pointing at the source skill of the same name, and that
# target exists.
check_mirror_shape() {
  local root="$1" mirror entry src want got rc=0
  src="$(policy_source "$root")"

  while IFS= read -r mirror; do
    [[ -z "$mirror" ]] && continue
    while IFS= read -r entry; do
      [[ -z "$entry" ]] && continue
      local path="$root/$mirror/$entry"

      if [[ ! -L "$path" ]]; then
        echo "    $mirror/$entry is not a symlink (a copy or directory, not a mirror)" >&2
        rc=1
        continue
      fi

      # Depth from the mirror root back to the repository root, so the expected
      # relative target is derived rather than hardcoded to two levels.
      local depth up=""
      depth="$(printf '%s' "$mirror" | tr -cd '/' | wc -c | tr -d ' ')"
      local i
      for ((i = 0; i <= depth; i++)); do up="../$up"; done
      want="${up}${src}/${entry}"

      got="$(readlink "$path")"
      if [[ "$got" != "$want" ]]; then
        echo "    $mirror/$entry points at '$got', expected '$want'" >&2
        rc=1
        continue
      fi

      if [[ ! -e "$path" ]]; then
        echo "    $mirror/$entry is a dangling symlink (target does not exist)" >&2
        rc=1
      fi
    done <<< "$(mirror_entries "$root" "$mirror")"
  done <<< "$(policy_roots "$root")"
  return $rc
}

# Check 4: the behavioral one. Open <mirror>/<name>/SKILL.md the way a consuming
# runtime does and require a non-empty regular file. This is the only check that
# would have caught the production fr-governance defect on its own; the shape checks
# above all passed against it for years.
check_skill_md_readable() {
  local root="$1" mirror entry rc=0

  while IFS= read -r mirror; do
    [[ -z "$mirror" ]] && continue
    while IFS= read -r entry; do
      [[ -z "$entry" ]] && continue
      local skill_md="$root/$mirror/$entry/SKILL.md"

      if [[ -d "$skill_md" ]]; then
        echo "    $mirror/$entry/SKILL.md resolves to a directory; any runtime opening it gets EISDIR" >&2
        rc=1
        continue
      fi
      if [[ ! -f "$skill_md" ]]; then
        echo "    $mirror/$entry/SKILL.md is not a readable regular file" >&2
        rc=1
        continue
      fi
      if [[ ! -s "$skill_md" ]]; then
        echo "    $mirror/$entry/SKILL.md is empty" >&2
        rc=1
      fi
    done <<< "$(mirror_entries "$root" "$mirror")"
  done <<< "$(policy_roots "$root")"
  return $rc
}

# Check 5: the policy file may not accumulate claims about things that no longer
# exist. A stale exemption is an exemption nobody has to justify again.
check_no_stale_claims() {
  local root="$1" src entry roots rc=0
  src="$(policy_source "$root")"
  roots="$(policy_roots "$root")"

  while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue
    if [[ ! -d "$root/$src/$entry" ]]; then
      echo "    notSkills names '$entry', which no longer exists under $src" >&2
      rc=1
    fi
  done <<< "$(policy_notskill "$root")"

  local pair skill mirror
  while IFS= read -r pair; do
    [[ -z "$pair" ]] && continue
    skill="${pair%%|*}"
    mirror="${pair##*|}"
    if [[ ! -d "$root/$src/$skill" ]]; then
      echo "    exemption names skill '$skill', which no longer exists under $src" >&2
      rc=1
    fi
    if ! grep -qxF "$mirror" <<< "$roots"; then
      echo "    exemption for '$skill' names mirrorRoot '$mirror', which is not declared" >&2
      rc=1
    fi
  done <<< "$(jq -r '.exemptions[] | "\(.skill)|\(.mirrorRoot)"' "$root/$POLICY_REL")"

  # An exemption without a written reason is an omission wearing a decision's clothes.
  local unreasoned
  unreasoned="$(jq -r '.exemptions[] | select((.reason // "") | length < 10) | .skill' "$root/$POLICY_REL")"
  if [[ -n "$unreasoned" ]]; then
    echo "    exemption(s) with no substantive reason:" >&2
    printf '      %s\n' $unreasoned >&2
    rc=1
  fi
  return $rc
}

# Check 6: single source. A skill's content may be tracked in exactly one place.
# Mirrors are symlinks, so git lists them as the link path (`.agents/skills/ops`) and
# never as `.agents/skills/ops/SKILL.md` — any tracked SKILL.md outside the source
# tree is therefore a real copy, which is how `skills/orchestrator-guide` drifted 32KB
# from the authority before FR-129 deleted it. Deleting it once does not keep it gone.
check_no_content_copies() {
  local root="$1" src path rc=0
  src="$(policy_source "$root")"

  while IFS= read -r path; do
    [[ -z "$path" ]] && continue
    if [[ "$path" != "$src/"* ]]; then
      echo "    $path is a tracked SKILL.md outside $src; mirrors must be symlinks, not copies" >&2
      rc=1
    fi
  done <<< "$(git -C "$root" ls-files '*SKILL.md')"
  return $rc
}

# Check 7: the set of mirror roots is discovered, not declared.
#
# The other six checks all operate on `mirrorRoots`, so they verify the roots
# somebody remembered to list and are silent about the rest. FR-134 built a
# `.windsurf/skills/` containing one correct symlink and one misnamed symlink
# pointing at the wrong skill, and all six passed.
#
# This is the failure mode that created FR-129 in the first place: its own text
# missed `.cursor/skills`, which held 16 of 29 skills and had never been checked.
# The repository's history says roots come and go — SKILLS.md once declared a
# `.gemini/skills/` that did not exist on disk — so the covered set has to come
# from the repository and the declaration has to be the exemption, not the scope.
#
# Tracked symlinks are read from the index (mode 120000) with their targets, so
# this costs one git call and no filesystem walk. The target is matched as text:
# a symlink that points into the source is mirror-shaped wherever it happens to
# live, including a broken one inside the source directory itself.
check_mirror_roots_discovered() {
  local root="$1"
  local policy="$root/$POLICY_REL" rc=0 source_dir link target root_dir
  source_dir="$(jq -r '.source' "$policy")"

  while IFS= read -r link; do
    [[ -z "$link" ]] && continue
    target="$(git -C "$root" cat-file blob ":$link" 2>/dev/null || true)"
    [[ "$target" == *"$source_dir/"* ]] || continue
    root_dir="$(dirname "$link")"
    # A symlink directly inside the source is not a mirror of it.
    [[ "$root_dir" == "$source_dir" ]] && continue
    if ! jq -e --arg r "$root_dir" '.mirrorRoots | index($r)' "$policy" >/dev/null; then
      echo "    $link -> $target" >&2
      echo "      points into $source_dir from '$root_dir', which is not a declared mirrorRoot" >&2
      echo "      declare it so coverage, shape and readability apply to it, or delete it" >&2
      rc=1
    fi
  done < <(git -C "$root" ls-files -s | awk '$1 == "120000" { $1=$2=$3=""; sub(/^[[:space:]]+/, ""); print }')
  return $rc
}

# Checks 8 and 9 depend on readable authoritative source files. When a negative
# fixture deliberately turns SKILL.md into a directory, the dedicated readability
# check owns that defect and these derived checks defer so fixture isolation stays
# meaningful.
source_skill_docs_readable() {
  local root="$1" src skill
  src="$(policy_source "$root")"
  while IFS= read -r skill; do
    [[ -z "$skill" ]] && continue
    [[ -f "$root/$src/$skill/SKILL.md" ]] || return 1
  done <<< "$(source_skills "$root")"
}

# Check 8: paths taught by SKILL.md must resolve in the repository or skill,
# or have one exact, live template/output/companion declaration. The Ruby helper
# parses inline code and shell fences and checks declarations in both directions.
check_skill_path_scopes() {
  local root="$1"
  source_skill_docs_readable "$root" || return 0
  ruby "$root/$SKILL_HELPER_REL" check-paths "$root"
}

# Check 9: SKILLS.md is generated from the authoritative frontmatter set, so a
# new, renamed, or removed skill changes the required registry automatically.
check_skills_registry() {
  local root="$1"
  source_skill_docs_readable "$root" || return 0
  ruby "$root/$SKILL_HELPER_REL" check-registry "$root"
}

ALL_CHECKS=(check_source_inventory check_mirror_coverage check_mirror_shape
            check_skill_md_readable check_no_stale_claims check_no_content_copies
            check_mirror_roots_discovered check_skill_path_scopes
            check_skills_registry)

run_all_checks() {
  local root="$1" check rc=0
  for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
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
  echo "=== FR-129: skill mirror integrity (negative fixtures) ==="
  echo ""

  FIXTURE_ROOT="$(mktemp -d)"
  cleanup_fixtures() { chmod -R u+w "$FIXTURE_ROOT" 2>/dev/null || true; rm -rf "$FIXTURE_ROOT"; }
  trap cleanup_fixtures EXIT

  # Copy tracked inputs from the index so path targets and the generated registry
  # are present, while ignored build output never enters the fixture. tar preserves
  # mirror symlinks; fixtures cannot reach the working tree.
  BASE="$FIXTURE_ROOT/base"
  mkdir -p "$BASE"
  (cd "$REPO_ROOT" && git ls-files | tar cf - -T -) | (cd "$BASE" && tar xf -)

  # check_no_content_copies asks git which SKILL.md files are tracked, so the fixture
  # tree has to be a repository. Throwaway, under $TMPDIR, never pushed or committed —
  # the index alone answers `ls-files`.
  git -C "$BASE" init -q
  git -C "$BASE" add -A

  new_case() {
    local dir="$FIXTURE_ROOT/$1"
    mkdir -p "$dir"
    (cd "$BASE" && tar cf - .) | (cd "$dir" && tar xf -)
    echo "$dir"
  }

  # A fixture must fail the checks it targets and leave every other check passing,
  # so that it demonstrates the named check rather than tripping an earlier one.
  # Two fixtures legitimately target a pair of checks: a mirror that is a bare
  # directory and a mirror that dangles are both malformed *and* unreadable. Their
  # expectation lists say so rather than papering over it.
  # Every check a fixture has targeted, so the run can prove none went unexercised.
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
    for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
      grep -qxF "$check" <<< "$(printf '%s\n' $targets)" && continue
      if ! "$check" "$dir" >/dev/null 2>&1; then
        fail "$name: defect also tripped $check, so it does not isolate [$targets]"
        return
      fi
    done
    pass "$name: $why (isolated to [$targets])"
  }

  # Positive control: the copy must be clean before any defect means anything.
  if run_all_checks "$BASE" > "$FIXTURE_ROOT/base.log" 2>&1; then
    pass "positive control: unmodified repository passes all registered checks"
  else
    fail "positive control: unmodified repository does not pass"
    cat "$FIXTURE_ROOT/base.log" >&2
  fi

  # Also prove the copy preserved symlinks. Without this, every fixture below could
  # pass against a tree of directories that no longer models the repository.
  d="$BASE"
  if [[ -L "$d/.agents/skills/fr-governance" && -L "$d/.cursor/skills/qa-doc-gen" ]]; then
    pass "positive control: fixture copy preserved mirror symlinks"
  else
    fail "positive control: fixture copy flattened symlinks into directories"
  fi

  # 1. A new skill that nobody mirrored and nobody exempted.
  d="$(new_case f1)"
  mkdir -p "$d/.claude/skills/tmp-unmirrored"
  printf -- '---\nname: tmp-unmirrored\ndescription: Synthetic coverage fixture.\n---\nbody\n' > "$d/.claude/skills/tmp-unmirrored/SKILL.md"
  expect_fail "fixture 1" "$d" "check_mirror_coverage check_skills_registry" \
    "a new skill mirrored nowhere and absent from the generated registry fails both derived inventories"

  # 2. A mirror that became a real directory holding a real copy — the drift the
  #    single-source rule exists to prevent. Readable, and still wrong.
  d="$(new_case f2)"
  rm "$d/.agents/skills/qa-doc-gen"
  mkdir -p "$d/.agents/skills/qa-doc-gen"
  cp "$d/.claude/skills/qa-doc-gen/SKILL.md" "$d/.agents/skills/qa-doc-gen/SKILL.md"
  expect_fail "fixture 2" "$d" "check_mirror_shape" \
    "a mirror replaced by a real directory with a real copy fails shape while still reading fine"

  # 3. A mirror pointing at nothing.
  d="$(new_case f3)"
  rm "$d/.cursor/skills/ops"
  ln -s "../../.claude/skills/ops-renamed" "$d/.cursor/skills/ops"
  expect_fail "fixture 3" "$d" "check_mirror_shape check_skill_md_readable" \
    "a dangling mirror symlink fails both shape and the read"

  # 4a. The read check standing alone: shape is perfect, the source SKILL.md is a
  #     directory. Only opening the file distinguishes this from a healthy mirror.
  d="$(new_case f4a)"
  rm "$d/.claude/skills/ticket-fix/SKILL.md"
  mkdir -p "$d/.claude/skills/ticket-fix/SKILL.md"
  expect_fail "fixture 4a" "$d" "check_skill_md_readable" \
    "a SKILL.md that is a directory passes every structural check and fails only the read"

  # 4b. The production defect verbatim: <name>/SKILL.md -> <directory>, exactly the
  #     shape found in .agents/skills/fr-governance. This is the regression fixture.
  d="$(new_case f4b)"
  rm "$d/.agents/skills/fr-governance"
  mkdir -p "$d/.agents/skills/fr-governance"
  ln -s "../../../.claude/skills/fr-governance" "$d/.agents/skills/fr-governance/SKILL.md"
  expect_fail "fixture 4b" "$d" "check_mirror_shape check_skill_md_readable" \
    "the FR-129 production defect (<name>/SKILL.md -> directory) is caught, and the read catches it independently"

  # 5. An exemption for a skill that does not exist.
  d="$(new_case f5)"
  jq '.exemptions += [{"skill":"no-such-skill","mirrorRoot":".agents/skills","reason":"a reason long enough to look deliberate"}]' \
    "$BASE/$POLICY_REL" > "$d/$POLICY_REL"
  expect_fail "fixture 5" "$d" "check_no_stale_claims" \
    "an exemption naming a skill that no longer exists fails the stale-claim check"

  # 6. A directory beside the skills that is neither a skill nor declared as one.
  d="$(new_case f6)"
  mkdir -p "$d/.claude/skills/tmp-helper"
  printf 'echo hi\n' > "$d/.claude/skills/tmp-helper/helper.sh"
  expect_fail "fixture 6" "$d" "check_source_inventory" \
    "a directory with no SKILL.md and no notSkills entry fails the inventory check"

  # 7. An exemption that exists but explains nothing.
  d="$(new_case f7)"
  jq '.exemptions += [{"skill":"playwright-cli","mirrorRoot":".cursor/skills","reason":"n/a"}]' \
    "$BASE/$POLICY_REL" > "$d/$POLICY_REL"
  expect_fail "fixture 7" "$d" "check_no_stale_claims" \
    "an exemption without a substantive reason is rejected"

  # 8. The deleted third copy, restored. FR-129 removed skills/orchestrator-guide,
  #    but a deletion is not a rule — nothing stops the copy from reappearing and
  #    drifting again, which is exactly what it had already done.
  d="$(new_case f8)"
  mkdir -p "$d/skills/orchestrator-guide"
  cp "$d/.claude/skills/orchestrator-guide/SKILL.md" "$d/skills/orchestrator-guide/SKILL.md"
  git -C "$d" add -A
  expect_fail "fixture 8" "$d" "check_no_content_copies" \
    "a tracked SKILL.md outside the source tree is a content copy, not a mirror"

  # 9. An undeclared mirror root containing nothing but symlinks.
  #
  #    The reproduction from FR-134: a `.windsurf/skills/` holding one correct
  #    symlink and one misnamed symlink pointing at the wrong skill. All six
  #    original checks passed on it, because all six read mirrorRoots.
  #
  #    Symlinks specifically, not copies. check_no_content_copies already sees an
  #    undeclared root that holds real files, so the copy case was never the gap;
  #    the symlink case was, and symlinks are the shape this repository documents.
  d="$(new_case f9)"
  mkdir -p "$d/.windsurf/skills"
  ln -s ../../.claude/skills/fr-governance "$d/.windsurf/skills/fr-governance"
  ln -s ../../.claude/skills/qa-doc-gen "$d/.windsurf/skills/BROKEN-wrong-target"
  git -C "$d" add -A >/dev/null 2>&1
  expect_fail "fixture 9" "$d" "check_mirror_roots_discovered" \
    "a root full of symlinks into the source is discovered even though nothing declared it"

  # 9b. The other half, and the one that decides whether this is worth having:
  #     declaring the root must subject it to the existing checks, not excuse it
  #     from them. If declaring were the escape hatch, fixture 9 would just be
  #     teaching people to write one line of JSON.
  d="$(new_case f9b)"
  mkdir -p "$d/.windsurf/skills"
  ln -s ../../.claude/skills/qa-doc-gen "$d/.windsurf/skills/BROKEN-wrong-target"
  jq '.mirrorRoots += [".windsurf/skills"]' "$BASE/$POLICY_REL" > "$d/$POLICY_REL"
  git -C "$d" add -A >/dev/null 2>&1
  if check_mirror_roots_discovered "$d" >/dev/null 2>&1; then
    if check_mirror_coverage "$d" >/dev/null 2>&1 && check_mirror_shape "$d" >/dev/null 2>&1; then
      fail "fixture 9b: declaring a root excused it from coverage and shape"
    else
      pass "fixture 9b: declaring a root subjects it to the other checks rather than exempting it"
    fi
  else
    fail "fixture 9b: the root was declared but discovery still rejected it"
  fi

  # 10. A path-like inline code span that resolves nowhere must not survive.
  d="$(new_case f10)"
  printf '\nSynthetic missing input: `docs/no-such-skill-input.md`.\n' >> "$d/.claude/skills/ops/SKILL.md"
  expect_fail "fixture 10" "$d" "check_skill_path_scopes" \
    "a parsed repository path with no target and no exact scope declaration is rejected"

  # 11. Declarations are checked in reverse so an exception cannot outlive the
  # reference that justified it.
  d="$(new_case f11)"
  jq '.declarations += [{"skill":"ops","path":"docs/never-referenced.md","scope":"repo","target":"docs","reason":"synthetic stale declaration with an existing target"}]' \
    "$BASE/$PATH_SCOPE_REL" > "$d/$PATH_SCOPE_REL"
  expect_fail "fixture 11" "$d" "check_skill_path_scopes" \
    "an exact declaration with no parsed reference is stale"

  # 12. A subtree wildcard is not an exact decision, even if its target exists.
  d="$(new_case f12)"
  jq '.declarations += [{"skill":"ops","path":"docs/*","scope":"output","target":"docs","reason":"synthetic blanket declaration that must be rejected"}]' \
    "$BASE/$PATH_SCOPE_REL" > "$d/$PATH_SCOPE_REL"
  expect_fail "fixture 12" "$d" "check_skill_path_scopes" \
    "a wildcard declaration cannot authorize a subtree"

  # 13. Manual registry prose cannot drift from authoritative frontmatter.
  d="$(new_case f13)"
  printf '\nmanual drift\n' >> "$d/SKILLS.md"
  expect_fail "fixture 13" "$d" "check_skills_registry" \
    "a manual SKILLS.md edit fails exact generated comparison"

  # FR-134's lesson applied to this script itself: a check that is deleted from
  # ALL_CHECKS stops running in verification mode, and nothing above would notice —
  # the fixtures call their targets by name. So assert the two directions that make
  # ALL_CHECKS load-bearing: it names every check function this file defines, and
  # every check it names is proven by at least one fixture.
  defined="$(grep -oE '^check_[a-z_]+\(\)' "${BASH_SOURCE[0]}" | sed 's/()//' | LC_ALL=C sort)"
  registered="$(printf '%s\n' ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"} | LC_ALL=C sort)"
  if [[ "$defined" == "$registered" ]]; then
    pass "meta: ALL_CHECKS registers every check function defined in this script"
  else
    fail "meta: ALL_CHECKS drifted from the defined check functions"
    diff <(printf '%s\n' "$defined") <(printf '%s\n' "$registered") >&2 || true
  fi

  untargeted=""
  for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
    grep -qxF "$check" <<< "$(printf '%s\n' $TARGETED)" || untargeted="$untargeted $check"
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

echo "=== FR-129: skill mirror integrity ==="
echo ""
echo "source:      $(policy_source "$REPO_ROOT")"
echo "mirrorRoots: $(policy_roots "$REPO_ROOT" | tr '\n' ' ')"
echo "skills:      $(source_skills "$REPO_ROOT" | wc -l | tr -d ' ')"
echo ""

run_all_checks "$REPO_ROOT" || true

echo ""
echo "=== skill mirror integrity: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]] || exit 1
