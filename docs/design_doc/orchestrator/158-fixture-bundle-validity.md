---
lifecycle: active
related_fr: FR-148
---

# DD-158: A Fixture Nobody Validates, And A Verdict That Depends On Where You Stand

**Status**: Released

## The mechanism

`scripts/qa/test-coordination-collapse.sh` could not finish for four days. It
applies `fixtures/manifests/bundles/coordination-legacy-baseline.yaml`, which
carries `behavior.captures`; DD-137 (`1b0937ca`, 2026-07-25) removed that
construct by design, and `manifest apply` is all-or-nothing over a bundle. One
rejected Workflow took the whole file, three of twelve assertions ran, and the
symptom was a summary line that did not print.

Nothing about that needed the gate to run. The fixture said `behavior.captures`.
The validator rejected `behavior.captures`. No artifact in the repository put the
two side by side, and the gate was `manual-runbook`, so no CI job was watching.

## What the fact-check found that the FR did not

FR-148 was filed on one broken gate and four bundles it believed were
intentionally invalid. Rebuilding every claim at `1b6628eb` moved the numbers by
an order of magnitude, and moved two of them in the opposite direction from the
one the FR assumed.

**31 of 93 bundles are rejected, not 4.** Nineteen declare constructs DD-137
retired: `behavior.captures` in nine, and `generate_items` JSONPath post-actions
in ten. Five are Workflow-only fragments. Four depend on an ambient path or a
base policy. One depends on another bundle. One is prehook schema drift. Two are
intentional.

**`scripts/qa/test-wp05-integration.sh` is broken the same way**, and had been
since the same commit. It runs `orchestrator apply -f` wholesale on three of the
rotted bundles (`scripts/qa/test-wp05-integration.sh:250,282,312`). One
undiscovered instance is a bug; two is a class, which is the argument for a
mechanism rather than a second repair.

**Two of the FR's four "intentionally invalid" fixtures are not.**
`qa105-s1-capture-wrong-level.yaml` is **accepted** — its step-level `capture:`
key is an unknown field and is silently ignored, so the product does not reject
the placement the fixture was written to condemn.
`crd-test-invalid.yaml` is rejected, but on `no CustomResourceDefinition found
for kind 'PromptLibrary'` rather than the missing-`prompts` schema violation it
demonstrates; that violation is only reachable once `crd-test.yaml` has
registered the CRD. A fifth intentionally-invalid bundle went unmentioned, and it
is the only one with a live consumer asserting the fact:
`coordination-strangler-parity.yaml`, whose rejection `test-coordination-strangler.sh`
(**ci-required**) checks at lines 126-133.

**Four QA documents still read `lifecycle: active` while describing the removed
mechanism**: `51-primitive-composition.md`, `83-generate-items-mixed-text-extraction.md`,
`84-generate-items-regression-narrowing.md`, `92-dynamic-items-cycle-overflow.md`.

## The design

### The entry point is the daemon's own, called offline

`orchestrator manifest validate` is a control-plane call — it goes over gRPC
(`crates/cli/src/commands/manifest.rs:13-21`) and needs a running daemon, so it
cannot be the mechanism. `service::system::validate_manifests`
(`core/src/service/system.rs:250`) is synchronous, public, and is exactly what
`crates/daemon/src/server/system.rs:317-327` calls when the daemon receives
`ManifestValidate`. The check runs against that, inside the existing `Rust test`
CI job: **no new governance step, no share of the 9% budget headroom**.

### The verdict depends on the base, so the base is declared

`validate_manifests` merges the manifest into the *current* config before
`build_active_config_for_project`. Validity is therefore never a property of the
file alone, and choosing the base is a design decision rather than a detail.
Measured at `1b6628eb`:

| base | accepted / rejected | what the difference is |
|---|---|---|
| `TestState::new()` as seeded | 60 / 33 | five bundles rejected for `[SELF_REF_POLICY_VIOLATION] ... workflow 'basic'` — the *scaffolding's own* workflow, dragged in by a bundle that introduces a self-referential workspace |
| agents and workflows cleared | **62 / 31** | the five self-ref verdicts disappear; five Workflow-only fragments turn red, which is a true statement about them |

The second is the base, and `TestState::without_seeded_agents_and_workflows()`
carries that reasoning in its doc comment rather than in this file alone. It
answers "would a fresh daemon accept this bundle standalone", which is what a
fixture has to satisfy before any gate can apply it.

One `InnerState` serves all 93 calls — `validate_manifests` only reads state.
**1.77s**, against 24.34s for a fresh `TestState` per bundle.

