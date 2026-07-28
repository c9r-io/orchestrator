#!/usr/bin/env bash
#
# Fixture premises and mutations, for the negative fixtures of the governance
# gates. Sourced by a gate, never executed.
#
# A negative fixture names a specific statement in a specific file and changes
# it. The target is enumerated, and the governed code moving is exactly what
# these gates exist to permit. Nine recorded times a target moved out from under
# a fixture; eight of those nine stayed green — the fixture aborted the run, or
# passed vacuously, or mutated a file the gate was seeing for the first time and
# reported through the wrong branch. The worst of the nine did not merely go
# quiet: it reported a defect that was not there, and sent the auditor to read
# the gate instead of the fixture (FR-143, DD-155).
#
# Three contracts, and between them they close the ways a fixture reports
# without proving:
#
#   fixture_premise  the command must succeed. Its own stderr is the diagnosis,
#                    so a `ruby -e ... abort "..."` premise check is correct
#                    here rather than forbidden — what was wrong was nobody
#                    catching it.
#   fixture_mutate   the target must be an existing regular file, the command
#                    must succeed, and the file must actually change.
#   fixture_produce  the command must succeed and leave a non-empty file where
#                    there was none. Distinct from fixture_mutate because the
#                    two want opposite preconditions, and collapsing them would
#                    mean neither could check one.
#
# Both report through the caller's fail() and return non-zero rather than
# exiting, so a stale fixture costs one failed assertion and the rest of the
# suite still runs. That is the whole difference from `abort`: `set -e` takes
# the run down, the summary line never prints, and a run that stopped early is
# indistinguishable from one that finished (QA 186 §certification conditions).
#
# Callers must define pass() and fail(). Both are conventions every gate in
# scripts/qa already follows.
#
# bash 3.2 clean: scripts/qa/bash32-compat.rb scans `git ls-files '*.sh'`, which
# includes this directory. No mapfile, no associative arrays, no namerefs.
#
# Every function here is called in condition position — `if fixture_mutate ...`
# — which disables `set -e` for everything beneath it. So nothing below may lean
# on `set -e`, and a status is never read from `$?` after an assignment: in a
# context where ERR is live the assignment leaves before `$?` is consulted, and
# the record goes with it. FR-144 shipped that defect inside its own fix.

# The digest.
#
# Only shasum, mktemp and awk are used below, and all three are in the
# governance job's declared runnerBaseline — deliberately, because
# check_job_dependencies derives a gate's requirements from its own `command -v`
# preamble and does not follow a sourced library. A dependency introduced here
# would be invisible to it, so this file stays inside the baseline rather than
# relying on a check that cannot see it. Recorded in DD-155 as a known limit of
# that gate, not as a property of this one.
#
# `${digest%% *}` rather than `| cut`: one fewer command, and a pipe would hand
# back cut's status instead of shasum's.
fixture__digest() {
  local digest
  digest="$(shasum "$1")"
  printf '%s' "${digest%% *}"
}

# Runs a command, and on failure fails the case with the command's own stderr.
#
# Nothing is swallowed: the full stderr goes to the gate's stderr, because
# check_diagnostics_preserved is right that a gate which can fail without saying
# why is not much better than one that cannot fail.
fixture__run() {
  local label="$1" what="$2"
  shift 2
  local err status
  err="$(mktemp "${TMPDIR:-/tmp}/gate-fixture.XXXXXX")"
  if "$@" >/dev/null 2>"$err"; then
    status=0
  else
    status=$?
  fi
  if [[ "$status" -ne 0 ]]; then
    fail "$label: $what (exit $status); this case proved nothing"
    awk '{ print "      " $0 }' "$err" >&2
    rm -f "$err"
    return 1
  fi
  rm -f "$err"
  return 0
}

# fixture_premise <label> <command...>
#
# The command establishes that the case's premise still holds. A premise that
# has stopped holding is a failed assertion, never a skip and never an abort:
# `test-persistence-dependency.sh` case 8 aborted the whole run with "no
# statement to neutralise" when FR-141 B3 moved the statement, and
# `test-persistence-extraction.sh` case 6 counted an empty `git log --grep` as a
# pass.
fixture_premise() {
  local label="$1"
  shift
  fixture__run "$label" "the fixture's premise no longer holds" "$@"
}

# fixture_mutate <label> <file> <command...>
#
# Applies a mutation and proves it landed. Generalised from inject() in
# test-qa-gate-surface.sh, which was written after two pattern-based fixtures
# there stopped matching the moment ci.yml's steps gained `id:` lines.
#
# The regular-file check is not defensive padding. core-boundary case 5 read its
# removal target from a ledger map FR-141 B4 had emptied, the read returned the
# empty string, and the case wrote to a directory.
#
# A file that does not change is the case that reports and proves nothing, and
# it is the one that produced the worst of the nine: a `db.rs` that had become a
# re-export shell, a substitution that matched nothing, and a fixture announcing
# that the gate had failed to notice a removal nobody had made.
fixture_mutate() {
  local label="$1" file="$2"
  shift 2

  if [[ ! -f "$file" ]]; then
    fail "$label: the fixture target is not a regular file, so the mutation had nowhere to land: $file"
    return 1
  fi

  local before after
  before="$(fixture__digest "$file")"

  fixture__run "$label" "the mutation command failed" "$@" || return 1

  after="$(fixture__digest "$file")"
  if [[ "$before" == "$after" ]]; then
    fail "$label: the mutation did not apply to $file; the fixture proves nothing"
    return 1
  fi
  return 0
}

# fixture_produce <label> <file> <command...>
#
# Derives one fixture input from another. The command must succeed and must
# leave something behind: a producer whose source moved raises, and without this
# the raise takes the run down before the summary line prints — the same defect
# as an uncaught premise, wearing the shape of a build step.
#
# An empty result counts as a failure rather than as a small one. The gate that
# reads it would then be reading nothing, which is the FR-144 lesson: zero rows
# and N passing rows are indistinguishable from the exit code alone.
fixture_produce() {
  local label="$1" file="$2"
  shift 2

  fixture__run "$label" "the fixture producer failed" "$@" || return 1

  if [[ ! -s "$file" ]]; then
    fail "$label: the producer left nothing at $file, so whatever reads it examines nothing"
    return 1
  fi
  return 0
}
