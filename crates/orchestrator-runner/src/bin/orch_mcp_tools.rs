//! `orch-mcp-tools` — the orchestrator-owned MCP tool server.
//!
//! A minimal newline-delimited JSON-RPC (MCP) server over stdio, spawned by the
//! agent CLI via `--mcp-config`. It exposes typed tools whose results the
//! orchestrator owns and computes — the substrate that lets coordination logic
//! move out of YAML/CEL and into tools the agent calls during a step.
//!
//! First cut: `run_tests` (canned structured result) and `mark_done` (a
//! completion signal the loop guard converges on — see
//! `docs/showcases/streaming-mark-done-convergence.md`). Replace the canned
//! bodies with real orchestrator logic (or a callback into the daemon over HTTP)
//! as the pivot progresses. See
//! `docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`.

use serde_json::{Value, json};
use std::io::{BufRead, Write};

/// Default MCP protocol version echoed when the client does not specify one.
const DEFAULT_PROTOCOL_VERSION: &str = "2024-11-05";

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    log("ready on stdio");

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                log(&format!("stdin read error: {e}"));
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                log(&format!("ignoring non-JSON line: {e}"));
                continue;
            }
        };

        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let id = request.get("id").cloned();
        log(&format!("recv method={method} id={id:?}"));

        let response = match method {
            "initialize" => Some(handle_initialize(&request, id)),
            "tools/list" => Some(handle_tools_list(id)),
            "tools/call" => Some(handle_tools_call(&request, id)),
            // Notifications (no `id`) require no response.
            "notifications/initialized" => None,
            _ => id.map(|id| error_response(id, -32601, "method not found")),
        };

        if let Some(response) = response {
            if let Err(e) = write_message(&mut stdout, &response) {
                log(&format!("stdout write error: {e}"));
                break;
            }
        }
    }
}

/// Responds to `initialize`, echoing the client's protocol version.
fn handle_initialize(request: &Value, id: Option<Value>) -> Value {
    let protocol_version = request
        .get("params")
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);
    result_response(
        id,
        json!({
            "protocolVersion": protocol_version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "orch-mcp-tools", "version": env!("CARGO_PKG_VERSION") },
        }),
    )
}

/// Advertises the orchestrator-owned tools.
fn handle_tools_list(id: Option<Value>) -> Value {
    result_response(
        id,
        json!({
            "tools": [
                {
                    "name": "run_tests",
                    "description": "Run the project's test suite and return structured pass/fail counts.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "target": { "type": "string", "description": "test target, e.g. 'core'" }
                        },
                        "required": ["target"]
                    }
                },
                {
                    "name": "mark_done",
                    "description": "Signal that the task is complete. Call this once the work is finished; the orchestrator's loop guard converges when this tool has been called.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "summary": { "type": "string", "description": "one-line summary of what was completed" }
                        },
                        "required": ["summary"]
                    }
                }
            ]
        }),
    )
}

/// Executes a tool call. The orchestrator owns and computes the result.
fn handle_tools_call(request: &Value, id: Option<Value>) -> Value {
    let params = request.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let target = params
        .and_then(|p| p.get("arguments"))
        .and_then(|a| a.get("target"))
        .and_then(Value::as_str)
        .unwrap_or("<none>");

    let tool_result = match name {
        "run_tests" => {
            log(&format!(
                ">>> EXECUTING tool 'run_tests' target='{target}' — orchestrator computing the result"
            ));
            // Canned result; the exact failing-test name is a fingerprint the
            // e2e test asserts on to prove the orchestrator owned the result.
            json!({
                "target": target,
                "passed": 3,
                "failed": 1,
                "failures": ["core::selection::picks_healthy_agent"]
            })
        }
        "mark_done" => {
            let summary = params
                .and_then(|p| p.get("arguments"))
                .and_then(|a| a.get("summary"))
                .and_then(Value::as_str)
                .unwrap_or("");
            log(&format!(
                ">>> EXECUTING tool 'mark_done' summary='{summary}'"
            ));
            // The orchestrator's loop guard converges on the call itself
            // (`'mark_done' in tools_called`); the body just acknowledges.
            json!({ "status": "done", "summary": summary })
        }
        _ => return error_response(id.unwrap_or(Value::Null), -32602, "unknown tool"),
    };

    result_response(
        id,
        json!({
            "content": [
                { "type": "text", "text": tool_result.to_string() }
            ]
        }),
    )
}

/// Builds a JSON-RPC success response.
fn result_response(id: Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id.unwrap_or(Value::Null), "result": result })
}

/// Builds a JSON-RPC error response.
fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Writes one newline-delimited JSON-RPC message and flushes.
fn write_message(out: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let mut line = message.to_string();
    line.push('\n');
    out.write_all(line.as_bytes())?;
    out.flush()
}

/// Diagnostic logging to stderr (stdout is reserved for the JSON-RPC channel).
fn log(msg: &str) {
    eprintln!("[orch-mcp-tools] {msg}");
}
