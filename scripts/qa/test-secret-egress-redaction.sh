#!/usr/bin/env bash

# FR-175: SecretStore values must not leave the daemon in cleartext.
#
# The unit tests in core assert this at the service boundary. This gate asserts
# it at the surface a user actually reaches: a real daemon, a real `apply`, and
# the real CLI. That distinction is the whole reason acceptance criterion 7
# exists — the ticket this FR came from was first probed at the pure-function
# layer, which is where it looked closed and was not.
#
# Every check asserts the pair — the secret is **absent** and the placeholder is
# **present**. Absence alone is satisfied by a regression that drops SecretStore
# from the output entirely, which is a different bug wearing this bug's colour.
#
# Isolation: its own ORCHESTRATORD_DATA_DIR and HOME under mktemp -d. It never
# reads or writes ~/.orchestratord. No Agent is applied and no task is created,
# so no provider binary is reachable from anything this starts.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

. "$REPO_ROOT/scripts/lib/gate_runlog.sh"
. "$REPO_ROOT/scripts/lib/gate_daemon.sh"

ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
PROJECT="qa-fr175-egress"
STORE="fr175-api-keys"
KEY="OPENAI_API_KEY"
# The value the daemon must hold and must never emit. Distinctive enough that a
# match anywhere is a match on this and not on incidental text.
SECRET="sk-fr175-egress-cleartext-sentinel"
PLACEHOLDER="[ENCRYPTED]"

PASS=0
FAIL=0
DAEMON_PID=""
SUMMARY_REACHED=0

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

for command in jq mktemp rg; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

if [[ ! -x "$ORCHD" || ! -x "$ORCH" ]]; then
  echo "debug binaries not found; run: cargo build -p orchestratord -p orchestrator-cli" >&2
  exit 1
fi

QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
# Everything a read surface produces goes in OUT and nothing else does, because
# OUT is what the sweep at the end reads. The fixture — which legitimately
# carries the secret — lives outside it, and so does the control file.
OUT="$QA_ROOT/out"
mkdir -p "$OUT"

