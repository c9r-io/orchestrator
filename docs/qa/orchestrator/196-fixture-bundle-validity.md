---
lifecycle: active
related_fr: FR-148
---

# Orchestrator - Does The Product Still Accept What The Fixture Says?

**Module**: CI / Governance / Manifest fixtures
**Scope**: `core/src/fixture_corpus_tests.rs` and
`config/governance/fixture-bundle-validity.json`, over the 74 tracked bundles
under `fixtures/manifests/bundles/` (`git ls-files 'fixtures/manifests/bundles/*.yaml' | wc -l`,
at `c410d485`; 93 at `ef458f16`, before FR-149 deleted the 19 rotted ones)
**Scenarios**: 5
**Priority**: Medium

## Background

`scripts/qa/test-coordination-collapse.sh` could not finish for four days. A
bundle it applies carries `behavior.captures`, which DD-137 (`1b0937ca`,
2026-07-25) removed by design, and `apply` is all-or-nothing over a bundle — one
rejected Workflow took the whole file, so three of twelve assertions ran and the
summary line never printed.

The defect never needed the gate to run. The fixture said `behavior.captures`,
the validator rejected `behavior.captures`, and nothing in the repository put the
two side by side. This check is that comparison.

Design record: `docs/design_doc/orchestrator/158-fixture-bundle-validity.md`.

**Safety**: read-only against the working tree and self-referentially safe. Every
scenario runs `cargo test` or reads a file; the corpus check builds its own
`TestState` under `$TMPDIR`, no daemon starts, the runtime database is never
touched, no provider is invoked, nothing reaches the network. The negative
scenarios below edit `config/governance/fixture-bundle-validity.json` and restore
it with `git checkout --` in the same step.

## Why the assertions are shaped the way they are

**A rejection is not evidence on its own.** Capability validation runs *before*
the retirement checks (`core/src/config_load/validate/workflow_steps.rs:49-74`),
so a bundle that merely omits its Agent fails with `no agent supports capability`
— and an exit code cannot tell that apart from the retirement the fixture was
written to demonstrate. Every declared entry therefore names the diagnostic it
must fail by, and scenario 3 is the case that separates the two.

**The scope is derived, and a scope that derives nothing is a failure.** The
corpus comes from `git ls-files`. A `git` that cannot run, a pathspec that
matches nothing, or a ledger that will not parse each abort the test rather than
leaving it green over an empty comparison (§4.4 shape 7).

**No fixture here names its target, and none restates a number.** Every
mutation below derives what it touches from the ledger at run time — the first
declared entry, the current `rotted_count` — and every expected diagnostic is
built from what was derived.

This was not true when the document was written, and FR-149 is what proved it
had to be. Scenarios 2a, 3 and 5 originally named `stagger-test-scenario3`,
`qa107-s1-parallel`, and the literal string `"rotted_count": 19`. FR-149 deleted
the first two bundles and moved the number to 0, and all three fixtures were
re-run **as written** against the resulting tree:

| | what the mutation did | what the run reported | what the document claimed |
|---|---|---|---|
| 2a | filter matched 0 entries | red, on `rot ratchet: rotted_count says 18 but 0 entries are declared rotted` | `undeclared rejection: …stagger-test-scenario3.yaml` |
| 3 | loop matched 0 entries; ledger unchanged | **green** | Fails |
| 5 | literal absent; file unchanged | **green** | Fails |

Two of three passed while proving nothing, and the third failed through a branch
it never claimed to test — which reads as working. That is §4.4 shape 7 in all
three of its recorded forms, in a document whose own prose cited shape 7. A gate
whose subject is a number that is meant to move cannot have fixtures that only
work while it does not.

**The verdict depends on the base, so the base is declared.** Against the fully
seeded `TestState`, five bundles are rejected for `[SELF_REF_POLICY_VIOLATION]
... workflow 'basic'` — the scaffolding's own workflow, dragged in by a bundle
that introduces a self-referential workspace. That is a verdict about the
harness, not the fixture. `TestState::without_seeded_agents_and_workflows()`
removes it and makes the question "would a fresh daemon accept this standalone".

---

## Scenario 1: The whole corpus agrees with the ledger

**Steps**

```bash
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked_bundle_is_accepted_or_declared
```

**Expected result**

Passes. 74 bundles are validated against one shared `InnerState`; 62 are
accepted, and each of the 12 rejections matches a declaration in
`config/governance/fixture-bundle-validity.json` carrying its reason and the
diagnostic it fails by. Runtime under 3s.

