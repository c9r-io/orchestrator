---
lifecycle: active
related_fr: FR-149
---

# DD-159: Retiring the DD-137 fixture residue, and what the retirement broke

**Status**: Released
**Date**: 2026-07-30
**FR**: FR-149
**Predecessor**: [DD-158](158-fixture-bundle-validity.md) (the ledger that measured this residue)

## The problem

DD-137 (`1b0937ca`, 2026-07-25) retired `behavior.captures` and the
`GenerateItems` / `SpawnTasks` JSONPath post-actions. The rejection lives at
`core/src/config_load/validate/workflow_steps.rs:57-74` and is unconditional.

The corpus that depended on those constructs did not follow. FR-148 built
`core/src/fixture_corpus_tests.rs` to put the fixture and the validator side by
side, found 19 bundles declaring constructs the product no longer accepts, and
froze the number in `config/governance/fixture-bundle-validity.json` as
`rotted_count: 19`, compared for equality rather than as a ceiling.

Freezing is not cleaning, and the residue had a live symptom.
`scripts/qa/test-wp05-integration.sh` runs `orchestrator apply -f` wholesale on
three of the 19, and `apply` is all-or-nothing over a bundle, so those three
scenarios could not run. FR-148 concluded from that the gate had been dying at
L1-C since 2026-07-25.

It had not. That conclusion is the fourth finding below, and it is the one worth
the reading time: the gate dies four months earlier, for an unrelated reason,
and nobody found out because nobody ran it. It is `manual-runbook`.

## What was actually there

FR-149's own claims were rebuilt before planning, and three did not survive.
A fourth — inherited from FR-148 — only fell when the work forced the gate to be
executed rather than read. Recording them here because they are the interesting
part of this FR.

**The FR's orphan tier was wrong, and wrong in the direction that matters.** It
listed 14 bundles with no consumer. Two of them — `qa107-s1-parallel` and
`stagger-test-scenario3` — are the *named targets* of QA 196's negative
fixtures, and a third bundle, `cycle-overflow-test`, is named in that document's
expected results. QA 196 is FR-148's own verification document: the gate that
measured this residue had fixtures pointing at the very files the cleanup would
delete. The FR inherited the omission from the ledger's `consumers` field, which
has the same gap. Both were repaired.

**DD-158 miscounted the split.** "`behavior.captures` in nine, and
`generate_items` JSONPath post-actions in ten" survives neither derivation: by
rejection diagnostic it is 8 `[legacy_coordination_removed]`, 10
`[legacy_json_path_removed]` and 1 prehook schema drift; by file content it is
10 files containing `captures:`, because `wp05-items-select.yaml` and
`wp05-store-items-select.yaml` carry both constructs and are rejected on
whichever the `HashMap` merge reaches first. The paragraph also enumerated
19 + 5 + 4 + 1 + 1 + 2 = 32 against its own stated total of 31, because
`prehook-test.yaml` is inside the nineteen rather than beside them. Corrected in
DD-158 and in the FR-148 closure note that repeated it.

**A gate's failure was attributed by reading it.** FR-148 recorded
`test-wp05-integration.sh` as failing for the same reason and since the same
commit as `test-coordination-collapse.sh`. Neither half holds; the detail is
under "the three WP05 scenarios" below, because the correction only surfaced
when FR-149 had to make that gate green and therefore had to run it.

**The blast radius of a deletion was derived rather than guessed.** The FR said
this needed checking and had not checked it. Deleting all 19 bundles removes
**22 workflow IDs** from the set `scripts/qa-doc-lint.sh` derives from
`fixtures/manifests/bundles/*.yaml`. Two independent derivations agree exactly —
the lint's own `rg -A3 'kind: Workflow' | rg 'name:'`, and a Ruby
`YAML.load_stream` walk taking `metadata.name` — both giving 158 → 136 with zero
parse failures. Two of the 22 (`fixed_no_dynamic`, `wp05-verify-winner`) are
named by no `expect` in the ledger at all: a bundle defines more workflows than
its rejection diagnostic mentions, so deletion impact is counted per ID, not per
diagnostic.

Exactly one of the 22 collides with a QA document's `--workflow` reference:
`narrow-test`, in the document being superseded in the same commit.

## Decisions

### The three WP05 scenarios are excised, and the harness is rewritten

