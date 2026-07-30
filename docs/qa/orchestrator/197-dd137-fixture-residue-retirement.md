---
lifecycle: active
related_fr: FR-149
self_referential_safe: true
---

# Orchestrator - Retiring The DD-137 Fixture Residue

**Module**: CI / Governance / Manifest fixtures / QA documentation lint
**Scope**: the deletion of the 19 `rotted` bundles, the WP05 gate excision, and
the `lifecycle: active` scope FR-149 gave `qa-doc-lint.sh`'s workflow-ID
cross-reference
**Scenarios**: 5
**Priority**: Medium

## Background

DD-137 (`1b0937ca`, 2026-07-25) retired `behavior.captures` and the
`GenerateItems` / `SpawnTasks` JSONPath post-actions. FR-148 measured what was
left behind and froze it at `rotted_count: 19`. FR-149 deleted it.

Three things made the deletion more than a `git rm`. Deleting a bundle removes
its workflows from the set `qa-doc-lint.sh` derives by glob, which can turn a QA
document's `--workflow <id>` into an `Unknown workflow ID`. QA 196 — the
verification document for the gate that measured this residue — had negative
fixtures naming two of the bundles about to be deleted, and a third restating
`rotted_count: 19`. And the WP05 gate, which FR-148 recorded as failing on three
of those bundles, turned out to be failing four months earlier for an unrelated
reason — discovered only because making it green meant running it.

Design record: `docs/design_doc/orchestrator/159-dd137-fixture-residue-retirement.md`.

**Safety**: scenarios 1, 2, 3 and 5 are read-only or mutate only copies under
`$TMPDIR`, restoring with `git checkout --` in the same step. Scenario 4 runs
the WP05 gate, which builds both binaries and starts a daemon — under a
throwaway `ORCHESTRATORD_DATA_DIR` beneath `$TMPDIR`, reaped on exit. **The
runtime database at `~/.orchestratord/` is never opened**, and the scenario
asserts that afterwards rather than trusting the exports. No provider is
reachable: both bundles drive self-contained `command:` steps. Nothing reaches
the network — the daemon runs without `--bind`, over its unix socket only.

---

## Scenario 1: The corpus, the ledger and the count agree at zero

**Steps**

```bash
jq '{rotted_count, declared: (.bundles | length), by_status: (.bundles | group_by(.status) | map({(.[0].status): length}) | add)}' \
  config/governance/fixture-bundle-validity.json
git ls-files 'fixtures/manifests/bundles/*.yaml' | wc -l
cargo test -p agent-orchestrator fixture_corpus
```

**Expected result**

`rotted_count` is `0` and no entry carries `status: "rotted"`. 12 declarations
remain: 5 `fragment`, 4 `environment`, 2 `intentional`, 1 `dependent`. 74
bundles are tracked. All 12 `fixture_corpus` tests pass, which is what
establishes that the 12 declarations still match the 12 rejections — the count
above is a claim and the test is the check.

93 − 19 = 74 and 31 − 19 = 12; the accepted count is unchanged at 62, because
every deleted bundle was a rejected one. An accepted count that moved would mean
the corpus gained or lost a bundle the product accepts.

---

## Scenario 2: What left the workflow-ID set, by two derivations

The subject is a *set difference*, so it is computed twice by different means.
A single derivation of a set is how FR-144 got 17 where the answer was 39.

**Steps**

Both revisions are pinned: `293671cf` is the commit before the deletion,
`c410d485` is the deletion itself. Each derivation is run against a worktree of
each revision, so nothing depends on the current checkout.

```bash
BEFORE_REV=293671cf   # last commit with all 93 bundles
AFTER_REV=c410d485    # the deletion
W=$(mktemp -d)
git worktree add -q --detach "$W/before" "$BEFORE_REV"
git worktree add -q --detach "$W/after"  "$AFTER_REV"

# Derivation A — the lint's own extraction, verbatim.
extract_a() { (cd "$1" && rg -A3 'kind: Workflow' fixtures/manifests/bundles/*.yaml \
  | rg 'name:' | sed 's/.*name: //' | sort -u); }

# Derivation B — a real YAML parse, taking metadata.name from every Workflow document.
extract_b() { (cd "$1" && ruby -ryaml -e '
  names = []
  Dir.glob("fixtures/manifests/bundles/*.yaml").sort.each do |f|
    YAML.load_stream(File.read(f)) do |d|
      next unless d.is_a?(Hash) && d["kind"] == "Workflow"
      n = d.dig("metadata", "name")
      names << n if n
    end
  end
  puts names.uniq.sort
'); }

for fn in extract_a extract_b; do
  $fn "$W/before" > "$W/$fn.before"; $fn "$W/after" > "$W/$fn.after"
  echo "$fn: before=$(wc -l < "$W/$fn.before") after=$(wc -l < "$W/$fn.after") \
departed=$(comm -23 "$W/$fn.before" "$W/$fn.after" | wc -l)"
done
diff "$W/extract_a.before" "$W/extract_b.before" && echo "A and B agree on the before set"
diff <(comm -23 "$W/extract_a.before" "$W/extract_a.after") \
     <(comm -23 "$W/extract_b.before" "$W/extract_b.after") && echo "A and B agree on the departed set"
comm -23 "$W/extract_a.before" "$W/extract_a.after"

git worktree remove --force "$W/before"; git worktree remove --force "$W/after"; rm -rf "$W"
```

