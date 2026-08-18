---
lifecycle: active
related_fr: FR-173
self_referential_safe: true
---

# Orchestrator - Legacy Retirement at the v0.7 Window

**Module**: Config types / manifest validation / scheduler spawn / governance ledgers
**Scope**: that each retired surface is refused rather than silently dropped; that
which mechanism refuses it depends on whether the owning struct has a flattened
catch-all; that the one guard kept without its code still guards; that the ledgers
record diagnostics the product actually emits; and that a design-conforming
blueprint still applies
**Design**: [DD-191](../../design_doc/orchestrator/191-legacy-retirement-window-v07.md)
**Safety**: in-process fixtures and read-only ledger checks. One scenario starts an
isolated daemon through `scripts/lib/gate_daemon.sh`; none touches
`~/.orchestratord`.

## Prerequisites

```bash
cargo --version
```

> **This release requires a database reset.** `StepBehavior` and `RunnerSpec` now
> declare `deny_unknown_fields`, and both carry always-serialized fields, so a
> daemon on 0.7 cannot load a database written by 0.6. That is expected, not a
> defect — see the CHANGELOG entry before running anything against a real data
> directory.

## Scenario 1: each retired surface is refused, by the mechanism its struct allows

**Steps**

```bash
cargo test -p agent-orchestrator --lib fr173_retirement -- --nocapture
```

**Expected result**

Exit 0, four tests.

| Case | Mechanism | Assertion |
|---|---|---|
| `store_inputs`, `store_outputs`, `step_vars` | `WorkflowStepSpec`'s flattened `extra` | a named apply-time warning carrying the field |
| `behavior.captures` | `StepBehavior` + `deny_unknown_fields` | a deserialisation error naming `captures` |
| `post_actions: [{type: generate_items}]` | variant deleted | an error naming `generate_items` |
| a step using none of them | — | parses, warns about nothing |

The fourth is the negative half and is not decoration: without it an
implementation that rejected every step would satisfy the first three.

The same module's prehook cases moved with it. `steps.<id>.<var>` used to lint
clean and now warns, because `build_step_prehook_cel_context` binds no `steps`
variable and never did — verify that claim directly rather than taking it from
here:

```bash
rg -n 'steps' core/src/prehook/context.rs || echo "no steps binding: the dotted form cannot resolve"
cargo test -p agent-orchestrator --lib config_load::validate::workflow_steps
```

The warning must name `steps` itself. Naming only the trailing member would send
an author looking for a variable to define, when the root is what does not exist.

**Why the split matters.** Neither `StepBehavior` nor `RunnerSpec` has a
catch-all. Deleting a field there without `deny_unknown_fields` would have serde
drop the key in silence — worse than the named rejection it replaced. These cases
are the only thing standing between the retirement and that outcome. Verify by
mutation: remove `#[serde(deny_unknown_fields)]` from `StepBehavior` and case two
must fail.

## Scenario 2: a command-only Agent is refused, and nothing is persisted

**Steps**

```bash
cargo test -p agent-orchestrator --lib apply_command_only_agent_is_rejected_and_not_persisted
cargo test -p agent-orchestrator --lib agent::tests
```

**Expected result**

Exit 0. Applying an Agent with `spec.command` and no `spec.driver` fails, the
error contains both `agent.spec.driver is required` and `provider: shell`, and the
Agent is **absent from the active config afterwards**.

Both halves are required. Asserting only the error passes on an implementation
that stores the Agent and then complains; asserting only "some error" passes on
one whose diagnostic tells an author nothing about what to write.

## Scenario 3: the guard that outlived its error code

**Steps**

```bash
rg -n -A6 'FR-173 retired the `\[legacy_agent_execution_removed\]`' \
  crates/orchestrator-scheduler/src/scheduler/phase_runner/spawn.rs
```

**Expected result**

The guard is still there and its message no longer advises a re-apply. Read the
lines below it: `spawn_with_runner_and_capture` is the direct-command substrate
and is explicitly *not* Agent execution. Deleting the guard would let a driverless
Agent run through it — the pre-driver execution path the driver abstraction
exists to have removed.

This scenario is an inspection rather than an assertion, and that is a known
weakness: nothing fails if someone deletes the guard while keeping the tests
green, because reaching it requires a stored Agent with no driver, which the
retirement makes unconstructible through any supported path.

## Scenario 4: the ledgers record what the product actually says

**Steps**