L1-C, L1-D and L2-A drive `generate_items` and `item_select`. There is no typed
replacement to point them at, because the primitive was retired rather than
re-implemented, so they are removed. L1-A (Store × Spawning) and L1-B (Store ×
Invariants) test primitives that still exist; retiring the whole script would
have traded two live primitives for three dead ones.

**Then running it found that FR-148's account of this gate was wrong twice
over.** DD-158 said it "runs `apply` wholesale on three rotted bundles and had
been unable to finish since the same commit". Executed, it dies in `ensure_db`
on `orchestrator init` with `daemon socket not found`, **before L1-A**, having
never reached the rotted bundles. The cause is `1be4666d` (2026-03-26), which
split the CLI from the daemon and made every `orchestrator` invocation a
control-plane client call — this script started no daemon. Four months longer
than recorded, and a different cause.

That claim was derived by reading the gate. This ledger's entire premise is that
reading a fixture is not the same as running the product against it, and the
claim was made the way the ledger exists to prevent. What made it plausible is
that the three rotted bundles really are at those lines and really would have
failed — a wrong cause that predicts the observed colour is the hardest kind to
notice.

Two more harness faults were hidden behind the first:

- `(cd core && cargo build --release)` builds the `agent-orchestrator` *library*
  package. `orchestrator` comes from `crates/cli` and `orchestratord` from
  `crates/daemon`, so `$ORCH` was whatever stale artifact happened to sit in
  `target/` — eight days old when measured. Now built by package, with both
  binaries asserted executable afterwards.
- `DB=data/agent_orchestrator.db` named a repository-local path the product
  stopped using when the runtime root became `ORCHESTRATORD_DATA_DIR`.

Isolation now follows `test-agent-driver-production-parity.sh`: a throwaway
`ORCHESTRATORD_DATA_DIR`, a daemon the script starts and reaps, and the data-dir
prefix **asserted rather than assumed** — if the exports failed to take, every
assertion below would still pass while the developer's runtime root took the
writes. Verified after the run: `~/.orchestratord` does not exist.

No `--bind`, and that took two attempts. With a TCP bind the daemon serves TCP
and never creates the UDS socket, so the CLI has nothing to dial; the parity
gate gets away with it because it also supplies a control-plane config and
connects over TLS. The first attempt here exported a control-plane config
pointing at a nonexistent file, which routed the client down the TLS branch —
an explicit config outranks the local socket in the client's priority list. The
readiness check now prints the *client's* error as well as the daemon's log,
because the daemon logged "listening on TCP" and looked perfectly healthy while
the CLI was dialling somewhere else.

The gate passes for the first time since 2026-03-26: SELECTED 2, PASS 8, FAIL 0,
summary line present. The `qa-gate-surface.json` entry is unchanged and still
`manual-runbook`.

One hardening while there, and it needed a second pass of its own. A `--layer`
or `--scenario` selection matching no scenario reached the summary with
`PASS: 0 FAIL: 0` and exited 0 — indistinguishable from a clean full run (§4.4
shape 5). The first version of the guard called `fail` as the last command of an
`if`, so under `set -e` the compound's non-zero status ended the run *before the
summary line* — the same defect, relocated. Found by running `--layer 2` and
reading the output rather than the exit code. `--layer 2` and `--scenario L1C`
are precisely the values this change made stale, so the check has a live subject
rather than a hypothetical one.

### The lint's workflow-ID check is scoped to `lifecycle: active`

The check requires every `--workflow <id>` in `docs/qa/orchestrator` to name a
workflow some bundle defines. Its premise is that the document's commands can be
pasted into a shell and run. That premise does not hold for a superseded
document, which describes a mechanism that was removed and names fixtures that
were deleted with it — so **retiring a construct properly is the thing that
turned the check red**. Scoping to active documents fixes it once instead of at
every future retirement, which was the alternative: editing the one offending
line, and editing the next one next time.

An exemption shaped like a subtree absorbs instances that do not exist yet and
never produces a line in any log (§4.4 shape 8), so this one carries three
obligations, and `scripts/qa/test-qa-doc-lint-workflow-scope.sh` holds a case
per obligation:

1. **Derived from the repository, never from a list** — from each document's own
   frontmatter, through the real YAML parser already in
   `scripts/qa/doc-lifecycle.rb`. Deliberately *not* from the committed
   `doc-lifecycle-index.json`, which would make this check's correctness depend
   on a different gate's freshness.
