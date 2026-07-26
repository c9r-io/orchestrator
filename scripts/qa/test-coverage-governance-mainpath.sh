#!/usr/bin/env bash
#
# FR-135 coverage governance main path — QA gate.
#
# `scripts/coverage-governance.sh` has two entry paths that share nothing. With
# `--fixture-test` it `exec`s node on line 16 and the process is replaced; every
# line below that belongs to the other path. `coverage-policy-fixtures` runs the
# first, `boundary-coverage` runs the second, and for the whole life of the
# boundary-coverage job the fixtures job was green while the main path died on
# its first command. Two jobs, one script, disjoint coverage, and the green one
# said nothing about the red one.
#
# This gate covers the main path directly and cheaply: the toolchain is shadowed
# by stubs, so the shell flow, the assembled `cargo llvm-cov` argv and the
# summarize/check hand-off are observed without collecting any coverage. It runs
# under /bin/bash, which on macOS is 3.2 — the interpreter where the defect
# lived.
#
# Safety: everything happens inside a temporary tree under $TMPDIR. The working
# tree is never written, no daemon is started, no database is touched, and no
# provider is invoked. `cargo`, `node`, `npm`, `npx`, `rustc` and `rg` are all
# stubbed, so nothing real is compiled or fetched.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SUBJECT="scripts/coverage-governance.sh"

command -v ruby >/dev/null 2>&1 || { echo "missing required command: ruby" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/fr135-mainpath.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

PASS=0
FAIL=0
pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# The defect this gate exists for is a bash 3.2 semantic, so the interpreter is
# chosen rather than inherited. /bin/bash is 3.2 on macOS and on the GitHub
# macOS runner image; on Linux it is 4+ and the run still proves flow and argv,
# which is said out loud rather than left to be assumed.
INTERPRETER="/bin/bash"
INTERPRETER_VERSION="$("$INTERPRETER" -c 'echo "${BASH_VERSION}"')"
case "$INTERPRETER_VERSION" in
  3.2.*) SEMANTICS="bash 3.2 semantics in force" ;;
  *) SEMANTICS="bash $INTERPRETER_VERSION — flow and argv only, 3.2 semantics NOT exercised here" ;;
esac
echo "interpreter: $INTERPRETER ($INTERPRETER_VERSION) — $SEMANTICS"

# A case is a throwaway checkout skeleton holding the script under test and the
# directories it writes into, plus a stub bin directory placed ahead of PATH.
new_case() {
  local name="$1" channel="$2" root bin
  root="$WORK/$name"
  bin="$root/bin"
  mkdir -p "$root/scripts/coverage" "$root/gui" "$bin"
  cp "$REPO_ROOT/$SUBJECT" "$root/$SUBJECT"

  cat > "$bin/rustc" <<EOF
#!/bin/sh
echo "rustc 1.90.0 ($channel)"
EOF

  # The capability probe (`cargo llvm-cov --help`) is answered rather than
  # logged: it is not part of the argv contract being asserted, and advertising
  # --branch here is what lets the nightly case exercise a non-empty array.
  cat > "$bin/cargo" <<'EOF'
#!/bin/sh
if [ "$1" = "llvm-cov" ] && [ "$2" = "--help" ]; then
  echo "      --branch                  Enable branch coverage"
  exit 0
fi
printf '%s\n' "$*" >> "$ARGV_LOG"
exit 0
EOF

  # Present only so the script's `command -v cargo-llvm-cov` preamble passes.
  cat > "$bin/cargo-llvm-cov" <<'EOF'
#!/bin/sh
exit 0
EOF

  cat > "$bin/node" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "$NODE_LOG"
exit 0
EOF

  # `npm run test:coverage` is expected to leave a summary behind; the stub
  # produces the file the script then copies, and nothing else.
  cat > "$bin/npm" <<'EOF'
#!/bin/sh
mkdir -p coverage
echo '{}' > coverage/coverage-summary.json
exit 0
EOF

  cat > "$bin/npx" <<'EOF'
#!/bin/sh
echo '{}'
exit 0
EOF

  # coverage-governance.sh probes `cargo llvm-cov --help ... | rg -q -- --branch`.
  # This stands in for that one call and nothing else: the pattern is whatever
  # follows `--`, which matters because the pattern here starts with a dash and
  # a "last non-flag argument" rule would silently read it as one.
  cat > "$bin/rg" <<'EOF'
#!/bin/sh
pattern=""
seen_separator=""
for argument in "$@"; do
  if [ -n "$seen_separator" ]; then
    pattern="$argument"
    break
  fi
  if [ "$argument" = "--" ]; then
    seen_separator=1
    continue
  fi
  case "$argument" in
    -*) ;;
    *) pattern="$argument" ;;
  esac
