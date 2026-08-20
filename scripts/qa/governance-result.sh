#!/usr/bin/env bash
# The step that fails the `governance` job, and the tier assertion that FR-174
# needs it to make.
#
# Every gate in that job runs under `continue-on-error: true` and records its
# real outcome, so one run reports every problem instead of only the first —
# FR-134's workspace-scope defect sat invisible behind the ledger tooling's
# self-lock for two runs. This reads those outcomes and decides the job.
#
# Why this is a script and not the `run:` block it used to be. FR-174 makes 19
# of those gates conditional, and the assertion below has to be exercised
# against states CI does not produce on demand — a deferred run where a gate
# ran anyway, a full run where one was skipped. A block inside ci.yml can only
# be tested by reading it. Here `scripts/qa/test-ci-tier.sh` drives it with
# synthetic inputs and observes what it does, which is the difference §4.4 draws
# between a proxy and an observation. Being a `*.sh` file also puts it inside
# `bash32-compat.rb` and the pipefail scanner, both of which glob `*.sh` and
# neither of which can see inside a workflow — a scope that was sufficient only
# while no workflow carried non-trivial shell.
#
# Bash 3.2: no associative arrays. macOS ships 3.2 and the gate above enforces
# it repository-wide; membership is a `case` over a newline-delimited list.
#
# Input is three environment variables, so the caller is a workflow step with no
# argument marshalling:
#   TIER     full | deferred
#   META     newline-delimited ids of the tier-conditional gates
#   OUTCOMES newline-delimited `id=outcome` pairs, one per gate
set -uo pipefail

fail=0
note() { printf '    ^ %s\n' "$1" >&2; }

# An unset or unrecognised tier is a failure, never a default. The workflow's
# tier step already fails closed to `full`; if something got past it, this job
# does not know what it was meant to run and must not say it passed.
case "${TIER:-}" in
  full | deferred) ;;
  *)
    echo "tier is '${TIER:-unset}', neither full nor deferred" >&2
    exit 1
    ;;
esac

# Normalised membership list: every id surrounded by newlines, so `case` can
# match a whole line and never a prefix. `cost-fixtures` must not match
# `cost-fixtures-extra`.
meta_list=$'\n'
meta_declared=0
while IFS= read -r raw; do
  id="$(printf '%s' "$raw" | tr -d '[:space:]')"
  [ -n "$id" ] || continue
  meta_list="${meta_list}${id}"$'\n'
  meta_declared=$((meta_declared + 1))
done <<EOF
${META:-}
EOF

is_meta() {
  case "$meta_list" in
    *$'\n'"$1"$'\n'*) return 0 ;;
  esac
  return 1
}

echo "meta-verification tier: $TIER"
echo ""

seen_list=$'\n'
meta_seen=0
total=0
while IFS= read -r line; do
  [ -n "$line" ] || continue
  gate="${line%%=*}"
  outcome="${line#*=}"
  gate="$(printf '%s' "$gate" | tr -d '[:space:]')"
  outcome="$(printf '%s' "$outcome" | tr -d '[:space:]')"
  [ -n "$gate" ] || continue
  total=$((total + 1))
  seen_list="${seen_list}${gate}"$'\n'

  if is_meta "$gate"; then
    meta_seen=$((meta_seen + 1))
    printf '%-38s %-9s meta\n' "$gate" "$outcome"
    if [ "$TIER" = "deferred" ]; then
      # A `success` here is as much a violation as a `failure`: it means the
      # step ran when the tier said it would not, so the condition did not do
      # what the run reported it did.
      if [ "$outcome" != "skipped" ]; then
        note "tier is deferred but this ran ('$outcome'); the condition did not hold"
        fail=1
      fi
    else
      if [ "$outcome" = "skipped" ]; then
        note "tier is full but this was skipped; meta-verification did not run"
        fail=1
      elif [ "$outcome" != "success" ]; then
        fail=1
      fi
    fi
  else
    printf '%-38s %s\n' "$gate" "$outcome"
    if [ "$outcome" != "success" ]; then
      note "gate did not pass"
      fail=1
    fi
  fi
done <<EOF
${OUTCOMES:-}
EOF

# Reading nothing is not passing. Without this the whole check is §4.4 shape 5:
# an empty or unset OUTCOMES prints a clean report and exits 0.
if [ "$total" -eq 0 ]; then
  echo "OUTCOMES named no gates; this job asserted nothing" >&2
  exit 1
fi

# Both directions of the roster. A META entry naming no gate drops that gate out
# of the tier assertion; a tier-conditional gate missing from META gets judged as
# if it were mandatory. Each is silent on its own.
while IFS= read -r m; do
  [ -n "$m" ] || continue
  case "$seen_list" in
    *$'\n'"$m"$'\n'*) ;;
    *)
      echo "META names '$m', which is not in OUTCOMES" >&2
      fail=1
      ;;
  esac
done <<EOF
$(printf '%s' "$meta_list")
EOF

if [ "$meta_seen" -ne "$meta_declared" ]; then
  echo "META declares $meta_declared gates but $meta_seen were read from OUTCOMES" >&2
  fail=1
fi

echo ""
if [ "$fail" -ne 0 ]; then
  echo "one or more governance gates failed; each is reported above and in its own step" >&2
  exit 1
fi
if [ "$TIER" = "deferred" ]; then
  echo "all $total governance gates passed; $meta_seen meta-verification gates deferred to nightly-governance.yml"
else
  echo "all $total governance gates passed, including $meta_seen meta-verification gates"
fi
