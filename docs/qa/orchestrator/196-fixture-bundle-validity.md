---
lifecycle: active
related_fr: FR-148
---

# Orchestrator - Does The Product Still Accept What The Fixture Says?

**Module**: CI / Governance / Manifest fixtures
**Scope**: `core/src/fixture_corpus_tests.rs` and
`config/governance/fixture-bundle-validity.json`, over the 93 tracked bundles
under `fixtures/manifests/bundles/` (`git ls-files 'fixtures/manifests/bundles/*.yaml' | wc -l`,
at `ef458f16`)
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

**The injection fixture does not name its target.** It mutates the first bundle
the product accepts and the ledger does not mention — derived at run time. A
fixture that names a file goes stale the day the file moves, and §4.4 shape 7
records eight of nine such fixtures staying green while blind.

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

Passes. 93 bundles are validated against one shared `InnerState`; 62 are
accepted, and each of the 31 rejections matches a declaration in
`config/governance/fixture-bundle-validity.json` carrying its reason and the
diagnostic it fails by. Runtime under 3s.

---

## Scenario 2: The ledger and the tree disagree, in both directions

Two mutations, because the mismatch has two halves and only one of them is the
shape people expect. A fixture the product stopped accepting is the originating
ticket's shape; a declaration whose bundle the product now accepts is the half
that looks like a working exemption and is not.

### 2a — rejected, and in no declaration

**Steps**

```bash
python3 - <<'PY'
import json, collections
p = 'config/governance/fixture-bundle-validity.json'
d = json.load(open(p), object_pairs_hook=collections.OrderedDict)
d['bundles'] = [b for b in d['bundles'] if 'stagger-test-scenario3' not in b['path']]
d['rotted_count'] = 18
json.dump(d, open(p, 'w'), indent=2)
PY
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked; echo "exit=$?"
git checkout -- config/governance/fixture-bundle-validity.json
```

**Expected result**

Fails, and the failure names the file *and* carries the diagnostic:

```
undeclared rejection: fixtures/manifests/bundles/stagger-test-scenario3.yaml is
rejected by the product and appears in no declaration:
[legacy_coordination_removed] workflow 'stagger-step-override' step 'process' ...
```

This is the shape the originating ticket had: a fixture the product stopped
accepting, with nothing looking.

### 2b — declared invalid, and accepted

**Steps**

```bash
python3 - <<'PY'
import json, collections
p = 'config/governance/fixture-bundle-validity.json'
d = json.load(open(p), object_pairs_hook=collections.OrderedDict)
d['bundles'].append(collections.OrderedDict(
    path="fixtures/manifests/bundles/qa105-s1-capture-wrong-level.yaml",
    status="rotted", expect=["something"], reason="a plausible-sounding reason"))
d['rotted_count'] = 20
json.dump(d, open(p, 'w'), indent=2)
PY
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked; echo "exit=$?"
git checkout -- config/governance/fixture-bundle-validity.json
```

**Expected result**

Fails with `stale declaration: ... is declared invalid (Rotted) but the product
accepts it — delete the entry rather than leaving the reason to rot`.

The bundle chosen is not arbitrary: FR-148 listed
`qa105-s1-capture-wrong-level.yaml` among its four "intentionally invalid"
fixtures, and the product accepts it — a step-level `capture:` key is an unknown
field and is silently ignored. This is the half FR-133's `unmatched-skip` gap
recorded: an exemption that no longer exempts anything looks exactly like one
that does.

---

## Scenario 3: Rejected, declared, and still wrong

**Steps**

```bash
python3 - <<'PY'
import json, collections
p = 'config/governance/fixture-bundle-validity.json'
d = json.load(open(p), object_pairs_hook=collections.OrderedDict)
for b in d['bundles']:
    if 'qa107-s1-parallel' in b['path']:
        b['expect'] = ["[legacy_json_path_removed] workflow 'qa107-parallel-guard' step 'process'"]
json.dump(d, open(p, 'w'), indent=2)
PY
cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked; echo "exit=$?"
git checkout -- config/governance/fixture-bundle-validity.json
```

**Expected result**

Fails with `wrong diagnostic: ... is declared to fail with one of
["[legacy_json_path_removed] ..."] but failed with:
[legacy_coordination_removed] ...`.

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

```bash
cargo test -p agent-orchestrator fixture_corpus_tests::evaluator
for n in 18 20; do
  python3 -c "
p='config/governance/fixture-bundle-validity.json'
s=open(p).read(); open(p,'w').write(s.replace('\"rotted_count\": 19', '\"rotted_count\": $n', 1))"
  cargo test -p agent-orchestrator fixture_corpus_tests::every_tracked; echo "n=$n exit=$?"
  git checkout -- config/governance/fixture-bundle-validity.json
done
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

Then both loop iterations fail with `rot ratchet: rotted_count says <n> but 19
entries are declared rotted`. Equality, not a ceiling: retiring one rotted
fixture has to move the number down, so the debt cannot be quietly carried
forward. Compare FR-133's `deny.toml`, where 48 crates carry 70 individually
written reasons for the same purpose.

The unit test and the end-to-end run are both here on purpose — the unit test
proves the rule, the loop proves the rule is wired to the real ledger.

---

## Checklist

- [ ] Scenario 1 — the corpus and the ledger agree at `HEAD`, 62 accepted / 31 declared
- [ ] Scenario 2a — an undeclared rejection fails, and the message carries the diagnostic
- [ ] Scenario 2b — a declaration whose bundle now validates fails
- [ ] Scenario 3 — a bundle rejected for a reason other than the declared one fails
- [ ] Scenario 4 — an injected retired construct is named by `[legacy_coordination_removed]`
- [ ] Scenario 5 — the 10 evaluator tests pass and the ratchet trips in both directions
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

**19 rotted entries are recorded debt, not a passing grade.** They declare
constructs DD-137 removed; the ratchet freezes them and FR-149 retires them.
