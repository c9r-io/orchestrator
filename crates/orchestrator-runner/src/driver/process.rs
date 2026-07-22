use super::{
    DriverEvent, DriverEventStream, DriverInput, DriverOutcome, DriverSession, PermissionScope,
    SessionRef, TokenCounts,
};
use crate::runner::{RunnerStdioMode, kill_child_process_group};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use orchestrator_config::config::DriverProvider;
use serde_json::{Value, json};
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;

pub(super) struct ProcessSession {
    provider: DriverProvider,
    pid: Option<u32>,
    events: Mutex<Option<mpsc::UnboundedReceiver<Result<DriverEvent>>>>,
    stdin: Arc<AsyncMutex<Option<ChildStdin>>>,
    cancel: Mutex<Option<oneshot::Sender<()>>>,
    session_ref: Arc<RwLock<Option<SessionRef>>>,
}

impl ProcessSession {
    pub(super) async fn start(
        provider: DriverProvider,
        mut child: Child,
        stdout_file: File,
        stderr_file: File,
        redaction_patterns: Vec<String>,
        initial_input: Option<String>,
    ) -> Result<Self> {
        let pid = child.id();
        let stdout = child
            .stdout
            .take()
            .context("driver child missing stdout pipe")?;
        let stderr = child
            .stderr
            .take()
            .context("driver child missing stderr pipe")?;
        let stdin = Arc::new(AsyncMutex::new(child.stdin.take()));
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let session_ref = Arc::new(RwLock::new(None));
        let terminal_emitted = Arc::new(AtomicBool::new(false));

        let stdout_tx = event_tx.clone();
        let stdout_session = session_ref.clone();
        let stdout_terminal = terminal_emitted.clone();
        let stdout_patterns = redaction_patterns.clone();
        let stdout_task = tokio::spawn(async move {
            capture_provider_stdout(
                provider,
                stdout,
                stdout_file,
                stdout_patterns,
                stdout_tx,
                stdout_session,
                stdout_terminal,
            )
            .await
        });
        let stderr_task = tokio::spawn(async move {
            crate::output_capture::pipe_and_redact(stderr, stderr_file, redaction_patterns).await
        });

        if let Some(input) = initial_input {
            let mut guard = stdin.lock().await;
            let handle = guard
                .as_mut()
                .context("driver child missing stdin for initial message")?;
            handle.write_all(input.as_bytes()).await?;
            handle.flush().await?;
        }

        let wait_terminal = terminal_emitted.clone();
        tokio::spawn(async move {
            let (status, cancelled) = tokio::select! {
                status = child.wait() => (status, false),
                _ = cancel_rx => {
                    kill_child_process_group(&mut child).await;
                    (child.wait().await, true)
                }
            };
            if let Err(error) = stdout_task
                .await
                .context("driver stdout task panicked")
                .and_then(|r| r)
            {
                let _ = event_tx.send(Err(error));
            }
            if let Err(error) = stderr_task
                .await
                .context("driver stderr task panicked")
                .and_then(|r| r)
            {
                let _ = event_tx.send(Err(error));
            }
            if !wait_terminal.swap(true, Ordering::SeqCst) {
                let (outcome, exit_code) = match status {
                    Ok(status) if cancelled => {
                        (DriverOutcome::Cancelled, status.code().unwrap_or(-5))
                    }
                    Ok(status) if status.success() => (DriverOutcome::Success, 0),
                    Ok(status) => (DriverOutcome::Failed, status.code().unwrap_or(1)),
                    Err(_) => (DriverOutcome::Failed, -3),
                };
                let _ = event_tx.send(Ok(DriverEvent::Finished { outcome, exit_code }));
            }
        });

        Ok(Self {
            provider,
            pid,
            events: Mutex::new(Some(event_rx)),
            stdin,
            cancel: Mutex::new(Some(cancel_tx)),
            session_ref,
        })
    }
}

#[async_trait]
impl DriverSession for ProcessSession {
    fn take_events(&mut self) -> Result<DriverEventStream> {
        let receiver = self
            .events
            .lock()
            .map_err(|_| anyhow::anyhow!("driver event stream lock poisoned"))?
            .take()
            .context("driver event stream was already consumed")?;
        Ok(Box::pin(UnboundedReceiverStream::new(receiver)))
    }

    async fn send(&self, input: DriverInput) -> Result<()> {
        let encoded = encode_input(self.provider, input)?;
        let mut guard = self.stdin.lock().await;
        let handle = guard
            .as_mut()
            .context("driver session input is no longer available")?;
        handle.write_all(encoded.as_bytes()).await?;
        handle.flush().await?;
        Ok(())
    }

