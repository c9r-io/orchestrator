# Showcase: Loop convergence driven by a typed tool (`'mark_done' in tools_called`)

End-to-end demonstration of the streaming-runner pivot (design docs
[101](../design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md) →
[102](../design_doc/orchestrator/102-stream-json-event-ingestion.md) →
[103](../design_doc/orchestrator/103-cel-stream-run-signals.md)):

> A streaming agent expresses completion by **calling a typed MCP tool**
> (`mark_done`). The orchestrator parses the `stream-json` stream into structured
> signals (`tools_called`, `tool_error_count`, `run_cost_usd`, …), and the loop
> guard converges on **`'mark_done' in tools_called`** — coordination driven by
> what the agent *did*, not by regex-scraping stdout.

Manifest: [`docs/workflow/streaming-mark-done-convergence.yaml`](../workflow/streaming-mark-done-convergence.yaml).

## Run it

The `streaming` executor drives `claude` and hosts the orchestrator-owned
`mark_done` tool via the `orch-mcp-tools` MCP server. Use an **isolated data
dir** so the demo never touches your real runtime DB.

```bash
# Build daemon, CLI, and the MCP tool server (siblings in target/debug)
cargo build -p orchestratord -p orchestrator-cli -p orchestrator-runner

export ORCHESTRATORD_DATA_DIR=$(mktemp -d)/data
export ORCH_MCP_TOOLS_BIN=$PWD/target/debug/orch-mcp-tools   # else resolved next to orchestratord

# Start the daemon (isolated instance)
./target/debug/orchestratord --foreground --workers 2 &

# Apply + run
./target/debug/orchestrator apply -f docs/workflow/streaming-mark-done-convergence.yaml --project demo
TID=$(./target/debug/orchestrator task create --name demo \
  --goal "signal completion via mark_done" \
  --workflow mark_done_convergence --project demo | grep -oE '[0-9a-f-]{36}')

./target/debug/orchestrator task watch "$TID"
./target/debug/orchestrator task trace "$TID"
```

## What happens (captured run, claude-haiku)

The loop is configured to run **up to 3 cycles** (`mode: infinite`, `max_cycles: 3`),
but converges as soon as the agent calls `mark_done`.

The agent loads the tool and calls it — `agent_tool_call` events:

```
ToolSearch              # select:mcp__orch__mark_done
mcp__orch__mark_done    # the orchestrator-owned typed tool
```

The orchestrator projects structured signals onto the run (visible in the
finalize/convergence context vars):

```
tools_called   = ["ToolSearch","mark_done"]   # MCP prefixes stripped → bare names
num_tool_calls = 2
run_cost_usd   = 0.0233
run_turns      = 2
```

The loop guard evaluates `'mark_done' in tools_called` and **terminates at cycle 1**:

```
cycle_started        cycle=1
agent_tool_call      ToolSearch
agent_tool_call      mcp__orch__mark_done
agent_run_summary    num_tool_calls=1
workflow_terminated  reason="agent signaled completion via the mark_done tool"  cycle=1
task_completed
```

### The contrast (why this proves consumption, not coincidence)

With a prompt that does **not** get the agent to call `mark_done`, the same
workflow does **not** converge — it runs the full safety cap and stops for a
different reason:

```
cycle_started        cycle=1
loop_guard_decision  cycle=1  continue        # tools_called=["ToolSearch"] → 'mark_done' absent
cycle_started        cycle=2
loop_guard_decision  cycle=2  continue
cycle_started        cycle=3
loop_guard_decision  cycle=3  max_cycles_reached
```

The loop converges **iff** the agent signals via the typed tool. That is the
pivot's payoff: the agent's structured action — not parsed text — drives
orchestration.

## Notes

- The agent must reference the tool by its fully-qualified MCP name
  (`mcp__orch__mark_done`); MCP tools are surfaced to the agent under that name,
  while the orchestrator strips the `mcp__<server>__` prefix so CEL can use the
  bare `mark_done`.
- `mark_done` is currently a demo tool in `orch-mcp-tools`; real tools that share
  daemon state (e.g. over an in-process/HTTP MCP endpoint) are a follow-up noted
  in design doc 101.
