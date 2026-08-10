# fr013 gate cannot reach its daemon: no socket, no discovery config

- **Observed during**: FR-160 governance, Phase 3 batch A migration verification of `scripts/qa/test-fr013-control-plane-protection.sh`
- **Severity**: medium (a manual-runbook gate that cannot run; its subject — control-plane protection classes — is currently unverifiable by this gate)
- **Symptom**: `Error: daemon socket not found at $QA_HOME/.orchestratord/orchestrator.sock and no control-plane config was discovered` on the first `orchestrator apply`, ~3s after daemon start
- **Expected**: the gate's CLI reaches the TCP daemon it just started and the flood assertions run
- **Status**: open

---

## Mechanism

The gate starts `orchestratord --foreground --bind 127.0.0.1:51054` under an
isolated `HOME=$QA_HOME` and then calls the CLI with no
`ORCHESTRATOR_SOCKET`, no `ORCHESTRATOR_CONTROL_PLANE_CONFIG`, and no
`--uds-max-role` on the daemon. Measured directly (release build, fresh temp
HOME): after 15s the data directory holds the database, `control-plane/`,
`logs/`, `secrets/` — and **no `orchestrator.sock` ever appears**, and nothing
writes a discovery config the CLI's fallback path would find. The CLI's two
discovery routes both come up empty, so the gate dies before its first
assertion.

Sibling gates that work either export
`ORCHESTRATOR_SOCKET="$ORCHESTRATORD_DATA_DIR/orchestrator.sock"` with a
`--uds-max-role` daemon flag, or set `ORCHESTRATOR_CONTROL_PLANE_CONFIG`
(see `test-control-plane-action-audit.sh`). fr013 predates whatever change
made UDS/config discovery opt-in, and nothing has run it since: every
`lastRun` in `config/governance/manual-gate-freshness.json` was null until
FR-160's sweep — this is the FR-148/FR-149 shape again, rot discovered by
running rather than by any signal.

## Classification evidence

- Pre-existing, not FR-160-caused: identical failure at pre-migration commit
  `42711bf0` in a throwaway worktree.
- Not environment-specific: the missing socket is daemon behaviour on a fresh
  data dir, reproduced outside the gate; it would fail the same way on any
  machine.
- The FR-160 teardown migration is *positively* exercised by the failing run:
  the daemon was alive at the point of failure, and `gate_daemon_stop`
  stopped and confirmed it — post-run residue was 0 processes, 0 temp dirs.

## Repair sketch (for ticket-fix, needs its own verification)

Give the gate one explicit transport: either export
`ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"` plus
`ORCHESTRATOR_SOCKET="$ORCHESTRATORD_DATA_DIR/orchestrator.sock"` and start
the daemon with a `--uds-max-role` appropriate to the protection classes under
test, or set `ORCHESTRATOR_CONTROL_PLANE_CONFIG` the way the action-audit gate
does. The choice affects which transport the flood assertions exercise, so it
belongs with the gate's owner document, not with FR-160.
