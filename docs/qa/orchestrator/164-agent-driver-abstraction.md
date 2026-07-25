---
self_referential_safe: true
---

# Orchestrator - Agent Driver Abstraction

**Module**: Config / Runner / Scheduler / Resource Apply  
**Scope**: Driver resource model, provider conformance, capability gates, event folding, sandbox/cancel, session privacy, MCP isolation, and shell compatibility  
**Scenarios**: 5  
**Priority**: High

## Automated Entry Point

```bash
./scripts/qa/test-agent-driver-abstraction.sh
```

The script builds an isolated daemon, applies `fixtures/manifests/bundles/agent-driver-fixture.yaml`, runs both shell pilots, and destroys its temporary HOME/data/workspace on success. `FR116_ALLOW_DIRTY=1` is for local iteration only; closure evidence uses a clean worktree.

Codex session attachment has a separate exact-version protocol gate in `docs/qa/orchestrator/166-codex-session-resume-conformance.md`. The default FR-116 script remains network-free and does not invoke a live provider.

## Scenario 1: Resource Round Trip And Apply-Time Capability Rejection

### Preconditions

- Build current `orchestratord` and `orchestrator` binaries.
- Start an isolated daemon with Admin UDS policy.

### Steps

1. Apply the fixture to project `qa-agent-driver`.
2. Get/export all four Agents and confirm typed driver fields survive round trip.
3. Dry-run a shell driver workflow requiring `multiTurn`.
4. Repeat for missing `toolHosting`, missing `permissionEvents`, SDK workspace access, and cooperative cancellation with `non_idempotent_external`.
5. Inspect gRPC/CLI diagnostics.

### Expected

- Valid shell, Claude, and Codex Agents apply without provider flag strings in YAML.
- Every incompatible workflow is rejected before persistence or task creation.
- Diagnostics contain the stable `driver_*` code and `spec.steps[].behavior.driverRequirements` field path.
- SDK workspace failure is classified as `driver_workspace_sandbox_required`; an unavailable SDK never starts.

## Scenario 2: Provider Command And Protocol Conformance

### Steps

1. Run `cargo test -p orchestrator-runner driver::`.
2. Feed recorded Claude init/assistant/tool/result JSONL through the Claude adapter.
3. Replay `fixtures/driver/codex-cli-0.144.5-resume.json` through the Codex adapter and run the exact resume command assertion.
4. Build commands with normalized model/budget/permission/tool/cwd/env/timeout options and typed vendor options.
5. Inject unknown event fields and verify parsing remains forward-compatible.

### Expected

- `shell/cli`, `claude/cli`, and `codex/cli` expose the documented capability matrix.
- Provider flags occur only in the provider module; control-plane code and pilot YAML do not spell them.
- Text, tool I/O, usage, outcome, and session availability map deterministically; Codex initial/resume streams resolve the same session reference.
- Unknown records do not create fabricated events or abort a valid stream.

## Scenario 3: Direct Stream Folding, Event Persistence, Attention, And Privacy

### Steps

1. Run the scheduler driver projection tests.
2. Execute a provider fixture whose output includes assistant text, tool use/result, usage, a permission request, and terminal success.
3. Query `events` by task/run and inspect type/order/payload.
4. Search stdout/stderr artifacts, event payloads, task/command DTO output, daemon logs, and Action Audit for the fixture provider session token.
5. Verify the command succeeds even when the raw stdout artifact is larger than 256 KiB before the terminal provider record.

### Expected

- The scheduler folds the live normalized stream and does not reparse truncated stdout for terminal truth.
- Every normalized event creates one canonical event; tool input and output remain separate records.
- A permission request becomes `approval_requested` and is eligible for the existing Attention projector.
- The raw provider token appears nowhere in persisted/public evidence; `driver_started` exposes only `session_available`.
- Configured secret values are replaced with `[REDACTED]`, and assistant event text is bounded.

### Expected Data State

```sql
SELECT event_type, COUNT(*)
FROM events
WHERE task_id='{task_id}' AND event_type IN (
  'driver_started','driver_assistant_text','driver_tool_use',
  'driver_tool_result','approval_requested','driver_usage','driver_finished'
)
GROUP BY event_type;
-- Expected: one row per emitted normalized event type; no provider session value in payload_json
```

## Scenario 4: Sandbox, Cancellation, MCP Isolation, And Unsafe Escape Hatch

### Steps

1. Run a CLI driver under a workspace-scoped ExecutionProfile and verify it traverses the common shell/sandbox path.
2. Trigger timeout, stall-kill, and external pause; inspect process-group termination and terminal classification.
3. Start two Claude driver runs concurrently and compare their MCP config paths/content/modes.
4. Verify the run-scoped config carries distinct loopback callback URLs/tokens, rejects an unauthenticated callback, and never persists or logs either token.
5. Apply `rawArgs` without acknowledgement, without daemon unsafe mode, and as non-Admin; then apply reviewed `rawArgs` with all gates satisfied and query Action Audit.

### Expected

- Workspace execution retains policy, daemon-PID guard, sandbox, rlimits, env allowlist, and guaranteed process-group kill.
- SDK workspace/non-idempotent attempts never reach runtime.
- MCP files have distinct `{run_artifacts}/driver/mcp.json` paths and mode `0600`; no shared temporary file exists. Coordination business logic remains in the daemon behind authenticated loopback callbacks, while the stdio binary is transport-only.
- Raw arguments fail closed until `unsafeRawArgs`, unsafe daemon mode, Admin, and audit context all exist.
- Successful escape-hatch mutation records `agent.driver.raw_args.apply` without provider credential/session material.

## Scenario 5: Shell Pilot Equivalence And Repository Regression

### Steps

1. Apply the FR-116 fixture in the isolated daemon.
2. Create and start one task with the command-only compatibility
   `legacy-shell-pilot` and one with `explicit-shell-pilot`.
3. Compare task terminal state, command-run exit code, and event evidence.
4. Count the Agent YAML lines for legacy and explicit shell forms.
5. Run workspace build/tests, strict Clippy, and documentation lint.

### Expected

- Both pilots finish `completed` with command-run exit code `0`.
- Apply emits `[legacy_agent_command_deprecated]`, persists the compatibility
  Agent as `shell/cli`, and both paths emit normalized driver events; shell
  output semantics are unchanged.
- The explicit shell Agent adds five effective YAML lines for driver ownership; workflow behavior remains equivalent.
- All existing shell, scheduler, CLI/daemon, and gRPC tests remain green. The
  global streaming executor was subsequently retired by FR-126; use QA-176 for
  current removal evidence.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Resource round trip and capability rejection | PASS | 2026-07-22 | Codex | Five incompatibility classes and structured diagnostic unit gates pass |
| 2 | Provider command and protocol conformance | PASS | 2026-07-22 | Codex | Three drivers, recorded Codex 0.144.5 resume fixture, and exact command grammar pass |
| 3 | Stream folding, events, Attention, privacy | PASS | 2026-07-22 | Codex | Direct fold, full projection, redaction, and opaque session tests pass |
| 4 | Sandbox, cancellation, MCP, raw escape hatch | PASS | 2026-07-22 | Codex | Common spawn path, guaranteed cancel matrix, unique 0600 MCP, Admin/audit gates pass |
| 5 | Shell pilot and repository regression | PASS | 2026-07-22 | Codex | Isolated legacy/explicit tasks converge to completed/0; full gates recorded at closure |
