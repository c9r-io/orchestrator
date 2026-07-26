#!/usr/bin/env bash
#
# FR-135 bash 3.2 compatibility — QA gate.
#
# macOS ships bash 3.2 and the GitHub macOS runner image ships nothing newer on
# PATH, so every `#!/usr/bin/env bash` script this repository runs on a macOS
# job runs under 3.2. `scripts/coverage-governance.sh` expanded an empty array
# under `set -u` there, which 3.2 rejects and 4.4+ accepts, and the
# `boundary-coverage` job died on that line on every run it ever had.
#
# Two halves, because neither is worth much alone:
#
#   * `scripts/qa/bash32-compat.rb` scans every tracked shell file. A scan can
#     tell you the repository is clean; it cannot tell you the thing it scans
#     for is actually dangerous.
#   * the fixture corpus below executes each class under the real interpreter.
#     That can tell you the rule is true; it cannot tell you the repository
#     obeys it.
#
# `BASH_COMPAT=3.2` was measured against bash 5.3 for every class here and
# restores none of them, so on a bash 4+ host the executed half has nothing to
# observe. It says so loudly and case 9 keeps that from becoming a silent hole,
# by asserting from the parsed workflow that some CI job runs this on macOS.
#
# Safety: read-only against the working tree. Fixtures are written under
# $TMPDIR, no daemon is started, no database is touched, no provider is invoked.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GATE="$REPO_ROOT/scripts/qa/bash32-compat.rb"

for command_name in ruby git; do
  command -v "$command_name" >/dev/null 2>&1 || { echo "missing required command: $command_name" >&2; exit 1; }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr135-bash32.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
SKIP=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }
skip() { echo "  SKIP: $1" >&2; SKIP=$((SKIP + 1)); }

LEGACY_BASH="/bin/bash"
LEGACY_VERSION="$("$LEGACY_BASH" -c 'echo "${BASH_VERSION}"')"
case "$LEGACY_VERSION" in
  3.2.*) LEGACY_IS_32=1 ;;
  *) LEGACY_IS_32=0 ;;
esac
echo "legacy interpreter: $LEGACY_BASH ($LEGACY_VERSION)"

# A scratch git repository, because the gate derives its scanned set from
# `git ls-files` rather than from a list. Fixtures have to be added to the index
# to be visible, which is the property case 2 exists to prove.
new_repo() {
  local name="$1" dir
  dir="$WORK/$name"
  mkdir -p "$dir"
  git -C "$dir" init --quiet
  echo "$dir"
}

track() {
  git -C "$1" add -A
}

run_gate() {
  ruby "$GATE" --repo-root "$1"
}

echo "== case 1: the gate passes on this repository =="
if OUTPUT="$(run_gate "$REPO_ROOT" 2>&1)"; then
  pass "the working tree is free of bash 3.2 hazards — ${OUTPUT}"
else
  echo "$OUTPUT" >&2
  fail "the working tree carries bash 3.2 hazards"
fi

echo "== case 2: coverage is derived from git, not from a list =="
# The file lands in a directory that did not exist when the gate was written. A
# roster of scanned paths would miss it and report a clean tree.
REPO2="$(new_repo case2)"
mkdir -p "$REPO2/tools/newly-invented/deeply/nested"
cat > "$REPO2/tools/newly-invented/deeply/nested/fresh.sh" <<'FIXTURE'
#!/usr/bin/env bash
set -euo pipefail
items=()
printf '%s\n' "${items[@]}"
FIXTURE
track "$REPO2"
if OUTPUT="$(run_gate "$REPO2" 2>&1)"; then
  fail "a hazard in a brand-new directory was not scanned"
elif grep -q "tools/newly-invented/deeply/nested/fresh.sh:4" <<<"$OUTPUT"; then
  pass "a script in a directory the gate never heard of is scanned and located"
else
  echo "$OUTPUT" >&2
  fail "the hazard was reported, but not against the new file"
fi

echo "== case 3: a bare empty-array expansion is rejected, the guarded one is not =="
REPO3="$(new_repo case3)"
cat > "$REPO3/subject.sh" <<'FIXTURE'
#!/usr/bin/env bash
set -euo pipefail
args=()
cargo build "${args[@]}"
FIXTURE
track "$REPO3"
if OUTPUT="$(run_gate "$REPO3" 2>&1)"; then
  fail "a bare expansion of a possibly-empty array was accepted"
elif grep -q "empty-array-expansion" <<<"$OUTPUT" && grep -q 'subject.sh:4' <<<"$OUTPUT"; then
  pass "a bare expansion is rejected on its own rule, at its own line"
else
  echo "$OUTPUT" >&2
  fail "the rejection did not name the empty-array rule; it failed for another reason"
fi

cat > "$REPO3/subject.sh" <<'FIXTURE'
#!/usr/bin/env bash
set -euo pipefail
args=()
cargo build ${args[@]+"${args[@]}"}
FIXTURE
track "$REPO3"
if run_gate "$REPO3" >/dev/null 2>&1; then
  pass "the guarded form is accepted"
