---
self_referential_safe: true
---

# Orchestrator - Governance Ledger Regeneration And Review

**Module**: Governance / CI
**Scope**: regeneration of the coordination collapse ledger, the mismatch report, source-ratchet exactness, and the review workflow's same-commit constraint
**Scenarios**: 5
**Priority**: High

## Background

`config/governance/coordination-collapse-ledger.json` is compared by strict equality, so one changed
production Agent spec turns the gate red while naming nothing. FR-128 adds regeneration and diagnosis
without weakening the comparison, and corrects two defects the work uncovered: the source scan counted
`cfg(test)` lines the ledger's own `scope` excluded, and the ratchets were monotonic, leaving
`capturesOrJsonPath` at 54 against a reviewed 55 with the gate green.

Every scenario is read-only against the working tree. The automated gate builds throwaway git
repositories under `$TMPDIR`; it starts no daemon, touches no runtime database, and invokes no provider.

Primary entry point:

```bash
./scripts/qa/test-governance-ledger-tooling.sh
```

See [DD-140](../../design_doc/orchestrator/140-governance-ledger-regeneration.md).

---

## Scenario 1: Regeneration Emits The Value The Gate Compares

### Preconditions

- `ruby`, `jq`, and `git` are installed.

### Steps

1. `./scripts/qa/test-governance-ledger-tooling.sh`
2. `diff <(ruby scripts/qa/coordination-governance.rb --emit-inventory) <(jq '.retirement.shellRunnerExecutor.productionAgents' config/governance/coordination-collapse-ledger.json)`
3. `diff <(ruby scripts/qa/coordination-governance.rb --emit-baseline) <(jq '.sourceBaseline' config/governance/coordination-collapse-ledger.json)`
4. `rg -n 'production_agent_inventory' scripts/qa/coordination-governance.rb`

### Expected result

- Step 1 exits 0 and prints `FR-128 governance ledger tooling: 8 passed, 0 failed`.
- Steps 2 and 3 print nothing.
- Step 4 shows one definition and two call sites — the comparison and the emitter — so the emitted
  candidate cannot diverge in ordering or field selection from the compared value. Cases 1 and 6 of the
  gate assert this rather than relying on inspection.

## Scenario 2: Regeneration Cannot Be Performed By CI, And Is Byte-Quiet

A tool that lets CI rewrite the ledger converts the review gate into decoration; a tool that reformats
510 reviewed lines on a one-line change hides that change from the reviewer. Both are failure modes of
the fix rather than of the original problem.

### Steps

1. Record `shasum -a 256 config/governance/coordination-collapse-ledger.json`.
2. `CI=1 ruby scripts/qa/coordination-governance.rb --emit-inventory --emit-baseline --write`
3. Re-record the checksum.
4. Confirm cases 2 and 3 of `./scripts/qa/test-governance-ledger-tooling.sh` pass.

### Expected result

- Step 2 exits 2 and prints `refusing --write under CI: a regenerated ledger must be reviewed by a human`.
- The checksums in steps 1 and 3 are identical.
- Case 3 confirms that a no-op `--write` outside CI leaves the ledger byte-identical. Ruby's
  `JSON.pretty_generate` writes `[\n\n]` for an empty array where the reviewed ledger uses `[]`; without
  the normalisation in `ledger_json` a no-op write moves 19 lines.

## Scenario 3: A Real Spec Change Names The Agent And The Field

### Steps

1. In a throwaway copy of the repository, change `maxTurns: 6` to `maxTurns: 9` in the `session-agent`
   Agent of `docs/workflow/command-rules.yaml`.
2. `ruby scripts/qa/coordination-governance.rb`
3. `ruby scripts/qa/coordination-governance.rb --emit-inventory --write`
4. `ruby scripts/qa/coordination-governance.rb` again, and `git diff --numstat` on the ledger.
5. Commit the spec change *without* the ledger, then repeat step 2.

### Expected result

- Step 2 exits 1 and reports
  `~ docs/workflow/command-rules.yaml#session-agent: manifestFingerprint changed in spec key(s): driver`,
  followed by the instruction to regenerate and commit the ledger with the change that caused it.
- Step 4 exits 0 and the ledger diff is exactly `1` insertion and `1` deletion — regeneration touches only
  the entry that changed.