The accepted count did not move when FR-149 deleted 19 bundles, because all 19
were rejected ones: 93 − 19 = 74 tracked, 31 − 19 = 12 declared, 62 accepted
throughout. If a future run reports a different accepted count, the corpus
gained or lost a bundle the product accepts — a fact this scenario should
surface rather than absorb.

---

## Scenario 2: The ledger and the tree disagree, in both directions

Two mutations, because the mismatch has two halves and only one of them is the
shape people expect. A fixture the product stopped accepting is the originating
ticket's shape; a declaration whose bundle the product now accepts is the half
that looks like a working exemption and is not.

### 2a — rejected, and in no declaration

The victim is `.bundles[0]`, read at run time — whichever entry that is. Every
declared entry is a bundle the product rejects, so removing any one of them
produces an undeclared rejection; naming one would only add a way to go stale.
`rotted_count` is recomputed from what survives rather than typed, so the
mutation cannot trip the ratchet by accident and report through that branch
instead — which is exactly how the previous version of this fixture failed.

**Steps**

```bash
P=config/governance/fixture-bundle-validity.json
VICTIM=$(jq -r '.bundles[0].path' "$P")
echo "victim: $VICTIM"

# Before-run. A gate already red before the mutation satisfies every assertion
# below without the mutation having done anything (§4.4 shape 7).
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked; echo "clean before mutation: exit=$?"

python3 - "$VICTIM" <<'PY'
import collections, json, sys
p = 'config/governance/fixture-bundle-validity.json'
d = json.load(open(p), object_pairs_hook=collections.OrderedDict)
before = len(d['bundles'])
d['bundles'] = [b for b in d['bundles'] if b['path'] != sys.argv[1]]
assert len(d['bundles']) == before - 1, 'the victim was not in the ledger; the fixture proves nothing'
d['rotted_count'] = sum(1 for b in d['bundles'] if b['status'] == 'rotted')
json.dump(d, open(p, 'w'), indent=2)
PY
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked 2>&1 | tee /tmp/qa196-2a.log; echo "exit=${PIPESTATUS[0]}"
grep -F "undeclared rejection: $VICTIM" /tmp/qa196-2a.log
git checkout -- "$P"
```

**Expected result**

The before-run passes. The mutated run fails, and the failure names the derived
victim *and* carries the diagnostic — the `grep` is the assertion, not the exit
code, because an exit code cannot say which branch produced it:

```
undeclared rejection: fixtures/manifests/bundles/<derived>.yaml is
rejected by the product and appears in no declaration:
[legacy_coordination_removed] workflow '…' step '…' ...
```

This is the shape the originating ticket had: a fixture the product stopped
accepting, with nothing looking.

### 2b — declared invalid, and accepted

**Steps**

```bash
P=config/governance/fixture-bundle-validity.json
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked; echo "clean before mutation: exit=$?"

python3 - <<'PY'
import json, collections
p = 'config/governance/fixture-bundle-validity.json'
d = json.load(open(p), object_pairs_hook=collections.OrderedDict)
victim = "fixtures/manifests/bundles/qa105-s1-capture-wrong-level.yaml"
import os
assert os.path.exists(victim), "the fixture's premise no longer holds: %s is gone" % victim
assert all(b['path'] != victim for b in d['bundles']), \
    "the fixture's premise no longer holds: %s is already declared" % victim
d['bundles'].append(collections.OrderedDict(
    path=victim, status="fragment", expect=["something"],
    reason="a plausible-sounding reason"))
# Status `fragment`, not `rotted`, so the ratchet stays satisfied and the only
# violation the run can produce is the stale declaration this case is about.
json.dump(d, open(p, 'w'), indent=2)
PY
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked 2>&1 | tee /tmp/qa196-2b.log; echo "exit=${PIPESTATUS[0]}"
grep -F "stale declaration: fixtures/manifests/bundles/qa105-s1-capture-wrong-level.yaml" /tmp/qa196-2b.log
git checkout -- "$P"
```

**Expected result**

The before-run passes. The mutated run fails with `stale declaration: ... is
declared invalid (Fragment) but the product accepts it — delete the entry rather
than leaving the reason to rot`, and the `grep` names the file.

This is the one case that still names its target, because its target is the
*point*: it must be a bundle the product **accepts**, and the two assertions
guarding it — the file exists, and it is not already declared — turn a premise
that stopped holding into a failed assertion rather than a vacuous pass.

