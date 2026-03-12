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

/// Message envelope exchanged between collaborating agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Unique message identifier.
    pub id: Uuid,
    /// Delivery semantic of the message.
    pub msg_type: MessageType,
    /// Sending endpoint.
    pub sender: AgentEndpoint,
    /// Intended receiving endpoints.
    pub receivers: Vec<AgentEndpoint>,
    /// Message body.
    pub payload: MessagePayload,
    /// Correlation identifier used to tie responses to requests.
    pub correlation_id: Option<Uuid>,
    /// Creation timestamp.
    pub timestamp: DateTime<Utc>,
    /// Time-to-live before the message should be discarded.
    pub ttl: Duration,
    /// Delivery guarantees requested for the transport.
    pub delivery_mode: DeliveryMode,
}

impl AgentMessage {
    /// Creates a request message addressed to one or more receivers.
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

    /// Builds a response message for a prior request.
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

    /// Builds a broadcast-style publish message.
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

/// Address of an agent or a specific agent execution scope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AgentEndpoint {
    /// Agent identifier.
    pub agent_id: String,
    /// Optional phase scope.
    pub phase: Option<String>,
    /// Optional task scope.
    pub task_id: Option<String>,
    /// Optional task-item scope.
    pub item_id: Option<String>,
}

impl AgentEndpoint {
    /// Creates an endpoint scoped only to an agent identifier.
    pub fn agent(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            phase: None,
            task_id: None,
            item_id: None,
        }
    }

    /// Creates an endpoint scoped to a specific agent phase.
    pub fn for_phase(agent_id: &str, phase: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            phase: Some(phase.to_string()),
            task_id: None,
            item_id: None,
        }
    }

    /// Creates an endpoint scoped to a specific task item.
    pub fn for_task_item(agent_id: &str, task_id: &str, item_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            phase: None,
            task_id: Some(task_id.to_string()),
            item_id: Some(item_id.to_string()),
        }
    }
}

/// High-level message intent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageType {
    /// Request expecting a follow-up response.
    Request,
    /// Response correlated to a request.
    Response,
    /// Acknowledgement without a payload-specific result.
    Ack,
    /// Publish/subscribe broadcast.
    Publish,
    /// Relay or delegated message.
    Forward,
}

/// Delivery guarantees requested by the sender.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryMode {
    /// No delivery confirmation required.
    FireAndForget,
    /// Message may be delivered more than once but should not be lost.
    AtLeastOnce,
    /// Transport should avoid duplicate delivery.
    ExactlyOnce,
    /// Broadcast to all interested subscribers.
    Broadcast,
}

/// Message body variants supported by the collaboration layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessagePayload {
    /// Command or phase execution request.
    ExecutionRequest(ExecutionRequest),
    /// Result of an execution request.
    ExecutionResult(ExecutionResult),
    /// Standalone artifact transmission.
    Artifact(Artifact),
    /// Shared-context mutation event.
    ContextUpdate(ContextUpdate),
    /// Runtime control signal.
    ControlSignal(ControlSignal),
    /// Extensible custom JSON payload.
    Custom(serde_json::Value),
}

/// Request payload that asks another agent to perform work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRequest {
    /// Command or prompt to execute.
    pub command: String,
    /// Serialized execution context.
    pub context: AgentContextRef,
    /// Input artifacts made available to the receiver.
    pub input_artifacts: Vec<Artifact>,
    /// Optional expectations for validation and scoring.
    pub expectations: Option<ExecutionExpectations>,
}

/// Response payload carrying execution output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Run identifier of the delegated execution.
    pub run_id: Uuid,
    /// Structured output produced by the execution.
    pub output: AgentOutput,
    /// Success flag derived from validation and exit status.
    pub success: bool,
    /// Optional error message when execution failed.
    pub error: Option<String>,
}

/// Validation expectations associated with an execution request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionExpectations {
    /// Optional JSON schema used to validate structured output.
    pub output_schema: Option<serde_json::Value>,
    /// Additional validation rules evaluated by the caller.
    pub validation_rules: Vec<ValidationRule>,
    /// Minimum acceptable quality score.
    pub quality_threshold: f32,
}

/// A single named validation rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    /// Rule identifier.
    pub name: String,
    /// Expression or predicate body.
    pub expression: String,
    /// Error message emitted when validation fails.
    pub error_message: String,
}

/// Shared-context mutation payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUpdate {
    /// Shared-state key being updated.
    pub key: String,
    /// Value applied by the operation.
    pub value: serde_json::Value,
    /// Mutation operation kind.
    pub operation: ContextUpdateOp,
}

/// Supported shared-context mutation operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextUpdateOp {
    /// Replace the current value.
    Set,
    /// Append to an existing collection-like value.
    Append,
    /// Remove the key entirely.
    Remove,
}

/// Control-plane signal sent between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlSignal {
    /// Signal verb.
    pub signal: Signal,
    /// Optional human-readable reason.
    pub reason: Option<String>,
}

/// Runtime control actions understood by the collaboration layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Signal {
    /// Cancel work in progress.
    Cancel,
    /// Pause work in progress.
    Pause,
    /// Resume paused work.
    Resume,
    /// Request a retry.
    Retry,
    /// Skip the current work item.
    Skip,
}

/// In-memory message bus used by collaboration flows.
pub struct MessageBus {
    tx: mpsc::Sender<AgentMessage>,
    message_store: Arc<RwLock<HashMap<Uuid, AgentMessage>>>,
}

impl MessageBus {
    /// Creates an empty message bus.
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
