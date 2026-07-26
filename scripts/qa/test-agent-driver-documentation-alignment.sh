#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FIXTURE_TEST=0
PASS=0

if [[ "${1:-}" == "--fixture-test" ]]; then
  FIXTURE_TEST=1
elif [[ $# -gt 0 ]]; then
  echo "usage: $0 [--fixture-test]" >&2
  exit 2
fi

pass() {
  echo "  PASS: $1"
  PASS=$((PASS + 1))
}

fail() {
  echo "  FAIL: $1" >&2
  exit 1
}

for command in git jq rg; do
  command -v "$command" >/dev/null 2>&1 || fail "missing required command: $command"
done

cd "$REPO_ROOT"

STALE_PATTERN='executor:[[:space:]]*shell.*(default|默认).*(\||｜)[[:space:]]*streaming|When a step runs under the `streaming` runner executor|当步骤在 `streaming` 运行器下执行时|deprecated global streaming executor|global streaming executor and CEL path remain compatibility bridges|Legacy command Agents use the default shell executor|keep a legacy shell Agent|reassign the workflow capability to a legacy command Agent|保留旧 command Agent一个发布周期|保留旧 command Agent 一个发布周期|The `streaming` executor drives `claude`|End-to-end demonstration of the streaming-runner pivot'

# Every tracked Markdown surface is scanned by default. A target list enumerated
# by hand only ever covers the drift already discovered, so exemptions are
# subtracted here and each one must carry its reason. Released CHANGELOG
# sections and design records that fence themselves with an explicit historical
# banner do not need an exemption: their wording is not a stale claim.
EXEMPT_PATTERN='^$' # no exempted surfaces
ALIGNMENT_TARGETS=()
while IFS= read -r doc; do
  ALIGNMENT_TARGETS+=("$doc")
done < <(git ls-files '*.md' | grep -Ev "$EXEMPT_PATTERN")
[[ ${#ALIGNMENT_TARGETS[@]} -gt 0 ]] || fail "no Markdown surfaces resolved for alignment scan"

if rg -n -i "$STALE_PATTERN" ${ALIGNMENT_TARGETS[@]+"${ALIGNMENT_TARGETS[@]}"}; then
  fail "retired runner or command-only authoring guidance remains"
fi
pass "retired runner and command-only authoring phrases are absent from every tracked Markdown surface"

rg -q 'legacy_runner_executor_removed' docs/guide/02-resource-model.md ||
  fail "English resource guide omits the streaming executor rejection"
rg -q 'legacy_runner_executor_removed' docs/guide/zh/02-resource-model.md ||
  fail "Chinese resource guide omits the streaming executor rejection"
rg -q 'parse-only compatibility field' docs/guide/02-resource-model.md ||
  fail "English resource guide omits parse-only executor semantics"
rg -q 'parse-only 兼容字段' docs/guide/zh/02-resource-model.md ||
  fail "Chinese resource guide omits parse-only executor semantics"
pass "EN/ZH resource guides describe parse-only shell and streaming rejection"

for guide in docs/guide/04-cel-prehooks.md docs/guide/zh/04-cel-prehooks.md; do
  rg -q 'driver_terminal' "$guide" ||
    fail "$guide does not bind run signals to driver_terminal"
  rg -q 'typed.driver|Typed.Driver' "$guide" ||
    fail "$guide does not identify typed driver signals"
done
pass "EN/ZH CEL guides bind structured signals to typed driver artifacts"

rg -q 'legacy_agent_command_deprecated' docs/guide/agent-driver-model.md ||
  fail "Agent driver guide omits command-only promotion warning"
rg -q 'explicit `shell/cli`' docs/guide/agent-driver-model.md ||
  fail "Agent driver guide omits explicit shell rollback"
rg -q 'legacy_agent_command_deprecated' docs/architecture.md ||
  fail "architecture omits compatibility promotion"
rg -q 'legacy_agent_execution_removed' docs/architecture.md ||
  fail "architecture omits scheduler fail-closed boundary"
rg -q -U 'command:.*\n[[:space:]]+driver:\n[[:space:]]+provider: shell' \
  .claude/skills/orchestrator-guide/SKILL.md ||
  fail "orchestrator-guide minimal Agent is not explicit shell/cli"
rg -q 'legacy_agent_command_deprecated' \
  .claude/skills/orchestrator-guide/references/resource-and-steps.md ||
  fail "orchestrator-guide reference omits compatibility warning"
pass "authoring guides and architecture recommend explicit drivers"

rg -q 'deleted the global streaming executor' \
  docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md ||
  fail "DD-101 does not identify the executor as deleted"
rg -q 'historical pivot and are not current configuration guidance' \
  docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md ||
  fail "DD-101 does not fence historical configuration"
rg -q 'There is no global streaming executor or provider-owned compatibility bridge' \
  docs/design_doc/orchestrator/127-agent-driver-abstraction.md ||
  fail "DD-127 still exposes the removed compatibility bridge"
pass "released design records distinguish history from current execution"

SHOWCASE=docs/showcases/streaming-mark-done-convergence.md
for guide in docs/guide/04-cel-prehooks.md docs/guide/zh/04-cel-prehooks.md; do
  rg -q 'docs/showcases/streaming-mark-done-convergence\.md' "$guide" ||
    fail "$guide does not link the governed typed-driver showcase"
done
test -f "$SHOWCASE" || fail "linked typed-driver showcase is missing"
pass "EN/ZH CEL guides resolve to the governed mark-done showcase"

for term in 'claude/cli' 'driver_tool_use' 'driver_tool_result' 'driver_terminal'; do
  rg -q "$term" "$SHOWCASE" ||
    fail "typed-driver showcase omits current semantic: $term"
done
rg -q 'global `streaming` executor and its compatibility bridge have been removed' \
  "$SHOWCASE" ||
  fail "typed-driver showcase does not fence the removed execution seam"
pass "mark-done showcase describes only current typed-driver execution"

jq -e '
  .executionCases[]
  | select(.name == "new-command-only-agent-is-rejected")
  | .evaluationLayer == "production-manifest-governance"
    and (.rationale | length > 0)
    and .expectedAccepted == false
    and .runtimeCompatibility == {
      "accepted": true,
      "warningCode": "legacy_agent_command_deprecated",
      "persistedDriver": "shell/cli"
    }
' scripts/qa/fixtures/coordination-governance-cases.json >/dev/null ||
  fail "command-only governance fixture does not document both decision layers"
rg -q 'def production_execution_document_accepted?' \
  scripts/qa/coordination-governance.rb ||
  fail "governance helper is not scoped to production admission"
if rg -q 'def execution_document_accepted?' scripts/qa/coordination-governance.rb; then
  fail "ambiguous execution_document_accepted? helper remains"
fi
pass "governance fixtures distinguish production admission from runtime compatibility"

UNRELEASED="$(awk '/^## \[Unreleased\]/{flag=1;next} /^## \[/{flag=0} flag' CHANGELOG.md)"
printf '%s' "$UNRELEASED" | rg -q '^### Removed[[:space:]]*$' ||
  fail "CHANGELOG [Unreleased] does not record the retirement under ### Removed"
printf '%s' "$UNRELEASED" | rg -q 'RunnerExecutorKind' ||
  fail "CHANGELOG [Unreleased] does not name the removed runner selection seam"
printf '%s' "$UNRELEASED" | rg -q 'legacy_runner_executor_removed' ||
  fail "CHANGELOG [Unreleased] does not record the breaking manifest rejection"
printf '%s' "$UNRELEASED" | rg -q 'legacy_agent_command_deprecated' ||
  fail "CHANGELOG [Unreleased] does not record the command-only compatibility window"
pass "CHANGELOG records the retirement and its breaking manifest change"

rg -q 'legacy_runner_executor_removed' core/src crates --glob '*.rs' ||
  fail "documented runner rejection code is absent from source"
rg -q 'legacy_agent_command_deprecated' core/src crates --glob '*.rs' ||
  fail "documented command-only warning code is absent from source"
rg -q 'legacy_agent_execution_removed' core/src crates --glob '*.rs' ||
  fail "documented scheduler rejection code is absent from source"
pass "documented stable diagnostics exist in production source"

if [[ "$FIXTURE_TEST" == "1" ]]; then
  fixture_file="$(mktemp)"
  trap 'rm -f "$fixture_file"' EXIT
  printf '%s\n' \
    'executor: shell # shell (default) | streaming' \
    'When a step runs under the `streaming` runner executor' \
    'The deprecated global streaming executor calls a provider-owned compatibility bridge while manifests migrate.' \
    '- Explicit driver phases use setup; the deprecated global streaming executor is now a provider-owned compatibility bridge while legacy manifests migrate' \
    'End-to-end demonstration of the streaming-runner pivot' \
    'The `streaming` executor drives `claude` and hosts the mark_done tool.' \
    >"$fixture_file"
  if ! rg -q -i "$STALE_PATTERN" "$fixture_file"; then
    fail "negative fixture did not trigger retired-semantics detector"
  fi
  pass "negative fixture proves retired semantics fail closed"
fi

echo "Agent driver documentation alignment: $PASS passed, 0 failed"
