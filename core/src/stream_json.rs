//! Parser for the agent CLI `stream-json` event stream.
//!
//! The streaming agent runner captures `claude --output-format stream-json`
//! output as a sequence of newline-delimited JSON events. This module projects
//! that stream into a typed [`StreamRun`] summary — paired tool calls, the
//! agent's final text, and run economics — so the orchestrator can ingest tool
//! I/O and cost/turn data as structured records instead of opaque text. See
//! `docs/design_doc/orchestrator/102-stream-json-event-ingestion.md`.

use crate::collab::{Artifact, ArtifactKind};
use serde_json::Value;
use std::collections::HashMap;

/// Top-level `type` values that mark a line as a known stream-json event. Used
/// to decide whether stdout is a stream-json stream at all.
const KNOWN_EVENT_TYPES: &[&str] = &["system", "assistant", "user", "result", "rate_limit_event"];

/// One tool call observed in the stream, with its result paired by id.
#[derive(Debug, Clone)]
pub struct StreamToolCall {
    /// Fully-qualified tool name, e.g. `mcp__orch__run_tests`.
    pub name: String,
    /// Tool input arguments.
    pub input: Value,
    /// Tool result content, paired from the matching `tool_result` event.
    pub result: Option<Value>,
    /// Whether the tool result was flagged as an error.
    pub is_error: bool,
}

/// Structured summary of a streaming agent run.
#[derive(Debug, Clone, Default)]
pub struct StreamRun {
    /// Whether stdout was recognized as a stream-json event stream.
    pub detected: bool,
    /// Whether a terminal `result` event was observed (false implies truncation).
    pub saw_result: bool,
    /// The agent's final result text from the `result` event.
    pub result_text: Option<String>,
    /// Whether the run reported an error in its `result` event.
    pub is_error: bool,
    /// Total cost in USD reported by the `result` event.
    pub cost_usd: Option<f64>,
    /// Number of turns reported by the `result` event.
    pub num_turns: Option<u32>,
    /// Session identifier reported by the stream.
    pub session_id: Option<String>,
    /// Tool calls observed during the run, in order.
    pub tool_calls: Vec<StreamToolCall>,
    /// Non-empty assistant text blocks, in order.
    pub assistant_texts: Vec<String>,
}

/// Parses a stream-json stdout payload into a [`StreamRun`].
///
/// Tolerant by design: lines that fail to parse or carry unknown `type` values
/// are ignored. `detected` is set from the first JSON-parseable line; if that
/// line is not a known stream event, parsing stops and `detected` stays false so
/// callers fall back to the legacy (single-JSON / plain-text) path.
pub fn parse_stream_run(stdout: &str) -> StreamRun {
    let mut run = StreamRun::default();
    // Maps a `tool_use` id to its index in `run.tool_calls` for result pairing.
    let mut pending: HashMap<String, usize> = HashMap::new();
    let mut decided = false;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");

        if !decided {
            decided = true;
            run.detected = KNOWN_EVENT_TYPES.contains(&event_type);
            if !run.detected {
                return run;
            }
        }

        match event_type {
            "assistant" => collect_assistant(&value, &mut run, &mut pending),
            "user" => collect_tool_results(&value, &mut run, &pending),
            "result" => collect_result(&value, &mut run),
            _ => {}
        }
    }

    run
}

/// Collects `tool_use` and `text` blocks from an assistant event.
fn collect_assistant(value: &Value, run: &mut StreamRun, pending: &mut HashMap<String, usize>) {
    let Some(content) = value
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let input = block.get("input").cloned().unwrap_or(Value::Null);
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let index = run.tool_calls.len();
                run.tool_calls.push(StreamToolCall {
                    name,
                    input,
                    result: None,
                    is_error: false,
                });
                if !id.is_empty() {
                    pending.insert(id, index);
                }
            }
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    run.assistant_texts.push(text.to_string());
                }
            }
            _ => {}
        }
    }
}

/// Pairs `tool_result` blocks from a user event back to their tool calls.
fn collect_tool_results(value: &Value, run: &mut StreamRun, pending: &HashMap<String, usize>) {
    let Some(content) = value
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for block in content {
        if block.get("type").and_then(Value::as_str) != Some("tool_result") {
            continue;
        }
        let id = block
            .get("tool_use_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let is_error = block
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let result = extract_result_content(block.get("content"));
        if let Some(&index) = pending.get(id)
            && let Some(call) = run.tool_calls.get_mut(index)
        {
            call.result = result;
            call.is_error = is_error;
        }
    }
}

/// Reads economics and final text from the terminal `result` event.
fn collect_result(value: &Value, run: &mut StreamRun) {
    run.saw_result = true;
    run.result_text = value
        .get("result")
        .and_then(Value::as_str)
        .map(String::from);
    run.is_error = value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    run.cost_usd = value.get("total_cost_usd").and_then(Value::as_f64);
    run.num_turns = value
        .get("num_turns")
        .and_then(Value::as_u64)
        .map(|n| n as u32);
    run.session_id = value
        .get("session_id")
        .and_then(Value::as_str)
        .map(String::from);
}

/// Normalizes `tool_result` content (string, or array of text blocks) to a value.
fn extract_result_content(content: Option<&Value>) -> Option<Value> {
    match content {
        Some(Value::Array(blocks)) => {
            let text: String = blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");
            if text.is_empty() {
                content.cloned()
            } else {
                Some(Value::String(text))
            }
        }
        other => other.cloned(),
    }
}

/// Strips the `mcp__<server>__` prefix from a tool name so CEL expressions can
/// reference bare names (`'mark_done' in tools_called`). Non-MCP tool names
/// (built-in tools like `Bash`) are returned unchanged.
fn bare_tool_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("mcp__") {
        // rest = "<server>__<tool>"; the tool is everything after the server.
        if let Some((_, tool)) = rest.split_once("__") {
            return tool.to_string();
        }
    }
    name.to_string()
}

