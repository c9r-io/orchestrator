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

# FR-173 deleted runner.executor and the command-only promotion. These four
# assertions used to require the guides to name the [legacy_*] codes that
# announced them; naming a retired code is the drift this gate exists to catch,
# so they now require the two statements a reader still needs — the field is
# gone and a manifest carrying it is refused by name, and a command without a
# driver is refused rather than promoted. The second pair is not redundant with
# STALE_PATTERN above: that scan removes the old sentence, this one requires a
# replacement, and a guide that simply deleted the paragraph would satisfy the
# scan while telling an author nothing.
rg -q 'runner.executor` no longer exists' docs/guide/02-resource-model.md ||
  fail "English resource guide does not state that runner.executor is gone"
rg -q 'runner.executor` 已不存在' docs/guide/zh/02-resource-model.md ||
  fail "Chinese resource guide does not state that runner.executor is gone"
rg -q 'parse-only compatibility field' docs/guide/02-resource-model.md ||
  fail "English resource guide omits parse-only executor semantics"
rg -q 'parse-only 兼容字段' docs/guide/zh/02-resource-model.md ||
  fail "Chinese resource guide omits parse-only executor semantics"
pass "EN/ZH resource guides state that runner.executor was removed and why"

rg -q 'refused, not promoted' docs/guide/02-resource-model.md ||
  fail "English resource guide still describes command-only promotion"
rg -q '会被拒绝，不再被提升' docs/guide/zh/02-resource-model.md ||
  fail "Chinese resource guide still describes command-only promotion"
pass "EN/ZH resource guides describe the command-only Agent as refused"

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

# The FR-126 retirement was recorded under [Unreleased] until the 0.4.0 cut
# (FR-151) moved it, permanently, into the [0.4.0] section. The assertion's
# subject is "the CHANGELOG records the retirement", so the extraction names
# both the pre-release home and the release that shipped it — never "the
# newest section", which would silently stop covering the facts at 0.5.0.
RETIREMENT_SECTIONS="$(awk '/^## \[(Unreleased|0\.4\.0)\]/{flag=1;next} /^## \[/{flag=0} flag' CHANGELOG.md)"
# Here-strings, not `printf ... | rg -q`.
#
# `rg -q` exits on the first match. The [Unreleased] section is 90 KB, well past
# the 64 KB pipe buffer, so `printf` is still writing when rg leaves; it then
# dies of EPIPE and `set -o pipefail` hands that status to the `||`, and the
# assertion reports a CHANGELOG defect that is not there. Measured during
# FR-133: 10 spurious failures in 400 runs under CPU load, 0 in 400 idle, which
# is why it had never been seen — and why it surfaced first inside a
# certification sweep running 47 gates back to back. A here-string is written to
# a temporary file, so there is no pipe and no writer left to signal.
rg -q '^### Removed[[:space:]]*$' <<< "$RETIREMENT_SECTIONS" ||
  fail "CHANGELOG [Unreleased]/[0.4.0] does not record the retirement under ### Removed"
rg -q 'RunnerExecutorKind' <<< "$RETIREMENT_SECTIONS" ||
  fail "CHANGELOG [Unreleased]/[0.4.0] does not name the removed runner selection seam"
rg -q 'legacy_runner_executor_removed' <<< "$RETIREMENT_SECTIONS" ||
  fail "CHANGELOG [Unreleased]/[0.4.0] does not record the breaking manifest rejection"
rg -q 'legacy_agent_command_deprecated' <<< "$RETIREMENT_SECTIONS" ||
  fail "CHANGELOG [Unreleased]/[0.4.0] does not record the command-only compatibility window"
pass "CHANGELOG records the retirement and its breaking manifest change"

# These three used to require the [legacy_*] codes to exist in source, and
# FR-173 deleted all three codes. The general obligation they served — a code
# the guides name must exist in production source — is covered in derived form
# by test-error-code-glossary.sh, which walks both directions over a set it
# scans rather than a set someone typed here. What is left is the one binding
# specific to this gate: the guides now tell an author the refusal names the
# block to write, and that sentence is only true while the diagnostic does.
rg -q 'agent.spec.driver is required' core/src crates --glob '*.rs' ||
  fail "the driver-required diagnostic the guides describe is absent from source"
rg -q 'provider: shell' core/src crates --glob '*.rs' ||
  fail "the driver-required diagnostic does not show the block to write"
pass "the refusal the guides describe exists in production source, with its remedy"

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
