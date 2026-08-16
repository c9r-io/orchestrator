use anyhow::{Result, bail};
use async_trait::async_trait;
use futures_core::Stream;
use futures_util::StreamExt;
use orchestrator_config::config::{CancelSemantics, ToolHosting};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::path::Path;
use std::pin::Pin;

/// Object-safe stream returned by a running driver session.
pub type DriverEventStream = Pin<Box<dyn Stream<Item = Result<DriverEvent>> + Send + 'static>>;

/// Opaque provider session material.
///
/// It intentionally implements neither `Serialize` nor `Display`; `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionRef(String);

impl SessionRef {
    /// Creates a validated provider session reference inside the runner boundary.
    pub fn from_provider(value: String) -> Result<Self> {
        if value.trim().is_empty() || value.chars().any(char::is_control) {
            bail!("provider session reference is invalid");
        }
        Ok(Self(value))
    }

    /// Exposes the secret only to trusted persistence/resume code.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SessionRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionRef([REDACTED])")
    }
}

/// Provider-neutral token counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenCounts {
    /// Input tokens consumed by the provider.
    pub input: Option<u64>,
    /// Output tokens produced by the provider.
    pub output: Option<u64>,
}

/// Scope described by a provider permission request.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionScope {
    /// Stable provider-neutral permission kind.
    pub kind: String,
    /// Redacted, bounded detail used by the Attention projection.
    pub detail: Value,
}

/// Terminal driver outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverOutcome {
    /// Provider and process completed successfully.
    Success,
    /// Provider reported a governed failure.
    Failed,
    /// Orchestrator cancelled the run.
    Cancelled,
}

/// One normalized driver event. This is the only observation stream.
#[derive(Debug, Clone, PartialEq)]
pub enum DriverEvent {
    /// Driver process started. Provider session material is optional for shell runs.
    Started {
        /// Opaque provider session material, absent for one-shot shell runs.
        session: Option<SessionRef>,
    },
    /// Human-readable assistant output.
    AssistantText(String),
    /// Provider requested a tool invocation.
    ToolUse {
        /// Provider call identifier.
        call_id: String,
        /// Provider-neutral tool name.
        name: String,
        /// Structured tool arguments.
        args: Value,
    },
    /// Result paired with one tool invocation.
    ToolResult {
        /// Provider call identifier.
        call_id: String,
        /// Structured tool result.
        payload: Value,
        /// Whether the provider marked the result as an error.
        is_error: bool,
    },
    /// Provider requested an audited permission decision.
    PermissionRequested {
        /// Provider request identifier used to correlate the decision.
        request_id: String,
        /// Redacted requested scope.
        scope: PermissionScope,
    },
    /// Usage/cost observation.
    Usage {
        /// Provider-reported cost when available.
        cost_usd: Option<f64>,
        /// Provider-reported token counts.
        tokens: TokenCounts,
    },
    /// Terminal event.
    Finished {
        /// Normalized terminal outcome.
        outcome: DriverOutcome,
        /// Process-compatible exit code. `-1` when the process was killed by a
        /// signal and therefore has no exit code, matching the non-driver wait
        /// path in `phase_runner::wait`.
        exit_code: i32,
        /// The signal that killed the process, when one did.
        ///
        /// Without this the terminal event cannot distinguish "exited 1" from
        /// "killed by SIGXCPU", and a classifier that keys on the signal has no
        /// input at all. That was not hypothetical: the CPU-limit arm of
        /// `detect_resource_exceeded` was unreachable for every driver-executed
        /// step — which is all of them — because this field did not exist and
        /// the wait path substituted `None`. CPU exhaustion is the one sandbox
        /// limit that kills the process instead of making a call fail, so its
        /// stderr is empty and the signal is the only channel it has.
        /// `None` for protocol-driven providers, which report an outcome
        /// rather than a process status. See DD-188.
        exit_signal: Option<i32>,
    },
}