/// Derives well-known CEL pipeline variables from a structured run's artifacts.
/// Both legacy `stream_run_summary` and typed `driver_terminal` artifacts are
/// accepted; plain shell output still returns no signals.
///
/// Emitted (typed when bound into CEL): `tools_called` (list<string>),
/// `tool_error_count` / `num_tool_calls` / `run_turns` (int),
/// `agent_reported_error` (bool), `run_cost_usd` (double).
pub fn stream_signal_vars(artifacts: &[Artifact]) -> Vec<(String, String)> {
    let terminal = artifacts.iter().find_map(|artifact| match &artifact.kind {
        ArtifactKind::Data { schema }
            if schema == "stream_run_summary" || schema == "driver_terminal" =>
        {
            Some((
                schema.as_str(),
                artifact.content.clone().unwrap_or(Value::Null),
            ))
        }
        _ => None,
    });
    let Some((terminal_schema, summary)) = terminal else {
        return Vec::new();
    };

    let mut tools_called: Vec<String> = Vec::new();
    let mut tool_error_count: i64 = 0;
    let mut num_tool_calls: i64 = 0;
    for artifact in artifacts {
        if let ArtifactKind::ToolCall { tool } = &artifact.kind {
            num_tool_calls += 1;
            let bare = bare_tool_name(tool);
            if !tools_called.contains(&bare) {
                tools_called.push(bare);
            }
            let errored = artifact
                .content
                .as_ref()
                .and_then(|c| c.get("is_error"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if errored {
                tool_error_count += 1;
            }
        } else if let ArtifactKind::Data { schema } = &artifact.kind
            && schema == "driver_tool_result"
            && artifact
                .content
                .as_ref()
                .and_then(|content| content.get("is_error"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            tool_error_count += 1;
        }
    }
    let agent_reported_error = if terminal_schema == "driver_terminal" {
        summary.get("outcome").and_then(Value::as_str) != Some("success")
    } else {
        summary
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };

    let mut out = vec![
        (
            "tools_called".to_string(),
            serde_json::to_string(&tools_called).unwrap_or_else(|_| "[]".to_string()),
        ),
        ("tool_error_count".to_string(), tool_error_count.to_string()),
        ("num_tool_calls".to_string(), num_tool_calls.to_string()),
        (
            "agent_reported_error".to_string(),
            agent_reported_error.to_string(),
        ),
    ];
    if let Some(cost) = summary.get("cost_usd").and_then(Value::as_f64) {
        out.push(("run_cost_usd".to_string(), cost.to_string()));
    }
    if let Some(turns) = summary.get("num_turns").and_then(Value::as_u64) {
        out.push(("run_turns".to_string(), turns.to_string()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // A compact but faithful stream, mirroring a real `claude` run: a deferred
    // tool-schema load (ToolSearch), the orchestrator MCP tool call, and the
    // terminal result.
    const FIXTURE: &str = concat!(
        r#"{"type":"system","subtype":"init","session_id":"sess-1","model":"claude-haiku-4-5"}"#,
        "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"ToolSearch","input":{"query":"select:mcp__orch__run_tests"}}]}}"#,
        "\n",
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"tool_reference","tool_name":"mcp__orch__run_tests"}]}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"mcp__orch__run_tests","input":{"target":"core"}}]}}"#,
        "\n",
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t2","content":[{"type":"text","text":"{\"failed\":1,\"failures\":[\"core::selection::picks_healthy_agent\"],\"passed\":3}"}]}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"1 test failed: core::selection::picks_healthy_agent."}]}}"#,
        "\n",
        r#"{"type":"result","subtype":"success","is_error":false,"result":"1 test failed: core::selection::picks_healthy_agent.","num_turns":3,"total_cost_usd":0.0233,"session_id":"sess-1"}"#,
        "\n",
    );

    #[test]
    fn parses_full_stream() {
        let run = parse_stream_run(FIXTURE);
        assert!(run.detected);
        assert!(run.saw_result);
        assert!(!run.is_error);
        assert_eq!(run.num_turns, Some(3));
        assert_eq!(run.cost_usd, Some(0.0233));
        assert_eq!(run.session_id.as_deref(), Some("sess-1"));
        assert_eq!(run.tool_calls.len(), 2);

        let call = run
            .tool_calls
            .iter()
            .find(|c| c.name == "mcp__orch__run_tests")
            .expect("run_tests call present");
        assert_eq!(call.input, serde_json::json!({"target": "core"}));
        assert!(!call.is_error);
        let result = call.result.as_ref().expect("paired result");
        assert!(
            result
                .as_str()
                .unwrap_or_default()
                .contains("core::selection::picks_healthy_agent"),
            "tool result should carry the orchestrator-computed payload"
        );
        assert_eq!(run.assistant_texts.len(), 1);
    }

    #[test]
    fn single_json_blob_is_not_detected() {
        // An echo-style strict-phase payload must not be treated as a stream.
        let run = parse_stream_run(r#"{"confidence":0.9,"quality_score":0.8}"#);
        assert!(!run.detected);
        assert!(run.tool_calls.is_empty());
    }

    #[test]
    fn plain_text_is_not_detected() {
        let run = parse_stream_run("just some plain text output\nsecond line");
        assert!(!run.detected);
    }

    #[test]
    fn derives_signal_vars_from_artifacts() {
        use serde_json::json;
        let artifacts = vec![
            Artifact::new(ArtifactKind::ToolCall {
                tool: "mcp__orch__run_tests".to_string(),
            })
            .with_content(json!({"is_error": false})),
            Artifact::new(ArtifactKind::ToolCall {
                tool: "mcp__orch__mark_done".to_string(),
            })
            .with_content(json!({"is_error": false})),
            Artifact::new(ArtifactKind::Data {
                schema: "stream_run_summary".to_string(),
            })
            .with_content(json!({"is_error": false, "cost_usd": 0.02, "num_turns": 3})),
        ];
        let vars: HashMap<String, String> = stream_signal_vars(&artifacts).into_iter().collect();
        // MCP prefixes stripped so CEL can reference bare names.
        assert_eq!(
            vars.get("tools_called").map(String::as_str),
            Some(r#"["run_tests","mark_done"]"#)
        );
        assert_eq!(vars.get("tool_error_count").map(String::as_str), Some("0"));
        assert_eq!(vars.get("num_tool_calls").map(String::as_str), Some("2"));
        assert_eq!(vars.get("run_turns").map(String::as_str), Some("3"));
        assert_eq!(
            vars.get("agent_reported_error").map(String::as_str),
            Some("false")
        );
    }

    #[test]
    fn typed_driver_artifacts_derive_convergence_signals() {
        use serde_json::json;
        let artifacts = vec![
            Artifact::new(ArtifactKind::ToolCall {
                tool: "mcp__orch__mark_done".to_string(),
            })
            .with_content(json!({"call_id": "done-1", "args": {}})),
            Artifact::new(ArtifactKind::Data {
                schema: "driver_tool_result".to_string(),
            })
            .with_content(json!({"call_id": "done-1", "is_error": false})),
            Artifact::new(ArtifactKind::Data {
                schema: "driver_terminal".to_string(),
            })
            .with_content(json!({"outcome": "success", "exit_code": 0})),
        ];
        let vars: HashMap<String, String> = stream_signal_vars(&artifacts).into_iter().collect();
        assert_eq!(
            vars.get("tools_called").map(String::as_str),
            Some(r#"["mark_done"]"#)
        );
        assert_eq!(vars.get("tool_error_count").map(String::as_str), Some("0"));
        assert_eq!(
            vars.get("agent_reported_error").map(String::as_str),
            Some("false")
        );
    }

    #[test]
    fn errored_tool_increments_error_count() {
        use serde_json::json;
        let artifacts = vec![
            Artifact::new(ArtifactKind::ToolCall {
                tool: "run_tests".to_string(),
            })
            .with_content(json!({"is_error": true})),
            Artifact::new(ArtifactKind::Data {
                schema: "stream_run_summary".to_string(),
            })
            .with_content(json!({"is_error": false})),
        ];
        let vars: HashMap<String, String> = stream_signal_vars(&artifacts).into_iter().collect();
        assert_eq!(vars.get("tool_error_count").map(String::as_str), Some("1"));
    }

    #[test]
    fn non_streaming_artifacts_yield_no_signals() {
        let artifacts = vec![Artifact::new(ArtifactKind::Custom {
            name: "whatever".to_string(),
        })];
        assert!(stream_signal_vars(&artifacts).is_empty());
    }

    #[test]
    fn detected_without_result_signals_truncation() {
        let truncated = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"mcp__orch__run_tests","input":{"target":"core"}}]}}"#,
            "\n",
        );
        let run = parse_stream_run(truncated);
        assert!(run.detected);
        assert!(!run.saw_result);
        assert_eq!(run.tool_calls.len(), 1);
    }
}