The bundle chosen is not arbitrary: FR-148 listed
`qa105-s1-capture-wrong-level.yaml` among its four "intentionally invalid"
fixtures, and the product accepts it — a step-level `capture:` key is an unknown
field and is silently ignored. This is the half FR-133's `unmatched-skip` gap
recorded: an exemption that no longer exempts anything looks exactly like one
that does.

---

## Scenario 3: Rejected, declared, and still wrong

**Steps**

The target is `.bundles[0]`, read at run time. Its `expect` is replaced with a
tag no validator can emit, rather than with the *other* retirement tag — the
substitute must be wrong for whichever entry the derivation lands on, and
`[legacy_json_path_removed]` is the right answer for ten of the entries this
could pick.

**Steps**

```bash
P=config/governance/fixture-bundle-validity.json
TARGET=$(jq -r '.bundles[0].path' "$P")
echo "target: $TARGET"

cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked; echo "clean before mutation: exit=$?"

python3 - "$TARGET" <<'PY'
import collections, json, sys
p = 'config/governance/fixture-bundle-validity.json'
d = json.load(open(p), object_pairs_hook=collections.OrderedDict)
hits = [b for b in d['bundles'] if b['path'] == sys.argv[1]]
assert len(hits) == 1, 'the target is not in the ledger exactly once; the fixture proves nothing'
hits[0]['expect'] = ["[no_validator_emits_this] a diagnostic that cannot occur"]
json.dump(d, open(p, 'w'), indent=2)
PY
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked 2>&1 | tee /tmp/qa196-3.log; echo "exit=${PIPESTATUS[0]}"
grep -F "wrong diagnostic: $TARGET" /tmp/qa196-3.log
git checkout -- "$P"
```

**Expected result**

The before-run passes. The mutated run fails with `wrong diagnostic: <derived>
is declared to fail with one of ["[no_validator_emits_this] …"] but failed with:
…`, and the `grep` names the derived target.

**This is the scenario an exit-code check cannot produce.** The bundle *is*
rejected and *is* declared; only the reason differs. Without it, a fixture could
drift from one failure mode to another — from the retirement it demonstrates to a
missing agent, say — and the ledger would keep certifying it.

---


## Scenario 4: A retired construct appended to a live bundle

**Steps**

```bash
cargo test -p agent-orchestrator \
  fixture_corpus_tests::an_injected_retired_construct_is_rejected_by_its_own_diagnostic
```

**Expected result**

Passes. The fixture appends a Workflow carrying `behavior.captures` to the first
bundle that is accepted and undeclared — derived, never named — and asserts two
things: the validator returns an error containing **both**
`[legacy_coordination_removed]` and the injected workflow's name, and the
evaluator reports that bundle as an undeclared rejection.

The mutation is an *appended document*, not an edited step, because that is the
shape the regression actually takes: someone adds a workflow without knowing the
construct is gone. The injected step sets `command:`, which makes it
self-contained so capability validation is skipped — otherwise `no agent supports
capability` fires first and the fixture would be asserting the wrong thing.

---

## Scenario 5: The evaluator's own rules, and the ratchet end to end

**Steps**

Both directions, neither of them typed. `cur` is read from the ledger; the two
mutations move the declared count up and the observed count up, so the ratchet
is exercised from both sides without ever needing `cur - 1` — which would be
`-1` now that FR-149 has driven `rotted_count` to 0, and `usize` would reject it
during deserialisation, failing through a third branch that has nothing to do
with the ratchet.

**Steps**

```bash
P=config/governance/fixture-bundle-validity.json
cargo test -p agent-orchestrator fixture_corpus_tests::evaluator

CUR=$(jq -r '.rotted_count' "$P")
echo "declared rotted at HEAD: $CUR"
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked; echo "clean before mutation: exit=$?"

# Direction 1 — the ledger claims more rot than it declares.
python3 - "$CUR" <<'PY'
import collections, json, sys
p = 'config/governance/fixture-bundle-validity.json'
d = json.load(open(p), object_pairs_hook=collections.OrderedDict)
d['rotted_count'] = int(sys.argv[1]) + 1
json.dump(d, open(p, 'w'), indent=2)
PY
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked 2>&1 | tee /tmp/qa196-5a.log; echo "exit=${PIPESTATUS[0]}"
grep -F "rot ratchet: rotted_count says $((CUR + 1)) but $CUR entries are declared rotted" /tmp/qa196-5a.log
git checkout -- "$P"

# Direction 2 — an entry becomes rotted and the count is left behind.
python3 - <<'PY'
import collections, json
p = 'config/governance/fixture-bundle-validity.json'
d = json.load(open(p), object_pairs_hook=collections.OrderedDict)
victim = next(b for b in d['bundles'] if b['status'] != 'rotted')
victim['status'] = 'rotted'
json.dump(d, open(p, 'w'), indent=2)
PY
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked 2>&1 | tee /tmp/qa196-5b.log; echo "exit=${PIPESTATUS[0]}"
grep -F "rot ratchet: rotted_count says $CUR but $((CUR + 1)) entries are declared rotted" /tmp/qa196-5b.log
git checkout -- "$P"
```