**Expected result**

Both derivations agree: 158 workflow IDs before, 136 after, **22 departed**, zero
parse failures. The 22:

```
fixed_no_dynamic  fixed_with_dynamic_items  infinite_with_dynamic_items  narrow-test
prehook_test  qa107-parallel-guard  s1-mixed-text  s2-fenced-block  s3-pure-json
s4-malformed-json  s5-multi-json  s5_pipeline_var  stagger-no-delay
stagger-sequential-ignored  stagger-step-override  stagger-workflow-level
test_s3_correct  test_s5_prehook_declared  wp05-items-invariant  wp05-items-select
wp05-store-items-select  wp05-verify-winner
```

`fixed_no_dynamic` and `wp05-verify-winner` appear in no ledger `expect`: a
bundle defines more workflows than its rejection diagnostic names, so impact is
counted per ID, not per diagnostic.

---

## Scenario 3: `qa-doc-lint` is green, and the exemption is why

Green is the uninteresting half. The assertion that matters is that the
exemption is **load-bearing** — that removing it turns the tree red, on exactly
one document and no others. Without this, "the lint passes" is equally
consistent with the exemption doing nothing.

**Steps**

```bash
bash scripts/qa-doc-lint.sh > /tmp/lint.log 2>&1; echo "exit=$?"
grep -c 'Unknown workflow ID' /tmp/lint.log
sed -n '/exempt (lifecycle/,+4p' /tmp/lint.log

# Neutralise the exemption in a scratch copy — the working tree is not touched.
sed 's|if \[\[ -n "$superseded_docs" \]\] && rg -qxF "$file" <<< "$superseded_docs"; then|if false; then|' \
  scripts/lib/qa_doc_workflow_ids.sh > /tmp/lib-noexempt.sh
grep -c 'if false; then' /tmp/lib-noexempt.sh   # must be 1; 0 means the sed missed and the run proves nothing
bash -c '. /tmp/lib-noexempt.sh && qa_doc_workflow_ids_check "[no-exempt]"' > /tmp/noexempt.log 2>&1; echo "exit=$?"
grep 'Unknown workflow ID' /tmp/noexempt.log

bash scripts/qa/test-qa-doc-lint-workflow-scope.sh; echo "exit=$?"
```

**Expected result**

The lint exits 0 with zero `Unknown workflow ID` lines, and names its exempt
set: QA 83, 84 and 92.

With the exemption neutralised the check exits 1 and reports `narrow-test` in
`84-generate-items-regression-narrowing.md` — and nothing else. One document,
which is the one superseded in the same commit as the bundle deletion.

The scope gate reports `6 passed, 0 failed`.

> Run `bash scripts/qa-doc-lint.sh` before the deletion commit as well. Zero
> `Unknown workflow ID` lines before **and** after is the FR's requirement 4;
> the after-run alone cannot distinguish "no collision" from "the check stopped
> looking".

---

## Scenario 4: The WP05 gate reaches its summary line again

**Steps**

```bash
bash scripts/qa/test-wp05-integration.sh > /tmp/wp05.log 2>&1; echo "exit=$?"
grep -E 'SELECTED:|PASS:|FAIL:|RESULT:' /tmp/wp05.log
grep -c 'items_generated\|item_select' /tmp/wp05.log

# Isolation, asserted after the fact rather than assumed from the exports.
ls -d ~/.orchestratord 2>&1        # must not exist
ls -d data 2>&1                    # must not exist
git status --porcelain             # must be empty

# The stale-selection check has a live subject: both values were valid before FR-149.
for sel in "--layer 2" "--scenario L1C"; do
  bash scripts/qa/test-wp05-integration.sh $sel > /tmp/wp05-sel.log 2>&1
  echo "$sel exit=$?"
  grep -E 'no scenario matched|SELECTED:|RESULT:' /tmp/wp05-sel.log
done
```

**Expected result**