# An abort before the summary line reads exactly like a completed run to anyone
# holding only the exit code, and `set -e` reaching into a helper is the usual
# way that happens. Three lines convert a silent truncation into a stated one.
cleanup() {
  local status=$?
  gate_daemon_stop "$DAEMON_PID" || true
  DAEMON_PID=""
  if [[ "$SUMMARY_REACHED" -eq 0 ]]; then
    echo "" >&2
    echo "TRUNCATED: this gate exited (status $status) before printing its summary line." >&2
    echo "  Nothing below the last PASS/FAIL above was evaluated; do not read this run as green." >&2
  fi
  if [[ "${KEEP_QA:-0}" == "1" ]]; then
    echo "QA_ROOT=$QA_ROOT" >&2
    echo "QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT
gate_runlog_arm "scripts/qa/test-secret-egress-redaction.sh"

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/runtime"
unset ORCHESTRATOR_SOCKET
unset ORCHESTRATOR_CONTROL_PLANE_CONFIG

FIXTURE="$QA_ROOT/secret-store.yaml"
cat > "$FIXTURE" <<EOF
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: $STORE
spec:
  data:
    $KEY: "$SECRET"
EOF

# `&` on its own statement, not appended to a `cd && ...` list: under bash the
# latter backgrounds the whole list and $! is the wrapper shell rather than the
# daemon, which is how two daemons leaked for 22 hours while their run reported
# all checks passed (CLAUDE.md, DD-174).
(
  cd "$QA_ROOT"
  "$ORCHD" --foreground --webhook-bind none --workers 1 \
    --uds-max-role admin > "$OUT/daemon.log" 2>&1 &
  echo $! > daemon.pid
)
DAEMON_PID="$(gate_daemon_pid_from_file "$QA_ROOT/daemon.pid")"
if ! gate_daemon_wait_ready "$ORCH"; then
  echo "isolated daemon failed to start" >&2
  sed 's/^/  /' "$OUT/daemon.log" >&2
  exit 1
fi

"$ORCH" apply --project "$PROJECT" -f "$FIXTURE" > "$QA_ROOT/apply.log" 2>&1

# ── Premise ─────────────────────────────────────────────────────────────────
#
# Everything below asserts that a value is absent from an output, and a daemon
# that never stored the value satisfies all of it. So first: prove the store is
# there and declares the key. Its cleartext value is deliberately unreachable
# through every read command — that is the property under test — so the premise
# is established from the store's presence, not from reading its value back.
"$ORCH" get "secretstore/$STORE" --project "$PROJECT" -o yaml > "$OUT/get-single.yaml" 2>&1
premise_named=0
premise_keyed=0
rg -q -F -- "$STORE" "$OUT/get-single.yaml" && premise_named=1
rg -q -F -- "$KEY" "$OUT/get-single.yaml" && premise_keyed=1
if [[ "$premise_named" -eq 1 && "$premise_keyed" -eq 1 ]]; then
  pass "premise: the applied SecretStore exists and declares $KEY"
else
  fail "premise: the fixture SecretStore did not apply (named=$premise_named keyed=$premise_keyed); every assertion below would pass vacuously"
  sed 's/^/    /' "$QA_ROOT/apply.log" >&2
fi

# ── The assertion ───────────────────────────────────────────────────────────
#
# `rg -q <file>` throughout, never `producer | rg -q`. Under `set -o pipefail` a
# reader that leaves on the first match kills the producer with EPIPE, and the
# same shape with the match on the failing branch reports a real violation as
# clean (FR-145/FR-146). Files, so there is no pipeline to short-circuit.
assert_redacted() {
  local surface="$1" file="$2"
  local secret_present=0 placeholder_present=0
  rg -q -F -- "$SECRET" "$file" && secret_present=1
  rg -q -F -- "$PLACEHOLDER" "$file" && placeholder_present=1

  if [[ "$secret_present" -eq 1 ]]; then
    fail "$surface emitted the SecretStore value in cleartext"
    return 0
  fi
  if [[ "$placeholder_present" -eq 0 ]]; then
    fail "$surface omits the value but carries no $PLACEHOLDER; an output that dropped the SecretStore entirely would satisfy absence alone"
    return 0
  fi
  pass "$surface redacts: value absent and $PLACEHOLDER present"
  return 0
}

# AC1 and AC2. Two formats, two assertions. They share `builtin_docs` today, and
# an assertion that leans on that is asserting an implementation detail.
"$ORCH" manifest export -o yaml > "$OUT/export.yaml" 2>&1
assert_redacted "manifest export -o yaml" "$OUT/export.yaml"

"$ORCH" manifest export -o json > "$OUT/export.json" 2>&1
if jq -e . "$OUT/export.json" >/dev/null 2>&1; then
  assert_redacted "manifest export -o json" "$OUT/export.json"
else
  fail "manifest export -o json did not produce parseable json; a text scan over unparseable output asserts nothing"
fi

# AC3.
"$ORCH" debug --component config > "$OUT/debug-config.txt" 2>&1
assert_redacted "debug --component config" "$OUT/debug-config.txt"

# The mirror specifically. `manifest export` renders the typed secret_stores map;
# this path renders the whole config, which also carries the resource_store copy
# `crd::writeback` keeps. Redacting one and not the other leaves a leak that a
# whole-file scan reports without saying which half broke, so the two renderings
# of the store are counted rather than merely detected.
MIRROR_COUNT="$(rg -c -F -- "$STORE" "$OUT/debug-config.txt" || true)"
MIRROR_COUNT="${MIRROR_COUNT:-0}"
if [[ "$MIRROR_COUNT" -ge 2 ]]; then
  pass "debug --component config renders the typed store and its resource_store mirror ($MIRROR_COUNT lines name $STORE)"
else
  fail "debug --component config names $STORE on $MIRROR_COUNT line(s); it must render both the typed map and the resource_store mirror, or the mirror's redaction is untested here"
fi

# AC4. The placeholder half is the load-bearing one: these components must not
# begin emitting config at all, so a *redacted* config appearing here is as much
# a regression as a cleartext one.
#
# Each carries a positive condition too. Three absences and nothing else are
# satisfied by a component that returned the empty string, which is a regression
# that reads as a pass — "emits no config" has to be distinguished from "emits
# nothing".
for component in state dag; do
  case "$component" in
    state) expected="Debug Information" ;;
    dag) expected="DAG Debug Information" ;;
  esac
  "$ORCH" debug --component "$component" > "$OUT/debug-$component.txt" 2>&1
  produced=0
  leaked=0
  emitted_config=0
  rg -q -F -- "$expected" "$OUT/debug-$component.txt" && produced=1
  rg -q -F -- "$SECRET" "$OUT/debug-$component.txt" && leaked=1
  rg -q -F -- "$PLACEHOLDER" "$OUT/debug-$component.txt" && emitted_config=1
  rg -q -F -- "$STORE" "$OUT/debug-$component.txt" && emitted_config=1
  if [[ "$produced" -eq 1 && "$leaked" -eq 0 && "$emitted_config" -eq 0 ]]; then
    pass "debug --component $component produces its own output and no config, redacted or otherwise"
  else
    fail "debug --component $component: produced=$produced secret=$leaked config=$emitted_config"
  fi
