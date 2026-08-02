---
lifecycle: active
related_fr: FR-156
---

# Pipeline Variable Surface Retirement

**Module**: Orchestrator Config / Scheduler / Workflow Governance
**Status**: Released
**Related Plan**: FR-156 pipelineVariables 清单授权面退役
**Related QA**: `docs/qa/orchestrator/207-pipeline-variable-surface-retirement.md`
**Created**: 2026-08-02
**Last Updated**: 2026-08-02

## Background

`config/governance/coordination-collapse-ledger.json` carried
`consumerInventory.pipelineVariables` at `deprecated-blocked` with two production
consumers from DD-137 onward, and its `next` field asked for a follow-up FR that
was never filed. It was the last coordinate of the coordination-collapse sequence
(FR-118 → 124 → 125 → 149) still mid-strangler; `capturesOrJsonPath` and
`shellRunnerExecutor` had both reached `removed`.

## What The Ledger Actually Said

FR-156's own text was a proposal, and four of its six factual claims did not
survive rebuilding against the tree at `aafe322d`. They are recorded here
because three of them are errors of a kind the ledger format invites.

**The counter and the blockers measured different categories.**
`productionConsumerCount: 2` counted *manifest step touches* under
`docs/workflow` and `config`. The four `blockers` recorded beside it described
*Rust-level* surfaces. DD-137:95 said so in prose, but the JSON put them under
one key, and for the life of the entry they read as arrears against the count.
They were not: driving the count to zero clears none of them, and none of them
ever blocked that count. One of the four — "step-local `store_inputs` bindings in
promotion and self-evolution" — was not a code-level blocker at all; it *was* the
manifest consumer, filed on the wrong side of its own ledger entry. The
remaining three are now `codeLevelBlockers`, with a note saying what they are
not.

**The retirement target was inverted.** `PipelineVariables` is the live carrier,
not the dead path. `PreservedExecutionChannels` has three references repo-wide,
all inside `crates/orchestrator-config/src/config/pipeline.rs`, so it is
reachable only through `PipelineVariables.preserved`; `ExecutionSignals` is the
same. Removing the named symbol would have removed the preserved channels with
it. What was dead was the *manifest authoring surface*.

**The source ratchet could not fall from the migration.** All 30 counted lines
were imports and type signatures of that live carrier; migrating both consumers
removes none of them. The reachable reduction came from deleting the `step_vars`
overlay pair, and the final number is 29 rather than 27 because two new doc
comments name the type in prose. **The ratchet counts textual occurrences,
including comments — it measures spelling, not reachability.** That is worth
knowing before anyone reads a movement in it as a movement in the code.

**One acceptance criterion was unsatisfiable.** It asked for a consumer count
"produced by the regeneration tool"; `--emit-inventory` covers production Agents
only, so that number was the one number in the ledger a human had to type.
`--emit-consumers` now exists.

## Goals

- Drive the manifest-level pipeline-variable authoring surface to zero
  production consumers and reject it at apply with a stable diagnostic.
- Record `PipelineVariables` as a **retained carrier** rather than outstanding
  debt, so the coordinate stops reading as unfinished work.
- Produce per-object migration evidence, not counts.

## Non-Goals

- Removing `PipelineVariables`, `PreservedExecutionChannels` or `ExecutionSignals`.
- Introducing typed pipeline state with reducers. That direction is closed, not
  deferred (DD-130); this is dismantling, not replacement.
- Clearing the three `codeLevelBlockers`. They are unrelated to this coordinate
  and remain open.

## The Retired Surface

Four constructs, each routing an author-chosen value through
`PipelineVariables.vars`:

| Construct | What it did |
|---|---|
| `store_inputs` | read a store entry into the generic map |
| `store_outputs` | wrote a variable from the map into a store |
| `step_vars` | overlaid the map for one step, then restored it |
| `store_put` post-action | wrote a named variable into a store |

