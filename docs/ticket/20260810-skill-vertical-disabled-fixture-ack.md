# skill-automation-vertical: previous daemon does not acknowledge the disabled fixture

- **Observed during**: FR-160 governance, Phase 3 batch C migration verification of `scripts/qa/test-slack-skill-automation-vertical.sh`
- **Severity**: medium (manual-runbook gate red; the rollback-while-disabled contract is unverified)
- **Symptom**: `FAIL: previous daemon did not acknowledge disabled fixture` — the gate expects HTTP 200 in `rollback-disabled.code` (line 442) and gets something else
- **Status**: open

## Classification evidence

- Pre-existing: identical failure at pre-migration commit `d7756525` in a
  throwaway worktree, so FR-160's teardown migration (which this gate's
  restart sequence exercises three times) is not the cause. The migrated stops
  themselves worked: every restart in the failing run got a live next daemon,
  and post-run residue was 0 processes / 0 temp dirs.
- Never observed green on any machine within recorded history: this gate's
  `lastRun` in `config/governance/manual-gate-freshness.json` was null before
  FR-160's sweep — FR-148/149-shape rot, found by running.
- Log: the failure is the only FAIL in the run; earlier scenarios (routing,
  concurrency, dedupe) pass.

## For ticket-fix

Reproduce with `bash scripts/qa/test-slack-skill-automation-vertical.sh` and
`KEEP_QA=1` to retain `$QA_ROOT/rollback-disabled.code` and the daemon logs.
The three-way question is whether the webhook returns non-200 for a disabled
fixture by design (gate stale), or the disabled-fixture path regressed
(product bug), or the fixture posting helper rotted (harness bug).
