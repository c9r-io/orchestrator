//! Run-scoped stdio MCP transport shim.
//!
//! The shim owns no coordination behavior. It forwards JSON-RPC messages to
//! the authenticated loopback callback created inside `orchestratord` for the
//! current command run. This preserves Claude CLI's stdio MCP integration while
//! keeping tool state and side effects inside the daemon boundary.

use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

fn main() {
    let url = std::env::var("ORCH_MCP_CALLBACK_URL").ok();
    let token = std::env::var("ORCH_MCP_CALLBACK_TOKEN").ok();
    let client = Client::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                log(&format!("invalid JSON-RPC request: {error}"));
                continue;
            }
        };
        let id = request.get("id").cloned();
        let response = match (&url, &token) {
            (Some(url), Some(token)) => forward(&client, url, token, &request),
            _ => Err("run-scoped daemon callback is unavailable".to_string()),
        };
        if id.is_none() {
            if let Err(error) = response {
                log(&error);
            }
            continue;
        }
        let message = match response {
            Ok(value) => value,
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "error": {"code": -32000, "message": error},
            }),
        };
        if write_message(&mut stdout, &message).is_err() {
            break;
        }
    }
}

fn forward(client: &Client, url: &str, token: &str, request: &Value) -> Result<Value, String> {
    let response = client
        .post(url)
        .bearer_auth(token)
        .json(request)
        .send()
        .map_err(|error| format!("daemon callback request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("daemon callback rejected request with {status}"));
    }
    response
        .json()
        .map_err(|error| format!("daemon callback returned invalid JSON: {error}"))
}

fn write_message(out: &mut impl Write, message: &Value) -> std::io::Result<()> {
    writeln!(out, "{message}")?;
    out.flush()
}

fn log(message: &str) {
    eprintln!("[orch-mcp-tools] {message}");
}
