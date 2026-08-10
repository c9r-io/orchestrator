# source-task-binding: mutation/restart/audit contract assertion fails

- **Observed during**: FR-160 governance, Phase 3 batch C migration verification of `scripts/qa/test-source-task-binding.sh`
- **Severity**: medium (manual-runbook gate red; binding suspend/resume/restart and audit evidence contract unverified)
- **Symptom**: `FAIL: binding mutation, restart, or audit contract differs` (line 218) — the compound assertion covers: suspend reason code, binding revision stable across resume and daemon restart, three audit actions succeeded, and no raw Slack fields in the audit dump; the gate does not say which leg broke
- **Status**: open

## Classification evidence

- Pre-existing: identical failure (5 passed, 1 failed, same assertion) at
  pre-migration commit `d7756525` in a throwaway worktree. The restart the
  assertion spans uses the FR-160-migrated stop_daemon and got a live daemon
  back; post-run residue 0/0.
- `lastRun` was null before FR-160's sweep — no recorded green run to compare
  against.

## For ticket-fix

The assertion is a five-legged AND with one FAIL line, so first split it to
find the failing leg (run with `KEEP_QA=1`-style retention if available, or
temporarily echo each leg). Note the §4.4 angle for the repair: a compound
assertion that cannot name its failing leg costs an extra reproduce-run every
time it fires.