done

# AC5. FR-171 redacted these; this pins them against a regression introduced by
# the present change rather than re-testing FR-171's work.
assert_redacted "get secretstore/<name>" "$OUT/get-single.yaml"

"$ORCH" get secretstores --project "$PROJECT" -o yaml > "$OUT/get-list.yaml" 2>&1
listed_named=0
listed_leaked=0
rg -q -F -- "$STORE" "$OUT/get-list.yaml" && listed_named=1
rg -q -F -- "$SECRET" "$OUT/get-list.yaml" && listed_leaked=1
if [[ "$listed_named" -eq 1 && "$listed_leaked" -eq 0 ]]; then
  pass "get secretstores names the store without emitting its value"
else
  fail "get secretstores: named=$listed_named leaked=$listed_leaked"
fi

# AC6. Redaction and refusal are two halves of one contract: without the refusal
# a redacted export applied back would overwrite real secrets with the literal
# placeholder. Three conditions, because an exit code cannot say which branch
# refused.
apply_back_status=0
"$ORCH" apply --project "$PROJECT" -f "$OUT/export.yaml" \
  > "$OUT/apply-back.log" 2>&1 || apply_back_status=$?
named_code=0
named_key=0
rg -q -F -- "secret_value_placeholder_rejected" "$OUT/apply-back.log" && named_code=1
rg -q -F -- "$KEY" "$OUT/apply-back.log" && named_key=1
if [[ "$apply_back_status" -ne 0 && "$named_code" -eq 1 && "$named_key" -eq 1 ]]; then
  pass "re-applying the redacted export is refused by name, and the diagnostic names $KEY"
else
  fail "re-applying the redacted export: status=$apply_back_status code_named=$named_code key_named=$named_key"
  sed 's/^/    /' "$OUT/apply-back.log" >&2
fi

# ...and the store survived the refusal rather than being overwritten with the
# placeholder. A refusal that had already written is not a refusal.
"$ORCH" get "secretstore/$STORE" --project "$PROJECT" -o yaml > "$OUT/get-after.yaml" 2>&1
after_keyed=0
after_placeholder=0
rg -q -F -- "$KEY" "$OUT/get-after.yaml" && after_keyed=1
rg -q -F -- "$PLACEHOLDER" "$OUT/get-after.yaml" && after_placeholder=1
if [[ "$after_keyed" -eq 1 && "$after_placeholder" -eq 1 ]]; then
  pass "the refused apply left the store in place"
else
  fail "the store did not survive the refused apply (keyed=$after_keyed placeholder=$after_placeholder)"
fi

# ── The sweep ───────────────────────────────────────────────────────────────
#
# Every check above names a surface it knew to look at, which is the enumeration
# defect this FR was filed against: the next egress lands outside the list and
# nothing says so. This reads everything the run produced — including the
# daemon's own log — and needs no edit when a surface is added, because whatever
# a later check writes into OUT is swept with the rest.
#
# The control comes first. A sweep that cannot see a secret it is standing on
# reports clean for the same reason a sweep over a genuinely clean tree does.
CONTROL="$QA_ROOT/control"
mkdir -p "$CONTROL"
printf 'planted: %s\n' "$SECRET" > "$CONTROL/planted.txt"
if rg -l -F -- "$SECRET" "$CONTROL" >/dev/null 2>&1; then
  pass "sweep control: the sweep finds a planted secret, so a clean result below means something"
else
  fail "sweep control: the sweep did not find a secret planted directly in front of it; the result below proves nothing"
fi

SWEEP_HITS="$QA_ROOT/sweep-hits.txt"
sweep_status=0
rg -l -F -- "$SECRET" "$OUT" > "$SWEEP_HITS" 2>/dev/null || sweep_status=$?
case "$sweep_status" in
  0)
    fail "the secret reached $(wc -l < "$SWEEP_HITS" | tr -d ' ') file(s) produced by this run:"
    sed 's/^/    /' "$SWEEP_HITS" >&2
    ;;
  1)
    # rg exits 1 on no matches and 2 on error. Distinguished, because "found
    # nothing" and "could not look" are the same colour to a bare status check.
    pass "no output this run produced carries the secret, daemon.log included"
    ;;
  *)
    fail "the sweep could not run (rg exit $sweep_status); it asserted nothing"
    ;;
esac

SUMMARY_REACHED=1
echo ""
echo "Secret egress redaction QA: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
