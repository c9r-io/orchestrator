// Phase 2 spike: prove the agent calls OUR typed MCP tool, we compute the
// result, and the agent continues the loop — all structured, no text parsing.
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const MODEL = process.env.SPIKE_MODEL || "haiku";

const mcpConfig = JSON.stringify({
  mcpServers: { spike: { command: "node", args: [join(here, "mcp_server.mjs")] } },
});

const env = { ...process.env };
delete env.CLAUDECODE;

const args = [
  "-p",
  "--input-format", "stream-json",
  "--output-format", "stream-json",
  "--verbose",
  "--model", MODEL,
  "--mcp-config", mcpConfig,
  "--strict-mcp-config",
  "--allowedTools", "mcp__spike__run_tests",
  "--permission-mode", "bypassPermissions",
];

console.error(`[driver] spawning claude with MCP server 'spike'`);
const child = spawn("claude", args, { env, stdio: ["pipe", "pipe", "pipe"] });

const PROMPT =
  "Use the run_tests tool with target 'core' to run the tests. " +
  "Then tell me in one sentence how many tests failed and the name of the failing test.";

child.stdin.write(JSON.stringify({
  type: "user",
  message: { role: "user", content: [{ type: "text", text: PROMPT }] },
}) + "\n");

let sawToolUse = false, sawToolResult = false;
const rl = createInterface({ input: child.stdout });
rl.on("line", (line) => {
  if (!line.trim()) return;
  let evt; try { evt = JSON.parse(line); } catch { return; }
  const tag = evt.subtype ? `${evt.type}/${evt.subtype}` : evt.type;

  if (evt.type === "assistant" && evt.message?.content) {
    for (const c of evt.message.content) {
      if (c.type === "tool_use") {
        sawToolUse = true;
        console.error(`[event] assistant TOOL_USE name=${c.name} input=${JSON.stringify(c.input)}`);
      } else if (c.type === "text" && c.text.trim()) {
        console.error(`[event] assistant TEXT :: ${c.text.trim()}`);
      }
    }
  } else if (evt.type === "user" && evt.message?.content) {
    // tool_result is delivered back into the conversation as a user-role event
    for (const c of (Array.isArray(evt.message.content) ? evt.message.content : [])) {
      if (c.type === "tool_result") {
        sawToolResult = true;
        const txt = Array.isArray(c.content) ? c.content.map(x => x.text).join("") : c.content;
        console.error(`[event] TOOL_RESULT (fed back) :: ${txt}`);
      }
    }
  } else if (evt.type === "result") {
    console.error(`[event] result/${evt.subtype} :: ${JSON.stringify(evt.result)}`);
    console.error(`\n[verdict] sawToolUse=${sawToolUse} sawToolResult=${sawToolResult}`);
    child.stdin.end();
  } else {
    console.error(`[event] ${tag}`);
  }
});

child.stderr.on("data", (d) => process.stderr.write(`${d}`));
child.on("exit", (code) => console.error(`[driver] claude exited code=${code}`));