/// Inputs accepted by an active driver session.
#[derive(Debug, Clone, PartialEq)]
pub enum DriverInput {
    /// Additional user turn.
    UserMessage(String),
    /// Result produced by an orchestrator-hosted tool.
    ToolResult {
        /// Provider call identifier.
        call_id: String,
        /// Structured tool result.
        payload: Value,
    },
    /// Audited human permission decision.
    PermissionDecision {
        /// Provider request identifier.
        request_id: String,
        /// Approved when true, rejected when false.
        approved: bool,
    },
    /// Cooperative interrupt request.
    Interrupt,
}

/// Static driver descriptor used by apply-time validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverCapabilities {
    /// Supports multiple user turns in one session.
    pub multi_turn: bool,
    /// Supported orchestrator tool transport.
    pub tool_hosting: ToolHosting,
    /// Supports provider session attachment.
    pub session_resume: bool,
    /// Can emit governed permission requests.
    pub permission_events: bool,
    /// Cancellation guarantee.
    pub cancel: CancelSemantics,
    /// Can execute through the orchestrator sandbox path.
    pub sandboxable: bool,
    /// Reports provider cost.
    pub cost_reporting: bool,
}

/// Folded terminal result derived solely from `DriverEvent`.
#[derive(Debug, Clone, PartialEq)]
pub struct DriverRunResult {
    /// Folded terminal outcome.
    pub outcome: DriverOutcome,
    /// Folded process-compatible exit code.
    pub exit_code: i32,
    /// Assistant text joined in event order.
    pub assistant_text: String,
    /// Normalized events observed before the terminal event.
    pub events: Vec<DriverEvent>,
}

/// Private run-scoped callback used by the stdio MCP transport shim.
///
/// The callback is created by the embedded scheduler, binds only to loopback,
/// and is destroyed with the command run. Its bearer token deliberately has no
/// serialization or display implementation.
#[derive(Clone, PartialEq, Eq)]
pub struct McpCallbackConfig {
    url: String,
    token: String,
}

impl McpCallbackConfig {
    /// Creates a callback descriptor for one command run.
    pub fn new(url: String, token: String) -> Result<Self> {
        let parsed = reqwest::Url::parse(&url);
        let is_loopback_http = parsed.as_ref().is_ok_and(|parsed| {
            parsed.scheme() == "http"
                && parsed.host_str() == Some("127.0.0.1")
                && parsed.port().is_some()
        });
        if !is_loopback_http || token.trim().is_empty() {
            bail!("MCP callback must use loopback HTTP with a non-empty token");
        }
        Ok(Self { url, token })
    }

    /// Returns the loopback callback URL for provider configuration.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Exposes the bearer token only while assembling private provider state.
    pub fn expose_token(&self) -> &str {
        &self.token
    }
}

impl fmt::Debug for McpCallbackConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpCallbackConfig")
            .field("url", &self.url)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

/// Inputs required to start one driver process.
pub struct DriverStartRequest<'a> {
    /// Agent-scoped driver configuration.
    pub driver: &'a orchestrator_config::config::AgentDriverConfig,
    /// Process-wide runner policy shared by every driver.
    pub runner: &'a orchestrator_config::config::RunnerConfig,
    /// Governed command template consumed only by the shell driver.
    pub shell_command: &'a str,
    /// Optional initial stdin payload for shell prompt delivery.
    pub stdin_payload: Option<&'a str>,
    /// Rendered prompt delivered according to provider protocol.
    pub prompt: &'a str,
    /// Governed workspace root.
    pub cwd: &'a Path,
    /// Redacted stdout artifact.
    pub stdout: File,
    /// Redacted stderr artifact.
    pub stderr: File,
    /// Values removed from provider output before it reaches artifacts.
    pub redaction_patterns: &'a [String],
    /// Agent environment resolved from direct values and stores.
    pub extra_env: &'a HashMap<String, String>,
    /// Resolved host/sandbox process boundary.
    pub execution_profile: &'a crate::runner::ResolvedExecutionProfile,
    /// Per-run artifacts directory.
    pub artifacts_dir: &'a Path,
    /// Optional provider session material loaded inside the daemon boundary.
    pub session_ref: Option<&'a SessionRef>,
    /// Private daemon callback used by orchestrator-owned MCP tools.
    pub mcp_callback: Option<&'a McpCallbackConfig>,
}