All four are rejected at apply with `[legacy_pipeline_variables_removed]`, one
arm per field so the diagnostic names what the author wrote. The spec types stay
deserializable (DD-137's rule): a removed field that still parses gets a stable
retirement diagnostic, where a deleted field surfaces as an opaque unknown-key
error.

`store_put` deserves a note: no coordinate counted it. `capture_consumers` takes
a `post_action` only when it carries `json_path`, and `pipeline_consumer_kinds`
matched on `kind`, which for every post-action is `"post_action"`. It was a live,
uncounted consumer of the very channel the ledger was tracking — §4.4 shape 2
inside the gate's own enumeration.

`outputs` and `pipe_to` were also counted by that gate and are gone for a
different reason: **they were never wired.** Neither is a field of
`WorkflowStepSpec`, and every conversion path set the `WorkflowStepConfig`
fields to empty unconditionally, so no manifest could populate them and nothing
read them. Counting them as production consumers counted something that could
not have one. The `pipe_to_unknown` check went with them; it validated a
reference no manifest could express.

### The validator now recurses

`validate_workflow_steps` walked `spec.steps` only, while `chain_steps` children
are dispatched through the same `execute_step` path. A retired field one level
down therefore ran exactly as it always had, and the workflow validated clean.
The FR-156 check recurses, and the two pre-existing retirement checks
(`legacy_coordination_removed`, `legacy_json_path_removed`) recurse with it —
they had the same hole.

## The Retained Carrier

`PipelineVariables` stays, and the ledger says so in a `retainedCarrier` object
rather than leaving it implicit. The precedent is `celInterpreter`, held at a
non-zero `sourceBaseline` because deterministic governance gates depend on it.

Its boundary, which is the constraint this document exists to fix in place:

- It carries exactly two typed structures — `PreservedExecutionChannels` (one
  user-intent value, three scheduler-owned sandbox safety signals) and
  `ExecutionSignals` (self-test, tool and metric observations) — plus the
  generic `vars` map that remains the substrate for public initial/item variable
  bindings and template compatibility.
- **It does not expand.** A new field on either typed structure, or a new writer
  into `vars`, is a new coordination channel and needs its own governance, not
  an incremental addition here. The whole point of naming a retained carrier is
  that "it already exists" stops being an argument for putting things in it.
- The generic map has no *manifest* authoring surface at all now. Every
  remaining writer is scheduler-owned code.

## Migration And Its Evidence

The two consumers now read their own store:

```yaml
command: >-
  LAST_SHA="$(orchestrator store get promotion last_published_sha
  --project {project_id} 2>/dev/null || true)" && ...
```

`{project_id}` is new — a context template variable alongside `{task_id}` and
`{workspace_root}`, resolved from `AgentContext`, never entering
`PipelineVariables.vars`, and not shadowable by a generic var of the same name
because context substitution runs first. Both render paths now build a context
when a template mentions it, which they previously did only for pipeline state
or `{workspace_root}`; the migrated `self-evolution` prompt carries no pipeline
state at all, so without that the placeholder would have reached the agent
unsubstituted.

`fixtures/qa/fr156-pipeline-variable-baseline.json` records the pre-migration
contract of both objects, captured on `1cf6f3cc` before any removal, and
`scripts/qa/test-pipeline-variable-retirement.sh` replays each object against it.

### Two defects the baseline found

Running the old path found what no count or symbol check could.

**`promotion#gather_updates` had a dead branch.** Its guard was
`[ -n "$LAST_SHA" ] && [ "$LAST_SHA" != "{last_published_sha}" ]`, intended to
detect an unsubstituted placeholder. Template substitution is global, so when the
key was set both occurrences became the same value and the comparison was false;
when it was unset both stayed literal and it was false again. The scoped-log
branch was unreachable in every case, and the recording shows both branches
emitting identical output.

**`store put` and `store get` were not inverses.** The backend stores JSON and
parses on read, and nothing encoded the value on the way in, so
`orchestrator store put k 280e3c` succeeded and the matching get failed with
`failed to parse stored JSON value` — write accepted, entry unreadable, by every
reader of that store. `promotion.yaml` had been writing a bare SHA that way for
its whole life. A stored JSON string also came back quoted, where the retired
binding unwrapped it (`Value::String(s) => s.clone()`) before handing it to a
step. Both are fixed at the service boundary, and `store get` now unwraps a
top-level string while leaving every other shape as JSON.

So the coordinate's last two consumers were, between them, one dead branch and
one unreadable round trip. Neither would have been visible in a structural
retirement; both are why §4.3 asks for a recorded baseline.

## Known Limits

**CLI store access depends on daemon discovery from inside a step.** The runner
clears the environment to `PATH, HOME, USER, LANG, TERM`. That is enough only
when the daemon sits on the default data directory under the same `HOME`. Under
an isolated daemon — a non-default `ORCHESTRATORD_DATA_DIR`, or an explicit
control-plane config — the CLI in a step cannot discover it, and the
`|| true` idiom turns that into an empty read indistinguishable from an absent
key. Such a workflow must add `ORCHESTRATORD_DATA_DIR` and
`ORCHESTRATOR_CONTROL_PLANE_CONFIG` to `RuntimePolicy.runner.env_allowlist`; the
parity fixture does, and the guide says so where it shows the pattern. The
retired binding did not have this failure mode — it ran in-process. This is the
one respect in which the migration is a step back, and it is recorded rather
than hidden.

**The parity probe for `self-evolution#evo_apply_winner` measures data delivery,
not agent behaviour.** The real step is an agent whose prompt now instructs it
to run the CLI. The gate asserts that the same selection bytes are retrievable
by the step; it cannot assert that a given provider will actually run the
command. That is inherent to migrating an instruction into a prompt.

**The source ratchet counts prose.** See above — two of the 29 counted
occurrences are doc comments.

## Interfaces And Data

- `[legacy_pipeline_variables_removed]` — new error code, documented in
  `docs/guide/error-codes.md` and its ZH mirror, which the FR-152 glossary gate
  derives from source and asserts in both directions.
- `{project_id}` — new context template variable.
- `store get` returns a stored JSON string unwrapped; `store put` encodes a
  non-JSON value as a JSON string.
- `coordination-governance.rb --emit-consumers` regenerates the consumer counts
  and nothing else. `state`, `scope`, `retainedCarrier` and the code-level
  blockers are judgements about what a count *means*; a tool that rewrote them
  would be deciding rather than measuring.

## Rollback

`2ff80872` (the removal commit) reverts mechanically:
`git revert --no-commit 2ff80872` applies with no conflicts across 36 files,
verified at `ceccf4f5`. The `-legacy` half of
`fixtures/manifests/bundles/fr156-pipeline-variable-parity.yaml` is retained as
the pre-migration shape, declared in
`config/governance/fixture-bundle-validity.json` with the exact diagnostic each
document is now rejected by.

## References

- `docs/design_doc/orchestrator/130-coordination-collapse-mcp-tools.md`
- `docs/design_doc/orchestrator/136-coordination-strangler-completion.md`
- `docs/design_doc/orchestrator/137-legacy-coordination-decommission.md`
- `docs/design_doc/orchestrator/140-governance-ledger-regeneration.md`
- `docs/qa/orchestrator/207-pipeline-variable-surface-retirement.md`
