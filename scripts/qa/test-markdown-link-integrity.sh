#!/usr/bin/env bash
#
# FR-131: Markdown relative-link integrity.
#
# 603 tracked markdown files, 87k lines, and until now no link checker at all.
#
# Finding the broken links is the easy half — there was exactly one. The hard half
# is not reporting the ones that are fine, and FR-131's own list of "three broken
# links" is what a naive checker produces:
#
#   core/README.md               [runner.rs](core/src/runner.rs)      real, and doubly
#                                                                     wrong: repo-relative
#                                                                     inside a file-relative
#                                                                     context
#   docs/qa/.../125b-*.md        `[title](resource-model.md)`         a code span, in a
#                                                                     sentence telling a
#                                                                     tester to check links
#   playwright-cli/SKILL.md      inside a ``` fence                   sample CLI output
#
# and six more it never found, all valid: site showcase pages linking `](fr-watch)`
# with no extension, which VitePress resolves and a file-existence check does not.
#
# So every resolution rule below was earned by a false positive, and the positive
# fixtures — the ones asserting a link is *not* broken — are the load-bearing half of
# --fixture-test.
#
# Usage:
#   test-markdown-link-integrity.sh                 verify the real repository
#   test-markdown-link-integrity.sh --fixture-test  prove the checks fail on an injected defect

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
POLICY_REL="config/governance/markdown-links.json"

# shellcheck source=../lib/gate_jq.sh
. "$REPO_ROOT/scripts/lib/gate_jq.sh"

for required in jq git awk; do
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

# ── Extraction ─────────────────────────────────────────────────────────────────

