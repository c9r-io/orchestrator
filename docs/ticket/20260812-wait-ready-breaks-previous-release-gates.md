# `--wait-ready` broke every gate that starts the *previous* release binary

- **Observed during**: FR-165 R1 triage sweep, 2026-08-12, at `bd0e2389` on a clean tree
- **Severity**: medium-high (four manual gates red, including the entire behavioural half of the forward-only rollback contract)
- **Status**: open

## Symptom

`scripts/qa/test-slack-skill-automation-vertical.sh` fails at its isolated-daemon
step:

```
[gate-daemon] daemon not ready within 25s:
  error: unexpected argument '--wait-ready' found

  Usage: orchestrator daemon status [OPTIONS]

  FAIL: isolated daemon failed readiness
```

Three parent gates wrap it as their `release-vertical` sub-gate and fail with it.
Every other sub-gate in all three passes:

| Gate | Result | Sub-gates |
|---|---|---|
| `test-slack-skill-automation-release.sh` | FAIL 451s | 5 pass, `release-vertical` fails |
| `test-slack-dedicated-app-provisioning.sh` | FAIL 477s | 5 pass, `release-vertical` fails |
| `test-slack-managed-shared-oauth.sh` | FAIL 424s | 5 pass, `release-vertical` fails |

## Cause

`c1060338` (2026-08-11, *"publish readiness, and stop 23 gates from guessing at
it"*) centralised readiness in `scripts/lib/gate_daemon.sh:125`:

```bash
output="$("$cli" daemon status --wait-ready --timeout "$timeout" 2>&1)"
```

The FR-113 vertical gate does not point that helper at the current build. It
pins `PREVIOUS_REF=58166a9f` — the **v0.5.0 release cut on 2026-08-01** — builds
that tree, and starts *its* daemon to prove the previous binary still serves the
current schema. `58166a9f` predates `c1060338` (verified with
`git merge-base --is-ancestor`), so its CLI has no `--wait-ready` and rejects the
argument.

The centralisation was right; it just has one caller whose binary is, by
construction, always older than the helper.

## Why this matters more than four red gates

This is the *behavioural* half of the forward-only rollback contract — "the
previous release binary must be able to serve the current schema". FR-165's
requirement 2 records that the contract exists in prose across 14 documents and
17 sites, with zero lines of code in the daemon migration kernel. The one thing
that actually exercised it is this gate, and it has been unable to start the
previous binary since 2026-08-11.

It also demonstrates the defect FR-165 R1 fixed, on a real object. The gate has
a recorded green run at `685525af` dated 2026-08-10; `c1060338` broke it on
08-11. Under the pre-FR-165 criterion the ledger would have reported it `ok`,
because staleness was recency alone and a one-day-old record is recent whatever
it exited with.

## For ticket-fix

1. **Do not resolve this by pinning `PREVIOUS_REF` forward.** The gate's subject
   is backward compatibility; a `PREVIOUS_REF` chosen to be new enough to accept
   the helper's flags tests nothing. It is the same error as a fixture whose
   expected value is edited until it matches.

2. The repair belongs in `gate_daemon.sh`: readiness must degrade when the CLI
   under test does not support `--wait-ready`. Options, in preference order:
   - probe support once (`daemon status --help`) and fall back to the pre-`c1060338`
     poll for older binaries;
   - accept an explicit `GATE_DAEMON_LEGACY_READINESS=1` from the one caller that
     knows its binary is old.

   Prefer the probe: an opt-in flag is an enumeration of the callers that
   currently need it (§4.4 shape 2), and the next old-binary caller will not
   know to set it.

3. **Whatever the fix, assert the fallback path directly.** A gate that only
   ever runs against the current binary cannot observe that the legacy branch
   works — it would pass while the compatibility check silently never runs,
   which is how this arrived. `c1060338` touched 23 gates and this is the one
   whose binary is not the one being built.

4. Check the other two `previous`-binary consumers for the same assumption
   (`rg -l 'PREVIOUS_REF|previous-compatible' scripts/`).