    async fn cancel(&self) -> Result<()> {
        let sender = self
            .cancel
            .lock()
            .map_err(|_| anyhow::anyhow!("driver cancellation lock poisoned"))?
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(());
        }
        Ok(())
    }

    fn session_ref(&self) -> Option<SessionRef> {
        self.session_ref.read().ok().and_then(|value| value.clone())
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }
}

fn encode_input(provider: DriverProvider, input: DriverInput) -> Result<String> {
    if provider != DriverProvider::Claude {
        bail!("driver does not support live session input");
    }
    let value = match input {
        DriverInput::UserMessage(message) => json!({
            "type": "user",
            "message": {"role": "user", "content": message}
        }),
        DriverInput::ToolResult { call_id, payload } => json!({
            "type": "user",
            "message": {"role": "user", "content": [{
                "type": "tool_result", "tool_use_id": call_id, "content": payload
            }]}
        }),
        DriverInput::PermissionDecision {
            request_id,
            approved,
        } => json!({
            "type": "permission_result", "request_id": request_id, "approved": approved
        }),
        DriverInput::Interrupt => bail!("use DriverSession::cancel for interruption"),
    };
    Ok(format!("{value}\n"))
}

async fn capture_provider_stdout<R: tokio::io::AsyncRead + Unpin>(
    provider: DriverProvider,
    reader: R,
    file: File,
    redaction_patterns: Vec<String>,
    sender: mpsc::UnboundedSender<Result<DriverEvent>>,
    session_ref: Arc<RwLock<Option<SessionRef>>>,
    terminal_emitted: Arc<AtomicBool>,
) -> Result<()> {
    let mut lines = BufReader::new(reader).lines();
    let mut writer = tokio::fs::File::from_std(file);
    while let Some(line) = lines.next_line().await? {
        let parsed = serde_json::from_str::<Value>(&line).ok();
        let (events, provider_session) = parsed
            .as_ref()
            .map(|value| parse_provider_event(provider, value))
            .unwrap_or_default();
        if let Some(provider_session) = provider_session {
            match SessionRef::from_provider(provider_session) {
                Ok(reference) => {
                    if let Ok(mut guard) = session_ref.write() {
                        *guard = Some(reference.clone());
                    }
                    let _ = sender.send(Ok(DriverEvent::Started {
                        session: Some(reference),
                    }));
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                }
            }
        }
        for event in events {
            if matches!(event, DriverEvent::Finished { .. }) {
                terminal_emitted.store(true, Ordering::SeqCst);
            }
            let _ = sender.send(Ok(event));
        }

        let sanitized = sanitize_provider_line(parsed, &line, &redaction_patterns);
        writer.write_all(sanitized.as_bytes()).await?;
        writer.write_all(b"\n").await?;
    }
    writer.flush().await?;
    Ok(())
}

fn sanitize_provider_line(parsed: Option<Value>, raw: &str, patterns: &[String]) -> String {
    let mut value = match parsed {
        Some(value) => value,
        None => return crate::runner::redact_text(raw, patterns),
    };
    redact_session_fields(&mut value);
    crate::runner::redact_text(&value.to_string(), patterns)
}

fn redact_session_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                if matches!(
                    key.as_str(),
                    "session_id" | "sessionId" | "thread_id" | "threadId" | "conversation_id"
                ) {
                    *nested = Value::String("[REDACTED]".to_string());
                } else {
                    redact_session_fields(nested);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(redact_session_fields),
        _ => {}
    }
}

fn parse_provider_event(
    provider: DriverProvider,
    value: &Value,
) -> (Vec<DriverEvent>, Option<String>) {
    match provider {
        DriverProvider::Claude => parse_claude_event(value),
        DriverProvider::Codex => parse_codex_event(value),
        DriverProvider::Shell => (Vec::new(), None),
    }
}

