---
lifecycle: active
related_fr: FR-156
self_referential_safe: true
---

# Orchestrator - Pipeline Variable Surface Retirement

**Module**: Orchestrator Config / Scheduler / Workflow Governance
**Scope**: manifest pipeline-variable rejection, per-object migration parity,
retained-carrier ledger convergence, store round-trip
**Scenarios**: 6
**Priority**: High

## Background

FR-156 retires the step-level pipeline-variable authoring surface —
`store_inputs`, `store_outputs`, `step_vars` and the `store_put` post-action —
and records `PipelineVariables` as a retained carrier rather than outstanding
debt. Design: `docs/design_doc/orchestrator/169-pipeline-variable-surface-retirement.md`.

The automated gate is `scripts/qa/test-pipeline-variable-retirement.sh`,
`ci-required` in the `coordination-strangler` job. It starts its own daemon on
port 19327 with its own `ORCHESTRATORD_DATA_DIR` and `HOME` under `mktemp`, and
never touches the developer's daemon, database or config.

## Scenario 1: Every retired construct is rejected, and the diagnostic names it

**Steps**

1. `cargo build -p orchestratord -p orchestrator-cli`
2. `bash scripts/qa/test-pipeline-variable-retirement.sh`
3. Read the four `is rejected and the diagnostic names its field` lines.

**Expected result**

Each of `fr156-gather-updates-legacy` (`store_inputs`),
`fr156-store-outputs-legacy` (`store_outputs`), `fr156-step-vars-legacy`
(`step_vars`) and `fr156-store-put-legacy` (a `store_put` post-action) fails
`manifest validate`, and the output contains the exact string
`[legacy_pipeline_variables_removed] workflow '<name>' step '<step>' uses <field>`.

The subject is the diagnostic text, never the exit code: an exit code cannot
distinguish which branch a validator failed through. One workflow per construct,
so a validator that detected the wrong field would fail rather than be absorbed
by a shared assertion.

## Scenario 2: A retired construct nested in `chain_steps` is rejected too

**Steps**

1. `cargo test -p agent-orchestrator retirement_tests`

**Expected result**

`a_retired_field_nested_in_chain_steps_is_rejected_too` passes. The parent step
authors nothing; only its chain child carries `store_inputs`.

This is the case the recursion exists for. `validate_workflow_steps` walked
`spec.steps` only, while chain children are dispatched through the same
`execute_step` path — so before FR-156 a retired field one level down ran
exactly as it always had and the workflow validated clean. The same hole applied
to `legacy_coordination_removed` and `legacy_json_path_removed`, which now
recurse with it.

`an_empty_step_vars_map_is_not_a_retired_construct` is the paired negative:
`step_vars: {}` deserializes to `Some(empty)`, and rejecting on `Some` alone
would fail a manifest that authors nothing.

## Scenario 3: Per-object migration parity against a recorded baseline

**Steps**

1. `bash scripts/qa/test-pipeline-variable-retirement.sh`
2. Read the three per-object comparison lines.

**Expected result**

- `promotion#gather_updates (store key absent)` matches
  `fixtures/qa/fr156-pipeline-variable-baseline.json` byte for byte after
  normalising SHAs and task ids.
- `self-evolution#evo_apply_winner` delivers the same decoded selection —
  terminal state and exit code exact, payload compared as parsed JSON because
  the CLI pretty-prints where the retired binding used compact `to_string()`.
- `promotion#gather_updates` now **distinguishes** its two branches, which the
  baseline records it could not.

The baseline was captured at `1cf6f3cc`, before any removal, with
`--capture-baseline`. Re-capturing it is not part of verification: a baseline
re-recorded after the change certifies nothing.

The third assertion is derived, not restated. Capture mode compares its own two
recordings and writes `branchesDistinguishable`; verify mode reads that field.
Had the old path worked, the check would have tightened to equality on its own
rather than requiring an edit.

## Scenario 4: The end-to-end behaviour the migrated step exists for