else
  run_gate "$REPO3" >&2 || true
  fail "the guarded form was rejected; the gate has no accepting state"
fi

echo "== case 4: every bash 4+ construct is rejected under its own rule =="
# One fixture per class, each asserted by rule name rather than by exit code. A
# gate that lost one rule and kept the others would still exit non-zero on a
# combined fixture, and would still be broken.
class_fixture() {
  case "$1" in
    associative-array) printf 'declare -A table=()\n' ;;
    mapfile) printf 'mapfile -t lines < /dev/null\n' ;;
    case-conversion) printf 'name=abc\necho "${name^^}"\n' ;;
    nameref) printf 'alias_to() { local -n target="$1"; }\n' ;;
    wait-n) printf 'sleep 0 &\nwait -n\n' ;;
    globstar) printf 'shopt -s globstar\n' ;;
  esac
}
REPO4="$(new_repo case4)"
for rule in associative-array mapfile case-conversion nameref wait-n globstar; do
  {
    printf '#!/usr/bin/env bash\nset -euo pipefail\n'
    class_fixture "$rule"
  } > "$REPO4/subject.sh"
  track "$REPO4"
  if OUTPUT="$(run_gate "$REPO4" 2>&1)"; then
    fail "$rule was accepted"
  elif grep -q "\[$rule\]" <<<"$OUTPUT"; then
    pass "$rule is rejected under its own rule name"
  else
    echo "$OUTPUT" >&2
    fail "$rule was rejected, but under a different rule"
  fi
done

echo "== case 5: the classes the gate rejects really do fail under bash 3.2 =="
# The half that makes the scan more than a pattern preference. Each snippet is
# executed; the hazardous one must fail and its replacement must succeed. On a
# bash 4+ host there is nothing to observe and that is reported, not passed.
hazard_snippet() {
  case "$1" in
    empty-array-expansion) echo 'set -euo pipefail; a=(); printf "%s\n" "${a[@]}"' ;;
    associative-array) echo 'set -euo pipefail; declare -A m=([x]=1); echo "${m[x]}"' ;;
    mapfile) echo 'set -euo pipefail; mapfile -t a < /dev/null' ;;
    case-conversion) echo 'set -euo pipefail; x=ab; echo "${x^^}"' ;;
    nameref) echo 'set -euo pipefail; f() { local -n r="$1"; r=2; }; v=1; f v' ;;
    wait-n) echo 'set -euo pipefail; sleep 0 & wait -n' ;;
    globstar) echo 'set -euo pipefail; shopt -s globstar' ;;
  esac
}
remedy_snippet() {
  case "$1" in
    empty-array-expansion) echo 'set -euo pipefail; a=(); printf "%s\n" ${a[@]+"${a[@]}"}' ;;
    associative-array) echo 'set -euo pipefail; lookup() { case "$1" in x) echo 1 ;; esac; }; [ "$(lookup x)" = 1 ]' ;;
    mapfile) echo 'set -euo pipefail; a=(); while IFS= read -r l; do a+=("$l"); done < /dev/null' ;;
    case-conversion) echo 'set -euo pipefail; x=ab; [ "$(printf %s "$x" | tr "[:lower:]" "[:upper:]")" = AB ]' ;;
    nameref) echo 'set -euo pipefail; f() { echo "$1"; }; [ "$(f 2)" = 2 ]' ;;
    wait-n) echo 'set -euo pipefail; sleep 0 & pid=$!; wait "$pid"' ;;
    globstar) echo 'set -euo pipefail; find . -maxdepth 0 >/dev/null' ;;
  esac
}
for rule in empty-array-expansion associative-array mapfile case-conversion nameref wait-n globstar; do
  if [[ "$LEGACY_IS_32" -ne 1 ]]; then
    skip "$rule: $LEGACY_BASH is $LEGACY_VERSION, so bash 3.2 semantics are NOT exercised on this host"
    continue
  fi
  if "$LEGACY_BASH" -c "$(hazard_snippet "$rule")" >/dev/null 2>&1; then
    fail "$rule: the hazardous form succeeded under bash $LEGACY_VERSION — the rule guards nothing"
  elif "$LEGACY_BASH" -c "$(remedy_snippet "$rule")" >/dev/null 2>&1; then
    pass "$rule: hazardous form fails and the prescribed replacement works under bash $LEGACY_VERSION"
  else
    fail "$rule: the prescribed replacement also failed under bash $LEGACY_VERSION"
  fi
done