2. **Fails closed, and loudly** — a derivation that cannot run, output that will
   not parse, or a document absent from the result all mean *check it anyway*
   **and fail the lint**. A scope predicate is an assertion and deserves the same
   attack as an assertion (§4.4 shape 9).
3. **Visible** — the exempt set prints on every run, including when empty.

The check moved to `scripts/lib/qa_doc_workflow_ids.sh` so the fixtures can drive
it directly rather than inferring its behaviour from the whole lint's exit code.

The known cost, stated plainly: `qa-doc-lint.sh` now hard-depends on `ruby`. The
governance job installs it and `doc-lifecycle.rb` already required it, but on a
machine without ruby the lint now fails where it previously passed. That is the
fail-closed direction, chosen over silently checking everything.

### QA 196's fixtures derive their targets and their numbers

Three of its five scenarios named a file or restated a number. Re-run **as
written** against the post-deletion tree:

| | mutation | run | document claimed |
|---|---|---|---|
| 2a | filtered `stagger-test-scenario3` — matched 0 entries | red, on `rot ratchet: rotted_count says 18 but 0 entries are declared rotted` | `undeclared rejection: …stagger-test-scenario3.yaml` |
| 3 | rewrote `qa107-s1-parallel`'s `expect` — matched 0 entries | **green** | Fails |
| 5 | replaced the literal `"rotted_count": 19` — absent | **green** | Fails |

Two of three passed while proving nothing; the third failed through a branch it
never claimed to test, which reads as working. All three now derive their target
(`.bundles[0]`) and their expected message from the ledger at run time, and each
`grep` is the assertion rather than the exit code. Scenario 5 gets both ratchet
directions without ever computing `cur - 1`, which is now `-1` and would fail
`usize` deserialisation — reporting through a third branch unrelated to the
ratchet. Every negative scenario gained a before-run, because a gate already red
satisfies every assertion without the mutation having done anything.

Scenario 2b keeps its named target, and that is correct: its target must be a
bundle the product *accepts*, which is a property of one specific file. It is
guarded by two premise assertions — the file exists, and it is not already
declared — so a premise that stops holding becomes a failed assertion rather
than a vacuous pass. Its injected status changed from `rotted` to `fragment` so
the ratchet no longer fires alongside the stale-declaration violation the case
exists to show.

### `rotted_count` stays declared, at 0

Dropping the field would read the same as `0` while accepting the first
regression silently. A bundle that rots tomorrow has to move the number back up.

## Results

| | before | after |
|---|---|---|
| tracked bundles | 93 | 74 |
| accepted | 62 | 62 |
| declared invalid | 31 | 12 |
| `rotted_count` | 19 | 0 |
| workflow IDs in the lint's derived set | 158 | 136 |
| WP05 scenarios | 5 | 2 |
| WP05 gate outcome | dead since 2026-03-26, no summary line | `RESULT: ALL PASSED`, 8 assertions |
| `qa-doc-lint` `Unknown workflow ID` lines | 0 | 0 |

The accepted count did not move, because all 19 deleted bundles were rejected
ones.

## What this does not cover

**The exemption is only as good as supersession discipline.** A document flipped
to `superseded` purely to silence the check would be exempt. Two things bound
that: `doc-lifecycle.rb` (ci-required) requires a `superseded_by` that resolves
to a real file and rejects cycles, and the exempt set is printed on every run so
the list is readable rather than inferred. Neither prevents a determined
misuse; both make it visible.

**Prose counts too.** The check reads any `--workflow <id>` occurrence in a
document, including one inside explanatory text rather than a command. QA 84's
new banner contains such a mention and is exempt along with the rest of the
document. On an active document, prose describing an unknown ID would be
reported — noisy but in the safe direction.

**`scripts/qa-doc-lint.sh` is still absent from the enforcement manifest**
although `ci.yml:222` runs it. That is FR-147's subject and was left alone; the
consequence here is that a manifest-derived sweep does not schedule the lint,
and this FR's certification ran it explicitly.

**This says nothing about whether the remaining 12 declarations are still
true.** They were re-verified only in the sense that `fixture_corpus_tests`
passes, which is what it has always asserted.

## Provenance

Opened at FR-148's closure, with the measurement recorded in
[DD-158](158-fixture-bundle-validity.md) and
[QA 196](../../qa/orchestrator/196-fixture-bundle-validity.md). Verified by
[QA 197](../../qa/orchestrator/197-dd137-fixture-residue-retirement.md).
