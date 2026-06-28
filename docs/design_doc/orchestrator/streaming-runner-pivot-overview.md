# Streaming Runner Pivot — Overview

**Module**: orchestrator
**Status**: Narrative overview (ties together design docs 101–103 + showcase)
**Last Updated**: 2026-06-28

A one-page tour of the streaming-runner pivot: why it happened, the four moves
that delivered it, and the end-to-end demo that proves it.

## The problem

The orchestrator treated an agent as a **one-shot shell black box**: an agent was
a shell command (`claude -p "{prompt}"`), executed via `/bin/sh -c`, and its only
contract was stdout text + an exit code. Because the orchestrator couldn't
converse with the agent or read structured intermediate state, **all coordination
intelligence was exiled into the declarative layer** — CEL prehooks, `captures`
with `json_path`, pipeline-var spill-to-disk, finalize rules, segment scopes,
`builtin:` magic strings. A production workflow ballooned to hundreds of lines of
YAML; ~70% of it was coordination "ransom" paid for the dumb agent contract. The
dumber the agent, the smarter the YAML had to be.

## The root-cause insight

The fix was **not** "remove shell". It was: keep the process boundary, but
replace the *text contract* with a *structured, tool-calling contract*. An agent
should be a long-lived, structured participant that calls **orchestrator-owned
typed tools** — not a process whose stdout we regex-scrape. Once the agent can
express intent through typed tools, coordination can move out of YAML/CEL and back
to where it belongs.

## The four moves

| Doc | Move | Result |
|-----|------|--------|
| [101](101-streaming-agent-runner-architecture-pivot.md) | `StreamingAgentRunner` behind the existing `RunnerExecutor` seam | spawns `claude` in `stream-json` mode + orchestrator-owned MCP tools; **additive**, the shell path is untouched |
| [102](102-stream-json-event-ingestion.md) | Parse the stream into structured records | `tool_use`/`tool_result`/`result` projected into the `events` table and onto `AgentOutput`; tool I/O + run economics become first-class data |
| [103](103-cel-stream-run-signals.md) | Surface signals to coordination CEL | `tools_called` / `tool_error_count` / `run_cost_usd` … injected as typed pipeline vars; **one unified `bind_pipeline_vars`** exposes them to prehook, convergence, and finalize |
| [showcase](../../showcases/streaming-mark-done-convergence.md) | End-to-end demo | a real workflow **converges at cycle 1** on `'mark_done' in tools_called` — the agent signals via a typed tool, the loop guard consumes it |

## The proof (captured live, claude-haiku)

```
agent_tool_call      ToolSearch
agent_tool_call      mcp__orch__mark_done          # the agent calls a typed MCP tool
agent_run_summary    num_tool_calls=1  run_cost_usd=0.0233
workflow_terminated  reason="agent signaled completion via the mark_done tool"  cycle=1
```

And the control: with a prompt that does **not** call `mark_done`, the same loop
does **not** converge — it runs to `max_cycles_reached`. The loop converges *iff*
the agent signals via the typed tool. Coordination is now driven by what the agent
*did*, not by parsed text.

## What it demonstrates

This arc is the engineering judgment, not just the feature: spotting that the
accidental complexity (YAML/CEL sprawl) had a single root cause (the black-box
text contract), de-risking the fix with a throwaway spike, then landing it as an
**additive** change behind an existing seam — runner → ingestion → CEL signals →
live demo — with every step verified by tests. No big-bang rewrite; the old shell
path still passes its full suite.

## Where it stands / next

- `mark_done` and `run_tests` are demo tools in `orch-mcp-tools` (a Rust stdio MCP
  server). Real tools that share daemon state (in-process / HTTP MCP) are the next
  step (doc 101 risks).
- Runner selection is a global `executor: streaming` switch today; per-agent
  routing is a follow-up.
- Spike: [`scripts/spikes/streaming-agent-runner/`](../../../scripts/spikes/streaming-agent-runner/).
