# Spike: Streaming Agent Runner (stream-json + MCP)

Throwaway validation for the architecture pivot in
[`docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`](../../../docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md).

It proves the foundation for replacing the one-shot shell-text agent contract
with a long-lived, structured, tool-calling contract — **without** rewriting the
agent loop: spawn `claude` in bidirectional `stream-json` mode and let it call
orchestrator-owned typed tools via MCP.

## Requirements

- `claude` CLI on PATH (validated with 2.1.195), authenticated.
- Node 18+ (validated with v24).

## Run

```sh
# Phase 1 — single long-lived process, multi-turn, all-structured events.
node driver.mjs

# Phase 2 — agent calls an orchestrator-owned MCP tool; we compute the result,
# feed it back, the agent continues. Proves the orchestrator owns tool execution.
node driver2.mjs
```

Both default to `--model haiku` (override with `SPIKE_MODEL=...`).

## What it demonstrates

- **driver.mjs**: one `claude` process consumes multiple `stream-json` user turns
  with a stable `session_id`; output is structured events (`system`/`assistant`/
  `result`), no text parsing; per-turn `total_cost_usd` for budget governance.
- **mcp_server.mjs**: a minimal hand-rolled MCP stdio server exposing a `run_tests`
  tool that returns a hard-coded structured result (stand-in for an
  orchestrator-owned tool). Logs to stderr to prove our code executed.
- **driver2.mjs**: registers the MCP server via `--mcp-config`, pre-approves the
  tool via `--allowedTools` + `--permission-mode bypassPermissions`, and verifies
  the `tool_use → tool_result → continue` loop. The injected failing-test name
  appearing in the final answer is conclusive proof the orchestrator owned the result.

## Gotchas surfaced (carried into the design doc)

- MCP tools may be **deferred**: the agent runs a `ToolSearch` to load the schema
  before the first call (one extra round-trip).
- Permissions must be granted explicitly (`--allowedTools` + `--permission-mode`).
- `CLAUDECODE` must be unset when running nested (mirrors `spawn.rs` `env_remove`).

This is a spike, not production code. Do not depend on it.
