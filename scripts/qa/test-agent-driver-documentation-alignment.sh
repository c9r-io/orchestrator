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

for command in jq rg; do
  command -v "$command" >/dev/null 2>&1 || fail "missing required command: $command"
done

cd "$REPO_ROOT"

STALE_PATTERN='executor:[[:space:]]*shell.*(default|默认).*(\||｜)[[:space:]]*streaming|When a step runs under the `streaming` runner executor|当步骤在 `streaming` 运行器下执行时|deprecated global streaming executor calls a provider-owned compatibility bridge|global streaming executor and CEL path remain compatibility bridges|Legacy command Agents use the default shell executor|keep a legacy shell Agent|reassign the workflow capability to a legacy command Agent|保留旧 command Agent一个发布周期|保留旧 command Agent 一个发布周期'
ALIGNMENT_TARGETS=(
  docs/guide/02-resource-model.md
  docs/guide/zh/02-resource-model.md
  docs/guide/04-cel-prehooks.md
  docs/guide/zh/04-cel-prehooks.md
  docs/guide/agent-driver-model.md
  docs/architecture.md
  docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md
  docs/design_doc/orchestrator/127-agent-driver-abstraction.md
  .claude/skills/orchestrator-guide/SKILL.md
  .claude/skills/orchestrator-guide/references/resource-and-steps.md
)

if rg -n -i "$STALE_PATTERN" "${ALIGNMENT_TARGETS[@]}"; then
  fail "retired runner or command-only authoring guidance remains"
fi
pass "retired runner and command-only authoring phrases are absent"

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
    >"$fixture_file"
  if ! rg -q -i "$STALE_PATTERN" "$fixture_file"; then
    fail "negative fixture did not trigger retired-semantics detector"
  fi
  pass "negative fixture proves retired semantics fail closed"
fi

echo "Agent driver documentation alignment: $PASS passed, 0 failed"