```bash
cargo test -p agent-orchestrator --lib fixture_corpus
rg -n 'unknown field .captures.|unknown variant .store_put.' \
  config/governance/fixture-bundle-validity.json
ruby scripts/qa/coordination-governance.rb
```

**Expected result**

15 passed. The three intentionally-invalid bundles are declared with the
diagnostics they now produce — `unknown field \`captures\`` and
`unknown variant \`store_put\`` — and the two `agent-driver` bundles no longer
appear as undeclared rejections, because their Agents declare drivers.

Those strings were **re-observed by running the corpus test and reading the
output**, not rewritten from the old wording. The gate's own diagnostic names the
failure mode it exists to prevent: *"it is rejected for a reason nobody wrote
down."* A plausible-looking guess would have made the ledger a record of a
sentence the product never emits.

`coordination-governance.rb` exits 0 with `pipeline_consumer_kinds` empty: the
scan still runs and still derives the zero the ledger asserts, rather than the
count becoming an assertion nobody re-derives.

## Scenario 5: a design-conforming blueprint still applies

**Steps**

```bash
cargo test -p agent-orchestrator --lib quickstart_bundle_applies_without_warnings
./scripts/qa/test-agent-driver-abstraction.sh
rg -c 'executor:' $(git ls-files '*.yaml' '*.yml') | rg -v ':0$' || echo "no executor keys remain"
```

**Expected result**

All exit 0, and no tracked YAML carries `executor:`. The ten fixture bundles that
had `executor: shell` — valid before this change and refused after it — had the
line removed as part of the retirement, which is what keeps the breakage confined
to stored state rather than reaching blueprints.

`test-agent-driver-abstraction.sh` now writes its own command-only Agent and
asserts the refusal, replacing three assertions that had been built on the
promotion. Deleting them instead would have left the command-only case
uncovered on both sides of the retirement.

**Two gates were holding the retired surface in place, and both are in the doc
lint.** `test-error-code-glossary.sh` checks its own derivation against a
known-present code, and that anchor was one of the six; it now anchors on
`driver_config_invalid`, from a different file and layer, so the check cannot
agree with itself. `test-agent-driver-documentation-alignment.sh` required the
guides to *name* a retired code — the drift it exists to catch — and now requires
the replacement statement instead. Mutation check: revert either anchor to a
retired code and `./scripts/qa-doc-lint.sh` must go red naming it.

**The parity fixture's legacy half was collapsed, not deleted quietly.** Its three
`<name>-legacy` Agents and three `parity-*-legacy` Workflows differed from the
typed half only in capability name once the command-only form was gone, so the
gate's first conjunct compared a run against a rename of itself. What remains is
the comparison that always carried the evidence:

```bash
rg -n 'baseline_hash' scripts/qa/test-agent-driver-production-parity.sh
```

The right-hand side is a hash recorded **before** the migration
(`fixtures/driver/legacy-agent-execution-baseline.json`), not a value the gate
produces. An absent baseline entry is now a named failure — `jq -r` returns the
string `"null"`, and two of those compare equal, so the missing-entry case would
otherwise have passed on two things nobody measured.

## Checklist

- [ ] Scenario 1: four cases pass; removing `deny_unknown_fields` from
      `StepBehavior` turns case two red
- [ ] Scenario 2: the command-only Agent is refused, the diagnostic names the
      field and the remedy, and nothing is persisted
- [ ] Scenario 3: the spawn guard is present and its message no longer advises a
      re-apply
- [ ] Scenario 4: corpus 15/15 with re-observed diagnostics; coordination
      governance exits 0
- [ ] Scenario 5: quickstart applies clean, the driver gate passes, no tracked
      YAML carries `executor:`, and `qa-doc-lint` goes red if either gate anchor
      is reverted to a retired code

## Known limits

- **Scenario 3 is an inspection.** The state it guards against is unconstructible
  through supported paths, so no test drives it.
- **The `generate_items` coordination tool has no task-workspace guard.** The
  one-implicit-item invariant is enforced against the declarative surface only.
  Pre-dates this change; neither created nor closed by it.
- **The production-parity gate needs a daemon and is not run by `qa-doc-lint`.**
  Its collapse is verified by reading the script and by the full gate sweep, not
  by any scenario here.
- **No scenario loads a 0.6 database to observe the failure.** The reset
  requirement is derived from the serialization attributes, not from a run
  against a real pre-upgrade data directory.
