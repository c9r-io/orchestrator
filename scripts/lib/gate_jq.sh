# Reading JSON into a gate check, with jq's exit status actually observed.
#
# The shape this replaces:
#
#   while IFS=$'\t' read -r path mode; do
#     ...
#   done < <(jq -r '<query>' "$manifest")
#
# Nobody looks at the process substitution's exit status. `set -euo pipefail`
# does not help: a failure inside `< <(...)` is not the loop's status, and the
# loop's status is not the function's. So when jq errors the loop reads zero
# rows, the body never runs, and the check returns 0 — it reports PASS having
# verified nothing.
#
# That is not hypothetical. FR-140 wrote `"providerIsolation": "no-provider"`
# where the manifest requires `{"mode": "no-provider"}`. jq exited 5 with
# `Cannot index string with string "mode"`, and test-qa-gate-surface.sh printed
# `13 passed, 0 failed` on a manifest it could not read. Only the negative
# fixtures disagreed, because they ask "is an injected defect rejected" and the
# check had stopped rejecting anything.
#
# There is a second, quieter channel, which is why this file exists rather than
# a rule saying "do not use process substitution". Every check is invoked as
# `"$check" "$root" || return 1` — condition position — and that disables
# `set -e` for the entire call tree beneath it. So a plain capture is equally
# silent:
#
#   A) condition position:  jq errors -> declared='' -> check continues -> rc=0
#   B) bare, set -e live:   jq errors -> script exits 5
#
# Assignment from a command substitution *does* carry the status, so the fix for
# both channels is the same: capture, then look.
#
#   rows="$(gate_jq_rows require-rows "$manifest" '<query>')" || return 1
#   while IFS=$'\t' read -r path mode; do
#     [ -z "$path" ] && continue
#     ...
#   done <<< "$rows"
#
# A here-string rather than a pipe, so the loop body stays in the current shell
# and the check's own `rc=1` accumulation still works.
#
# Sourced, not executed:
#   . "$REPO_ROOT/scripts/lib/gate_jq.sh"
#
# bash 3.2: no mapfile, no associative arrays, no namerefs. macOS ships 3.2 and
# scripts/qa/bash32-compat.rb enforces it across `git ls-files '*.sh'`, which
# includes this directory.

# Runs jq and prints its rows, having observed the exit status.
#
# Usage: gate_jq_rows <require-rows|allow-empty> <file> <jq-args...>
#
# The first argument is mandatory and has no default. Zero rows means two
# different things in the same manifest — `staleClaimExemptions` is legitimately
# empty, while `enforcement == "ci-required"` selecting nothing could only mean
# the query or the file is broken — and the two are indistinguishable at the
# call site unless somebody writes down which one they meant. A default would be
# a way to forget, and forgetting is precisely the defect being fixed here.
#
# Emptiness also decides which way a silent failure falls, which is the reason
# it cannot be left implicit. An empty result in check_surface_complete makes
# every file on disk look unclassified: it fails closed, loudly and safely. An
# empty result in check_provider_isolation makes the loop body never run: it
# fails open. Same silence, opposite consequence.
gate_jq_rows() {
  local emptiness="$1"
  case "$emptiness" in
    require-rows|allow-empty)
      shift
      ;;
    *)
      echo "    gate_jq_rows: emptiness not declared" >&2
      echo "      expected 'require-rows' or 'allow-empty' as the first argument, got: ${emptiness:-<nothing>}" >&2
      return 2
      ;;
  esac

  if [ "$#" -lt 2 ]; then
    echo "    gate_jq_rows: expected a file and a jq query" >&2
    return 2
  fi

  local file="$1"
  shift

  local err rows status diagnostic
  err="${TMPDIR:-/tmp}/gate_jq.$$.$RANDOM.err"
  rows="$(jq -r "$@" "$file" 2>"$err")"
  status=$?
  diagnostic="$(cat "$err" 2>/dev/null)"
  rm -f "$err"

  if [ "$status" -ne 0 ]; then
    echo "    $file: jq exited $status; the check reading this file did not run" >&2
    if [ -n "$diagnostic" ]; then
      printf '      %s\n' "$diagnostic" >&2
    fi
    return 1
  fi

  # A warning on a successful run would otherwise be indistinguishable from a
  # data row once the two streams are merged, so it is reported rather than
  # silently prepended to the result.
  if [ -n "$diagnostic" ]; then
    echo "    $file: jq succeeded but wrote to stderr; treating that as a defect" >&2
    printf '      %s\n' "$diagnostic" >&2
    return 1
  fi

  if [ "$emptiness" = "require-rows" ] && [ -z "$rows" ]; then
    echo "    $file: query was declared to require at least one row and read none" >&2
    echo "      query: $*" >&2
    return 1
  fi

  # Nothing at all when empty, rather than one blank line, so `allow-empty`
  # callers iterate zero times instead of once over "".
  if [ -n "$rows" ]; then
    printf '%s\n' "$rows"
  fi
}
