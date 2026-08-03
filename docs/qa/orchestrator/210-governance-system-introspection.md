---
lifecycle: active
related_fr: FR-158
self_referential_safe: true
---

# Orchestrator - Governance System Introspection

**Module**: Governance ratchets / Enforcement manifest / Manual-gate freshness
**Scope**: Masked ratchet counting, script surface completeness, manual-runbook
execution freshness, the governance expansion boundary
**Scenarios**: 5
**Priority**: Medium

## Background

FR-158 closed three structural weaknesses in the governance machinery and wrote
down the boundary that decides whether the next gate gets built. See
[DD-172](../../design_doc/orchestrator/172-governance-expansion-boundary.md) for
the rule and
[DD-173](../../design_doc/orchestrator/173-ratchet-masking-and-surface-closure.md)
for the repairs.

Every scenario below is read-only or runs against a throwaway tree under
`$TMPDIR`. Nothing here starts a daemon, touches `~/.orchestratord`, or writes to
the working tree.

## Scenario 1: Ratchets count code, not prose

**Steps**

```bash
ruby scripts/qa/coordination-governance.rb
ruby scripts/qa/persistence-dependency.rb
ruby scripts/qa/core-boundary.rb
jq -c '.sourceBaseline' config/governance/coordination-collapse-ledger.json
jq -c '.totals' config/governance/persistence-dependency-ledger.json
```

**Expected result**

All three exit 0. The coordination baseline reads `capturesOrJsonPath: 12`,
`pipelineVariables: 26`, `celInterpreter: 8`, `legacyRunnerSelection: 0` — down
from 23 / 29 / 9 / 0, every change a decrease because masking only ever removes
prose. The persistence totals read `rusqlite: 207` (from 215) and **`sql: 597`,
unchanged**: the SQL ruler reads the unmasked source because its pattern anchors
inside a string literal, and masking it would report 0 statements for every file
in the workspace.

## Scenario 2: The prose fixtures fail on the defect they describe

The negative fixtures matter more than the counts here, because a ratchet that
reads the wrong number still passes against a ledger regenerated from the same
wrong reading.

**Steps**

```bash
bash scripts/qa/test-persistence-dependency.sh
```

Then confirm the two cases are load-bearing rather than decorative, by reverting
each half of the repair in a scratch copy and re-running:

```bash
# M1: driver ruler back to unmasked source
perl -0pi -e 's/driver = strip_test_modules\(masked, masked\)\.scan/driver = source.scan/' \
  scripts/qa/persistence-dependency.rb
ruby scripts/qa/persistence-dependency.rb --emit-baseline --write
bash scripts/qa/test-persistence-dependency.sh; echo "exit=$?"
git checkout scripts/qa/persistence-dependency.rb config/governance/persistence-dependency-ledger.json
```

**Expected result**

Unmutated: `FR-136 persistence dependency chokepoint: 24 passed, 0 failed`,
exit 0, summary line present.

Under M1: **case 20 alone fails** — `23 passed, 1 failed` — with the diagnostic
"the driver named in a comment or a string was counted as a reference … masking
has been dropped". Under the mirror mutation that unifies both rulers onto the
masked source, **case 21 alone fails** among the new cases and the ledger's
`sql` total emits as `0`. Two mutations, two different signatures.

> Restore the ledger with `git checkout`, **not** by re-emitting. A mutated gate
> drops entries whose counts fall to zero and their reviewed `category` fields go
> with them; regenerating afterwards cannot reinvent them.

## Scenario 3: Every script under scripts/ is classified, engine included

**Steps**

```bash
bash scripts/qa/test-qa-gate-surface.sh
bash scripts/qa/test-qa-gate-surface.sh --fixture-test

# The acceptance criterion, derived rather than asserted:
comm -23 \
  <(find scripts -type f \( -name '*.sh' -o -name '*.rb' -o -name '*.mjs' \) | sort) \
  <(jq -r '(.scripts[].path), (.supportFiles[].path)' \
      config/governance/qa-gate-surface.json | sort -u)
```

**Expected result**

Main mode `16 passed, 0 failed`; fixture mode `44 passed, 0 failed` with the
summary line present. The `comm` prints **nothing** — no tracked script under
`scripts/` is unclassified, including all twelve shared libraries under
`scripts/lib` that the ci-required gates source.

Fixtures 25, 26, 27, 28, 28b, 31 and 32 each fail exactly one check
(`expect_fail` re-runs every other check on the same tree to prove isolation).

## Scenario 4: Manual-gate freshness records real runs

**Steps**

