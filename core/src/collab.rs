//! Agent Collaboration Module
//!
//! Provides structured agent-to-agent communication, message bus,
//! shared context, and DAG-based workflow execution.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

// ============================================================================
// Helpers
// ============================================================================

/// Escape a string for safe embedding inside a bash double-quoted string.
///
/// Inside bash double quotes, the characters `\`, `$`, `` ` ``, `"`, and `!`
/// are special. This function escapes them so that the shell passes the
/// literal content to the target program without interpretation.
///
/// This is critical for pipeline variables (plan_output, build_output, diff,
/// etc.) whose content may contain markdown with backticks, dollar signs, or
/// other shell metacharacters.
fn escape_for_bash_dquote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            '`' => out.push_str("\\`"),
            '"' => out.push_str("\\\""),
            '!' => out.push_str("\\!"),
            _ => out.push(ch),
        }
    }
    out
}

// ============================================================================
// Core Data Structures
// ============================================================================

/// Agent output with structured data (replaces exit_code-only results)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    pub run_id: Uuid,
    pub agent_id: String,
    pub phase: String,
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub artifacts: Vec<Artifact>,
    pub metrics: ExecutionMetrics,
    pub confidence: f32,
    pub quality_score: f32,
    pub created_at: DateTime<Utc>,
    /// Structured build errors (populated for build/lint phases)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_errors: Vec<crate::config::BuildError>,
    /// Structured test failures (populated for test phases)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub test_failures: Vec<crate::config::TestFailure>,
}

impl AgentOutput {
    pub fn new(
        run_id: Uuid,
        agent_id: String,
        phase: String,
        exit_code: i64,
        stdout: String,
        stderr: String,
    ) -> Self {
        Self {
            run_id,
            agent_id,
            phase,
            exit_code,
            stdout,
            stderr,
            artifacts: Vec::new(),
            metrics: ExecutionMetrics::default(),
            confidence: 1.0,
            quality_score: 1.0,
            created_at: Utc::now(),
            build_errors: Vec::new(),
            test_failures: Vec::new(),
        }
    }

    pub fn with_artifacts(mut self, artifacts: Vec<Artifact>) -> Self {
        self.artifacts = artifacts;
        self
    }

    pub fn with_metrics(mut self, metrics: ExecutionMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn with_quality_score(mut self, score: f32) -> Self {
        self.quality_score = score.clamp(0.0, 1.0);
        self
    }

    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Execution metrics from agent run
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    pub duration_ms: u64,
    pub tokens_consumed: Option<u64>,
    pub api_calls: Option<u32>,
    pub retry_count: u32,
}

/// Artifact produced by an agent (replaces ticket file scanning)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: Uuid,
    pub kind: ArtifactKind,
    pub path: Option<String>,
    pub content: Option<serde_json::Value>,
    pub checksum: String,
    pub created_at: DateTime<Utc>,
}

impl Artifact {
    pub fn new(kind: ArtifactKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            path: None,
            content: None,
            checksum: String::new(),
            created_at: Utc::now(),
        }
    }

    pub fn with_path(mut self, path: String) -> Self {
        self.path = Some(path);
        self
    }

    pub fn with_content(mut self, content: serde_json::Value) -> Self {
        self.content = Some(content);
        self
    }

    pub fn with_checksum(mut self, checksum: String) -> Self {
        self.checksum = checksum;
        self
    }
}

