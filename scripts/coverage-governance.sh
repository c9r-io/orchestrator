#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

OUTPUT_DIR="${COVERAGE_OUTPUT_DIR:-target/coverage-governance}"
BASELINE="${COVERAGE_BASELINE:-coverage/boundary-baseline.json}"
BRANCH_MODE="${COVERAGE_BRANCH_MODE:-auto}"
SKIP_PLAYWRIGHT="${COVERAGE_SKIP_PLAYWRIGHT:-0}"
SKIP_FRONTEND="${COVERAGE_SKIP_FRONTEND:-0}"

mkdir -p "$OUTPUT_DIR"

if [[ "${1:-}" == "--fixture-test" ]]; then
  exec node scripts/coverage/test-coverage-governance.mjs
fi

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "cargo-llvm-cov is required (pinned CI version: 0.8.5)" >&2
  exit 1
fi

branch_status="unsupported"
branch_args=()
rust_channel="$(rustc --version)"

# The help text is captured and then searched, rather than piped into `rg -q`.
# `rg -q` leaves on the first match, and under `set -o pipefail` a producer still
# writing when it leaves dies of EPIPE and hands that status to the caller — so
# a help text that *does* advertise `--branch` can read as one that does not
# (FR-145). A function keeps the probe lazy: the earlier conditions still
# short-circuit it, so `cargo llvm-cov --help` runs exactly when it ran before.
llvm_cov_supports_branch() {
  local help
  help="$(cargo llvm-cov --help 2>&1 || true)"
  rg -q -- '--branch' <<< "$help"
}

if [[ "$BRANCH_MODE" != "unsupported" ]] \
  && [[ "$rust_channel" == *nightly* ]] \
  && llvm_cov_supports_branch; then
  branch_status="supported"
  branch_args=(--branch)
elif [[ "$BRANCH_MODE" == "required" ]]; then
  echo "branch coverage requires nightly Rust and cargo-llvm-cov --branch support" >&2
  exit 1
fi

echo "[coverage] collecting instrumented Rust tests"
cargo llvm-cov --workspace --all-targets --all-features \
  ${branch_args[@]+"${branch_args[@]}"} --json --output-path "$OUTPUT_DIR/rust.json"
cargo llvm-cov report ${branch_args[@]+"${branch_args[@]}"} --lcov --output-path "$OUTPUT_DIR/rust.lcov"

if [[ "$SKIP_FRONTEND" == "1" ]]; then
  if [[ ! -f "$OUTPUT_DIR/frontend.json" ]]; then
    echo "COVERAGE_SKIP_FRONTEND=1 requires $OUTPUT_DIR/frontend.json" >&2
    exit 1
  fi
else
  echo "[coverage] collecting React coverage"
  (
    cd gui
    npm run test:coverage
  )
  cp gui/coverage/coverage-summary.json "$OUTPUT_DIR/frontend.json"
fi

if [[ "$SKIP_PLAYWRIGHT" == "1" ]]; then
  if [[ ! -f "$OUTPUT_DIR/playwright.json" ]]; then
    echo "COVERAGE_SKIP_PLAYWRIGHT=1 requires $OUTPUT_DIR/playwright.json" >&2
    exit 1
  fi
else
  echo "[coverage] executing Playwright scenario coverage"
  (
    cd gui
    npx playwright test --reporter=json
  ) >"$OUTPUT_DIR/playwright.json"
fi

node scripts/coverage/coverage-governance.mjs summarize \
  --rust "$OUTPUT_DIR/rust.json" \
  --frontend "$OUTPUT_DIR/frontend.json" \
  --playwright "$OUTPUT_DIR/playwright.json" \
  --output "$OUTPUT_DIR/summary.json" \
  --repo-root "$ROOT_DIR" \
  --branch-status "$branch_status"

node scripts/coverage/coverage-governance.mjs check \
  --summary "$OUTPUT_DIR/summary.json" \
  --baseline "$BASELINE"

echo "[coverage] artifacts:"
echo "  $OUTPUT_DIR/summary.json"
echo "  $OUTPUT_DIR/rust.json"
echo "  $OUTPUT_DIR/rust.lcov"
echo "  $OUTPUT_DIR/frontend.json"
echo "  $OUTPUT_DIR/playwright.json"