The full run exits 0 and **prints its summary line** — `SELECTED: 2`, `PASS: 8`,
`FAIL: 0`, `RESULT: ALL PASSED`. The summary's presence is the assertion, not
the exit code: §4.6 condition 5 is that a missing final line means the run
terminated early regardless of what it reported, and this gate spent four months
exiting non-zero with no summary at all.

**Not since 2026-07-25, and not at L1-C.** FR-148 recorded it that way from
reading the script. Run, it dies in `ensure_db` on `orchestrator init` with
`daemon socket not found`, *before* L1-A, and has since `1be4666d`
(2026-03-26) split the CLI from the daemon. The three rotted bundles really were
at :250/282/312 and really would have failed — which is why a wrong cause
survived being written into a design record. FR-149 rewrote the harness; that
is what this scenario now verifies.

No line mentions `items_generated` or `item_select`; those scenarios are gone.

`--layer 2` and `--scenario L1C` each exit 1 with
`no scenario matched the selection` **and still print the summary**
(`SELECTED: 0`, `RESULT: FAILED`). Both were valid selections before FR-149.
Check the summary, not just the exit code: the first version of this guard
called `fail` as the last command of an `if`, and `set -e` ended the run on the
compound's status before the summary printed — the defect the guard exists to
prevent, in the guard.

---

## Scenario 5: QA 196's fixtures survive the number they are about

The gate's subject is `rotted_count`, and FR-149 moved it from 19 to 0. A
fixture that restates the number works only while it does not move.

**Steps**

Run scenarios 2a, 2b, 3 and 5 of
`docs/qa/orchestrator/196-fixture-bundle-validity.md` exactly as written, and for
each record the **before-run** as well as the mutated run.

**Expected result**

Every before-run passes — a gate already red satisfies each assertion below
without the mutation having done anything.

| | derived from | must fail with |
|---|---|---|
| 2a | `.bundles[0].path` | `undeclared rejection: <that path>` |
| 2b | a named bundle the product accepts, guarded by two premise assertions | `stale declaration: …qa105-s1-capture-wrong-level.yaml`, and no other violation |
| 3 | `.bundles[0].path` | `wrong diagnostic: <that path>` |
| 5 | `cur = .rotted_count` | `rot ratchet: rotted_count says <cur+1> but <cur> entries are declared rotted`, then `says <cur> but <cur+1>` |

Each `grep` is the assertion. An exit code cannot say which branch produced it,
and that is precisely how the previous version of 2a failed: it went red on the
ratchet while claiming to test the undeclared-rejection branch.

`git status --porcelain` is empty afterwards.

---

## Checklist

- [ ] Scenario 1 — `rotted_count: 0`, 12 declarations, 74 bundles, 12/12 `fixture_corpus` tests
- [ ] Scenario 2 — 158 → 136 by two independent derivations, 22 departed, 0 parse failures
- [ ] Scenario 3 — lint green with 0 unknown IDs before **and** after; neutralising the exemption reports `narrow-test` and nothing else; scope gate 6/6
- [ ] Scenario 4 — WP05 prints `RESULT: ALL PASSED` **with its summary line**; `--layer 2` and `--scenario L1C` both fail
- [ ] Scenario 5 — all four QA 196 fixtures fail on the branch they name, each after a passing before-run
- [ ] `cargo test --workspace --exclude orchestrator-gui` and strict Clippy green
- [ ] `git status --porcelain` empty at start and end; `git rev-parse HEAD` unchanged

---

## Known limits

**The exemption trusts supersession.** A document flipped to `superseded` merely
to silence the workflow-ID check would be exempt. `doc-lifecycle.rb` requires a
`superseded_by` that resolves to a real file and rejects cycles, and the exempt
set prints on every lint run — visible, not prevented.

**Scenario 4 cannot run in CI.** The WP05 gate is `manual-runbook` because it
builds a release binary and drives a live CLI. That is why it went four months
unnoticed, and nothing here changes it; FR-149 removed the broken scenarios but
not the reason the breakage was invisible.

**`scripts/qa-doc-lint.sh` is not in the enforcement manifest** although
`ci.yml` runs it, so a manifest-derived sweep will not schedule it. FR-147 owns
that gap. Any certification of this FR has to invoke the lint explicitly.

**Two ledgers go stale on any `ci.yml` change and can only be refreshed after a
real run.** `ci-liveness.rb` and `ci-cost.rb` read `gh run` on `main`, so
between a workflow edit and the next merged CI run they report the edit rather
than a defect. FR-149 added a governance step, so both applied; the cost ledger
records the new step under `pendingMeasurement` and the liveness ledger is
refreshed from the run that certifies this change.
