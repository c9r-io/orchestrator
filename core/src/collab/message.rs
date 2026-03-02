use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use super::artifact::Artifact;
use super::context::AgentContextRef;
use super::output::AgentOutput;

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
            ttl: Duration::from_secs(300),
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
            receivers: Vec::new(),
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

        {
            let mut store = self.message_store.write().await;
            store.insert(msg_id, msg.clone());
        }

        let receivers = match msg.delivery_mode {
            DeliveryMode::Broadcast => self.find_subscribers(&msg).await,
            _ => msg.receivers.clone(),
        };

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
