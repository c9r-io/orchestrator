# FR-086: CLI Command to Simulate Agent Selection Logic

## Status: Open
## Priority: P3
## Created: 2026-03-28

## Problem

QA-110b S1 requires verifying that a diseased agent with 35% capability success rate is still selected when `capability_success_threshold` is set to 30%. This runtime behavior cannot be verified through CLI or configuration checks alone — it requires actual task execution with controlled agent health states.

## Acceptance Criteria

1. A CLI command (e.g., `orchestrator agent simulate-selection`) that:
   - Takes a capability name, agent health state, and success rate as inputs
   - Returns whether the agent would be selected based on current policy
2. OR: Accept configuration-only verification as sufficient for this scenario (update QA doc accordingly)

## Reproduction Steps (from ticket qa110b)

1. Configure agent with `capability_success_threshold: 0.3`
2. Mark agent as diseased with 35% qa capability success rate
3. **Expected**: Agent is still selected for qa tasks (35% >= 30%)
4. **Actual**: No way to verify this without running real tasks

## Related

- QA doc: `docs/qa/orchestrator/110b-agent-health-policy-advanced.md` (S1 partial)
- Ticket: `qa110b-s1-capability-selection` (closed with FR reference)
