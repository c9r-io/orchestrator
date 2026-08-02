#!/usr/bin/env bash
#
# FR-156: the manifest-level pipeline-variable authoring surface is retired.
#
# Two modes:
#
#   --capture-baseline   run the PRE-migration shape of each of the two
#                        production store_inputs consumers and record its
#                        observable contract into the committed baseline. Run
#                        once, on the tree before the removal.
#   (default)            replay each object's POST-migration shape and compare
#                        against its own recorded baseline, per object; then
#                        assert the retired kinds are rejected at apply.
#
# Structural evidence is not enough for a removal FR (fr-governance §4.3):
# counting consumers proves the old path is gone, not that the new one works.
# Hence a recorded baseline, a per-object comparison, and — for the object whose
# whole purpose is a conditional — both branches of that conditional.
#
# Self-referential safety: this gate never touches the developer's daemon,
# database or config. It starts its own daemon on port 19327 with its own
# ORCHESTRATORD_DATA_DIR and HOME under mktemp, and removes both on success.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORCHD="${ORCHD:-$REPO_ROOT/target/debug/orchestratord}"
ORCH="${ORCH:-$REPO_ROOT/target/debug/orchestrator}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1:19327}"
FIXTURE="$REPO_ROOT/fixtures/manifests/bundles/fr156-pipeline-variable-parity.yaml"
BASELINE="$REPO_ROOT/fixtures/qa/fr156-pipeline-variable-baseline.json"
QA_ROOT="$(mktemp -d)"
QA_HOME="$(mktemp -d)"
DAEMON_PID=""
PASS=0
FAIL=0
MODE="verify"

pass() { echo "  PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "  FAIL: $1" >&2; FAIL=$((FAIL + 1)); }

# shellcheck source=../lib/gate_fixture.sh
. "$REPO_ROOT/scripts/lib/gate_fixture.sh"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --capture-baseline) MODE="capture" ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

summary() {
  echo "pipeline variable retirement QA: $PASS passed, $FAIL failed"
}

cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  if [[ "$FAIL" -gt 0 || "${KEEP_FR156_QA:-0}" == "1" ]]; then
    echo "FR-156 QA retained at QA_ROOT=$QA_ROOT QA_HOME=$QA_HOME" >&2
  else
    rm -rf "$QA_ROOT" "$QA_HOME"
  fi
}
trap cleanup EXIT

for command in git jq mktemp rg ruby sqlite3; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

cd "$REPO_ROOT"
if [[ "${FR156_ALLOW_DIRTY:-0}" != "1" && -n "$(git status --porcelain)" ]]; then
  echo "FR-156 QA requires a clean worktree (or FR156_ALLOW_DIRTY=1)" >&2
  git status --short >&2
  exit 1
fi

# ── Split the parity bundle ──────────────────────────────────────────
#
# The bundle deliberately mixes shapes the product accepts with shapes it must
# reject, so it never applies whole. Selection is by an explicit name list, not
# by a suffix in a document this gate does not own: a renamed workflow must be a
# failed premise, not a silently empty file that everything downstream reads as
# "nothing to check" (FR-143).
split_workflows() {
  local label="$1" output="$2"
  shift 2
  fixture_produce "$label" "$output" ruby -ryaml -e '
    source, output, *keep = ARGV
    documents = YAML.load_stream(File.read(source)).compact
    workflows = documents.select { |d| d["kind"] == "Workflow" }
                         .map { |d| d.dig("metadata", "name") }
    missing = keep - workflows
    unless missing.empty?
      abort "parity fixture no longer defines: #{missing.join(", ")}"
    end
    selected = documents.select do |document|
      document["kind"] != "Workflow" ||
        keep.include?(document.dig("metadata", "name"))
    end
    File.open(output, "w") do |file|
      selected.each_with_index do |document, index|
        file.write("---\n") unless index.zero?
        file.write(YAML.dump(document).sub(/\A---\s*\n/, ""))
      end
    end
  ' "$FIXTURE" "$output" "$@"
}

if [[ "$MODE" == "capture" ]]; then
  OBJECT_SUFFIX="legacy"
else
  OBJECT_SUFFIX="migrated"
