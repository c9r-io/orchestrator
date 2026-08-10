# The two webhook gates run their daemons against the user's real ~/.orchestratord

- **Observed during**: FR-160 post-closure residue audit (2026-08-11)
- **Severity**: high (a QA gate mutating the production runtime location on the operator's machine; on a machine with real orchestrator state it would interleave test webhook/trigger state with live data)
- **Symptom**: `~/.orchestratord` — absent since the 2026-08-05 machine rebuild — exists after the FR-160 sweep: created 2026-08-10 22:28:24 (inside batch D's webhook-gate window), last written 22:42:49 (their post-fix reruns), containing a fully migrated `agent_orchestrator.db` (0 tasks), a generated `secrets/secretstore.key`, and `control-plane/protection.yaml`
- **Status**: open

## Mechanism

`scripts/qa/test-webhook-trigger.sh` (four sequential daemons) and
`scripts/qa/test-per-trigger-webhook-auth.sh` (two) set **no isolation at
all**: no `HOME` override, no `ORCHESTRATORD_DATA_DIR`, no `QA_HOME` —
grep for any of the three returns nothing in either file. Every other
daemon gate in the tree exports an isolated `HOME` and/or
`ORCHESTRATORD_DATA_DIR` before its first start; these two predate or
missed that convention, and their non-standard ports (19xxx) isolate the
control plane's *listener* while the *data directory* silently defaults to
the real one.

This is the isolation gap CLAUDE.md's forbidden-operations section
anticipates: the fix is proper isolation in the gates, never a reset of
the runtime directory.

## Why nobody saw it before

- Both gates were green (their assertions don't inspect the data dir).
- FR-160's residue bookkeeping (QA 211) counted **processes and $TMPDIR
  directories** — writes into `$HOME` were outside its scan, which is now
  recorded as a known limit in DD-174. It took the machine-rebuild
  coincidence (`~/.orchestratord` known-absent at session start) to make
  the creation visible at all.

## Current residue

`~/.orchestratord` on this machine holds only the two gates' test residue
(zero tasks; schema fully migrated; a test-generated secret key). Per
CLAUDE.md it is **not deleted by tooling**; the operator may remove it by
hand if desired — nothing else on this machine has ever written there.

## For ticket-fix

Give both gates the standard preamble other daemon gates use
(`QA_ROOT`/`QA_HOME` mktemp, `export HOME="$QA_HOME"`,
`export ORCHESTRATORD_DATA_DIR="$QA_ROOT/data"`, cleanup `rm -rf` of both),
and consider a ratchet: a check that any `scripts/qa/*.sh` starting
`orchestratord` also assigns `ORCHESTRATORD_DATA_DIR` or an isolated
`HOME` first — the same §4.4 shape 2 argument as FR-160's check 16, on the
start side instead of the stop side.
