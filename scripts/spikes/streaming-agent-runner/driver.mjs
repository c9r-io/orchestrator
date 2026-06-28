// Throwaway spike: drive `claude` over bidirectional stream-json.
// Goal: prove (1) single long-lived process, (2) multi-turn, (3) all-structured events.
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const MODEL = process.env.SPIKE_MODEL || "haiku";

// Newline-delimited JSON "user" messages we will feed, one per turn.
const TURNS = [
  "Reply with exactly the word: ALPHA. Nothing else.",
  "Now reply with exactly the word: BRAVO. Nothing else.",
];

const env = { ...process.env };
delete env.CLAUDECODE; // avoid nested-session refusal (mirrors orchestrator's env_remove)

const args = [
  "-p",
  "--input-format", "stream-json",
  "--output-format", "stream-json",
  "--verbose",
  "--model", MODEL,
];

console.error(`[driver] spawning: claude ${args.join(" ")}`);
const child = spawn("claude", args, { env, stdio: ["pipe", "pipe", "pipe"] });

let turnIdx = 0;
function sendTurn(i) {
  const msg = {
    type: "user",
    message: { role: "user", content: [{ type: "text", text: TURNS[i] }] },
  };
  const line = JSON.stringify(msg) + "\n";
  console.error(`\n[driver] >>> sending turn ${i}: ${TURNS[i]}`);
  child.stdin.write(line);
}

sendTurn(turnIdx); // turn 0

const rl = createInterface({ input: child.stdout });
rl.on("line", (line) => {
  if (!line.trim()) return;
  let evt;
  try { evt = JSON.parse(line); }
  catch { console.error(`[driver] NON-JSON stdout line: ${line}`); return; }

  const tag = evt.subtype ? `${evt.type}/${evt.subtype}` : evt.type;
  // Show a compact view of each structured event.
  let extra = "";
  if (evt.type === "assistant" && evt.message?.content) {
    extra = " :: " + JSON.stringify(evt.message.content.map(c => c.type === "text" ? {t: c.text} : {tool: c.name, input: c.input}));
  }
  if (evt.type === "result") {
    extra = ` :: result=${JSON.stringify(evt.result)} session=${evt.session_id} cost=$${evt.total_cost_usd ?? "?"}`;
  }
  console.error(`[event] ${tag}${extra}`);

  if (evt.type === "result") {
    turnIdx++;
    if (turnIdx < TURNS.length) {
      sendTurn(turnIdx);
    } else {
      console.error("\n[driver] all turns done; closing stdin");
      child.stdin.end();
    }
  }
});

child.stderr.on("data", (d) => process.stderr.write(`[claude-stderr] ${d}`));
child.on("exit", (code) => console.error(`\n[driver] claude exited code=${code}`));
