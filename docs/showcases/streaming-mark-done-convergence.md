# Showcase: Typed-driver loop convergence (`'mark_done' in tools_called`)

This end-to-end example uses a per-Agent `claude/cli` typed driver to converge a
workflow from a structured tool call. The current execution model is documented
in design docs [127](../design_doc/orchestrator/127-agent-driver-abstraction.md)
and [138](../design_doc/orchestrator/138-agent-driver-execution-migration.md);
design docs [101](../design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md)–[103](../design_doc/orchestrator/103-cel-stream-run-signals.md)
record the historical first cut that preceded typed drivers.

> The `claude/cli` driver expresses completion by **calling a typed MCP tool**
> (`mark_done`). Orchestrator normalizes the provider stream into driver events
> and typed artifacts, derives signals such as `tools_called`, and evaluates
> **`'mark_done' in tools_called`** without scraping stdout.

The global `streaming` executor and its compatibility bridge have been removed.
Provider execution is selected only by `Agent.spec.driver`.

Manifest: [`docs/workflow/streaming-mark-done-convergence.yaml`](../workflow/streaming-mark-done-convergence.yaml).

## Run it

The manifest selects the `claude/cli` driver and allows the orchestrator-owned
`mark_done` tool through the `orch-mcp-tools` MCP server. Use an **isolated data
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

## What happens

The loop is configured to run **up to 3 cycles** (`mode: infinite`, `max_cycles: 3`),
but converges as soon as the agent calls `mark_done`.

The driver normalizes the provider interaction into current event types:

```
driver_started
driver_tool_use         name="mcp__orch__mark_done"
driver_tool_result      is_error=false
driver_finished         outcome="success"
```

Validation converts those events into a `ToolCall` artifact, a
`driver_tool_result` artifact, and a `driver_terminal` artifact. Only after the
structured terminal exists does Orchestrator derive and promote the CEL signals:

```
"mark_done" in tools_called = true   # MCP prefix stripped → bare name
tool_error_count           = 0
agent_reported_error = false
```

The provider may also emit discovery tools such as `ToolSearch`; convergence
depends only on the normalized `mark_done` entry.

The loop guard evaluates `'mark_done' in tools_called` and **terminates at cycle 1**:

```
cycle_started        cycle=1
...                  optional provider discovery events
driver_tool_use      mcp__orch__mark_done
driver_tool_result   is_error=false
driver_finished      outcome=success
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

The loop converges **iff** the typed artifacts record the tool call and a
successful `driver_terminal`. The agent's structured action—not parsed
text—drives orchestration.

## Notes

- The agent must reference the tool by its fully-qualified MCP name
  (`mcp__orch__mark_done`); MCP tools are surfaced to the agent under that name,
  while the orchestrator strips the `mcp__<server>__` prefix so CEL can use the
  bare `mark_done`.
- `mark_done` is currently a demo tool in `orch-mcp-tools`; real tools that share
  daemon state (e.g. over an in-process/HTTP MCP endpoint) are a follow-up noted
  in design doc 101.
