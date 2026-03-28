# FR-085: Long-Running Agent Test Fixture for Inflight Wait Scenarios

## Status: Open
## Priority: P3
## Created: 2026-03-28

## Problem

QA scenarios for inflight wait timeout (QA-106 S1/S2/S4) require agents that remain running (exit_code=-1, process alive) during the post-loop `wait_for_inflight_runs()` phase. Current test fixtures use `mock_echo` which exits immediately, making it impossible to test heartbeat-aware timeout behavior.

## Acceptance Criteria

1. A test workflow step that spawns a long-running agent process staying alive for a configurable duration.
2. The agent keeps `exit_code=-1` (in-flight) until either timeout or external signal.
3. QA-106 S1 (heartbeat keeps agent alive), S2 (timeout without heartbeat), and S4 (diagnostic fields) can be verified end-to-end.

## Reproduction Steps (from ticket qa106)

1. Apply fixture: `orchestrator apply -f fixtures/manifests/bundles/qa106-long-running-agent.yaml --project qa106`
2. Create task with workflow targeting mock_long_running agent
3. **Expected**: Agent stays in-flight, heartbeat/timeout events observable
4. **Actual**: mock_echo exits immediately; no in-flight state to observe

## Related

- QA doc: `docs/qa/orchestrator/106-inflight-wait-heartbeat-aware-timeout.md` (S1/S2/S4 blocked)
- Ticket: `qa106-inflight-wait-fixture` (closed with FR reference)
