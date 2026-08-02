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

migration_count() {
  ruby -e '
    source = File.read(File.join(ARGV[0], "crates/orchestrator-persistence/src/migration.rs"))
    body = source[/pub fn registered_migrations\(\).*?(?=\/\/\/ Converts migration definitions)/m]
    abort "registered_migrations() body not found" unless body
    versions = body.scan(/\bversion:\s*(\d+)/).flatten.map(&:to_i)
    abort "registered_migrations() contains no versions" if versions.empty?
    expected = (1..versions.length).to_a
    abort "migration versions are not contiguous 1..#{versions.length}: #{versions.inspect}" unless versions == expected
    print versions.length
  ' "$1"
}

check_onboarding_contract() {
  local root="$1" rc=0
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
  return "$rc"
}

check_architecture_contract() {
  local root="$1" count rc=0
  if ! count="$(migration_count "$root" 2>/dev/null)"; then
    echo "    registered migration chain is not a contiguous source-derived sequence" >&2
    return 1
  fi
  for required in 'crates/orchestrator-persistence' 'gui/' 'crates/gui/' 'crates/slack-gateway' 'four binaries'; do
    if ! rg -qF "$required" "$root/docs/architecture.md"; then
      echo "    docs/architecture.md is missing: $required" >&2
      rc=1
    fi
  done
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

ALL_CHECKS=(check_onboarding_contract check_architecture_contract check_proto_canonical
            check_ticket_tracking check_retired_yaml_residue)

run_checks() {
  local root="$1" check rc=0
  for check in "${ALL_CHECKS[@]}"; do
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

  new_case() {
    local dir="$FIXTURE_ROOT/$1"
    mkdir -p "$dir"
    (cd "$BASE" && tar cf - .) | (cd "$dir" && tar xf -)
    echo "$dir"
  }

  expect_fail() {
    local name="$1" dir="$2" target="$3" check
    for check in "${ALL_CHECKS[@]}"; do
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

  if run_checks "$BASE" >/dev/null 2>&1; then
    pass "positive control: copied repository passes every reality check"
  else
    fail "positive control: copied repository is not a valid fixture baseline"
  fi

  d="$(new_case onboarding)"
  ruby -e 'path=ARGV[0]; text=File.read(path); abort "anchor" unless text.include?("work_dir:"); File.write(path, text.sub("work_dir:", "root_path:"))' "$d/AGENTS.md"
  expect_fail "fixture onboarding" "$d" check_onboarding_contract

  d="$(new_case architecture)"
  ruby -e 'path=ARGV[0]; text=File.read(path); abort "anchor" unless text.include?("version: 37"); File.write(path, text.sub("version: 37", "version: 38"))' "$d/crates/orchestrator-persistence/src/migration.rs"
  expect_fail "fixture migration drift" "$d" check_architecture_contract

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

  echo "=== fixtures: $PASS passed, $FAIL failed ==="
  [[ "$FAIL" -eq 0 ]] || exit 1
  exit 0
fi

echo "=== FR-155 docs reality alignment ==="
run_checks "$REPO_ROOT" || true
echo "=== docs reality alignment: $PASS passed, $FAIL failed ==="
[[ "$FAIL" -eq 0 ]] || exit 1