fi
APPLY_FIXTURE="$QA_ROOT/fr156-objects.yaml"
if ! split_workflows "parity objects ($OBJECT_SUFFIX)" "$APPLY_FIXTURE" \
  "fr156-gather-updates-$OBJECT_SUFFIX" "fr156-apply-winner-$OBJECT_SUFFIX"; then
  summary >&2
  exit 1
fi

# ── Isolated daemon ──────────────────────────────────────────────────

export HOME="$QA_HOME"
export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"
unset ORCHESTRATOR_SOCKET
export ORCHESTRATOR_CONTROL_PLANE_CONFIG="$QA_HOME/.orchestrator/control-plane/config.yaml"
mkdir -p "$QA_ROOT/workspace/docs/qa" "$QA_ROOT/workspace/docs/ticket" "$QA_ROOT/bin"
# The migrated commands call `orchestrator` by name, exactly as a workflow
# author would. Putting the build under test first on PATH is what makes the
# migrated shape reachable at all.
ln -sf "$ORCH" "$QA_ROOT/bin/orchestrator"
export PATH="$QA_ROOT/bin:$PATH"

(
  cd "$QA_ROOT/workspace"
  git init -q
  git config user.email qa@example.invalid
  git config user.name "FR-156 QA"
  printf 'seed\n' > seed.txt
  git add .
  git commit -qm "first commit"
  printf 'second\n' > second.txt
  git add .
  git commit -qm "second commit"
  printf 'third\n' > third.txt
  git add .
  git commit -qm "third commit"
  "$ORCHD" --foreground --bind "$BIND_ADDR" --workers 1 --webhook-bind none \
    --uds-max-role admin > "$QA_ROOT/daemon.log" 2>&1 &
  echo $! > "$QA_ROOT/daemon.pid"
)
DAEMON_PID="$(<"$QA_ROOT/daemon.pid")"
for _ in {1..80}; do
  "$ORCH" task list -o json >/dev/null 2>&1 && break
  sleep 0.25
done
if ! "$ORCH" task list -o json >/dev/null 2>&1; then
  sed -n '1,260p' "$QA_ROOT/daemon.log" >&2
  fail "isolated daemon did not become ready"
  summary >&2
  exit 1
fi

PROJECT="qa-fr156"
FIRST_SHA="$(cd "$QA_ROOT/workspace" && git rev-parse HEAD~2)"
WINNER_JSON='{"winner_id":"item-7","eliminated_ids":["item-3"],"winner_vars":{"score":"0.91"}}'

if ! (cd "$QA_ROOT/workspace" && "$ORCH" apply --project "$PROJECT" \
      -f "$APPLY_FIXTURE" > "$QA_ROOT/apply.out" 2>&1); then
  cat "$QA_ROOT/apply.out" >&2
  fail "parity objects ($OBJECT_SUFFIX) did not apply"
  summary >&2
  exit 1
fi
pass "parity objects ($OBJECT_SUFFIX) apply"

# ── Run one object and record what it observably did ─────────────────

run_object() {
  local workflow="$1" created_ids task_id status exit_code stdout
  created_ids="$(
    cd "$QA_ROOT/workspace"
    "$ORCH" task create --project "$PROJECT" --workspace fr156-ws \
      --workflow "$workflow" --goal "FR-156 per-object parity" \
      --name "$workflow" --no-start | rg -o '[0-9a-f-]{36}'
  )"
  task_id="${created_ids%%$'\n'*}"
  "$ORCH" task start "$task_id" >/dev/null
  status="pending"
  for _ in {1..240}; do
    status="$("$ORCH" task info "$task_id" -o json | jq -r '.task.status')"
    [[ "$status" =~ ^(completed|failed|cancelled)$ ]] && break
    sleep 0.25
  done
  # The step's real stdout, off disk, plus the exit code the scheduler recorded.
  #
  # The events table carries only a truncated `command_preview`, never the
  # output. A recording built from it would have been a string of empty stdouts
  # that compares equal to every later run -- the check that passes having read
  # no input (§4.4 shape 5). The daemon writes the true stream to
  # $ORCHESTRATORD_DATA_DIR/logs/<task_id>/<step>_<run_id>.stdout.
  local log_dir="$QA_ROOT/data/logs/$task_id"
  if [[ ! -d "$log_dir" ]]; then
    fail "$workflow left no step log directory; nothing to record"
    return 1
  fi
  stdout="$(cat "$log_dir"/*.stdout)"
  if [[ -z "$stdout" ]]; then
    fail "$workflow produced no stdout; an empty recording would match anything"
    return 1
  fi
  exit_code="$(sqlite3 "$QA_ROOT/data/agent_orchestrator.db" \
    "SELECT COALESCE(json_extract(payload_json, '\$.exit_code'), -1) FROM events
     WHERE task_id='$task_id' AND event_type='step_finished'
       AND json_extract(payload_json, '\$.step_id') != 'loop_guard'
     ORDER BY id LIMIT 1;")"
  jq -nc --arg terminal "$status" --arg stdout "$stdout" \
    --argjson exit_code "${exit_code:--1}" \
    '{terminal:$terminal, exit_code:$exit_code, stdout:$stdout}'
}