- Step 5 reports `manifestFingerprint changed but the spec already matches HEAD, so the spec was committed
  without its ledger update; they must land in one commit`. The report derives the previous spec from
  `git show HEAD:<file>`, so the same-commit rule is the diff's precondition and its violation is
  self-diagnosing rather than silently degrading into a wrong key list.
- Cases 4 and 5 of the automated gate cover the first four steps; case 5 additionally asserts that a
  ledger whose fingerprint was pasted in without the accompanying `classification` update still fails.

## Scenario 4: The Ratchets Say What The Ledger Claims

### Steps

1. `jq '.sourceBaseline' config/governance/coordination-collapse-ledger.json`
2. `ruby scripts/qa/coordination-governance.rb --emit-baseline`
3. In a throwaway copy, raise `capturesOrJsonPath` by one and re-run the gate.
4. In a throwaway copy, insert a mid-file `#[cfg(test)] mod fr128_scope_probe { … }` containing
   `captures`, `json_path`, and `PipelineVariables` tokens into a scanned source file, then re-run
   `--emit-baseline`.

### Expected result

- Steps 1 and 2 both report `53 / 30 / 9 / 0`. FR-125 recorded `55 / 39 / 9 / 0`; the difference is the
  ten `cfg(test)` lines the previous scan counted plus one stale capture touch.
- Step 3 exits 1 with `source touch capturesOrJsonPath decreased from 54 to 53`, proving the comparison is
  exact rather than monotonic. Before FR-128 this state passed silently, and did: the reviewed 55 stood
  against a real 54.
- Step 4 leaves the emitted baseline unchanged. The probe module is deliberately not named `tests` and not
  at end of file — the two properties the previous implementation depended on. Asserting the emitted
  baseline rather than the stripper function is what makes this an execution fact rather than the
  existence of a function.

## Scenario 5: The Gate Is Wired And Classified

### Steps

1. `rg -n 'test-governance-ledger-tooling' .github/workflows/ci.yml config/governance/qa-gate-surface.json`
2. `./scripts/qa/test-qa-gate-surface.sh`
3. `./scripts/qa/test-qa-gate-surface.sh --fixture-test`

### Expected result

- Step 1 shows the script as a `run:` step of the `governance` job and as a `ci-required` entry with
  `providerIsolation.mode = no-provider`.
- Step 2 exits 0 and reports `Enforcement surface: 13 of 46 gates are ci-required`.
- Step 3 exits 0 with `8 passed, 0 failed`. Under FR-127's manifest an unclassified script in
  `scripts/qa/` fails the build, so this gate could not have landed unwired.

---

## Certification conditions

A run of this document counts as closure evidence only when all of the following hold:

1. `git status --porcelain` is empty at start and at end.
2. `git rev-parse HEAD` matches before and after.
3. Each script is invoked as `bash <script> > log 2>&1` with `$?` captured directly, never through a pipe.
4. Each log ends with the script's own summary line.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Regeneration emits the value the gate compares | ☑ PASS | 2026-07-25 | Claude | Gate `8/8`; both emitters diff clean against the ledger, and one `production_agent_inventory` definition serves the comparison and the emitter. |
| 2 | Regeneration cannot be performed by CI, and is byte-quiet | ☑ PASS | 2026-07-25 | Claude | `CI=1 --write` exited 2 with the ledger checksum unchanged; a no-op `--write` was byte-identical. Removing the empty-collection normalisation moved 19 lines. |
| 3 | A real spec change names the agent and the field | ☑ PASS | 2026-07-25 | Claude | Reported `spec key(s): driver`; regeneration restored green with a 1-insertion/1-deletion ledger diff; the split-commit case produced its own diagnostic. |
| 4 | The ratchets say what the ledger claims | ☑ PASS | 2026-07-25 | Claude | Baseline retightened to `53 / 30 / 9 / 0`; an inflated baseline fails with `decreased from`; a mid-file non-`tests` probe module does not move the emitted baseline. |
| 5 | The gate is wired and classified | ☑ PASS | 2026-07-25 | Claude | Surface gate `5/5` at 13 of 46 `ci-required`; FR-127 fixtures `8/8`. |
