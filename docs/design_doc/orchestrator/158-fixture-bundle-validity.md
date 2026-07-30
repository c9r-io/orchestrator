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

**31 of 93 bundles are rejected, not 4.** Nineteen are `rotted`: eight fail on
`[legacy_coordination_removed]`, ten on `[legacy_json_path_removed]`, and one on
prehook schema drift (`missing field \`when\``). Five are Workflow-only
fragments. Four depend on an ambient path or a base policy. One depends on
another bundle. Two are intentional. 19 + 5 + 4 + 1 + 2 = 31.

> **Corrected at FR-149.** This paragraph originally read "`behavior.captures`
> in nine, and `generate_items` JSONPath post-actions in ten", and separately
> "One is prehook schema drift" — which enumerated 32 against its own stated
> total of 31, because `prehook-test.yaml` is inside the nineteen rather than
> beside them. The 9 does not survive either derivation: counting **rejection
> diagnostics** gives 8, and counting **files containing `captures:`** gives 10,
> because `wp05-items-select.yaml` and `wp05-store-items-select.yaml` carry both
> constructs and are rejected on whichever the `HashMap` merge reaches first.
> The split above is by diagnostic, which is what the ledger's `expect` records.
> The same "9 个" was repeated in the FR-148 closure note in
> `docs/feature_request/README.md` and is corrected there too.

**`scripts/qa/test-wp05-integration.sh` is broken too**, and it runs
`orchestrator apply -f` wholesale on three of the rotted bundles
(`scripts/qa/test-wp05-integration.sh:250,282,312` at `ef458f16`). One
undiscovered instance is a bug; two is a class, which is the argument for a
mechanism rather than a second repair.

> **Corrected at FR-149.** This paragraph originally continued "broken the same
> way, and had been since the same commit". It is not the same way and it was
> not the same commit. FR-149 *ran* the gate rather than reading it: it dies in
> `ensure_db` on `orchestrator init` with `daemon socket not found`, before
> L1-A, and never reaches the rotted bundles at all. The cause is `1be4666d`
> (2026-03-26), the CLI/daemon split — four months earlier than recorded. The
> three rotted bundles were really at those lines and really would have failed,
> which is exactly what made the wrong cause plausible enough to write down
> without executing anything. Two further harness faults were hidden behind it:
> the build step built the `agent-orchestrator` library rather than the `cli`
> and `daemon` packages that produce the binaries, so the gate ran a stale
> artifact; and `DB` pointed at a repository-local path the product had stopped
> using. FR-149 rewrote the harness and the gate now passes.
>
> The general lesson belongs with the FR's subject rather than beside it: **this
> ledger's whole premise is that reading a fixture is not the same as running
> the product against it, and the claim above was made by reading a gate.**

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
`ManifestValidate`. The check — `core/src/fixture_corpus_tests.rs`, a
`#[cfg(test)]` module of the core crate — runs against that, inside the existing
`Rust test` CI job: **no new governance step, no share of the 9% budget
headroom**. It is deliberately not a `scripts/qa/*.sh` gate and so is deliberately
absent from `config/governance/qa-gate-surface.json`. Anyone reading that absence
as an FR-147-shaped hole should read this paragraph instead: a `cargo test` is
enforced by the job that runs the workspace's tests, and adding a shell wrapper
solely to appear in the manifest would buy a manifest row and a minute of CI for
nothing.

> **FR-147 note.** This paragraph originally justified the absence by the
> manifest's declared scope, "every `scripts/qa` gate". That premise no longer
> holds — the manifest now also declares every script outside `scripts/qa` that a
> workflow job executes, and `check_workflow_execution_declared` fails on one that
> is missing. The conclusion is unaffected and in fact rests on firmer ground now
> that the rule is stated in terms of execution: the new check's subject is a
> `scripts/**.{sh,rb}` path appearing in a job's executable text, and a
> `#[cfg(test)]` module of the core crate is not one. It is reached through
> `cargo test`, which is the enforcement. Had the check been written to ask "what
> does CI verify" rather than "what does a job execute", this module would have
> been in scope and the wrapper would have been unavoidable.

