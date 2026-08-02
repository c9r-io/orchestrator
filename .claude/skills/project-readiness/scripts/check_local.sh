#!/usr/bin/env bash
set -euo pipefail

if git rev-parse --show-toplevel >/dev/null 2>&1; then
  READINESS_ROOT="$(git rev-parse --show-toplevel)"
else
  READINESS_ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
fi
cd "$READINESS_ROOT"

say() { printf '%s\n' "$*"; }
run() {
  say "==> $*"
  "$@"
}

say "== revision =="
git rev-parse HEAD
git status --short

if [[ -f Cargo.toml && -d crates && -f gui/package.json ]]; then
  say "== Agent Orchestrator repository =="
  run cargo fmt --all -- --check
  run cargo test --workspace
  run cargo clippy --workspace --all-targets -- -D warnings

  say "== GUI =="
  (
    cd gui
    run npm test
    run npm run build
  )

  say "== governance =="
  run scripts/qa-doc-lint.sh
  say "OK: Agent Orchestrator local readiness checks completed"
  exit 0
fi

say "== generic repository discovery =="
found=0
for build_file in Cargo.toml package.json Makefile; do
  if [[ -e "$build_file" ]]; then
    say "found: $build_file"
    found=$((found + 1))
  fi
done
if [[ "$found" -eq 0 ]]; then
  say "ERROR: no supported build entrypoint discovered" >&2
  exit 1
fi

say "No repository-specific readiness recipe is declared; run commands from the discovered build files."
say "Docker and Kubernetes checks are not applicable unless their assets exist in this target."