### The ledger records why, and what by

`config/governance/fixture-bundle-validity.json` sits with the other governance
ledgers. Each rejected bundle carries a `status`, a one-sentence `reason`, its
`consumers`, and `expect` — the diagnostics it must fail by.

`expect` is the load-bearing field. Capability validation runs *before* the
retirement checks (`core/src/config_load/validate/workflow_steps.rs:49-74`), so a
bundle that merely omits its Agent fails with `no agent supports capability`, and
an exit code cannot distinguish that from the retirement the fixture exists to
demonstrate. A declared bundle that starts failing for a different reason is a
violation, not a pass.

It is a **list**, because a bundle holding several retired constructs is rejected
on whichever the merge reaches first and the merge walks a `HashMap` —
`cycle-overflow-test.yaml` was measured naming a different workflow on two
consecutive runs. The alternatives are spelled out per workflow
(`coordination-strangler-parity.yaml` has six), so the list stays an enumeration
rather than degrading to the rule tag. An empty `expect`, or one holding a blank
string, is itself a violation: a blank matches every error and would turn the
entry into a blanket acceptance of every future rejection — §4.4 shape 8, arrived
at by omission rather than by a `skip-tree`.

`status` distinguishes five reasons a fixture may be rejected, and the point of
the distinction is that only one of them is debt:

| status | n | |
|---|---|---|
| `intentional` | 2 | rejection is the point, and a live gate asserts it |
| `rotted` | 19 | declares a construct the product no longer accepts, and nobody wants it to |
| `fragment` | 5 | contents current; not self-sufficient |
| `environment` | 4 | contents current; needs an ambient path or base policy |
| `dependent` | 1 | valid only after another bundle is applied |

`rotted_count` is compared for **equality**, not as a ceiling. A ceiling lets the
debt sit; equality means retiring one fixture has to move the number, so the
ledger cannot drift out of step with the tree in either direction. This is
FR-133's `deny.toml` shape — 48 crates with 70 individually written reasons —
applied to fixtures.

### Scope is derived, and a derivation that yields nothing fails

The corpus comes from `git ls-files 'fixtures/manifests/bundles/*.yaml'`, so a
bundle added tomorrow is in scope tomorrow; a hand-listed set would guard exactly
what its author remembered (§4.4 shape 2). A `git` that cannot run, a pathspec
matching nothing, or a ledger that will not parse each **abort the test**. A
corpus check that silently compares nothing is green and worthless, and §4.4
shape 7 is explicit that a premise which no longer holds is a failed assertion
rather than a skip.

The injection fixture follows the same rule for its own target: it mutates the
first bundle that is accepted and undeclared, derived at run time, never a named
file. Nine recorded times a negative fixture's named target moved; eight stayed
green.

## What this does not cover, and the number that names it

**Assertion rot inside the gate shells.** The ticket that produced FR-148 carried
a second defect of exactly that kind: `normalize_preserved_channels` moved `goal`
and three sandbox signals into a typed carrier, and the assertion still queried
the generic variable table. No static comparison between a fixture and a
validator can see that. FR-148 said so in its own text rather than leaving a
reader to infer it, and this record repeats it for the same reason.

**The 19 rotted entries are recorded debt.** They are frozen, not fixed. FR-149
retires them together with `scripts/qa/test-wp05-integration.sh` and the four QA
documents that still describe the removed mechanism.

**Deleting a bundle moves something else.** `scripts/qa-doc-lint.sh:64-66`
derives its set of known workflow IDs from `fixtures/manifests/bundles/*.yaml` by
glob, and cross-references every `--workflow <id>` in `docs/qa/orchestrator`
against it. Every one of the 93 bundles feeds that check even when nothing
references it as a fixture, so removing one can turn a QA document's workflow ID
into an "unknown" finding. FR-148 added and removed nothing
(`git diff --stat 1b6628eb..HEAD -- fixtures/` is empty, and the derived ID set
is identical across the range), so the coupling is untouched here — but FR-149
deletes bundles and has to reconcile it.

## Provenance

Found while fixing
`docs/ticket/coordination_collapse_scenario_legacy_apply_260729_000000.md`
(classified false positive, closed and deleted; the substance is in
`docs/qa/orchestrator/168-coordination-collapse-mcp-tools.md` and commit
`8a3ee0d9`). Filed alongside FR-146 (`| head` under `pipefail`) and FR-147
(scripts `ci.yml` runs that the enforcement manifest does not list); the three
are independent.