The file name is not incidental. `scripts/lib/rust_source.rb` excludes test
sources from every ledger scan by **filename** — a path component `tests`, or a
basename matching `/test.*\.rs\z/` — not by `cfg`. All fourteen pre-existing
file-level `#[cfg(test)] mod x;` bodies in this workspace happen to be named
`*test*.rs` and are excluded; the first version of this module was called
`fixture_corpus.rs` and was not, so its four lines mentioning `behavior.captures`
walked straight into the coordination ledger's `capturesOrJsonPath` ratchet and
took `test-coordination-strangler.sh` (ci-required) from 53 to 57. Renaming it
made the scanner classify it correctly rather than raising the baseline to
accommodate test code, which would have diluted a ratchet whose whole subject is
production consumers. **The trap remains**: the exclusion is a naming convention
wearing the costume of a `cfg` predicate, and the next test-only module named
without "test" in it will inflate four ledgers silently. That is §4.4's "a scope
predicate is an assertion" in shared tooling, recorded here because measuring it
cost a red gate.

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

| status | n at FR-148 | n now | |
|---|---|---|---|
| `intentional` | 2 | 2 | rejection is the point, and a live gate asserts it |
| `rotted` | 19 | **0** | declares a construct the product no longer accepts, and nobody wants it to |
| `fragment` | 5 | 5 | contents current; not self-sufficient |
| `environment` | 4 | 4 | contents current; needs an ambient path or base policy |
| `dependent` | 1 | 1 | valid only after another bundle is applied |

`rotted_count` is compared for **equality**, not as a ceiling. A ceiling lets the
debt sit; equality means retiring one fixture has to move the number, so the
ledger cannot drift out of step with the tree in either direction. This is
FR-133's `deny.toml` shape — 48 crates with 70 individually written reasons —
applied to fixtures.

FR-149 is the demonstration: it deleted all 19 rotted bundles, and the equality
check is what forced their ledger entries out in the same commit. The field
stays declared at 0 rather than being dropped — 0 is a claim, and a bundle that
rots tomorrow has to move it back up.

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

**The 19 rotted entries were recorded debt, and FR-149 paid it.** They were
frozen, not fixed. FR-149 deleted all 19 bundles and their entries, excised the
three `test-wp05-integration.sh` scenarios that applied them, and superseded QA
83, 84 and 92. See `docs/design_doc/orchestrator/159-dd137-fixture-residue-retirement.md`.

**Deleting a bundle moves something else.** `scripts/qa-doc-lint.sh`
derives its set of known workflow IDs from `fixtures/manifests/bundles/*.yaml` by
glob, and cross-references every `--workflow <id>` in `docs/qa/orchestrator`
against it. Every bundle feeds that check even when nothing references it as a
fixture, so removing one can turn a QA document's workflow ID into an "unknown"
finding. FR-148 added and removed nothing
(`git diff --stat 1b6628eb..HEAD -- fixtures/` was empty, and the derived ID set
was identical across the range), so the coupling was untouched there. FR-149
deleted 19 bundles, which removed 22 workflow IDs and collided with exactly one
QA document reference; the check is now scoped to `lifecycle: active` documents
and lives in `scripts/lib/qa_doc_workflow_ids.sh`.

**This ledger's own negative fixtures named their targets, and FR-149 moved
them.** Three of QA 196's five scenarios named a bundle file or restated
`rotted_count: 19`; against the post-FR-149 tree two passed vacuously and the
third failed through a branch it never claimed to test. The finding is not that
those three were careless — it is that a fixture belonging to a gate whose
subject is *a number meant to move* cannot restate the number. Recorded in §4.4
shape 7 and repaired in QA 196.

## Provenance

Found while fixing
`docs/ticket/coordination_collapse_scenario_legacy_apply_260729_000000.md`
(classified false positive, closed and deleted; the substance is in
`docs/qa/orchestrator/168-coordination-collapse-mcp-tools.md` and commit
`8a3ee0d9`). Filed alongside FR-146 (`| head` under `pipefail`) and FR-147
(scripts `ci.yml` runs that the enforcement manifest does not list); the three
are independent.