fn parse_claude_event(value: &Value) -> (Vec<DriverEvent>, Option<String>) {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let session = value
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|_| event_type == "system" || event_type == "result")
        .map(ToOwned::to_owned);
    let mut events = Vec::new();
    if event_type == "assistant" {
        if let Some(content) = value.pointer("/message/content").and_then(Value::as_array) {
            for item in content {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            events.push(DriverEvent::AssistantText(text.to_string()));
                        }
                    }
                    Some("tool_use") => events.push(DriverEvent::ToolUse {
                        call_id: item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        name: item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        args: item.get("input").cloned().unwrap_or(Value::Null),
                    }),
                    _ => {}
                }
            }
        }
    } else if event_type == "user" {
        if let Some(content) = value.pointer("/message/content").and_then(Value::as_array) {
            for item in content {
                if item.get("type").and_then(Value::as_str) == Some("tool_result") {
                    events.push(DriverEvent::ToolResult {
                        call_id: item
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        payload: item.get("content").cloned().unwrap_or(Value::Null),
                        is_error: item
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    });
                }
            }
        }
    } else if event_type == "permission_request" {
        events.push(DriverEvent::PermissionRequested {
            request_id: value
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            scope: PermissionScope {
                kind: value
                    .get("permission")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                detail: value.get("scope").cloned().unwrap_or(Value::Null),
            },
        });
    } else if event_type == "result" {
        events.push(DriverEvent::Usage {
            cost_usd: value.get("total_cost_usd").and_then(Value::as_f64),
            tokens: TokenCounts {
                input: value.pointer("/usage/input_tokens").and_then(Value::as_u64),
                output: value
                    .pointer("/usage/output_tokens")
                    .and_then(Value::as_u64),
            },
        });
        let failed = value
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        events.push(DriverEvent::Finished {
            outcome: if failed {
                DriverOutcome::Failed
            } else {
                DriverOutcome::Success
            },
            exit_code: if failed { 1 } else { 0 },
        });
    }
    (events, session)
}

fn parse_codex_event(value: &Value) -> (Vec<DriverEvent>, Option<String>) {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let session = (event_type == "thread.started")
        .then(|| {
            value
                .get("thread_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .flatten();
    let mut events = Vec::new();
    match event_type {
        "item.completed" => {
            let item = value.get("item").unwrap_or(&Value::Null);
            match item.get("type").and_then(Value::as_str) {
                Some("agent_message") => {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        events.push(DriverEvent::AssistantText(text.to_string()));
                    }
                }
                Some("mcp_tool_call") => events.push(DriverEvent::ToolUse {
                    call_id: item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: item
                        .get("tool")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    args: item.get("arguments").cloned().unwrap_or(Value::Null),
                }),
                _ => {}
            }
        }
        "turn.completed" => events.push(DriverEvent::Usage {
            cost_usd: value.pointer("/usage/cost_usd").and_then(Value::as_f64),
            tokens: TokenCounts {
                input: value.pointer("/usage/input_tokens").and_then(Value::as_u64),
                output: value
                    .pointer("/usage/output_tokens")
                    .and_then(Value::as_u64),
            },
        }),
        "turn.failed" | "error" => events.push(DriverEvent::Finished {
            outcome: DriverOutcome::Failed,
            exit_code: 1,
        }),
        _ => {}
    }
    (events, session)
}

pub(super) fn piped_stdio() -> RunnerStdioMode {
    RunnerStdioMode::Piped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_fixture_maps_session_text_tool_usage_and_terminal() {
        let lines = [
            json!({"type":"system","subtype":"init","session_id":"secret-session"}),
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"done"},{"type":"tool_use","id":"t1","name":"mcp__orch__run_tests","input":{"target":"core"}}]}}),
            json!({"type":"result","is_error":false,"total_cost_usd":0.02,"usage":{"input_tokens":10,"output_tokens":5},"session_id":"secret-session"}),
        ];
        let mut events = Vec::new();
        let mut session = None;
        for line in lines {
            let (mapped, found) = parse_claude_event(&line);
            events.extend(mapped);
            session = found.or(session);
        }
        assert_eq!(session.as_deref(), Some("secret-session"));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, DriverEvent::AssistantText(text) if text == "done"))
        );
        assert!(events.iter().any(|event| matches!(event, DriverEvent::ToolUse { name, .. } if name == "mcp__orch__run_tests")));
        assert!(events.iter().any(|event| matches!(
            event,
            DriverEvent::Finished {
                outcome: DriverOutcome::Success,
                ..
            }
        )));
    }

    #[test]
    fn persisted_json_redacts_session_fields_recursively() {
        let raw = r#"{"type":"system","session_id":"secret","nested":{"thread_id":"also-secret"}}"#;
        let sanitized = sanitize_provider_line(serde_json::from_str(raw).ok(), raw, &[]);
        assert!(!sanitized.contains("also-secret"));
        assert!(!sanitized.contains("\"secret\""));
        assert_eq!(sanitized.matches("[REDACTED]").count(), 2);
    }

    #[test]
    fn codex_fixture_maps_thread_and_message() {
        let (events, session) =
            parse_codex_event(&json!({"type":"thread.started","thread_id":"thread-secret"}));
        assert!(events.is_empty());
        assert_eq!(session.as_deref(), Some("thread-secret"));
        let (events, _) = parse_codex_event(
            &json!({"type":"item.completed","item":{"type":"agent_message","text":"complete"}}),
        );
        assert!(
            matches!(events.as_slice(), [DriverEvent::AssistantText(text)] if text == "complete")
        );
    }
}