# Normalise the volatile parts of a recording: the git SHAs and the task ids are
# regenerated on every run and are not part of any contract. Everything else --
# the branch header, the number of log lines, the winner payload -- is.
normalise() {
  jq -c '
    .stdout |= (
      gsub("[0-9a-f]{40}"; "<sha40>")
      | gsub("[0-9a-f]{7,8}(?=[ )])"; "<sha-short>")
      | gsub("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"; "<uuid>")
    )
  '
}

store_put() {
  "$ORCH" store put "$1" "$2" "$3" --project "$PROJECT" >/dev/null
}

store_delete() {
  "$ORCH" store delete "$1" "$2" --project "$PROJECT" >/dev/null 2>&1 || true
}

# Object A has a conditional at its heart -- "changes since the last promotion"
# versus "recent changes when there is none". A recording of only the populated
# branch would let the empty branch break silently, so both are cases.
record_object_a_present() {
  store_put promotion last_published_sha "\"$FIRST_SHA\""
  run_object "fr156-gather-updates-$OBJECT_SUFFIX" | normalise
}

record_object_a_absent() {
  store_delete promotion last_published_sha
  run_object "fr156-gather-updates-$OBJECT_SUFFIX" | normalise
}

record_object_b() {
  store_put evolution winner_latest "$WINNER_JSON"
  run_object "fr156-apply-winner-$OBJECT_SUFFIX" | normalise
}

OBJECT_A_PRESENT="$(record_object_a_present)"
OBJECT_A_ABSENT="$(record_object_a_absent)"
OBJECT_B="$(record_object_b)"

if [[ "$MODE" == "capture" ]]; then
  mkdir -p "$(dirname "$BASELINE")"
  # Whether the old path could tell its two branches apart is derived from the
  # two recordings, not asserted by whoever ran the capture. Verify mode reads
  # this field rather than a restated expectation, so if the old path turns out
  # to have worked, the post-migration check tightens to equality on its own
  # (§4.4 shape 7: derive the expected value, never restate it).
  BRANCHES_DISTINGUISHABLE=true
  if [[ "$(jq -r '.stdout' <<<"$OBJECT_A_PRESENT")" == \
        "$(jq -r '.stdout' <<<"$OBJECT_A_ABSENT")" ]]; then
    BRANCHES_DISTINGUISHABLE=false
  fi
  jq -n \
    --arg schemaVersion 1 \
    --arg revision "$(git rev-parse HEAD)" \
    --argjson a_present "$OBJECT_A_PRESENT" \
    --argjson a_absent "$OBJECT_A_ABSENT" \
    --argjson b "$OBJECT_B" \
    --argjson distinguishable "$BRANCHES_DISTINGUISHABLE" \
    '{
       schemaVersion: ($schemaVersion|tonumber),
       capturedAtRevision: $revision,
       note: "Pre-migration contract of the two production store_inputs consumers. Recorded by scripts/qa/test-pipeline-variable-retirement.sh --capture-baseline before FR-156 removed the binding. Replayed per object by the same script in verify mode.",
       objects: {
         "promotion#gather_updates": {
           "branchesDistinguishable": $distinguishable,
           "branchesDistinguishableNote": "Derived by comparing the two recordings below, not asserted. False means the old path produced identical output whether or not promotion/last_published_sha was set: both {last_published_sha} occurrences in the command substitute, so the guard [ \"$LAST_SHA\" != \"{last_published_sha}\" ] compares the value to itself and the scoped-log branch is unreachable. Verify mode requires the migrated step to distinguish them.",
           "store-key-present": $a_present,
           "store-key-absent": $a_absent
         },
         "self-evolution#evo_apply_winner": $b
       }
     }' > "$BASELINE"
  if [[ "$BRANCHES_DISTINGUISHABLE" == "false" ]]; then
    pass "recorded that the pre-migration step could not distinguish its two branches"
  else
    pass "recorded that the pre-migration step distinguished its two branches"
  fi
  pass "recorded pre-migration baseline at $(git rev-parse --short HEAD)"
  summary
  exit 0