/// Types of artifacts an agent can produce
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactKind {
    Ticket {
        severity: Severity,
        category: String,
    },
    CodeChange {
        files: Vec<String>,
    },
    TestResult {
        passed: u32,
        failed: u32,
    },
    Analysis {
        findings: Vec<Finding>,
    },
    Decision {
        choice: String,
        rationale: String,
    },
    Data {
        schema: String,
    },
    Custom {
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Finding {
    pub title: String,
    pub description: String,
    pub severity: Severity,
    pub location: Option<String>,
    pub suggestion: Option<String>,
}

// ============================================================================
// Message Bus
// ============================================================================

/// Message envelope for agent communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: Uuid,
    pub msg_type: MessageType,
    pub sender: AgentEndpoint,
    pub receivers: Vec<AgentEndpoint>,
    pub payload: MessagePayload,
    pub correlation_id: Option<Uuid>,
    pub timestamp: DateTime<Utc>,
    pub ttl: Duration,
    pub delivery_mode: DeliveryMode,
}

impl AgentMessage {
    pub fn new(
        sender: AgentEndpoint,
        receivers: Vec<AgentEndpoint>,
        payload: MessagePayload,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            msg_type: MessageType::Request,
            sender,
            receivers,
            payload,
            correlation_id: None,
            timestamp: Utc::now(),
            ttl: Duration::from_secs(300), // 5 minutes default
            delivery_mode: DeliveryMode::AtLeastOnce,
        }
    }

    pub fn response_to(original: &AgentMessage, payload: MessagePayload) -> Self {
        Self {
            id: Uuid::new_v4(),
            msg_type: MessageType::Response,
            sender: original
                .receivers
                .first()
                .cloned()
                .unwrap_or(original.sender.clone()),
            receivers: vec![original.sender.clone()],
            payload,
            correlation_id: Some(original.id),
            timestamp: Utc::now(),
            ttl: Duration::from_secs(300),
            delivery_mode: DeliveryMode::AtLeastOnce,
        }
    }

    pub fn publish(sender: AgentEndpoint, payload: MessagePayload) -> Self {
        Self {
            id: Uuid::new_v4(),
            msg_type: MessageType::Publish,
            sender,
            receivers: Vec::new(), // Will be resolved by subscriptions
            payload,
            correlation_id: None,
            timestamp: Utc::now(),
            ttl: Duration::from_secs(60),
            delivery_mode: DeliveryMode::Broadcast,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AgentEndpoint {
    pub agent_id: String,
    pub phase: Option<String>,
    pub task_id: Option<String>,
    pub item_id: Option<String>,
}

impl AgentEndpoint {
    pub fn agent(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            phase: None,
            task_id: None,
            item_id: None,
        }
    }

    pub fn for_phase(agent_id: &str, phase: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            phase: Some(phase.to_string()),
            task_id: None,
            item_id: None,
        }
    }

    pub fn for_task_item(agent_id: &str, task_id: &str, item_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            phase: None,
            task_id: Some(task_id.to_string()),
            item_id: Some(item_id.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageType {
    Request,
    Response,
    Ack,
    Publish,
    Subscribe,
    Forward,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryMode {
    FireAndForget,
    AtLeastOnce,
    ExactlyOnce,
    Broadcast,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessagePayload {
    ExecutionRequest(ExecutionRequest),
    ExecutionResult(ExecutionResult),
    Artifact(Artifact),
    ContextUpdate(ContextUpdate),
    ControlSignal(ControlSignal),
    Custom(serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRequest {
    pub command: String,
    pub context: AgentContextRef,
    pub input_artifacts: Vec<Artifact>,
    pub expectations: Option<ExecutionExpectations>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub run_id: Uuid,
    pub output: AgentOutput,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionExpectations {
    pub output_schema: Option<serde_json::Value>,
    pub validation_rules: Vec<ValidationRule>,
    pub quality_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    pub name: String,
    pub expression: String,
    pub error_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUpdate {
    pub key: String,
    pub value: serde_json::Value,
    pub operation: ContextUpdateOp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextUpdateOp {
    Set,
    Append,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlSignal {
    pub signal: Signal,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Signal {
    Cancel,
    Pause,
    Resume,
    Retry,
    Skip,
}

// ============================================================================
// Message Bus Implementation
// ============================================================================

pub struct MessageBus {
    tx: mpsc::Sender<AgentMessage>,
    subscriptions: Arc<RwLock<HashMap<AgentEndpoint, Vec<MessagePattern>>>>,
    message_store: Arc<RwLock<HashMap<Uuid, AgentMessage>>>,
}

impl MessageBus {
    pub fn new() -> Self {
        let (tx, _rx) = mpsc::channel(1000);
        Self {
            tx,
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            message_store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Publish a message to the bus
    pub async fn publish(&self, msg: AgentMessage) -> Result<Uuid> {
        let msg_id = msg.id;

        // Store message for tracking
        {
            let mut store = self.message_store.write().await;
            store.insert(msg_id, msg.clone());
        }

        // Determine receivers based on delivery mode
        let receivers = match msg.delivery_mode {
            DeliveryMode::Broadcast => self.find_subscribers(&msg).await,
            _ => msg.receivers.clone(),
        };

        // Send to channel (in real impl, would have a background worker)
        for _receiver in receivers {
            let _ = self.tx.send(msg.clone()).await;
        }

        Ok(msg_id)
    }

    /// Subscribe to messages matching a pattern
    pub async fn subscribe(&self, endpoint: AgentEndpoint, pattern: MessagePattern) {
        let mut subs = self.subscriptions.write().await;
        subs.entry(endpoint).or_default().push(pattern);
    }

    /// Get latest message for a specific phase
    pub async fn get_latest_output(&self, phase: &str) -> Result<Option<AgentOutput>> {
        let store = self.message_store.read().await;

        // Find latest execution result for the phase
        let mut latest: Option<(DateTime<Utc>, &AgentMessage)> = None;

        for msg in store.values() {
            if let MessagePayload::ExecutionResult(ref result) = msg.payload {
                if result.output.phase == phase {
                    match latest {
                        None => latest = Some((msg.timestamp, msg)),
                        Some((ts, _)) if msg.timestamp > ts => latest = Some((msg.timestamp, msg)),
                        _ => {}
                    }
                }
            }
        }

        if let Some((_, msg)) = latest {
            if let MessagePayload::ExecutionResult(ref result) = msg.payload {
                return Ok(Some(result.output.clone()));
            }
        }

        Ok(None)
    }

    async fn find_subscribers(&self, msg: &AgentMessage) -> Vec<AgentEndpoint> {
        let subs = self.subscriptions.read().await;
        let mut matches = Vec::new();

        for (endpoint, patterns) in subs.iter() {
            for pattern in patterns {
                if pattern.matches(msg) {
                    matches.push(endpoint.clone());
                    break;
                }
            }
        }

        matches
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Pattern for matching messages
pub enum MessagePattern {
    ByType(MessageType),
    ByPhase(String),
    ByAgent(String),
    ByTaskItem(String, String),
    Custom(Box<dyn Fn(&AgentMessage) -> bool + Send + Sync>),
}

impl Clone for MessagePattern {
    fn clone(&self) -> Self {
        match self {
            MessagePattern::ByType(t) => MessagePattern::ByType(t.clone()),
            MessagePattern::ByPhase(p) => MessagePattern::ByPhase(p.clone()),
            MessagePattern::ByAgent(a) => MessagePattern::ByAgent(a.clone()),
            MessagePattern::ByTaskItem(t, i) => MessagePattern::ByTaskItem(t.clone(), i.clone()),
            MessagePattern::Custom(_) => panic!("Cannot clone Custom pattern"),
        }
    }
}

impl std::fmt::Debug for MessagePattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessagePattern::ByType(t) => f.debug_tuple("ByType").field(t).finish(),
            MessagePattern::ByPhase(p) => f.debug_tuple("ByPhase").field(p).finish(),
            MessagePattern::ByAgent(a) => f.debug_tuple("ByAgent").field(a).finish(),
            MessagePattern::ByTaskItem(t, i) => {
                f.debug_tuple("ByTaskItem").field(t).field(i).finish()
            }
            MessagePattern::Custom(_) => f.debug_tuple("Custom").finish(),
        }
    }
}

impl MessagePattern {
    fn matches(&self, msg: &AgentMessage) -> bool {
        match self {
            MessagePattern::ByType(t) => msg.msg_type == *t,
            MessagePattern::ByPhase(p) => {
                if let MessagePayload::ExecutionRequest(ref req) = msg.payload {
                    req.context.phase.as_deref() == Some(p)
                } else {
                    false
                }
            }
            MessagePattern::ByAgent(agent) => msg.sender.agent_id == *agent,
            MessagePattern::ByTaskItem(task_id, item_id) => {
                msg.sender.task_id.as_ref() == Some(task_id)
                    && msg.sender.item_id.as_ref() == Some(item_id)
            }
            MessagePattern::Custom(f) => f(msg),
        }
    }
}

// ============================================================================
// Agent Context & Registry
// ============================================================================

/// Lightweight reference to agent context (for message payloads)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContextRef {
    pub task_id: String,
    pub item_id: String,
    pub cycle: u32,
    pub phase: Option<String>,
    pub workspace_root: String,
    pub workspace_id: String,
}

/// Full agent context available during execution
#[derive(Debug, Clone)]
pub struct AgentContext {
    pub task_id: String,
    pub item_id: String,
    pub cycle: u32,
    pub phase: String,
    pub workspace_root: PathBuf,
    pub workspace_id: String,
    pub execution_history: Vec<PhaseRecord>,
    pub upstream_outputs: Vec<AgentOutput>,
    pub artifacts: ArtifactRegistry,
    pub shared_state: SharedState,
}

impl AgentContext {
    pub fn new(
        task_id: String,
        item_id: String,
        cycle: u32,
        phase: String,
        workspace_root: PathBuf,
        workspace_id: String,
    ) -> Self {
        Self {
            task_id,
            item_id,
            cycle,
            phase,
            workspace_root,
            workspace_id,
            execution_history: Vec::new(),
            upstream_outputs: Vec::new(),
            artifacts: ArtifactRegistry::default(),
            shared_state: SharedState::default(),
        }
    }

    /// Add upstream output to context
    pub fn add_upstream_output(&mut self, output: AgentOutput) {
        self.upstream_outputs.push(output.clone());

        // Also register as artifact
        for artifact in output.artifacts {
            self.artifacts.register(self.phase.clone(), artifact);
        }
    }

    /// Render template with context variables
    ///
    /// Note: Pipeline variable values are escaped for safe use inside
    /// bash double-quoted strings. This prevents content like markdown
    /// backticks from triggering shell command substitution.
    pub fn render_template(&self, template: &str) -> String {
        self.render_template_with_pipeline(template, None)
    }

    /// Render template with context variables and optional pipeline variables
    pub fn render_template_with_pipeline(
        &self,
        template: &str,
        pipeline: Option<&crate::config::PipelineVariables>,
    ) -> String {
        let mut result = template.to_string();

        // Basic placeholders
        result = result.replace("{task_id}", &self.task_id);
        result = result.replace("{item_id}", &self.item_id);
        result = result.replace("{cycle}", &self.cycle.to_string());
        result = result.replace("{phase}", &self.phase);
        result = result.replace("{workspace_root}", &self.workspace_root.to_string_lossy());
        // Self-bootstrap variables: {source_tree} is an alias for {workspace_root}
        result = result.replace("{source_tree}", &self.workspace_root.to_string_lossy());

        // Pipeline variables from previous steps
        if let Some(pipeline) = pipeline {
            result = result.replace(
                "{build_output}",
                &escape_for_bash_dquote(&pipeline.prev_stdout),
            );
            result = result.replace(
                "{test_output}",
                &escape_for_bash_dquote(&pipeline.prev_stdout),
            );
            result = result.replace("{diff}", &escape_for_bash_dquote(&pipeline.diff));

            // Build errors as JSON for AI agents to parse
            if !pipeline.build_errors.is_empty() {
                let errors_json = serde_json::to_string(&pipeline.build_errors).unwrap_or_default();
                result = result.replace("{build_errors}", &errors_json);
            } else {
                result = result.replace("{build_errors}", "[]");
            }

            // Test failures as JSON
            if !pipeline.test_failures.is_empty() {
                let failures_json =
                    serde_json::to_string(&pipeline.test_failures).unwrap_or_default();
                result = result.replace("{test_failures}", &failures_json);
            } else {
                result = result.replace("{test_failures}", "[]");
            }

            // Custom pipeline vars
            for (key, value) in &pipeline.vars {
                result = result.replace(&format!("{{{}}}", key), &escape_for_bash_dquote(value));
            }
        }

        // Upstream outputs
        for (i, output) in self.upstream_outputs.iter().enumerate() {
            let prefix = format!("upstream[{}]", i);

            result = result.replace(
                &format!("{}.exit_code", prefix),
                &output.exit_code.to_string(),
            );
            result = result.replace(
                &format!("{}.confidence", prefix),
                &output.confidence.to_string(),
            );
            result = result.replace(
                &format!("{}.quality_score", prefix),
                &output.quality_score.to_string(),
            );
            result = result.replace(
                &format!("{}.duration_ms", prefix),
                &output.metrics.duration_ms.to_string(),
            );

            // Artifacts
            for (j, artifact) in output.artifacts.iter().enumerate() {
                if let Some(content) = &artifact.content {
                    let key = format!("{}.artifacts[{}].content", prefix, j);
                    result = result.replace(
                        &format!("{{{}}}", key),
                        &serde_json::to_string(content).unwrap_or_default(),
                    );
                }
            }
        }

        // Shared state
        result = self.shared_state.render_template(&result);

        // Artifact registry shortcuts
        result = result.replace("{artifacts.count}", &self.artifacts.count().to_string());

        result
    }

    /// Convert to lightweight reference for message payloads
    pub fn to_ref(&self) -> AgentContextRef {
        AgentContextRef {
            task_id: self.task_id.clone(),
            item_id: self.item_id.clone(),
            cycle: self.cycle,
            phase: Some(self.phase.clone()),
            workspace_root: self.workspace_root.to_string_lossy().to_string(),
            workspace_id: self.workspace_id.clone(),
        }
    }
}

/// Record of a single phase execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseRecord {
    pub phase: String,
    pub agent_id: String,
    pub run_id: Uuid,
    pub exit_code: i64,
    pub output: Option<AgentOutput>,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
}

/// Registry of artifacts available in current context
#[derive(Debug, Default)]
pub struct ArtifactRegistry {
    artifacts: HashMap<String, Vec<Artifact>>,
}

impl Clone for ArtifactRegistry {
    fn clone(&self) -> Self {
        Self {
            artifacts: self.artifacts.clone(),
        }
    }
}

impl ArtifactRegistry {
    pub fn register(&mut self, phase: String, artifact: Artifact) {
        self.artifacts.entry(phase).or_default().push(artifact);
    }

    pub fn get_by_phase(&self, phase: &str) -> Vec<&Artifact> {
        self.artifacts
            .get(phase)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    pub fn get_by_kind(&self, kind: &ArtifactKind) -> Vec<&Artifact> {
        self.artifacts
            .values()
            .flatten()
            .filter(|a| &a.kind == kind)
            .collect()
    }

    pub fn get_latest(&self, phase: &str) -> Option<&Artifact> {
        self.artifacts.get(phase).and_then(|v| v.last())
    }

    pub fn count(&self) -> usize {
        self.artifacts.values().map(|v| v.len()).sum()
    }

    pub fn all(&self) -> HashMap<String, Vec<&Artifact>> {
        self.artifacts
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().collect()))
            .collect()
    }
}

/// Key-value store for shared state between agents
#[derive(Debug, Default, Clone)]
pub struct SharedState {
    data: HashMap<String, serde_json::Value>,
}

impl SharedState {
    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.data.insert(key.into(), value);
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.data.get(key)
    }

    pub fn remove(&mut self, key: &str) -> Option<serde_json::Value> {
        self.data.remove(key)
    }

    pub fn render_template(&self, template: &str) -> String {
        let mut result = template.to_string();
        for (key, value) in &self.data {
            let placeholder = format!("{{{}}}", key);
            if let Some(s) = value.as_str() {
                result = result.replace(&placeholder, s);
            } else if let Ok(s) = serde_json::to_string(value) {
                result = result.replace(&placeholder, &s);
            }
        }
        result
    }
}

/// Parse artifacts from agent stdout/stderr output
pub fn parse_artifacts_from_output(output: &str) -> Vec<Artifact> {
    let mut artifacts = Vec::new();

    // Try to parse as JSON array of artifacts
    if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(output) {
        for value in parsed {
            if let Some(kind) = extract_artifact_kind(&value) {
                let mut artifact = Artifact::new(kind);
                if let Some(path) = value.get("path").and_then(|v| v.as_str()) {
                    artifact = artifact.with_path(path.to_string());
                }
                if let Some(content) = value.get("content") {
                    artifact = artifact.with_content(content.clone());
                }
                artifacts.push(artifact);
            }
        }
        return artifacts;
    }

    // Try to parse as JSON object
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output) {
        if let Some(kind) = extract_artifact_kind(&parsed) {
            let mut artifact = Artifact::new(kind);
            if let Some(path) = parsed.get("path").and_then(|v| v.as_str()) {
                artifact = artifact.with_path(path.to_string());
            }
            if let Some(content) = parsed.get("content") {
                artifact = artifact.with_content(content.clone());
            }
            artifacts.push(artifact);
        }

        // Handle structured output with nested "artifacts" array
        // e.g. {"confidence":0.4,"artifacts":[{"kind":"ticket",...}]}
        if artifacts.is_empty() {
            if let Some(arr) = parsed.get("artifacts").and_then(|v| v.as_array()) {
                for value in arr {
                    if let Some(kind) = extract_artifact_kind(value) {
                        let mut artifact = Artifact::new(kind);
                        if let Some(path) = value.get("path").and_then(|v| v.as_str()) {
                            artifact = artifact.with_path(path.to_string());
                        }
                        if let Some(content) = value.get("content") {
                            artifact = artifact.with_content(content.clone());
                        }
                        artifacts.push(artifact);
                    }
                }
            }
        }
    }

    // Try to extract ticket markers from plain text
    for line in output.lines() {
        if let Some(ticket) = parse_ticket_from_line(line) {
            artifacts.push(ticket);
        }
    }

    artifacts
}

fn extract_artifact_kind(value: &serde_json::Value) -> Option<ArtifactKind> {
    let kind = value.get("kind")?.as_str()?;

    match kind {
        "ticket" => {
            let severity = value
                .get("severity")
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "critical" => Severity::Critical,
                    "high" => Severity::High,
                    "medium" => Severity::Medium,
                    "low" => Severity::Low,
                    _ => Severity::Info,
                })
                .unwrap_or(Severity::Info);

            let category = value
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("general")
                .to_string();

            Some(ArtifactKind::Ticket { severity, category })
        }
        "code_change" => {
            let files = value
                .get("files")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|f| f.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            Some(ArtifactKind::CodeChange { files })
        }
        "test_result" => {
            let passed = value.get("passed").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let failed = value.get("failed").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

            Some(ArtifactKind::TestResult { passed, failed })
        }
        "analysis" => {
            let findings = value
                .get("findings")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|f| {
                            Some(Finding {
                                title: f.get("title")?.as_str()?.to_string(),
                                description: f
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                severity: f
                                    .get("severity")
                                    .and_then(|v| v.as_str())
                                    .map(|s| match s {
                                        "critical" => Severity::Critical,
                                        "high" => Severity::High,
                                        "medium" => Severity::Medium,
                                        "low" => Severity::Low,
                                        _ => Severity::Info,
                                    })
                                    .unwrap_or(Severity::Info),
                                location: f
                                    .get("location")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                                suggestion: f
                                    .get("suggestion")
                                    .and_then(|v| v.as_str())
                                    .map(String::from),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(ArtifactKind::Analysis { findings })
        }
        "decision" => {
            let choice = value
                .get("choice")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let rationale = value
                .get("rationale")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            Some(ArtifactKind::Decision { choice, rationale })
        }
        _ => None,
    }
}

fn parse_ticket_from_line(line: &str) -> Option<Artifact> {
    // Look for ticket markers like: [TICKET: severity=high, category=bug]
    if !line.contains("[TICKET:") {
        return None;
    }

    let severity = if line.contains("severity=critical") {
        Severity::Critical
    } else if line.contains("severity=high") {
        Severity::High
    } else if line.contains("severity=medium") {
        Severity::Medium
    } else if line.contains("severity=low") {
        Severity::Low
    } else {
        Severity::Info
    };

    let category = if line.contains("category=bug") {
        "bug".to_string()
    } else if line.contains("category=security") {
        "security".to_string()
    } else if line.contains("category=performance") {
        "performance".to_string()
    } else {
        "general".to_string()
    };

    Some(Artifact::new(ArtifactKind::Ticket { severity, category }))
}

// ============================================================================
// DAG Workflow
// ============================================================================

/// Directed Acyclic Graph workflow definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDag {
    pub id: String,
    pub name: String,
    pub nodes: HashMap<String, WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
}

impl WorkflowDag {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: WorkflowNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn add_edge(&mut self, edge: WorkflowEdge) {
        self.edges.push(edge);
    }

    pub fn get_entry_nodes(&self) -> Vec<&String> {
        let targets: std::collections::HashSet<_> = self.edges.iter().map(|e| &e.to).collect();

        self.nodes.keys().filter(|k| !targets.contains(k)).collect()
    }

    pub fn get_ready_nodes(&self, completed: &std::collections::HashSet<String>) -> Vec<String> {
        // A node is ready if all its dependencies are completed
        self.nodes
            .keys()
            .filter(|k| !completed.contains(*k))
            .filter(|k| {
                let deps = self.get_dependencies(k);
                deps.iter().all(|d| completed.contains(d))
            })
            .cloned()
            .collect()
    }

    fn get_dependencies(&self, node_id: &str) -> Vec<String> {
        self.edges
            .iter()
            .filter(|e| e.to == node_id)
            .map(|e| e.from.clone())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    pub step_type: StepType,
    pub agent_requirement: AgentRequirement,
    pub prehook: Option<String>,
    pub config: NodeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequirement {
    pub capability: Option<String>,
    pub preferred_agents: Vec<String>,
    pub min_success_rate: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    pub timeout_ms: Option<u64>,
    pub retry_enabled: bool,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub from: String,
    pub to: String,
    pub condition: Option<String>,
    pub transform: Option<OutputTransform>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputTransform {
    pub source_phase: String,
    pub extraction: OutputExtraction,
    pub target_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputExtraction {
    AllArtifacts,
    ArtifactKind(String),
    LastN(u32),
    Filter(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepType {
    InitOnce,
    Qa,
    TicketScan,
    Fix,
    Retest,
    LoopGuard,
    Custom(String),
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_output_creation() {
        let output = AgentOutput::new(
            Uuid::new_v4(),
            "qa_agent".to_string(),
            "qa".to_string(),
            0,
            "test output".to_string(),
            "".to_string(),
        );

        assert!(output.is_success());
        assert_eq!(output.confidence, 1.0);
    }

    #[test]
    fn test_artifact_registry() {
        let mut registry = ArtifactRegistry::default();

        let artifact = Artifact::new(ArtifactKind::Ticket {
            severity: Severity::High,
            category: "bug".to_string(),
        });

        registry.register("qa".to_string(), artifact);

        assert_eq!(registry.count(), 1);
        assert!(registry.get_latest("qa").is_some());
    }

    #[test]
    fn test_shared_state_template() {
        let mut state = SharedState::default();
        state.set("name", serde_json::json!("test"));
        state.set("count", serde_json::json!(42));

        let result = state.render_template("Hello {name}, count is {count}");
        assert_eq!(result, "Hello test, count is 42");
    }

    #[test]
    fn test_agent_context_template() {
        let ctx = AgentContext::new(
            "task1".to_string(),
            "item1".to_string(),
            1,
            "qa".to_string(),
            PathBuf::from("/workspace"),
            "ws1".to_string(),
        );

        let result = ctx.render_template("Task: {task_id}, Item: {item_id}, Cycle: {cycle}");
        assert_eq!(result, "Task: task1, Item: item1, Cycle: 1");
    }

    #[test]
    fn test_workflow_dag_entry_nodes() {
        let mut dag = WorkflowDag::new("test".to_string(), "Test Workflow".to_string());

        dag.add_node(WorkflowNode {
            id: "start".to_string(),
            step_type: StepType::InitOnce,
            agent_requirement: AgentRequirement {
                capability: None,
                preferred_agents: vec![],
                min_success_rate: None,
            },
            prehook: None,
            config: NodeConfig::default(),
        });

        dag.add_node(WorkflowNode {
            id: "qa".to_string(),
            step_type: StepType::Qa,
            agent_requirement: AgentRequirement {
                capability: Some("qa".to_string()),
                preferred_agents: vec![],
                min_success_rate: None,
            },
            prehook: None,
            config: NodeConfig::default(),
        });

        dag.add_edge(WorkflowEdge {
            from: "start".to_string(),
            to: "qa".to_string(),
            condition: None,
            transform: None,
        });

        let entries = dag.get_entry_nodes();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], "start");
    }

    #[test]
    fn test_escape_for_bash_dquote() {
        // Backticks (the exact issue: markdown with `resource.rs` etc.)
        assert_eq!(escape_for_bash_dquote("`resource.rs`"), "\\`resource.rs\\`");

        // Dollar signs (prevent variable expansion)
        assert_eq!(escape_for_bash_dquote("$HOME"), "\\$HOME");

        // Double quotes
        assert_eq!(escape_for_bash_dquote(r#"say "hello""#), r#"say \"hello\""#);

        // Backslashes
        assert_eq!(escape_for_bash_dquote(r"path\to"), r"path\\to");

        // History expansion
        assert_eq!(escape_for_bash_dquote("wow!"), "wow\\!");

        // Plain text passes through unchanged
        assert_eq!(escape_for_bash_dquote("hello world"), "hello world");

        // Combined: realistic markdown plan output
        let plan = "| `mod.rs` | ~200 | Core types, `pub(super)` |\n| $cost | ~$5 |";
        let escaped = escape_for_bash_dquote(plan);
        // Verify specific escaped sequences are present
        assert!(escaped.contains("\\`mod.rs\\`"));
        assert!(escaped.contains("\\`pub(super)\\`"));
        assert!(escaped.contains("\\$cost"));
        assert!(escaped.contains("\\$5"));
        // No unescaped shell metacharacters (backtick not preceded by backslash)
        assert!(!escaped.contains(" `m")); // would mean unescaped backtick
    }

    #[test]
    fn test_agent_output_failure() {
        let output = AgentOutput::new(
            Uuid::new_v4(),
            "impl_agent".to_string(),
            "implement".to_string(),
            1,
            "".to_string(),
            "error".to_string(),
        );
        assert!(!output.is_success());
    }

    #[test]
    fn test_agent_output_builder_methods() {
        let output = AgentOutput::new(
            Uuid::new_v4(),
            "agent".to_string(),
            "qa".to_string(),
            0,
            "ok".to_string(),
            "".to_string(),
        )
        .with_confidence(0.85)
        .with_quality_score(0.9)
        .with_metrics(ExecutionMetrics {
            duration_ms: 1000,
            tokens_consumed: Some(500),
            api_calls: Some(3),
            retry_count: 1,
        })
        .with_artifacts(vec![Artifact::new(ArtifactKind::Custom {
            name: "test".to_string(),
        })]);

        assert_eq!(output.confidence, 0.85);
        assert_eq!(output.quality_score, 0.9);
        assert_eq!(output.metrics.duration_ms, 1000);
        assert_eq!(output.artifacts.len(), 1);
    }

    #[test]
    fn test_agent_output_confidence_clamped() {
        let output = AgentOutput::new(
            Uuid::new_v4(),
            "a".to_string(),
            "p".to_string(),
            0,
            "".to_string(),
            "".to_string(),
        )
        .with_confidence(1.5)
        .with_quality_score(-0.5);

        assert_eq!(output.confidence, 1.0);
        assert_eq!(output.quality_score, 0.0);
    }

    #[test]
    fn test_artifact_builder() {
        let artifact = Artifact::new(ArtifactKind::CodeChange {
            files: vec!["main.rs".to_string()],
        })
        .with_path("/tmp/diff.patch".to_string())
        .with_content(serde_json::json!({"lines_added": 10}))
        .with_checksum("abc123".to_string());

        assert_eq!(artifact.path.unwrap(), "/tmp/diff.patch");
        assert!(artifact.content.is_some());
        assert_eq!(artifact.checksum, "abc123");
    }

    #[test]
    fn test_agent_endpoint_constructors() {
        let ep1 = AgentEndpoint::agent("qa_agent");
        assert_eq!(ep1.agent_id, "qa_agent");
        assert!(ep1.phase.is_none());

        let ep2 = AgentEndpoint::for_phase("impl_agent", "implement");
        assert_eq!(ep2.agent_id, "impl_agent");
        assert_eq!(ep2.phase.as_deref(), Some("implement"));

        let ep3 = AgentEndpoint::for_task_item("agent1", "task1", "item1");
        assert_eq!(ep3.task_id.as_deref(), Some("task1"));
        assert_eq!(ep3.item_id.as_deref(), Some("item1"));
    }

    #[test]
    fn test_agent_message_new() {
        let sender = AgentEndpoint::agent("sender");
        let receiver = AgentEndpoint::agent("receiver");
        let msg = AgentMessage::new(
            sender.clone(),
            vec![receiver.clone()],
            MessagePayload::Custom(serde_json::json!("hello")),
        );
        assert_eq!(msg.msg_type, MessageType::Request);
        assert_eq!(msg.sender.agent_id, "sender");
        assert_eq!(msg.receivers.len(), 1);
    }

    #[test]
    fn test_agent_message_response_to() {
        let original = AgentMessage::new(
            AgentEndpoint::agent("alice"),
            vec![AgentEndpoint::agent("bob")],
            MessagePayload::Custom(serde_json::json!("req")),
        );
        let response =
            AgentMessage::response_to(&original, MessagePayload::Custom(serde_json::json!("resp")));
        assert_eq!(response.msg_type, MessageType::Response);
        assert_eq!(response.correlation_id, Some(original.id));
        assert_eq!(response.sender.agent_id, "bob");
        assert_eq!(response.receivers[0].agent_id, "alice");
    }

    #[test]
    fn test_agent_message_publish() {
        let msg = AgentMessage::publish(
            AgentEndpoint::agent("broadcaster"),
            MessagePayload::Custom(serde_json::json!("broadcast")),
        );
        assert_eq!(msg.msg_type, MessageType::Publish);
        assert!(msg.receivers.is_empty());
    }

    #[test]
    fn test_message_pattern_by_type() {
        let msg = AgentMessage::new(
            AgentEndpoint::agent("a"),
            vec![AgentEndpoint::agent("b")],
            MessagePayload::Custom(serde_json::json!("test")),
        );
        assert!(MessagePattern::ByType(MessageType::Request).matches(&msg));
        assert!(!MessagePattern::ByType(MessageType::Response).matches(&msg));
    }

    #[test]
    fn test_message_pattern_by_agent() {
        let msg = AgentMessage::new(
            AgentEndpoint::agent("qa_agent"),
            vec![],
            MessagePayload::Custom(serde_json::json!("x")),
        );
        assert!(MessagePattern::ByAgent("qa_agent".to_string()).matches(&msg));
        assert!(!MessagePattern::ByAgent("other".to_string()).matches(&msg));
    }

    #[test]
    fn test_message_pattern_by_task_item() {
        let msg = AgentMessage::new(
            AgentEndpoint::for_task_item("agent", "t1", "i1"),
            vec![],
            MessagePayload::Custom(serde_json::json!("x")),
        );
        assert!(MessagePattern::ByTaskItem("t1".to_string(), "i1".to_string()).matches(&msg));
        assert!(!MessagePattern::ByTaskItem("t1".to_string(), "i2".to_string()).matches(&msg));
    }

    #[test]
    fn test_message_pattern_clone() {
        let pattern = MessagePattern::ByPhase("qa".to_string());
        let cloned = pattern.clone();
        if let MessagePattern::ByPhase(p) = cloned {
            assert_eq!(p, "qa");
        } else {
            panic!("unexpected pattern variant");
        }
    }

    #[test]
    fn test_message_pattern_debug() {
        let pattern = MessagePattern::ByType(MessageType::Request);
        let debug = format!("{:?}", pattern);
        assert!(debug.contains("ByType"));
    }

    #[test]
    fn test_artifact_registry_get_by_phase() {
        let mut registry = ArtifactRegistry::default();
        registry.register(
            "qa".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "a".to_string(),
            }),
        );
        registry.register(
            "implement".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "b".to_string(),
            }),
        );

        assert_eq!(registry.get_by_phase("qa").len(), 1);
        assert_eq!(registry.get_by_phase("implement").len(), 1);
        assert_eq!(registry.get_by_phase("nonexistent").len(), 0);
    }

    #[test]
    fn test_artifact_registry_get_by_kind() {
        let mut registry = ArtifactRegistry::default();
        let kind = ArtifactKind::TestResult {
            passed: 10,
            failed: 2,
        };
        registry.register("qa".to_string(), Artifact::new(kind.clone()));
        registry.register(
            "qa".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "x".to_string(),
            }),
        );

        let results = registry.get_by_kind(&kind);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_artifact_registry_get_latest() {
        let mut registry = ArtifactRegistry::default();
        assert!(registry.get_latest("qa").is_none());

        registry.register(
            "qa".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "first".to_string(),
            }),
        );
        registry.register(
            "qa".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "second".to_string(),
            }),
        );

        let latest = registry.get_latest("qa").unwrap();
        if let ArtifactKind::Custom { name } = &latest.kind {
            assert_eq!(name, "second");
        }
    }

    #[test]
    fn test_artifact_registry_all() {
        let mut registry = ArtifactRegistry::default();
        registry.register(
            "qa".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "a".to_string(),
            }),
        );
        registry.register(
            "plan".to_string(),
            Artifact::new(ArtifactKind::Custom {
                name: "b".to_string(),
            }),
        );

        let all = registry.all();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_shared_state_operations() {
        let mut state = SharedState::default();
        assert!(state.get("key").is_none());

        state.set("key", serde_json::json!("value"));
        assert_eq!(state.get("key").unwrap(), &serde_json::json!("value"));

        let removed = state.remove("key");
        assert!(removed.is_some());
        assert!(state.get("key").is_none());
    }

    #[test]
    fn test_shared_state_render_non_string_json() {
        let mut state = SharedState::default();
        state.set("data", serde_json::json!({"nested": true}));

        let result = state.render_template("result: {data}");
        assert!(result.contains("nested"));
    }

    #[test]
    fn test_agent_context_to_ref() {
        let ctx = AgentContext::new(
            "t1".to_string(),
            "i1".to_string(),
            2,
            "qa".to_string(),
            PathBuf::from("/ws"),
            "ws1".to_string(),
        );
        let r = ctx.to_ref();
        assert_eq!(r.task_id, "t1");
        assert_eq!(r.item_id, "i1");
        assert_eq!(r.cycle, 2);
        assert_eq!(r.phase, Some("qa".to_string()));
    }

    #[test]
    fn test_agent_context_add_upstream_output() {
        let mut ctx = AgentContext::new(
            "t1".to_string(),
            "i1".to_string(),
            1,
            "qa".to_string(),
            PathBuf::from("/ws"),
            "ws1".to_string(),
        );

        let output = AgentOutput::new(
            Uuid::new_v4(),
            "plan_agent".to_string(),
            "plan".to_string(),
            0,
            "plan output".to_string(),
            "".to_string(),
        )
        .with_artifacts(vec![Artifact::new(ArtifactKind::Custom {
            name: "plan_doc".to_string(),
        })]);

        ctx.add_upstream_output(output);
        assert_eq!(ctx.upstream_outputs.len(), 1);
        assert_eq!(ctx.artifacts.count(), 1);
    }

    #[test]
    fn test_agent_context_render_source_tree_alias() {
        let ctx = AgentContext::new(
            "t1".to_string(),
            "i1".to_string(),
            1,
            "qa".to_string(),
            PathBuf::from("/workspace"),
            "ws1".to_string(),
        );
        let result = ctx.render_template("root={source_tree}");
        assert_eq!(result, "root=/workspace");
    }

    #[test]
    fn test_parse_artifacts_from_output_json_object() {
        let input = r#"{"kind":"ticket","severity":"high","category":"bug"}"#;
        let artifacts = parse_artifacts_from_output(input);
        assert_eq!(artifacts.len(), 1);
        if let ArtifactKind::Ticket { severity, category } = &artifacts[0].kind {
            assert_eq!(*severity, Severity::High);
            assert_eq!(category, "bug");
        } else {
            panic!("expected Ticket");
        }
    }

    #[test]
    fn test_parse_artifacts_from_output_nested_artifacts_array() {
        let input = r#"{"confidence":0.4,"quality_score":0.25,"artifacts":[{"kind":"ticket","severity":"high","category":"capability","content":{"title":"qa-from-agent"}}]}"#;
        let artifacts = parse_artifacts_from_output(input);
        assert_eq!(artifacts.len(), 1);
        if let ArtifactKind::Ticket { severity, category } = &artifacts[0].kind {
            assert_eq!(*severity, Severity::High);
            assert_eq!(category, "capability");
        } else {
            panic!("expected Ticket from nested artifacts array");
        }
    }

    #[test]
    fn test_parse_artifacts_from_output_json_array() {
        let input = r#"[{"kind":"test_result","passed":5,"failed":1},{"kind":"code_change","files":["a.rs"]}]"#;
        let artifacts = parse_artifacts_from_output(input);
        assert_eq!(artifacts.len(), 2);
    }

    #[test]
    fn test_parse_artifacts_from_output_ticket_marker() {
        let input = "some output\n[TICKET: severity=high, category=bug]\nmore output";
        let artifacts = parse_artifacts_from_output(input);
        assert_eq!(artifacts.len(), 1);
        if let ArtifactKind::Ticket { severity, category } = &artifacts[0].kind {
            assert_eq!(*severity, Severity::High);
            assert_eq!(category, "bug");
        }
    }

    #[test]
    fn test_parse_artifacts_from_output_ticket_severity_levels() {
        let levels = [
            ("severity=critical", Severity::Critical),
            ("severity=medium", Severity::Medium),
            ("severity=low", Severity::Low),
            ("severity=unknown", Severity::Info),
        ];
        for (marker, expected) in levels {
            let input = format!("[TICKET: {}, category=bug]", marker);
            let artifacts = parse_artifacts_from_output(&input);
            assert_eq!(artifacts.len(), 1);
            if let ArtifactKind::Ticket { severity, .. } = &artifacts[0].kind {
                assert_eq!(*severity, expected, "failed for marker: {}", marker);
            }
        }
    }

    #[test]
    fn test_parse_artifacts_from_output_ticket_categories() {
        let categories = [
            ("category=security", "security"),
            ("category=performance", "performance"),
            ("category=other", "general"),
        ];
        for (marker, expected) in categories {
            let input = format!("[TICKET: severity=high, {}]", marker);
            let artifacts = parse_artifacts_from_output(&input);
            if let ArtifactKind::Ticket { category, .. } = &artifacts[0].kind {
                assert_eq!(category, expected);
            }
        }
    }

    #[test]
    fn test_parse_artifacts_from_output_no_artifacts() {
        let artifacts = parse_artifacts_from_output("plain text with no markers");
        assert!(artifacts.is_empty());
    }

    #[test]
    fn test_extract_artifact_kind_decision() {
        let value = serde_json::json!({
            "kind": "decision",
            "choice": "option_a",
            "rationale": "better performance"
        });
        let kind = extract_artifact_kind(&value).unwrap();
        if let ArtifactKind::Decision { choice, rationale } = kind {
            assert_eq!(choice, "option_a");
            assert_eq!(rationale, "better performance");
        } else {
            panic!("expected Decision");
        }
    }

    #[test]
    fn test_extract_artifact_kind_analysis() {
        let value = serde_json::json!({
            "kind": "analysis",
            "findings": [
                {"title": "Issue 1", "severity": "high", "description": "desc"}
            ]
        });
        let kind = extract_artifact_kind(&value).unwrap();
        if let ArtifactKind::Analysis { findings } = kind {
            assert_eq!(findings.len(), 1);
            assert_eq!(findings[0].title, "Issue 1");
            assert_eq!(findings[0].severity, Severity::High);
        }
    }

    #[test]
    fn test_extract_artifact_kind_unknown_returns_none() {
        let value = serde_json::json!({"kind": "unknown_type"});
        assert!(extract_artifact_kind(&value).is_none());
    }

    #[test]
    fn test_extract_artifact_kind_missing_kind_returns_none() {
        let value = serde_json::json!({"data": "value"});
        assert!(extract_artifact_kind(&value).is_none());
    }

    #[test]
    fn test_workflow_dag_get_ready_nodes() {
        let mut dag = WorkflowDag::new("test".to_string(), "Test".to_string());

        dag.add_node(WorkflowNode {
            id: "a".to_string(),
            step_type: StepType::InitOnce,
            agent_requirement: AgentRequirement {
                capability: None,
                preferred_agents: vec![],
                min_success_rate: None,
            },
            prehook: None,
            config: NodeConfig::default(),
        });
        dag.add_node(WorkflowNode {
            id: "b".to_string(),
            step_type: StepType::Qa,
            agent_requirement: AgentRequirement {
                capability: None,
                preferred_agents: vec![],
                min_success_rate: None,
            },
            prehook: None,
            config: NodeConfig::default(),
        });
        dag.add_edge(WorkflowEdge {
            from: "a".to_string(),
            to: "b".to_string(),
            condition: None,
            transform: None,
        });

        let completed = std::collections::HashSet::new();
        let ready = dag.get_ready_nodes(&completed);
        assert_eq!(ready.len(), 1);
        assert!(ready.contains(&"a".to_string()));

        let mut completed = std::collections::HashSet::new();
        completed.insert("a".to_string());
        let ready = dag.get_ready_nodes(&completed);
        assert_eq!(ready.len(), 1);
        assert!(ready.contains(&"b".to_string()));
    }

    #[test]
    fn test_pipeline_vars_escaped_in_template() {
        let ctx = AgentContext::new(
            "task1".to_string(),
            "item1".to_string(),
            1,
            "plan".to_string(),
            PathBuf::from("/workspace"),
            "ws1".to_string(),
        );

        let mut pipeline = crate::config::PipelineVariables::default();
        pipeline.vars.insert(
            "plan_output".to_string(),
            "Split `resource.rs` into `mod.rs` and `api.rs`".to_string(),
        );

        let template = r#"claude "Plan: {plan_output}""#;
        let rendered = ctx.render_template_with_pipeline(template, Some(&pipeline));

        // Backticks must be escaped so bash doesn't run command substitution
        assert!(rendered.contains("\\`resource.rs\\`"));
        assert!(rendered.contains("\\`mod.rs\\`"));
        assert!(rendered.contains("\\`api.rs\\`"));
        // Must NOT contain raw backticks (except in the escaped form)
        assert!(!rendered.contains(" `resource.rs` "));
    }
}