echo "== case 6: the shapes the gate deliberately allows are safe, and executed =="
# `${#a[@]}` and `${!a[@]}` on an empty array were measured as fine in 3.2.
# Flagging them would have sent scripts/regression/lib/probe-runner-lib.sh
# through a rewrite that fixes nothing, so the exemption is paired with the fact
# rather than asserted.
REPO6="$(new_repo case6)"
cat > "$REPO6/subject.sh" <<'FIXTURE'
#!/usr/bin/env bash
set -euo pipefail
entries=()
echo "${#entries[@]}"
for index in "${!entries[@]}"; do echo "$index"; done
FIXTURE
track "$REPO6"
if run_gate "$REPO6" >/dev/null 2>&1; then
  pass "length and index expansions of an empty array are not reported"
else
  run_gate "$REPO6" >&2 || true
  fail "a safe expansion shape is reported as a hazard"
fi
if [[ "$LEGACY_IS_32" -eq 1 ]]; then
  if "$LEGACY_BASH" "$REPO6/subject.sh" >/dev/null 2>&1; then
    pass "and they really do run under bash $LEGACY_VERSION"
  else
    fail "a shape the gate allows fails under bash $LEGACY_VERSION"
  fi
else
  skip "the allowed shapes were not executed: $LEGACY_BASH is $LEGACY_VERSION"
fi

echo "== case 7: comments and here-document bodies are not scanned =="
# Deliberate, and load-bearing: this wrapper writes hazardous fixtures as
# here-document bodies, and several QA wrappers write helper scripts the same
# way. The body is data to the enclosing script. The same text as code must
# still be caught, which is the second half of this case.
REPO7="$(new_repo case7)"
cat > "$REPO7/subject.sh" <<'OUTER'
#!/usr/bin/env bash
set -euo pipefail
# declare -A described_but_not_used
cat > /dev/null <<'INNER'
declare -A inside_a_heredoc=()
mapfile -t also_inside < /dev/null
INNER
echo done
OUTER
track "$REPO7"
if run_gate "$REPO7" >/dev/null 2>&1; then
  pass "a commented hazard and a here-document body are not reported"
else
  run_gate "$REPO7" >&2 || true
  fail "a comment or here-document body was read as code"
fi

printf '#!/usr/bin/env bash\nset -euo pipefail\ndeclare -A real=()\n' > "$REPO7/subject.sh"
track "$REPO7"
if run_gate "$REPO7" >/dev/null 2>&1; then
  fail "the same construct as real code was also ignored; the gate sees nothing"
else
  pass "the same construct as real code is still caught"
fi

echo "== case 8: a syntactically broken shell file is reported, not skipped =="
# A gate that swallows unreadable input reports a clean tree for a file it never
# understood. The scan is line-based, so this asserts the file is still scanned
# and its hazard still found.
REPO8="$(new_repo case8)"
printf '#!/usr/bin/env bash\nset -euo pipefail\nif [ -z "$1" ; then\nitems=()\nprintf "%%s" "${items[@]}"\nfi\n' > "$REPO8/broken.sh"
track "$REPO8"
if OUTPUT="$(run_gate "$REPO8" 2>&1)"; then
  fail "a hazard inside an unparseable script was not reported"
elif grep -q "broken.sh:5" <<<"$OUTPUT"; then
  pass "the hazard is still located inside a script that does not parse"
else
  echo "$OUTPUT" >&2
  fail "the unparseable script was reported at the wrong place"
fi

echo "== case 9: some CI job runs this gate where bash 3.2 actually is =="
# Without this, a green run on ubuntu means the executed half was skipped on
# every host that ever ran it, and nobody would know. Read from the parsed
# workflow: a `run:` line mentioning this script inside a job whose runs-on is
# macOS. Grepping the file would be satisfied by the string appearing in a
# comment or in a Linux-only job.
MACOS_JOBS="$(ruby -ryaml -e '
  workflow = YAML.safe_load(File.read(ARGV[0]), aliases: true)
  wanted = ARGV[1]
  hosting = []
  (workflow["jobs"] || {}).each do |job_name, job|
    runners = [job["runs-on"]].flatten.compact.join(" ")
    matrix = (job.dig("strategy", "matrix", "os") || []).join(" ")
    next unless "#{runners} #{matrix}".include?("macos")
    runs = (job["steps"] || []).map { |step| step["run"].to_s }
    next unless runs.any? { |line| line.include?(wanted) }
    hosting << job_name
  end
  puts hosting.join(" ")
' "$REPO_ROOT/.github/workflows/ci.yml" "scripts/qa/test-bash32-compat.sh")"
if [[ -n "$MACOS_JOBS" ]]; then
  pass "the executed half has a bash 3.2 host in CI: $MACOS_JOBS"
else
  fail "no macOS job runs this gate, so its executed half is skipped everywhere"
fi

echo
if [[ "$SKIP" -gt 0 ]]; then
  echo "WARNING: $SKIP case(s) skipped — this host's $LEGACY_BASH is $LEGACY_VERSION," >&2
  echo "         and BASH_COMPAT cannot restore 3.2 semantics. Only the macOS leg proves them." >&2
fi
echo "FR-135 bash 3.2 compatibility: $PASS passed, $FAIL failed, $SKIP skipped"
[[ "$FAIL" -eq 0 ]]
