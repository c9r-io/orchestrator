# Streaming typed driver reaches the real `claude` CLI through a path-shadow gate

- **Observed during**: FR-160 governance, Phase 2 migration verification of `scripts/qa/test-agent-driver-production-parity.sh`
- **Severity**: high (local-only today; it is a provider-isolation escape, so its cost is real credentials the day the machine is logged in)
- **Symptom**: `FAIL: streaming-mark-done typed Claude diverged from the recorded legacy contract` — the streaming task fails after 3 cycles with `No healthy agent found with capability: streaming_typed`
- **Expected**: every driver in the gate reaches `$QA_ROOT/bin/claude` (the fake), because the gate's declared isolation is `path-shadow` and `assert_provider_shadow` passed
- **Status**: open

---

## What actually ran

The step's stdout (`$QA_ROOT/data/logs/<task>/signal_*.stdout`) opens with a genuine
Claude Code init record — `"claude_code_version":"2.1.220"`, the local plugin/skill
roster — followed by `authentication_failed` / `Not logged in · Please run /login`.
The real CLI, launched with `HOME=$QA_HOME`, has no credentials, exits 1, and after
three cycles the scheduler marks the agent unhealthy. The verdict-conditional
retention kept the evidence.

The shadow itself was in place: `cp fake → $QA_ROOT/bin/claude`, `export PATH`
prepended, `assert_provider_shadow` passed, and **the four classic-driver parity
objects in the same run all reached the fake** (hello-world, scheduled-scan,
fr-watch all PASS with exact recorded contracts). Only the `streaming_typed` step
escaped. Whatever resolution the streaming transport uses for its provider binary,
it is not the gate's `PATH`.

## Classification evidence

- Pre-existing, not FR-160-caused: reproduced identically at pre-migration commit
  `d2e9b207` in a throwaway worktree (same single FAIL, same task failure).
- Environment-dependent: CI run `30807005685` (ubuntu, revision `2e9cb165`) passed
  this gate; no real `claude` exists on those runners, and the failing-stub
  backstop was not tripped — so on CI the streaming driver does reach the fake.
  The escape needs a machine with claude-code installed outside the shadow
  (here: `/opt/homebrew/Caskroom/claude-code/2.1.220`, macOS).

## The missing isolation mechanism

`path-shadow` (qa-gate-surface.json `providerIsolationModes`) assumes the daemon's
children resolve provider binaries through the inherited `PATH`. The streaming
typed driver evidently resolves through another channel (login-shell PATH
reconstruction via path_helper, an absolute-path probe, or an SDK-side lookup —
not yet isolated; the driver lives in `crates/orchestrator-runner`). Two candidate
repairs, for whoever picks this up:

1. Make the streaming driver honour the inherited `PATH` (fix in the runner), or
2. Extend the parity fixture to pin the streaming agent to an explicit fake
   binary path (`binary:` pin), accepting that path-shadow does not cover this
   transport and saying so in `providerIsolationModes`.

Either way, `assert_provider_shadow` certifying "the shadow is in effect" while a
driver resolves around it is §4.4 shape 1 for this transport: the assertion
observes the PATH entry, not the resolution the driver performs.

## Reproduce

On a machine with claude-code installed: build workspace, run
`bash scripts/qa/test-agent-driver-production-parity.sh`; observe the single
streaming FAIL and the init record naming the real CLI version in the retained
`signal_*.stdout`.