done
grep -q -F -- "$pattern"
EOF

  chmod +x "$bin"/*
  echo "$root"
}

run_case() {
  local root="$1"
  shift
  (
    cd "$root"
    export PATH="$root/bin:$PATH"
    export ARGV_LOG="$root/cargo-argv.log"
    export NODE_LOG="$root/node-argv.log"
    : > "$ARGV_LOG"
    : > "$NODE_LOG"
    env "$@" "$INTERPRETER" "$root/$SUBJECT"
  )
}

echo "== case 1: the main path reaches cargo llvm-cov with the stable argv =="
ROOT="$(new_case case1 stable)"
if run_case "$ROOT" COVERAGE_OUTPUT_DIR="$ROOT/out" > "$ROOT/stdout.log" 2> "$ROOT/stderr.log"; then
  pass "the main path runs to completion under $INTERPRETER"
else
  cat "$ROOT/stderr.log" >&2
  fail "the main path did not complete under $INTERPRETER"
fi

# The exact argv, not a substring. The defect turned this line into an error
# before cargo was ever reached, and a looser assertion would also accept a
# `--branch` that the stable toolchain must not receive.
EXPECTED_COLLECT="llvm-cov --workspace --all-targets --all-features --json --output-path $ROOT/out/rust.json"
EXPECTED_REPORT="llvm-cov report --lcov --output-path $ROOT/out/rust.lcov"
ACTUAL_COLLECT="$(sed -n '1p' "$ROOT/cargo-argv.log")"
ACTUAL_REPORT="$(sed -n '2p' "$ROOT/cargo-argv.log")"
if [[ "$ACTUAL_COLLECT" == "$EXPECTED_COLLECT" ]]; then
  pass "the collection argv is exactly the stable form, with no empty word and no --branch"
else
  echo "  expected: $EXPECTED_COLLECT" >&2
  echo "  actual:   $ACTUAL_COLLECT" >&2
  fail "the collection argv is not the stable form"
fi
if [[ "$ACTUAL_REPORT" == "$EXPECTED_REPORT" ]]; then
  pass "the report argv is exactly the stable form"
else
  echo "  expected: $EXPECTED_REPORT" >&2
  echo "  actual:   $ACTUAL_REPORT" >&2
  fail "the report argv is not the stable form"
fi

echo "== case 2: the comparison the job exists for is actually reached =="
# The whole point of recovering this job is that summarize and check run. An
# assertion that stops at cargo would pass on a script that never got there.
if grep -q -- "coverage-governance.mjs summarize" "$ROOT/node-argv.log" \
  && grep -q -- "--branch-status unsupported" "$ROOT/node-argv.log"; then
  pass "summarize is invoked with the resolved branch status"
else
  cat "$ROOT/node-argv.log" >&2
  fail "summarize was not invoked with a branch status"
fi
if grep -q -- "coverage-governance.mjs check" "$ROOT/node-argv.log" \
  && grep -q -- "--baseline coverage/boundary-baseline.json" "$ROOT/node-argv.log"; then
  pass "the non-regression comparison is invoked against the approved baseline"
else
  cat "$ROOT/node-argv.log" >&2
  fail "the non-regression comparison was not invoked"
fi

echo "== case 3: a non-empty branch_args still reaches cargo intact =="
# The guarded expansion has to keep working in the case it was not written for.
# A rewrite that dropped the array entirely would pass case 1 and fail here.
ROOT2="$(new_case case2 nightly)"
if run_case "$ROOT2" COVERAGE_OUTPUT_DIR="$ROOT2/out" > "$ROOT2/stdout.log" 2> "$ROOT2/stderr.log"; then
  if [[ "$(sed -n '1p' "$ROOT2/cargo-argv.log")" == \
    "llvm-cov --workspace --all-targets --all-features --branch --json --output-path $ROOT2/out/rust.json" ]]; then
    pass "a nightly toolchain passes --branch through in position"
  else
    echo "  actual: $(sed -n '1p' "$ROOT2/cargo-argv.log")" >&2
    fail "a nightly toolchain did not pass --branch through"
  fi
  if grep -q -- "--branch-status supported" "$ROOT2/node-argv.log"; then
    pass "summarize records the branch status as supported"
  else
    fail "summarize did not record the supported branch status"
  fi
else
  cat "$ROOT2/stderr.log" >&2
  fail "the nightly main path did not complete"
fi

echo "== case 4: COVERAGE_BRANCH_MODE=required refuses a stable toolchain =="
ROOT3="$(new_case case3 stable)"
if run_case "$ROOT3" COVERAGE_OUTPUT_DIR="$ROOT3/out" COVERAGE_BRANCH_MODE=required \
  > "$ROOT3/stdout.log" 2> "$ROOT3/stderr.log"; then
  fail "a required branch mode was accepted on a stable toolchain"
elif grep -q "branch coverage requires nightly Rust" "$ROOT3/stderr.log"; then
  pass "a required branch mode is refused, naming the reason"
else
  cat "$ROOT3/stderr.log" >&2
  fail "the refusal did not name the reason; the run failed for something else"
fi

echo "== case 5: the two jobs really do cover disjoint paths =="
# The FR's premise, asserted rather than assumed. If the exec on line 16 ever
# stopped short-circuiting, the fixtures job would begin covering the main path
# and this gate's reason for existing would change.
ROOT4="$(new_case case4 stable)"
(
  cd "$ROOT4"
  export PATH="$ROOT4/bin:$PATH"
  export ARGV_LOG="$ROOT4/cargo-argv.log"
  export NODE_LOG="$ROOT4/node-argv.log"
  : > "$ARGV_LOG"
  : > "$NODE_LOG"
  "$INTERPRETER" "$ROOT4/$SUBJECT" --fixture-test
) > "$ROOT4/stdout.log" 2> "$ROOT4/stderr.log" || true
if [[ ! -s "$ROOT4/cargo-argv.log" ]] \
  && grep -q "test-coverage-governance.mjs" "$ROOT4/node-argv.log"; then
  pass "--fixture-test hands off to node and never reaches the cargo main path"
else
  echo "  cargo argv: $(cat "$ROOT4/cargo-argv.log")" >&2
  echo "  node argv:  $(cat "$ROOT4/node-argv.log")" >&2
  fail "--fixture-test is no longer disjoint from the main path"
fi

echo "== case 6: a generation failure is not masked by the upload step =="
# `if: always()` with `if-no-files-found: error` produced the only ##[error] in
# the summary for six consecutive runs, pointing at a missing artifact directory
# while the real failure sat in the step before it. Read from the parsed
# workflow, because the two settings are separate keys and a grep for either one
# says nothing about the pair.
UPLOAD_REPORT="$(ruby -ryaml -e '
  workflow = YAML.safe_load(File.read(ARGV[0]), aliases: true)
  offenders = []
  (workflow["jobs"] || {}).each do |job_name, job|
    (job["steps"] || []).each do |step|
      next unless step["uses"].to_s.start_with?("actions/upload-artifact")
      condition = step["if"].to_s.gsub(/\s+/, "")
      setting = (step["with"] || {})["if-no-files-found"].to_s
      next unless condition.include?("always()") && setting == "error"
      offenders << "#{job_name}: #{step["name"]}"
    end
  end
  puts offenders.join("; ")
' "$REPO_ROOT/.github/workflows/ci.yml")"
if [[ -z "$UPLOAD_REPORT" ]]; then
  pass "no upload step combines if: always() with if-no-files-found: error"
else
  echo "  offending step(s): $UPLOAD_REPORT" >&2
  fail "an upload step still reports a missing artifact ahead of the real failure"
fi

echo
echo "FR-135 coverage governance main path: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
