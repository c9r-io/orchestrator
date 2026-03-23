#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

allow_core_rwlock_files=(
  "core/src/state.rs"
  "core/src/service/bootstrap.rs"
  "core/src/test_utils.rs"
)

allow_daemon_rwlock_files=(
  "crates/daemon/src/protection.rs"
)

allow_scheduler_rwlock_files=(
  "crates/orchestrator-scheduler/src/scheduler/runtime.rs"
)

deny_patterns=(
  'std::sync::RwLock'
  'RwLockReadGuard'
  'RwLockWriteGuard'
)

join_by_pipe() {
  local first=1
  for value in "$@"; do
    if [[ $first -eq 1 ]]; then
      printf '%s' "$value"
      first=0
    else
      printf '|%s' "$value"
    fi
  done
}

deny_regex="$(join_by_pipe "${deny_patterns[@]}")"

collect_matches() {
  local path="$1"
  rg -n -e "$deny_regex" "$path" || true
}

filter_allowed() {
  local matches="$1"
  shift
  local allowed=("$@")
  local filtered="$matches"

  for file in "${allowed[@]}"; do
    filtered="$(printf '%s\n' "$filtered" | grep -v "^${file}:" || true)"
  done

  printf '%s' "$filtered"
}

core_matches="$(collect_matches core/src)"
core_violations="$(filter_allowed "$core_matches" "${allow_core_rwlock_files[@]}")"

daemon_matches="$(collect_matches crates/daemon/src)"
daemon_violations="$(filter_allowed "$daemon_matches" "${allow_daemon_rwlock_files[@]}")"

scheduler_matches="$(collect_matches crates/orchestrator-scheduler/src)"
scheduler_violations="$(filter_allowed "$scheduler_matches" "${allow_scheduler_rwlock_files[@]}")"

if [[ -n "$core_violations" || -n "$daemon_violations" || -n "$scheduler_violations" ]]; then
  echo "Async lock governance check failed."
  echo
  if [[ -n "$core_violations" ]]; then
    echo "Unexpected std::sync::RwLock or guard usage in async-governed core paths:"
    printf '%s\n' "$core_violations"
    echo
  fi
  if [[ -n "$daemon_violations" ]]; then
    echo "Unexpected std::sync::RwLock or guard usage outside the approved daemon protection boundary:"
    printf '%s\n' "$daemon_violations"
    echo
  fi
  if [[ -n "$scheduler_violations" ]]; then
    echo "Unexpected std::sync::RwLock or guard usage in async-governed scheduler paths:"
    printf '%s\n' "$scheduler_violations"
    echo
  fi
  echo "Approved exceptions:"
  printf '  %s\n' "${allow_core_rwlock_files[@]}" "${allow_daemon_rwlock_files[@]}" "${allow_scheduler_rwlock_files[@]}"
  echo
  echo "Use config snapshots, tokio::sync::{Mutex,RwLock}, atomics, or message passing instead."
  exit 1
fi

echo "Async lock governance check passed."
echo "Approved sync exceptions:"
printf '  %s\n' "${allow_core_rwlock_files[@]}" "${allow_daemon_rwlock_files[@]}" "${allow_scheduler_rwlock_files[@]}"
