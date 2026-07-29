---
lifecycle: active
related_fr: FR-118
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

## Scenario 3: The Collapse, And The Baseline It Was Measured Against

> **The parity comparison retired with the mechanism it compared against.** `behavior.captures`
> was removed by design on 2026-07-25 (`1b0937ca`, DD-137), so `coordination-legacy` can no longer
> be applied at all — there is no legacy pilot to be at parity with. What survives is the
> measurement, which is a property of two YAML blocks and needs no runtime, plus a new assertion
> that the retired baseline is still rejected.
>
> Nobody noticed for four days. `apply` is all-or-nothing over a bundle, so the rejected workflow
> took the *whole* fixture down with it and the gate ended at the apply with **no summary line** —
> three of twelve assertions run, and a truncated run reads exactly like a complete one. This gate
> is `manual-runbook`; no CI job watches it. The apply now routes its failure through
> `abort_with_summary`, so the next fixture to rot says so instead of vanishing.

### Preconditions

- The isolated daemon is ready.
- Apply the pilot fixture to project `qa-coordination-collapse`.

### Goal

Measure the coordination cost the typed-tool workflow removed, and prove the baseline it was
measured against stays retired.

### Steps

1. Apply `fixtures/manifests/bundles/coordination-collapse-pilot.yaml`; only `coordination-tools`
   is in it now.
2. Apply `fixtures/manifests/bundles/coordination-legacy-baseline.yaml` and read the diagnostic.
3. Create a task with `coordination-tools`, start it, and wait for terminal state.
4. Count effective YAML and transitional coordination lines between the markers in each file.

### Expected

- The tool task converges to `completed`; its item converges to `qa_passed`.
- Applying the baseline **fails**, and the output names `[legacy_coordination_removed]` together
  with `coordination-legacy`. The diagnostic is asserted, not the exit code: capability validation
  runs before the captures check, so a baseline missing its agent fails with `no agent supports
  capability`, and an exit-code assertion could not tell the two apart. Measured — stripping the
  agent makes this assertion fail, as it must.
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
- **The generic pipeline-variable store holds nothing**, at task and item level alike. The same
  commit that retired `behavior.captures` moved the four residual channels out of it:
  `PipelineVariables::normalize_preserved_channels` migrates `goal`, `last_sandbox_denied`,
  `sandbox_denied_count` and `last_sandbox_denial_reason` into the typed
  `PreservedExecutionChannels` carrier, and the goal also lives on `tasks.goal`. Measured on a
  green run: both JSON columns are **NULL**, so the invariant now holds in its strongest form —
  there is no generic store to leak into.
- Because "the map is empty" would also be true of a run where nothing happened, it is paired with
  `tasks.goal` matching the goal the task was created with. A task carrying that intent is a task
  that actually ran, and the expected value is read from the variable the gate supplied rather
  than written down a second time.

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

SELECT COALESCE(pipeline_vars_json, '<unset>') FROM tasks WHERE id='{tool_task_id}';
SELECT COALESCE(dynamic_vars_json,  '<unset>') FROM task_items WHERE task_id='{tool_task_id}';
-- Expected: <unset> for both; nothing writes the generic store after the capture runtime retired

SELECT goal FROM tasks WHERE id='{tool_task_id}';
-- Expected: the goal the task was created with, which is what makes the check above non-vacuous
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
| 3 | Collapse measurement and baseline rejection | PASS | 2026-07-29 | Claude | tool pilot completed/qa_passed; 100% reduction; baseline rejected naming `[legacy_coordination_removed]`. The parity half retired on 2026-07-25 and this row had stood at 2026-07-23 since |
| 4 | Events and residual channels | PASS | 2026-07-29 | Claude | 12 tool events; the generic var store is unset at both levels and `tasks.goal` carries the supplied intent |
| 5 | Repository regression | PASS | 2026-07-23 | Codex | Closure gates recorded during FR governance |

Production-wide migration supersedes the pilot-only completion claim; see
[QA-174](174-coordination-strangler-completion.md).