# Emit "<line>\t<target>" for every inline markdown link in one file, after removing
# fenced code blocks and inline code spans. Text inside either is a sample, a shell
# transcript, or an instruction *about* links — never a link.
extract_links() {
  awk '
    function strip_code(s) {
      # Multi-backtick spans first, so ``[x](y)`` is removed whole rather than
      # having its delimiters eaten a pair at a time and its contents exposed.
      gsub(/``[^`]*``/, " ", s)
      gsub(/`[^`]*`/, " ", s)
      return s
    }
    {
      line = $0
      if (fence) {
        if (match(line, /^[ \t]*(```+|~~~+)[ \t]*$/) && \
            substr(line, RSTART, RLENGTH) ~ marker) { fence = 0 }
        next
      }
      if (match(line, /^[ \t]*(```+|~~~+)/)) {
        fence = 1
        marker = (line ~ /~~~/) ? "~~~" : "```"
        next
      }
      line = strip_code(line)
      while (match(line, /\]\([^)]*\)/)) {
        target = substr(line, RSTART + 2, RLENGTH - 3)
        line = substr(line, RSTART + RLENGTH)
        sub(/^[ \t]+/, "", target)
        sub(/[ \t].*$/, "", target)     # drop the optional "title"
        gsub(/^</, "", target)
        gsub(/>$/, "", target)
        if (target != "") printf "%d\t%s\n", NR, target
      }
    }
  ' "$1"
}

# Does one link target resolve from the directory holding the file that contains it?
# VitePress serves extensionless routes and directory indexes, so the repository's
# own links are written that way too.
target_resolves() {
  local dir="$1" target="$2" p base
  p="${target%%#*}"
  p="${p%%\?*}"
  [[ -z "$p" ]] && return 0                                   # pure anchor
  [[ "$p" =~ ^(https?|mailto|tel|data|ftp|file|ssh|git): ]] && return 0
  [[ "$p" == //* ]] && return 0                               # protocol-relative
  [[ "$p" == /* ]] && return 0                                # VitePress site route

  # -e covers files and directories, including through symlinks; the .md fallback
  # covers extensionless links, which VitePress resolves and GitHub does not. There is
  # deliberately no <dir>/README.md branch: a link to a directory already resolves on
  # the line above, so such a branch would be unreachable code masquerading as a rule.
  base="$dir/$p"
  [[ -e "$base" ]] && return 0
  [[ -e "$base.md" ]] && return 0
  return 1
}

exempt_targets_for() {
  jq -r --arg file "$2" '.exemptions[] | select(.file == $file) | .target' "$1/$POLICY_REL"
}

# ── The checks ─────────────────────────────────────────────────────────────────

# Check 1: every relative link target in every tracked markdown file resolves.
check_link_targets_resolve() {
  local root="$1" file line target exempt rc=0
  while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    [[ -f "$root/$file" ]] || continue
    exempt="$(exempt_targets_for "$root" "$file")"
    while IFS=$'\t' read -r line target; do
      [[ -z "$target" ]] && continue
      target_resolves "$(dirname "$root/$file")" "$target" && continue
      printf '%s\n' "$exempt" | grep -qxF "$target" && continue
      echo "    $file:$line links '$target', which does not resolve" >&2
      rc=1
    done < <(extract_links "$root/$file")
  done < <(git -C "$root" ls-files '*.md')
  return $rc
}

# Check 2: an exemption outlives the link it excuses. Left standing, it silently
# re-permits the same broken target when someone writes it again.
check_no_stale_exemptions() {
  local root="$1" file target reason rc=0 exemption_rows
  # allow-empty: no exemptions at all is the best state this list can be in, so
  # zero rows must not be a failure. It must also not be indistinguishable from
  # an unreadable policy, which is what the status check is for.
  exemption_rows="$(gate_jq_rows allow-empty "$root/$POLICY_REL" '.exemptions[] | "\(.file)\t\(.target)\t\(.reason // "")"')" || return 1
  while IFS=$'\t' read -r file target reason; do
    [[ -z "$file" ]] && continue
    if [[ ! -f "$root/$file" ]]; then
      echo "    exemption names '$file', which no longer exists" >&2
      rc=1
      continue
    fi
    if ! extract_links "$root/$file" | cut -f2 | grep -qxF "$target"; then
      echo "    exemption for '$file' names target '$target', which the file no longer links" >&2
      rc=1
    fi
    if target_resolves "$(dirname "$root/$file")" "$target"; then
      echo "    exemption for '$file' names target '$target', which resolves and needs no exemption" >&2
      rc=1
    fi
    if [[ "${#reason}" -lt 20 ]]; then
      echo "    exemption for '$file' → '$target' has no substantive reason" >&2
      rc=1
    fi
  done <<< "$exemption_rows"
  return $rc
}

ALL_CHECKS=(check_link_targets_resolve check_no_stale_exemptions)

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
  echo "=== FR-131: markdown link integrity (negative and positive fixtures) ==="
  echo ""

  FIXTURE_ROOT="$(mktemp -d)"
  trap 'rm -rf "$FIXTURE_ROOT"' EXIT

  # A small synthetic corpus rather than a copy of 603 files, so each case is legible
  # and the positive controls state exactly which link shape they defend.
  BASE="$FIXTURE_ROOT/base"
  mkdir -p "$BASE/config/governance" "$BASE/docs/sub" "$BASE/docs/dir" "$BASE/real"
  cp "$REPO_ROOT/$POLICY_REL" "$BASE/$POLICY_REL"

  printf '# real\n\n## Some Section\n' > "$BASE/docs/real.md"
  printf '# sibling\n' > "$BASE/docs/sibling.md"
  printf '# index\n' > "$BASE/docs/dir/README.md"
  printf '# deep\n' > "$BASE/real/deep.md"
  ln -s ../real "$BASE/docs/linked"

  # Every shape that must NOT be reported. Each line here is a false positive that a
  # naive checker produces, and seven of them are shapes this repository actually uses.
  cat > "$BASE/docs/positives.md" <<'MD'
# Positive controls

A VitePress site route: [route](/en/guide/quickstart)
A pure anchor: [anchor](#some-section)
An anchor on a real file: [real](real.md#some-section)
An extensionless sibling: [sibling](sibling)
A plain directory: [dir](dir)
Through a symlinked directory: [deep](linked/deep.md)
An external link: [ext](https://example.com/nope.md)
A link with a title: [real](real.md "the title")
A target in angle brackets: [real](<real.md>)
A target with a query string: [real](real.md?plain=1)
A code span that looks like a link: `[title](resource-model.md)`
A double-backtick span: ``[title](resource-model.md)``

```bash
> playwright-cli goto https://example.com
[Snapshot](.playwright-cli/page-2026-02-14T19-22-42-679Z.yml)
```

~~~
[also fenced](nope.md)
~~~
MD

  printf '# sub\n\nUp one level: [real](../real.md)\n' > "$BASE/docs/sub/child.md"

  git -C "$BASE" init -q
  git -C "$BASE" add -A

  new_case() {
    local dir="$FIXTURE_ROOT/$1"
    mkdir -p "$dir"
    (cd "$BASE" && tar cf - .) | (cd "$dir" && tar xf -)
    echo "$dir"
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
    for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
      printf '%s\n' $targets | grep -qxF "$check" && continue
      if ! "$check" "$dir" >/dev/null 2>&1; then
        fail "$name: defect also tripped $check, so it does not isolate [$targets]"
        return
      fi
    done
    pass "$name: $why (isolated to [$targets])"
  }

  # Positive control. Its ten link shapes are the assertion, not scenery: a scanner
  # that reported them would make the gate unusable and be disabled within a week.
  if run_all_checks "$BASE" > "$FIXTURE_ROOT/base.log" 2>&1; then
    pass "positive control: site routes, anchors, extensionless targets, plain"
    pass "                  directories, symlinked directories, link titles, code"
    pass "                  spans, and fenced blocks are all accepted as written"
  else
    fail "positive control: a link that resolves was reported broken"
    cat "$FIXTURE_ROOT/base.log" >&2
  fi

  # 1. A link to a file that does not exist.
  d="$(new_case f1)"
  printf '\nA broken link: [gone](./nowhere.md)\n' >> "$d/docs/positives.md"
  expect_fail "fixture 1" "$d" "check_link_targets_resolve" \
    "a link to a missing file is reported"

  # 2. The same, hidden behind an anchor — the fragment must be split off before the
  #    path is resolved, or every anchored link resolves to nothing and is skipped.
  d="$(new_case f2)"
  printf '\nBroken with anchor: [gone](./nowhere.md#section)\n' >> "$d/docs/positives.md"
  expect_fail "fixture 2" "$d" "check_link_targets_resolve" \
    "a missing target behind an anchor is still reported"

  # 3. A repo-relative path written inside a file-relative context — core/README.md's
  #    actual defect, which resolved to core/core/src/runner.rs.
  d="$(new_case f3)"
  printf '\nWrong base: [child](docs/sub/child.md)\n' >> "$d/docs/positives.md"
  expect_fail "fixture 3" "$d" "check_link_targets_resolve" \
    "a repo-relative path used where a file-relative one belongs is reported"

  # 4. An exemption for a file that no longer exists.
  d="$(new_case f4)"
  jq '.exemptions += [{"file":"docs/deleted.md","target":"./x.md","reason":"a reason long enough to look deliberate"}]' \
    "$BASE/$POLICY_REL" > "$d/$POLICY_REL"
  expect_fail "fixture 4" "$d" "check_no_stale_exemptions" \
    "an exemption naming a file that no longer exists is rejected"

  # 5. An exemption for a link the file no longer contains.
  d="$(new_case f5)"
  jq '.exemptions += [{"file":"docs/positives.md","target":"./removed-long-ago.md","reason":"a reason long enough to look deliberate"}]' \
    "$BASE/$POLICY_REL" > "$d/$POLICY_REL"
  expect_fail "fixture 5" "$d" "check_no_stale_exemptions" \
    "an exemption for a link the file no longer contains is rejected"

  # 6. An exemption for a link that resolves. Nothing to excuse, and it would quietly
  #    permit the target if the file it points at were ever deleted.
  d="$(new_case f6)"
  jq '.exemptions += [{"file":"docs/positives.md","target":"real.md#some-section","reason":"a reason long enough to look deliberate"}]' \
    "$BASE/$POLICY_REL" > "$d/$POLICY_REL"
  expect_fail "fixture 6" "$d" "check_no_stale_exemptions" \
    "an exemption for a link that already resolves is rejected"

  # FR-134's lesson applied to this script itself.
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

FILES="$(git -C "$REPO_ROOT" ls-files '*.md' | wc -l | tr -d ' ')"
LINKS="$(git -C "$REPO_ROOT" ls-files '*.md' | while IFS= read -r f; do
  [[ -f "$REPO_ROOT/$f" ]] && extract_links "$REPO_ROOT/$f"
done | wc -l | tr -d ' ')"

echo "=== FR-131: markdown link integrity ==="
echo ""
echo "files:      $FILES tracked markdown files"
echo "links:      $LINKS inline link targets outside code spans and fenced blocks"
echo "exemptions: $(jq -r '.exemptions | length' "$REPO_ROOT/$POLICY_REL")"
echo ""

run_all_checks "$REPO_ROOT" || true

echo ""
echo "=== markdown link integrity: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]] || exit 1