**Expected result**

The 10 evaluator tests pass, covering: a clean corpus; a stale declaration; a
wrong diagnostic; a diagnostic matched from any position in the error list, not
only the first (the merge walks a `HashMap`, and `cycle-overflow-test.yaml` was
measured naming a different workflow on two consecutive runs); an undeclared
rejection whose message carries the diagnostic; a declared path outside the
corpus; the ratchet in both directions; a blank `reason`; a blank or empty
`expect` — the dangerous half, since one blank string matches every error and
would turn the entry into a blanket acceptance of any future rejection; and a
duplicated path, which a map would otherwise swallow.

The before-run passes. Both mutations then fail on `rot ratchet`, and each
`grep` builds the exact expected sentence from `CUR` — so the assertion is the
message, not the exit code, and it moves with the ledger.

Equality, not a ceiling: retiring one rotted fixture has to move the number
down, and FR-149 moving it from 19 to 0 is the mechanism working as designed.
That is also what made the previous version of this scenario vacuous — it
searched for the literal `"rotted_count": 19`, and after FR-149 the search
matched nothing, the file was never written, and both iterations reported green
while asserting nothing. Compare FR-133's `deny.toml`, where 48 crates carry 70
individually written reasons for the same purpose.

The unit test and the end-to-end runs are both here on purpose — the unit test
proves the rule, the two mutations prove the rule is wired to the real ledger.

---

## Checklist

- [ ] Scenario 1 — the corpus and the ledger agree at `HEAD`, 62 accepted / 12 declared
- [ ] Scenario 2a — an undeclared rejection fails, and the message names the **derived** victim
- [ ] Scenario 2b — a declaration whose bundle now validates fails, and both premise assertions held
- [ ] Scenario 3 — a bundle rejected for a reason other than the declared one fails, naming the derived target
- [ ] Scenario 4 — an injected retired construct is named by `[legacy_coordination_removed]`
- [ ] Scenario 5 — the 10 evaluator tests pass and the ratchet trips in both directions, each `grep` built from `CUR`
- [ ] Every negative scenario's **before-run passed** — a gate already red proves nothing about the mutation
- [ ] `config/governance/fixture-bundle-validity.json` restored (`git status --porcelain` empty)
- [ ] `cargo test --workspace --exclude orchestrator-gui` and strict Clippy green

---

## Known limits

**This catches fixture rot, not assertion rot.** A gate shell whose assertion
reads a shape the product moved is invisible to any static comparison. The
ticket that produced FR-148 contained one: `normalize_preserved_channels` moved
`goal` and three sandbox signals into a typed carrier and the assertion still
queried the generic variable table. Nothing here would have seen it.

**A bundle can be `environment` for a reason that stops being true.** Four
entries depend on an ambient path or a base policy — if a developer happens to
have `/tmp/test-ws` on disk, `test-workspace.yaml` validates and scenario 2b's
rule fires. That is the correct direction (loud, not silent), but it is a
locally-red-in-CI-green case worth recognising rather than re-running.

**`rotted_count: 0` is a claim, not an absence.** FR-148 froze 19 entries
declaring constructs DD-137 removed; FR-149 deleted all 19 bundles and their
entries. The field stays declared at 0 rather than being dropped, because a
bundle that rots tomorrow has to move it back up — and a check that simply
stopped looking would read the same while accepting the first regression
silently.

**Deleting a bundle moves something outside this ledger.**
`scripts/qa-doc-lint.sh` derives its set of known workflow IDs from
`fixtures/manifests/bundles/*.yaml` by glob, so every bundle feeds that check
even when nothing references it as a fixture. FR-149's 19 deletions removed 22
workflow IDs from that set. Nothing here would have caught the resulting
`Unknown workflow ID`; `scripts/qa/test-qa-doc-lint-workflow-scope.sh` is where
that half now lives.

**A fixture that names its target is one revision from proving nothing, and
this document was the proof.** Three of its five scenarios named a file or
restated a number, and FR-149 moved all three. The general rule is in §4.4
shape 7; the specific residue is that a negative fixture belongs to the gate it
attacks, not to the tree it happened to be written against.