```bash
ruby scripts/qa/manual-gate-freshness.rb; echo "exit=$?"

# The set-agreement half is what can fail. Break it in a scratch copy:
cp config/governance/manual-gate-freshness.json /tmp/fresh-backup.json
ruby -rjson -e '
  p = "config/governance/manual-gate-freshness.json"
  d = JSON.parse(File.read(p)); d["gates"].delete(d["gates"].keys.first)
  File.write(p, JSON.pretty_generate(d) + "\n")'
ruby scripts/qa/manual-gate-freshness.rb; echo "exit=$?"
cp /tmp/fresh-backup.json config/governance/manual-gate-freshness.json
```

**Expected result**

Unmodified: exit 0, a 35-row table, `35 of 35 gate(s) stale or never recorded`,
and the closing line `freshness report only; staleness does not fail this check`.
Staleness must **not** change the exit status — that is the design, not a gap.

With a gate removed from the ledger: exit **1**, naming the missing path and
stating that a gate missing here is missing from every report, which reads
exactly like a gate that is fresh.

The composition itself is covered behaviourally inside
`test-qa-gate-surface.sh --fixture-test`: a probe gate with its own
`trap cleanup EXIT` that exits 7 must record `exitStatus: 7`, leave its cleanup
marker, and still exit 7. Any one of those three alone passes on a broken
composition — the record alone by clobbering the cleanup, the cleanup alone by
never arming.

## Scenario 5: The expansion boundary binds

**Steps**

```bash
ruby scripts/qa/ci-cost.rb
jq -r '.budget.jobs, .budget.seconds' config/governance/ci-step-cost.json
jq -r '[.jobs["governance"].seconds, .jobs["ci-environment-parity"].seconds] | add' \
  config/governance/ci-step-cost.json
jq '.shapeRationale.exemptions | length' config/governance/qa-gate-surface.json
```

**Expected result**

`ci-cost.rb` exits 0. The budget covers `governance` and `ci-environment-parity`
with a 2700s ceiling; the pair sums to **1793s**, so the ceiling binds with
907s of headroom. The all-job total is 3113s and is *not* what the ceiling
covers — FR-158 originally compared those two numbers and reported a breach that
had not happened.

`shapeRationale.exemptions` has **52** entries, the ci-required gates that
predate the rule. This list may only shrink: fixture 31 removes one and the check
fails; fixture 32 adds a path that is not a ci-required gate and the check fails.

## Checklist

- [ ] `coordination-governance.rb`, `persistence-dependency.rb`,
      `core-boundary.rb` all exit 0
- [ ] Coordination baseline is 12 / 26 / 8 / 0; persistence is 207 driver
      references and **597 SQL statements unchanged**
- [ ] `test-persistence-dependency.sh` reports 24 passed, 0 failed with the
      summary line present
- [ ] M1 fails case 20 alone; the ruler-unification mutation fails case 21 alone
- [ ] `test-qa-gate-surface.sh` 16/16 main, 44/0 fixtures, summary line present
- [ ] The `find scripts` versus manifest `comm` prints nothing
- [ ] `manual-gate-freshness.rb` exits 0 and reports 35 of 35 stale
- [ ] Removing a gate from the freshness ledger makes it exit 1
- [ ] `ci-cost.rb` exits 0; budgeted pair is 1793s against 2700s
- [ ] `shapeRationale.exemptions` has 52 entries

## Known limits

- **`ci-liveness.rb` is red, and was red before this work.** Its job records were
  taken at `45fbf3c4`, before `.github/workflows/ci.yml` last changed at
  `ceccf4f5`; both predate FR-158. Recorded in DD-171. Refreshing it needs a real
  CI run, so it is left failing rather than papered over.
- **The freshness ledger starts empty**, so scenario 4 verifies the mechanism and
  the set-agreement rule, not any real gate's recency. The first genuine
  recencies appear as the 35 gates are next run by a human.
- **Scenario 2's mutation edits tracked files.** Run it on a clean worktree and
  restore with `git checkout`, never by re-emitting the ledger.
- **A bulk sweep of the ci-required set must run with `CI=1`.** Seven ci-required
  call sites invoke a manual-runbook gate, and outside CI the invoked gate
  records a run — writing a tracked file mid-sweep, so the run cannot end on the
  clean worktree §4.6 requires. `CI=1` makes the recorder inert and is the
  environment these gates actually run in. Two of FR-158's own certification
  sweeps were voided before this precondition was written down.
- **The `shape` field is prose and nothing validates its content.** A gate can
  name a shape that does not fit and the check accepts it; what it buys is that
  the question appeared in a reviewed diff.
- **Cargo.toml manifests are counted unmasked** — there is no TOML lexer — so a
  `#` comment naming a coordinate in a manifest still counts toward
  `celInterpreter`.
