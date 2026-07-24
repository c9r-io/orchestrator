# Coordination Tools

Coordination tools let a structured agent ask the daemon to test work, update the current item, manage QA tickets, or create dynamic items. They replace workflow plumbing such as stdout captures, JSONPath, post-actions, and many CEL conditions with typed, auditable calls.

Use them when an agent needs to make an in-workflow decision from authoritative runtime state. Keep capabilities, selection, sandbox policy, budgets, permissions, and triggers in manifests: those are governance decisions and must not be delegated to the agent.

## Configure A Tool-Capable Agent

The current production path uses a Claude CLI driver. Declare only the tools the workflow needs:

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: qa-coordinator
spec:
  capabilities: [qa_coordination]
  driver:
    provider: claude
    transport: cli
    options:
      permissionMode: governed
      maxTurns: 6
      budgetCapUsd: 0.25
      allowedTools:
        - mcp__orch__run_tests
        - mcp__orch__scan_tickets
        - mcp__orch__create_ticket
        - mcp__orch__mark_item
```

Require tool hosting at the workflow step:

```yaml
behavior:
  side_effect_class: workspace_only
  driverRequirements:
    multiTurn: true
    toolHosting: stdio
    workspaceAccess: write
```

Apply-time validation rejects an Agent that cannot satisfy these requirements. `allowedTools` is a hard daemon allowlist, not merely prompt guidance.

Tool hosting is also a runtime opt-in. A Claude driver step that omits
`toolHosting: stdio` does not start the callback, even if the Agent declares
coordination tools.

## Available Tools

| Tool | Use it for | Important constraints |
|---|---|---|
| `run_tests` | Run tests and obtain structured pass/fail evidence | Target is limited to `workspace`, `core`, `runner`, or `scheduler`; execution uses the selected profile. |
| `mark_item` | Record the current item's governed status | The item and requested status are validated. |
| `create_ticket` | Create a QA ticket for a demonstrated failure | Requires the latest `run_tests` call in the same run to have failed; deduplicates through existing ticket logic. |
| `scan_tickets` | Read the current active-ticket set | Uses the Workspace ticket directory and existing scanner. |
| `generate_items` | Add work discovered during the run | Accepts 1–100 unique workspace-relative IDs with optional labels/string variables; rejects unsafe or duplicate input. |
| `record_metric` | Supply a numeric score for deterministic item selection | Name must match `[a-z][a-z0-9_]{0,63}`; value must be finite and within the governed range. |

`mark_done` is retained as a compatibility alias for older streaming demonstrations. Prefer `mark_item` for new workflows.

## What Happens During A Call

For each driver run, the daemon starts a token-authenticated callback on an ephemeral loopback port. Claude launches the `orch-mcp-tools` stdio shim from a private mode-`0600` MCP config. The shim forwards JSON-RPC; the daemon validates the token and allowlist, executes the tool, and returns the typed result. The callback and token expire with the run.

Each call produces four complementary event records:

- `driver_tool_use` and `driver_tool_result` show what the provider requested and received.
- `coordination_tool_started` and `coordination_tool_completed` are the daemon's authoritative execution receipts.

Inspect them with normal task events/logs tooling or query the `events` table during QA. Tokens and provider session identifiers are not persisted.

## Migrate A Declarative Step

1. Identify coordination-only fields: `prehook`, `captures`, `json_path`, `post_actions`, and pipeline variables used only to connect those fields.
2. Map effects to the smallest tool set. For example, replace an exit-code capture plus ticket post-action with `run_tests`, optional `create_ticket`, and `mark_item`.
3. Add the tools to the Agent's `allowedTools` and add `toolHosting: stdio` to the step requirements.
4. Tell the agent in its StepTemplate what outcome it must establish, without embedding credentials or policy.
5. Run the legacy and tool variants side by side. Compare task/item terminal state and event evidence before removing the legacy wiring.
6. Record any remaining cross-step variables. Do not automatically replace them with a general state store.

For private provider continuity, add `sessionResume: true` only to a step that
must continue the task's current provider context. Omit it for independent
review. The reference stays daemon-private; do not capture or template a
provider session ID. The current reference is task-scoped, so parallel
item-scoped steps should remain fresh.

The complete paired example is `fixtures/manifests/bundles/coordination-collapse-pilot.yaml`. Validate it with:

```bash
./scripts/qa/test-coordination-collapse.sh
```

The production migration matrix and freeze ratchet are covered by:

```bash
./scripts/qa/test-coordination-strangler.sh
```

## Troubleshooting

- **Tool is absent**: verify its fully qualified `mcp__orch__<name>` entry is in `allowedTools` and the step requires `toolHosting: stdio`.
- **Tool is not allowed**: the manifest allowlist is authoritative; adding a tool to a prompt does nothing.
- **`create_ticket` rejects the call**: call `run_tests` first in the same run and create a ticket only when its result fails.
- **Callback authentication fails**: do not copy or reuse MCP files. Check that the run artifact exists with mode `0600`, then inspect redacted driver/coordination events.
- **Legacy CEL still controls the result**: remove transitional coordination only after parity. CEL remains supported and may still be appropriate for deterministic governance gates.
- **A resumed step starts fresh**: verify that the step, not only its Agent, declares `sessionResume: true`.
- **Tests need arbitrary commands**: `run_tests` intentionally has a small target allowlist. Add a reviewed typed tool rather than turning it into a shell escape hatch.

For implementation and security boundaries, see [DD-130](../design_doc/orchestrator/130-coordination-collapse-mcp-tools.md). For production migration and retirement governance, see [DD-136](../design_doc/orchestrator/136-coordination-strangler-completion.md). Reproducible acceptance lives in [QA-168](../qa/orchestrator/168-coordination-collapse-mcp-tools.md) and [QA-174](../qa/orchestrator/174-coordination-strangler-completion.md).