fi

# ── Per-object comparison against the recorded baseline ──────────────

if [[ ! -f "$BASELINE" ]]; then
  fail "no recorded baseline at $BASELINE; run --capture-baseline before the removal"
  summary >&2
  exit 1
fi

compare_object() {
  local label="$1" observed="$2" expected
  expected="$(jq -c --arg path "$3" --arg branch "$4" '
    if $branch == "" then .objects[$path] else .objects[$path][$branch] end
  ' "$BASELINE")"
  if [[ "$expected" == "null" || -z "$expected" ]]; then
    fail "$label: the baseline has no recording for it"
    return
  fi
  if [[ "$expected" == "$observed" ]]; then
    pass "$label matches its pre-migration baseline"
  else
    echo "    expected: $expected" >&2
    echo "    observed: $observed" >&2
    fail "$label diverged from its pre-migration baseline"
  fi
}

BASELINE_DISTINGUISHABLE="$(jq -r \
  '.objects["promotion#gather_updates"].branchesDistinguishable' "$BASELINE")"

# The fallback branch must be byte-identical: it is the branch the old path
# actually took, so it is the one a regression would show up in.
compare_object "promotion#gather_updates (store key absent)" \
  "$OBJECT_A_ABSENT" "promotion#gather_updates" "store-key-absent"
compare_object "self-evolution#evo_apply_winner" \
  "$OBJECT_B" "self-evolution#evo_apply_winner" ""

# The populated branch is handled by whichever rule the recording earns. If the
# old path could distinguish its branches, the migrated one must match it
# exactly. If it could not -- which is what the capture found -- then matching
# the baseline would mean reproducing the defect, so the requirement inverts.
if [[ "$BASELINE_DISTINGUISHABLE" == "true" ]]; then
  compare_object "promotion#gather_updates (store key present)" \
    "$OBJECT_A_PRESENT" "promotion#gather_updates" "store-key-present"
elif [[ "$(jq -r '.stdout' <<<"$OBJECT_A_PRESENT")" == \
        "$(jq -r '.stdout' <<<"$OBJECT_A_ABSENT")" ]]; then
  fail "promotion#gather_updates still cannot distinguish its two branches"
else
  pass "promotion#gather_updates now distinguishes its two branches, which the baseline records it could not"
fi

# ── End-to-end behaviour, independent of the baseline ────────────────
#
# A per-object comparison says the migrated step still does what it used to. It
# does not say that what it used to do was the thing the step exists for. These
# two assert the behaviour directly, so a baseline recorded over an already
# broken step cannot certify itself.

if rg -q '^winner-data:\{"winner_id":"item-7"' <<<"$(jq -r '.stdout' <<<"$OBJECT_B")"; then
  pass "the migrated winner step still receives the stored winner payload"
else
  jq -r '.stdout' <<<"$OBJECT_B" >&2
  fail "the migrated winner step did not receive the stored winner payload"
fi

