#!/usr/bin/env bash
# FR-155: keep high-authority onboarding and architecture claims tied to source.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [[ "${1:-}" != "" && "${1:-}" != "--fixture-test" ]]; then
  echo "usage: $0 [--fixture-test]" >&2
  exit 2
fi

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

migration_count() {
  ruby -e '
    source = File.read(File.join(ARGV[0], "crates/orchestrator-persistence/src/migration.rs"))
    body = source[/pub fn registered_migrations\(\).*?(?=\/\/\/ Converts migration definitions)/m]
    unless body
      warn "registered_migrations() body not found"
      exit 1
    end
    versions = body.scan(/\bversion:\s*(\d+)/).flatten.map(&:to_i)
    if versions.empty?
      warn "registered_migrations() contains no versions"
      exit 1
    end
    expected = (1..versions.length).to_a
    unless versions == expected
      warn "migration versions are not contiguous 1..#{versions.length}: #{versions.inspect}"
      exit 1
    end
    print versions.length
  ' "$1"
}

agents_missing_repo_paths() {
  ruby -e '
    root = File.expand_path(ARGV[0])
    text = File.read(File.join(root, "AGENTS.md"))
    inline = text.scan(/`([^`\n]+)`/).flatten
    links = text.scan(/\]\(([^)\s]+)\)/).flatten.map { |value| value.sub(/#.*\z/, "") }
    candidates = (inline + links).uniq.select do |value|
      next false if value.start_with?("~/", "/") || value.match?(/\A[a-z]+:\/\//i)
      value.match?(%r{\A(?:[A-Za-z0-9_.-]+/)+[A-Za-z0-9_.-]*/?\z})
    end
    missing = candidates.reject do |value|
      absolute = File.expand_path(value, root)
      inside = absolute == root || absolute.start_with?(root + File::SEPARATOR)
      inside && File.exist?(absolute)
    end
    puts missing.sort
    exit(missing.empty? ? 0 : 1)
  ' "$1"
}

check_onboarding_contract() {
  local root="$1" rc=0 missing_paths
  if rg -n 'root_path' "$root/AGENTS.md" >/dev/null; then
    echo "    AGENTS.md still teaches the root_path compatibility alias" >&2
    rc=1
  fi
  for required in 'work_dir:' 'driver:' 'provider: shell' 'transport: cli'; do
    if ! rg -qF "$required" "$root/AGENTS.md"; then
      echo "    AGENTS.md is missing canonical example token: $required" >&2
      rc=1
    fi
  done
  if ! rg -q 'fn agents_md_manifests_apply_without_legacy_warnings' "$root/core/src/fixture_corpus_tests.rs"; then
    echo "    the behavioral Rust parse/validate/apply test for AGENTS.md is missing" >&2
    rc=1
  fi
  if ! missing_paths="$(agents_missing_repo_paths "$root")"; then
    echo "    AGENTS.md names repository paths that do not exist:" >&2
    while IFS= read -r path; do
      [[ -n "$path" ]] && echo "      $path" >&2
    done <<< "$missing_paths"
    rc=1
  fi
  return "$rc"
}

check_architecture_contract() {
  local root="$1" count rc=0
  if ! count="$(migration_count "$root" 2>/dev/null)"; then
    echo "    registered migration chain is not a contiguous source-derived sequence" >&2
    return 1
  fi
  for required in 'crates/orchestrator-persistence' 'crates/slack-gateway' 'four binaries'; do
    if ! rg -qF "$required" "$root/docs/architecture.md"; then
      echo "    docs/architecture.md is missing: $required" >&2
      rc=1
    fi
  done
  if ! rg -qF '**Web frontend** (`gui/`)' "$root/docs/architecture.md"; then
    echo "    docs/architecture.md does not identify root gui/ as the Web frontend" >&2
    rc=1
  fi
  if ! rg -qF '**Desktop shell** (`crates/gui/`)' "$root/docs/architecture.md"; then
    echo "    docs/architecture.md does not identify crates/gui/ as the desktop shell" >&2
    rc=1
  fi
  if ! rg -qF "contains $count migrations" "$root/docs/architecture.md"; then
    echo "    docs/architecture.md does not report source-derived migration count $count" >&2
    rc=1
  fi
  return "$rc"
}

check_proto_canonical() {
  local root="$1" rc=0 stale
  if [[ -e "$root/proto/orchestrator.proto" ]]; then
    echo "    root proto/orchestrator.proto duplicate exists" >&2
    rc=1
  fi
  if [[ ! -f "$root/crates/proto/orchestrator.proto" ]] ||
     ! rg -qF 'orchestrator.proto' "$root/crates/proto/build.rs"; then
    echo "    crate-local canonical proto or its build consumer is missing" >&2
    rc=1
  fi
  stale="$(rg -n '(?<!crates/)proto/orchestrator\.proto' "$root/docs" --pcre2 -g '*.md' 2>/dev/null |
    rg -v '/feature_request/|/qa/orchestrator/206-docs-reality-alignment\.md:[0-9]+:test ! -e proto/orchestrator\.proto$' || true)"
  if [[ -n "$stale" ]]; then
    echo "    non-FR docs still name the retired root proto path:" >&2
    printf '      %s\n' "$stale" >&2
    rc=1
  fi
  return "$rc"
}

check_ticket_tracking() {
  local root="$1" rc=0 ignore_status
  set +e
  git -C "$root" check-ignore -q docs/ticket/fr155-gate-probe.md
  ignore_status=$?
  set -e
  if [[ "$ignore_status" -eq 0 ]]; then
    echo "    active docs/ticket Markdown is ignored" >&2
    rc=1
  elif [[ "$ignore_status" -ne 1 ]]; then
    echo "    git check-ignore could not evaluate the ticket contract" >&2
    rc=1
  fi
  if ! rg -qF 'intentionally tracked' "$root/docs/ticket/README.md" ||
     ! rg -qF 'there is no separate `closed/` archive' "$root/docs/ticket/README.md"; then
    echo "    docs/ticket/README.md does not describe tracked-active / verified-delete semantics" >&2
    rc=1
  fi
  return "$rc"
}

check_retired_yaml_residue() {
  local root="$1" rc=0
  if [[ -d "$root/test-yaml-warnings" ]]; then
    echo "    retired test-yaml-warnings directory exists" >&2
    rc=1
  fi
  if rg -n 'test-yaml-warnings|EXCLUDED_PREFIXES|excluded_prefix' "$root/core/src/fixture_driverless_tests.rs" >/dev/null; then
    echo "    driverless fixture gate still contains a retired subtree exclusion" >&2
    rc=1
  fi
  return "$rc"
}

# FR-166: the guide's "built-in kinds" list is prose that names an enum, and until
# now nothing connected the two -- `rg 'ResourceKind' scripts/ config/` returned
# nothing. The list had drifted to ten entries: it omitted Project,
# SourceTaskTemplate and SourceTaskBinding, and named WorkflowStore, which is a CRD.
# A text-only repair re-drifts by §4.4 shape 2, so the expected set is derived from
# the enum on every run and the diagnostic names each kind rather than reporting a
# count, so a fixture can prove which way it broke.
check_resource_kind_catalog() {
  ruby -e '
    root = ARGV[0]
    source = File.read(File.join(root, "crates/orchestrator-config/src/cli_types.rs"))
    body = source[/pub enum ResourceKind \{$(.*?)^\}/m, 1]
    unless body
      warn "    the ResourceKind enum body was not found; the catalog check read nothing"
      exit 2
    end
    enum = body.scan(/^\s{4}([A-Z][A-Za-z0-9]*),$/).flatten
    if enum.empty?
      warn "    the ResourceKind enum body yielded no variants; the catalog check read nothing"
      exit 2
    end
    doc = File.read(File.join(root, "docs/guide/05-advanced-features.md"))
    raw = doc[/built-in kinds \(([^)]*)\)/, 1]
    unless raw
      warn "    docs/guide/05-advanced-features.md no longer states a built-in kinds list"
      exit 2
    end
    listed = raw.split(",").map(&:strip).reject(&:empty?)
    if listed.empty?
      warn "    the built-in kinds list in docs/guide/05-advanced-features.md is empty"
      exit 2
    end
    (enum - listed).each do |kind|
      warn "    the guide built-in kinds list omits ResourceKind::#{kind}"
    end
    (listed - enum).each do |kind|
      warn "    the guide built-in kinds list names #{kind}, which is not a ResourceKind variant"
    end
    exit(((enum - listed) + (listed - enum)).empty? ? 0 : 1)
  ' "$1"
}

# FR-166: docs/guide is the EN source and docs/guide/zh the ZH source, and the EN
# table of contents linked Chinese-only content on five rows. Both halves are
# derived rather than listed: the Chinese files are found by measuring each file,
# and the permitted ones come from the publishing policy's translationGaps, so a
# new Chinese file dropped into the EN source is caught without anyone editing a
# guard-list.
check_guide_language_parity() {
  ruby -e '
    require "json"
    root = ARGV[0]
    guide = File.join(root, "docs/guide")
    rc = 0

    # Measured per prose line, not per character. A character ratio under-reaches:
    # technical Chinese is dense with Latin tokens (CLI flags, YAML keys, product
    # names), so the four Chinese Slack guides scored 0.44-0.52 against pure English
    # at 0.00 and a partly-bilingual English chapter at 0.15 -- no threshold
    # separates them. Asking instead what fraction of prose lines are written in
    # Chinese gives 0.94-0.97 for those four, 0.37 for the bilingual chapter and
    # 0.00 for the rest, so a half threshold sits in a wide empty band.
    cjk_dominant = lambda do |path|
      lines = File.read(path).gsub(/```.*?```/m, "").lines.map(&:strip)
      lines.reject! { |line| line.empty? || line.start_with?("|---", "---") }
      return false if lines.empty?
      lines.count { |line| line.match?(/[一-鿿]/) } * 2 > lines.size
    end

    slug = lambda { |path| File.basename(path, ".md").sub(/\A\d+-/, "") }

    en_files = Dir.glob(File.join(guide, "*.md")).reject { |p| File.basename(p) == "README.md" }
    zh_files = Dir.glob(File.join(guide, "zh", "*.md")).reject { |p| File.basename(p) == "README.md" }
    if en_files.empty? || zh_files.empty?
      warn "    the guide source directories yielded no chapters; the parity check read nothing"
      exit 2
    end

    # 1. Every numbered ZH chapter has a same-numbered EN chapter.
    en_numbers = en_files.map { |p| File.basename(p)[/\A(\d+)-/, 1] }.compact
    zh_files.each do |path|
      number = File.basename(path)[/\A(\d+)-/, 1]
      next unless number
      next if en_numbers.include?(number)
      warn "    docs/guide/zh/#{File.basename(path)} has no same-numbered English chapter"
      rc = 1
    end

    # 2. Chinese text sitting in the EN source slot must be a declared translation gap.
    policy = JSON.parse(File.read(File.join(root, "config/governance/docs-publishing.json")))
    declared = policy.fetch("translationGaps", [])
                     .select { |gap| gap["collection"] == "guide" && gap["owedTranslation"] == "en" }
                     .map { |gap| gap["slug"] }
    chinese = en_files.select { |path| cjk_dominant.call(path) }.map { |path| slug.call(path) }
    (chinese - declared).sort.each do |name|
      warn "    docs/guide/#{name}.md is Chinese text in the English source slot and is not a declared translationGap"
      rc = 1
    end
    (declared - chinese).sort.each do |name|
      warn "    translationGaps owes an English #{name}, but no Chinese guide source by that slug exists"
      rc = 1
    end

    # 3. A TOC row pointing at Chinese text must say so in its link text.
    readme = File.read(File.join(guide, "README.md"))
    rows = readme.scan(/\[([^\]]+)\]\(([^)\s]+\.md)\)/)
    if rows.empty?
      warn "    docs/guide/README.md yielded no chapter links; the parity check read nothing"
      exit 2
    end
    rows.each do |label, target|
      path = File.expand_path(target, guide)
      next unless path.start_with?(guide + File::SEPARATOR) && File.file?(path)
      next unless cjk_dominant.call(path)
      next if label.match?(/中文|Chinese/)
      warn "    docs/guide/README.md links Chinese content as \"#{label}\" without saying so"
      rc = 1
    end

    exit rc
  ' "$1"
}

ALL_CHECKS=(check_onboarding_contract check_architecture_contract check_proto_canonical
            check_ticket_tracking check_retired_yaml_residue
            check_resource_kind_catalog check_guide_language_parity)

run_checks() {
  local root="$1" check rc=0
  for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
    if "$check" "$root"; then
      pass "$check"
    else
      fail "$check"
      rc=1
    fi
  done
  return "$rc"
}

if [[ "${1:-}" == "--fixture-test" ]]; then
  echo "=== FR-155 docs reality alignment (negative fixtures) ==="
  FIXTURE_ROOT="$(mktemp -d)"
  cleanup() { rm -rf "$FIXTURE_ROOT"; }
  trap cleanup EXIT

  BASE="$FIXTURE_ROOT/base"
  mkdir -p "$BASE"
  (cd "$REPO_ROOT" && git ls-files | tar cf - -T -) | (cd "$BASE" && tar xf -)
  git -C "$BASE" init -q
  git -C "$BASE" add -A
  TARGETED_CHECKS=()

  new_case() {
    local dir="$FIXTURE_ROOT/$1"
    mkdir -p "$dir"
    (cd "$BASE" && tar cf - .) | (cd "$dir" && tar xf -)
    echo "$dir"
  }

  expect_fail() {
    local name="$1" dir="$2" target="$3" check
    TARGETED_CHECKS+=("$target")
    for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
      if [[ "$check" == "$target" ]]; then
        if "$check" "$dir" >/dev/null 2>&1; then
          fail "$name: $target accepted the injected defect"
          return
        fi
      elif ! "$check" "$dir" >/dev/null 2>&1; then
        fail "$name: defect also tripped $check"
        return
      fi
    done
    pass "$name: isolated to $target"
  }

  # §4.4 shape 7: an exit code cannot say which branch a gate failed through, and a
  # gate that was already red satisfies "it failed". This variant additionally reads
  # the diagnostic and requires it to name the object the fixture just mutated.
  # The output is captured into a variable rather than piped into grep -- a producer
  # feeding `grep -q` dies of EPIPE under pipefail, which is the FR-145 defect.
  expect_fail_naming() {
    local name="$1" dir="$2" target="$3" needle="$4" check out status=0
    TARGETED_CHECKS+=("$target")
    out="$("$target" "$dir" 2>&1)" || status=$?
    if [[ "$status" -eq 0 ]]; then
      fail "$name: $target accepted the injected defect"
      return
    fi
    if [[ "$out" != *"$needle"* ]]; then
      fail "$name: $target failed, but its diagnostic never named '$needle': $out"
      return
    fi
    for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
      [[ "$check" == "$target" ]] && continue
      if ! "$check" "$dir" >/dev/null 2>&1; then
        fail "$name: defect also tripped $check"
        return
      fi
    done
    pass "$name: isolated to $target, naming $needle"
  }

  check_fixture_target_coverage() {
    local check target found rc=0
    for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
      found=0
      for target in ${TARGETED_CHECKS[@]+"${TARGETED_CHECKS[@]}"}; do
        [[ "$target" == "$check" ]] && found=1
      done
      if [[ "$found" -eq 0 ]]; then
        echo "    registered check has no negative fixture: $check" >&2
        rc=1
      fi
    done
    for target in ${TARGETED_CHECKS[@]+"${TARGETED_CHECKS[@]}"}; do
      found=0
      for check in ${ALL_CHECKS[@]+"${ALL_CHECKS[@]}"}; do
        [[ "$target" == "$check" ]] && found=1
      done
      if [[ "$found" -eq 0 ]]; then
        echo "    negative fixture targets an unregistered check: $target" >&2
        rc=1
      fi
    done
    return "$rc"
  }

  if run_checks "$BASE" >/dev/null 2>&1; then
    pass "positive control: copied repository passes every reality check"
  else
    fail "positive control: copied repository is not a valid fixture baseline"
  fi

  d="$(new_case onboarding)"
  if fixture_mutate "fixture onboarding" "$d/AGENTS.md" \
    ruby -e 'path=ARGV[0]; text=File.read(path); abort "anchor" unless text.include?("work_dir:"); File.write(path, text.sub("work_dir:", "root_path:"))' "$d/AGENTS.md"; then
    expect_fail "fixture onboarding" "$d" check_onboarding_contract
  fi

  d="$(new_case onboarding-path)"
  if fixture_mutate "fixture onboarding path" "$d/AGENTS.md" \
    ruby -e 'path=ARGV[0]; text=File.read(path); from="config/governance/qa-gate-surface.json"; abort "anchor" unless text.include?(from); File.write(path, text.sub(from, "config/governance/quality-gates.json"))' "$d/AGENTS.md"; then
    expect_fail "fixture onboarding path" "$d" check_onboarding_contract
  fi

  d="$(new_case architecture)"
  if fixture_mutate "fixture migration drift" "$d/crates/orchestrator-persistence/src/migration.rs" \
    ruby -e 'path=ARGV[0]; text=File.read(path); abort "anchor" unless text.include?("version: 37"); File.write(path, text.sub("version: 37", "version: 38"))' "$d/crates/orchestrator-persistence/src/migration.rs"; then
    expect_fail "fixture migration drift" "$d" check_architecture_contract
  fi

  d="$(new_case architecture-gui-collapse)"
  if fixture_mutate "fixture GUI collapse" "$d/docs/architecture.md" \
    ruby -e 'path=ARGV[0]; text=File.read(path); from="**Web frontend** (`gui/`)"; abort "anchor" unless text.include?(from); File.write(path, text.sub(from, "**Web frontend** (`crates/gui/`)"))' "$d/docs/architecture.md"; then
    expect_fail "fixture GUI collapse" "$d" check_architecture_contract
  fi

  d="$(new_case proto)"
  mkdir -p "$d/proto"
  printf 'syntax = "proto3";\n' > "$d/proto/orchestrator.proto"
  expect_fail "fixture duplicate proto" "$d" check_proto_canonical

  d="$(new_case tickets)"
  printf '\ndocs/ticket/*.md\n' >> "$d/.gitignore"
  expect_fail "fixture ignored tickets" "$d" check_ticket_tracking

  d="$(new_case retired-yaml)"
  mkdir -p "$d/test-yaml-warnings"
  printf 'kind: Agent\n' > "$d/test-yaml-warnings/stale.yaml"
  expect_fail "fixture retired YAML" "$d" check_retired_yaml_residue

  # Two mutations in opposite directions, because a matcher is exact in one
  # direction and wrong in the other (§4.4 shape 10). A kind added to the enum and
  # a kind added to the prose produce different diagnostics, so the log says which
  # way the catalog broke rather than only that it did.
  d="$(new_case resource-kind-enum)"
  if fixture_mutate "fixture new kind unlisted" "$d/crates/orchestrator-config/src/cli_types.rs" \
    ruby -e 'path=ARGV[0]; text=File.read(path); anchor="    /// Trigger manifest.\n    Trigger,\n}"; abort "anchor" unless text.include?(anchor); File.write(path, text.sub(anchor, "    /// Trigger manifest.\n    Trigger,\n    /// Probe manifest.\n    ProbeKind,\n}"))' "$d/crates/orchestrator-config/src/cli_types.rs"; then
    expect_fail_naming "fixture new kind unlisted" "$d" check_resource_kind_catalog \
      "omits ResourceKind::ProbeKind"
  fi

  d="$(new_case resource-kind-prose)"
  if fixture_mutate "fixture prose names a non-kind" "$d/docs/guide/05-advanced-features.md" \
    ruby -e 'path=ARGV[0]; text=File.read(path); anchor="EnvStore, SecretStore, Trigger)"; abort "anchor" unless text.include?(anchor); File.write(path, text.sub(anchor, "EnvStore, SecretStore, Trigger, WorkflowStore)"))' "$d/docs/guide/05-advanced-features.md"; then
    expect_fail_naming "fixture prose names a non-kind" "$d" check_resource_kind_catalog \
      "names WorkflowStore, which is not a ResourceKind variant"
  fi

  # The mutation the implementation is least likely to catch is not a deleted
  # translation but an *added* Chinese file, because nobody edits a guard-list when
  # they add one. The check finds it by measuring the file, so it does not have to.
  d="$(new_case guide-chinese-in-en-slot)"
  printf '# 探针指南\n\n这份文档完全是中文，放在英文源目录里，没有在 translationGaps 中声明。\n' \
    > "$d/docs/guide/zz-probe.md"
  expect_fail_naming "fixture Chinese in the EN slot" "$d" check_guide_language_parity \
    "docs/guide/zz-probe.md is Chinese text in the English source slot"

  d="$(new_case guide-zh-only-chapter)"
  printf '# 99 - 探针章节\n\n只有中文版本的编号章节。\n' \
    > "$d/docs/guide/zh/99-probe.md"
  expect_fail_naming "fixture ZH-only numbered chapter" "$d" check_guide_language_parity \
    "docs/guide/zh/99-probe.md has no same-numbered English chapter"

  ALL_CHECKS+=(check_uncovered_fixture_probe)
  if check_fixture_target_coverage >/dev/null 2>&1; then
    fail "meta fixture: an uncovered registered check passed target completeness"
  else
    pass "meta fixture: a registered check with no negative target is rejected"
  fi
  last_check_index=$((${#ALL_CHECKS[@]} - 1))
  unset "ALL_CHECKS[$last_check_index]"

  if check_fixture_target_coverage; then
    pass "meta: every registered check is targeted by at least one negative fixture"
  else
    fail "meta: registered checks and negative fixture targets differ"
  fi

  echo "=== fixtures: $PASS passed, $FAIL failed ==="
  [[ "$FAIL" -eq 0 ]] || exit 1
  exit 0
fi

echo "=== FR-155 docs reality alignment ==="
run_checks "$REPO_ROOT" || true
echo "=== docs reality alignment: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]] || exit 1