**Steps**

1. `bash scripts/qa/test-pipeline-variable-retirement.sh`
2. Read the two `with the key present/absent` lines.

**Expected result**

With `promotion/last_published_sha` set to the repository's third-newest commit,
the migrated step prints `=== Changes since last promotion (...) ===` and
**exactly the two commits after it**. With the key absent it prints
`=== Recent changes (no prior promotion recorded) ===` and all three.

Both branches are asserted. A check on the populated branch alone would let the
fallback break silently, and the pre-migration recording shows the fallback was
the only branch that ever ran.

## Scenario 5: `store put` and `store get` are inverses

**Steps**

1. `cargo test -p agent-orchestrator service::store`

**Expected result**

- `a_non_json_scalar_round_trips_as_itself` — `store put k <sha>` then
  `store get k` yields the SHA with no quote characters. Before FR-156 the write
  succeeded and the read failed with `failed to parse stored JSON value`.
- `an_explicit_json_string_reads_back_unwrapped_too` — an already-JSON string is
  not double-encoded.
- `a_json_object_is_still_returned_as_json` — the unwrapping is for strings only.

A shell step interpolates this output straight into a command, so a quoted
literal is a silent failure rather than an error. That is why the assertion is
on the exact bytes and not on "the read succeeded".

## Scenario 6: The ledger says what the tree says

**Steps**

1. `ruby scripts/qa/coordination-governance.rb`
2. `diff <(ruby scripts/qa/coordination-governance.rb --emit-consumers) <(jq '.consumerInventory' config/governance/coordination-collapse-ledger.json)`
3. `diff <(ruby scripts/qa/coordination-governance.rb --emit-baseline) <(jq '.sourceBaseline' config/governance/coordination-collapse-ledger.json)`
4. `bash scripts/qa/test-governance-ledger-tooling.sh`

**Expected result**

The gate passes; both diffs are empty. `consumerInventory.pipelineVariables` is
`state: removed`, `productionConsumerCount: 0`, with a `retainedCarrier` naming
`PipelineVariables` and `codeLevelBlockers` holding the three surfaces that were
never consumers of this one. `sourceBaseline.pipelineVariables` is 29.

The tooling gate's `--emit-consumers derives the count from the tree, not from
the ledger it rewrites` case moves a stored count to 7 and requires the emitted
candidate not to follow. An emitter that echoed the ledger back would satisfy a
plain equality check and fail this one.

**On the number 29**: it is not `30 − 3`. `dispatch.rs` drops from four
occurrences to one, and two new doc comments name the type in prose. The ratchet
counts textual occurrences including comments — it measures spelling, not
reachability. Read a movement in it accordingly.

## Rollback Evidence

`git revert --no-commit 2ff80872` applies with no conflicts across 36 files
(verified at `ceccf4f5`). The `-legacy` half of
`fixtures/manifests/bundles/fr156-pipeline-variable-parity.yaml` holds the
pre-migration shape, declared in `config/governance/fixture-bundle-validity.json`
with the exact diagnostic each document is now rejected by — so a shape that
silently started validating again would fail
`every_tracked_bundle_is_accepted_or_declared`.

## Known Limits

A step's `orchestrator` call must be able to discover its own daemon. The runner
clears the environment to `PATH, HOME, USER, LANG, TERM`, which suffices only
when the daemon is on the default data directory under the same `HOME`.
Otherwise `ORCHESTRATORD_DATA_DIR` and `ORCHESTRATOR_CONTROL_PLANE_CONFIG` must
be added to `RuntimePolicy.runner.env_allowlist` — the parity fixture does. With
the default allowlist the step gets `daemon socket not found ... and no
control-plane config was discovered`, and `|| true` turns that into an empty
read indistinguishable from an absent key.

The `self-evolution#evo_apply_winner` probe measures whether the selection is
retrievable by the step, not whether a given provider will run the command its
prompt now instructs. That is inherent to migrating an instruction into a prompt.