A_PRESENT_STDOUT="$(jq -r '.stdout' <<<"$OBJECT_A_PRESENT")"
A_ABSENT_STDOUT="$(jq -r '.stdout' <<<"$OBJECT_A_ABSENT")"
if rg -q 'Changes since last promotion' <<<"$A_PRESENT_STDOUT" &&
   [[ "$(rg -c '^[0-9a-f<]' <<<"$A_PRESENT_STDOUT" || true)" -eq 2 ]]; then
  pass "with the key present the migrated step logs exactly the two commits after it"
else
  echo "$A_PRESENT_STDOUT" >&2
  fail "the migrated step did not scope the log to the stored SHA"
fi
if rg -q 'no prior promotion recorded' <<<"$A_ABSENT_STDOUT" &&
   [[ "$(rg -c '^[0-9a-f<]' <<<"$A_ABSENT_STDOUT" || true)" -eq 3 ]]; then
  pass "with the key absent the migrated step falls back to the full recent log"
else
  echo "$A_ABSENT_STDOUT" >&2
  fail "the migrated step did not fall back when the key is absent"
fi

# ── The retired kinds are rejected, and the diagnostic names the field ──
#
# One case per kind, each isolating a single field, so a diagnostic that named
# the wrong field would fail rather than be absorbed by a shared assertion.
# The subject is the diagnostic text, never the exit code: an exit code cannot
# tell which branch a validator failed through (§4.4 shape 7).
assert_rejected() {
  local workflow="$1" expected="$2" candidate="$QA_ROOT/reject-$workflow.yaml"
  if ! split_workflows "rejection fixture $workflow" "$candidate" "$workflow"; then
    return
  fi
  if "$ORCH" manifest validate -f "$candidate" > "$QA_ROOT/reject-$workflow.out" 2>&1; then
    cat "$QA_ROOT/reject-$workflow.out" >&2
    fail "$workflow was accepted; the retired kind is still authorable"
    return
  fi
  if rg -qF "$expected" "$QA_ROOT/reject-$workflow.out"; then
    pass "$workflow is rejected and the diagnostic names its field"
  else
    cat "$QA_ROOT/reject-$workflow.out" >&2
    fail "$workflow was rejected without the diagnostic naming its field"
  fi
}

assert_rejected fr156-gather-updates-legacy \
  "[legacy_pipeline_variables_removed] workflow 'fr156-gather-updates-legacy' step 'gather_updates' uses store_inputs"
assert_rejected fr156-store-outputs-legacy \
  "[legacy_pipeline_variables_removed] workflow 'fr156-store-outputs-legacy' step 'plan' uses store_outputs"
assert_rejected fr156-step-vars-legacy \
  "[legacy_pipeline_variables_removed] workflow 'fr156-step-vars-legacy' step 'plan' uses step_vars"
assert_rejected fr156-store-put-legacy \
  "[legacy_pipeline_variables_removed] workflow 'fr156-store-put-legacy' step 'plan' uses a store_put post-action"

# ── The ledger says what the tree says ───────────────────────────────

LEDGER="$REPO_ROOT/config/governance/coordination-collapse-ledger.json"
LEDGER_STATE="$(jq -r '.consumerInventory.pipelineVariables.state' "$LEDGER")"
LEDGER_COUNT="$(jq -r '.consumerInventory.pipelineVariables.productionConsumerCount' "$LEDGER")"
if [[ "$LEDGER_STATE" == "removed" && "$LEDGER_COUNT" == "0" ]]; then
  pass "the ledger records the manifest surface as removed with zero consumers"
else
  fail "the ledger still reads state=$LEDGER_STATE count=$LEDGER_COUNT"
fi

# The retained carrier is the half of this FR that is a decision rather than a
# deletion, so it is asserted rather than left to prose. A ledger that dropped
# it would read as though PipelineVariables were still outstanding debt.
if [[ "$(jq -r '.consumerInventory.pipelineVariables.retainedCarrier.type' "$LEDGER")" == "PipelineVariables" ]]; then
  pass "the ledger records PipelineVariables as a retained carrier, not arrears"
else
  fail "the ledger does not record the retained carrier"
fi

if [[ "$FAIL" -ne 0 ]]; then
  sed -n '1,360p' "$QA_ROOT/daemon.log" >&2
  summary >&2
  exit 1
fi
summary
