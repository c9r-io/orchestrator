// Minimal hand-rolled MCP stdio server (newline-delimited JSON-RPC 2.0).
// Represents "the orchestrator's own typed tool" whose result WE compute.
// Logs to stderr so we can PROVE our code ran and produced the result.
import { createInterface } from "node:readline";

const log = (...a) => process.stderr.write(`[mcp-server] ${a.join(" ")}\n`);

const TOOLS = [
  {
    name: "run_tests",
    description: "Run the project's test suite and return structured pass/fail counts.",
    inputSchema: {
      type: "object",
      properties: { target: { type: "string", description: "test target, e.g. 'core'" } },
      required: ["target"],
    },
  },
];

function send(msg) {
  process.stdout.write(JSON.stringify(msg) + "\n");
}

const rl = createInterface({ input: process.stdin });
rl.on("line", (line) => {
  if (!line.trim()) return;
  let req;
  try { req = JSON.parse(line); } catch { log("bad json:", line); return; }
  log("recv method=", req.method, "id=", req.id);

  switch (req.method) {
    case "initialize":
      send({ jsonrpc: "2.0", id: req.id, result: {
        protocolVersion: req.params?.protocolVersion || "2024-11-05",
        capabilities: { tools: {} },
        serverInfo: { name: "spike-orchestrator-tools", version: "0.0.1" },
      }});
      break;
    case "notifications/initialized":
      break; // notification, no response
    case "tools/list":
      send({ jsonrpc: "2.0", id: req.id, result: { tools: TOOLS } });
      break;
    case "tools/call": {
      const name = req.params?.name;
      const target = req.params?.arguments?.target ?? "<none>";
      log(`>>> EXECUTING tool '${name}' target='${target}' — THIS IS THE ORCHESTRATOR COMPUTING THE RESULT`);
      // The orchestrator's owned, structured result — not parsed from text.
      const result = { passed: 3, failed: 1, failures: ["core::selection::picks_healthy_agent"] };
      send({ jsonrpc: "2.0", id: req.id, result: {
        content: [{ type: "text", text: JSON.stringify(result) }],
      }});
      break;
    }
    default:
      if (req.id !== undefined) {
        send({ jsonrpc: "2.0", id: req.id, error: { code: -32601, message: "method not found" } });
      }
  }
});
log("ready on stdio");