/// Provider-specific process adapter selected by an Agent manifest.
#[async_trait]
pub trait AgentDriver: Send + Sync {
    /// Stable provider/transport identifier.
    fn id(&self) -> &'static str;

    /// Static capabilities used by apply-time validation.
    fn capabilities(&self) -> DriverCapabilities;

    /// Starts one governed provider session.
    async fn start(&self, request: DriverStartRequest<'_>) -> Result<Box<dyn DriverSession>>;
}

/// One active provider session.
#[async_trait]
pub trait DriverSession: Send + Sync {
    /// Transfers ownership of the single event stream to the caller.
    fn take_events(&mut self) -> Result<DriverEventStream>;

    /// Sends one provider-neutral input into the live session.
    async fn send(&self, input: DriverInput) -> Result<()>;

    /// Requests cancellation according to the driver's advertised semantics.
    async fn cancel(&self) -> Result<()>;

    /// Returns an opaque provider session reference when one has been observed.
    fn session_ref(&self) -> Option<SessionRef>;

    /// Child process identifier used only for liveness and command-run bookkeeping.
    fn pid(&self) -> Option<u32>;

    /// Folds the event stream. This is not a second data path.
    async fn collect(mut self: Box<Self>) -> Result<DriverRunResult> {
        let mut stream = self.take_events()?;
        let mut events = Vec::new();
        let mut assistant_texts = Vec::new();
        let mut terminal = None;
        while let Some(event) = stream.next().await {
            let event = event?;
            if let DriverEvent::AssistantText(text) = &event {
                assistant_texts.push(text.clone());
            }
            if let DriverEvent::Finished {
                outcome,
                exit_code,
                exit_signal,
            } = event
            {
                terminal = Some((outcome, exit_code));
                events.push(DriverEvent::Finished {
                    outcome,
                    exit_code,
                    exit_signal,
                });
                break;
            }
            events.push(event);
        }
        let (outcome, exit_code) = terminal.unwrap_or((DriverOutcome::Failed, -3));
        Ok(DriverRunResult {
            outcome,
            exit_code,
            assistant_text: assistant_texts.join("\n"),
            events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ref_debug_is_always_redacted() {
        let reference =
            SessionRef::from_provider("provider-secret-123".to_string()).expect("reference");
        assert_eq!(format!("{reference:?}"), "SessionRef([REDACTED])");
        assert!(!format!("{reference:?}").contains(reference.expose_secret()));
    }

    #[test]
    fn mcp_callback_requires_loopback_and_redacts_token() {
        let callback = McpCallbackConfig::new(
            "http://127.0.0.1:19001/mcp".to_string(),
            "run-secret".to_string(),
        )
        .expect("callback");
        assert_eq!(callback.url(), "http://127.0.0.1:19001/mcp");
        assert!(!format!("{callback:?}").contains("run-secret"));
        assert!(
            McpCallbackConfig::new("http://0.0.0.0:19001/mcp".to_string(), "secret".to_string())
                .is_err()
        );
        assert!(
            McpCallbackConfig::new(
                "http://127.0.0.1:19001@evil.example/mcp".to_string(),
                "secret".to_string()
            )
            .is_err()
        );
        assert!(
            McpCallbackConfig::new(
                "https://127.0.0.1:19001/mcp".to_string(),
                "secret".to_string()
            )
            .is_err()
        );
        assert!(
            McpCallbackConfig::new("http://127.0.0.1:19001/mcp".to_string(), " ".to_string())
                .is_err()
        );
    }
}
