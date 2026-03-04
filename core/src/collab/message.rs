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
    message_store: Arc<RwLock<HashMap<Uuid, AgentMessage>>>,
}

impl MessageBus {
    pub fn new() -> Self {
        let (tx, _rx) = mpsc::channel(1000);
        Self {
            tx,
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

        for _receiver in &msg.receivers {
            let _ = self.tx.send(msg.clone()).await;
        }

        Ok(msg_id)
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
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

    #[tokio::test]
    async fn test_message_bus_publish_stores_message() {
        let bus = MessageBus::new();
        let msg = AgentMessage::new(
            AgentEndpoint::agent("sender"),
            vec![AgentEndpoint::agent("receiver")],
            MessagePayload::Custom(serde_json::json!({"data": "test"})),
        );
        let msg_id = msg.id;
        let result = bus.publish(msg).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), msg_id);

        // Verify the message is stored
        let store = bus.message_store.read().await;
        assert!(store.contains_key(&msg_id));
        assert_eq!(store[&msg_id].sender.agent_id, "sender");
    }

    #[tokio::test]
    async fn test_message_bus_publish_broadcast_no_panic() {
        let bus = MessageBus::new();
        // Broadcast messages have empty receivers — publish should not panic or hang
        let msg = AgentMessage::publish(
            AgentEndpoint::agent("broadcaster"),
            MessagePayload::Custom(serde_json::json!("event")),
        );
        let msg_id = msg.id;
        let result = bus.publish(msg).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), msg_id);

        // Message should still be stored even with no receivers
        let store = bus.message_store.read().await;
        assert!(store.contains_key(&msg_id));
    }
}
