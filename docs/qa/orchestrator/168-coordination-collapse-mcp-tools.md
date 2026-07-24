---
self_referential_safe: true
---

# Orchestrator-Owned Coordination MCP Tools

**Module**: Runner / Scheduler / Workflow
**Scope**: authenticated tool hosting, authoritative effects, event evidence, pilot parity, and coordination-line collapse
**Scenarios**: 5
**Priority**: High

## Automated Entry Point

```bash
FR118_ALLOW_DIRTY=1 ./scripts/qa/test-coordination-collapse.sh
```

The script is network-free. It builds the daemon, CLI, and real `orch-mcp-tools` shim; creates an isolated HOME, data directory, workspace, project, and loopback daemon; then applies `fixtures/manifests/bundles/coordination-collapse-pilot.yaml`. Omit `FR118_ALLOW_DIRTY=1` for clean-tree certification.

## Scenario 1: Authenticated Host And Real Tool Contracts

### Preconditions

- Rust, Cargo, `jq`, `rg`, `sqlite3`, and `awk` are installed.
- The repository can build the runner and scheduler crates.

### Goal

Prove that tool results come from daemon-owned logic rather than canned shim responses.

### Steps

1. Run `cargo test -p orchestrator-scheduler authenticated_host_executes_real_coordination_tools`.
2. Exercise `run_tests` with passing and failing Cargo fixtures.
3. Exercise `mark_item`, evidence-gated `create_ticket`, `scan_tickets`, bounded `generate_items`, and bounded `record_metric`.
4. Attempt a request without the run token.

### Expected

- Passing/failing results reflect the real fixture commands.
- Status, ticket, scan, and generated-item receipts reflect store state.
- Metric receipts reject invalid names/non-finite values and drive deterministic item selection.
- `create_ticket` requires prior failing evidence.
- The unauthenticated request returns HTTP 401 before tool dispatch.

## Scenario 2: Stdio Forwarding, Allowlist, And Secret Isolation

### Preconditions

- Build `orch-mcp-tools` and the runner integration test.

### Goal

Verify the transport shim forwards authenticated JSON-RPC without owning business logic or leaking the callback token.

### Steps

1. Run `cargo test -p orchestrator-runner --test mcp_shim`.
2. Start two driver requests and inspect their generated MCP config paths and permissions.
3. Request a tool outside `allowedTools`.
4. Search database dumps, daemon logs, and stderr for the callback token.

### Expected

- The actual shim forwards the Bearer token and returns the callback's response.
- Each run uses a distinct mode-`0600` MCP file and token.
- Unknown or disallowed tools are not advertised and fail closed.
- Tokens are absent from persistent state and logs.

## Scenario 3: Legacy And Tool Pilot Parity

### Preconditions

- The isolated daemon is ready.
- Apply the pilot fixture to project `qa-coordination-collapse`.

### Goal

Compare legacy declarative coordination with the typed-tool workflow.

### Steps

1. Create one task with `coordination-legacy` and one with `coordination-tools` against the same QA target.
2. Start both tasks and wait for terminal state.
3. Compare task and item status.
4. Count effective YAML and transitional coordination lines between the fixture markers.

### Expected

- Both tasks converge to `completed`; both items converge to `qa_passed`.
- The tool block contains no CEL prehook, capture, JSONPath, post-action, or pipeline-variable wiring.
- Effective lines change from 38 to 21; coordination lines change from 15 to 0, a 100% reduction.

## Scenario 4: Event Completeness And Residual Channels

### Preconditions

- Scenario 3 completed and its isolated SQLite database remains available.

### Goal

Confirm auditable tool execution and enumerate all cross-step state that remains after collapse.

### Steps

1. Query driver and coordination events for the tool-driven task.
2. Inspect task `pipeline_vars_json` and item `dynamic_vars_json`.
3. Generate the metrics JSON produced by the QA script.

### Expected

- The three pilot calls (`run_tests`, `scan_tickets`, `mark_item`) each have use, result, start, and completion evidence.
- Task-level pipeline variables are empty.
- Item-level residual keys are exactly `goal`, `last_sandbox_denied`, `sandbox_denied_count`, and `last_sandbox_denial_reason`; none spill.

### Expected Data State

```sql
SELECT event_type, COUNT(*)
FROM events
WHERE task_id='{tool_task_id}' AND event_type IN (
  'driver_tool_use','driver_tool_result',
  'coordination_tool_started','coordination_tool_completed'
)
GROUP BY event_type;
-- Expected: 3 for each event type

SELECT COALESCE(pipeline_vars_json, '{}') FROM tasks WHERE id='{tool_task_id}';
-- Expected: {}
```

## Scenario 5: Compatibility And Repository Regression

### Preconditions

- All prior scenarios pass.

### Goal

Ensure the additive tool path does not regress legacy shell/CEL or driver behavior.

### Steps

1. Run `cargo fmt --all --check`.
2. Run `cargo test --workspace --quiet`.
3. Run `cargo clippy --workspace --all-targets -- -D warnings`.
4. Run `FR116_ALLOW_DIRTY=1 ./scripts/qa/test-agent-driver-abstraction.sh`.
5. Run `./scripts/qa-doc-lint.sh`.

### Expected

- All commands pass.
- Existing shell/CEL workflows remain supported.
- The provider driver and private MCP configuration contract remains compatible.

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Authenticated host and real tools | PASS | 2026-07-25 | Codex | Real pass/fail command execution and six primary tool contracts verified |
| 2 | Shim, allowlist, and isolation | PASS | 2026-07-23 | Codex | Actual shim, 401, 0600 config, and token non-disclosure verified |
| 3 | Pilot parity and line collapse | PASS | 2026-07-23 | Codex | completed/qa_passed parity; 100% coordination-line reduction |
| 4 | Events and residual channels | PASS | 2026-07-23 | Codex | 12 tool events and exactly four classified residual keys |
| 5 | Repository regression | PASS | 2026-07-23 | Codex | Closure gates recorded during FR governance |

Production-wide migration supersedes the pilot-only completion claim; see
[QA-174](174-coordination-strangler-completion.md).
