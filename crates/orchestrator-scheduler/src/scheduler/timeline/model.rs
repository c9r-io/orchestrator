use serde::Serialize;

/// Version of the semantic timeline projection contract.
pub const TIMELINE_PROJECTION_VERSION: u32 = 1;

/// Stable semantic category used by timeline clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineCategory {
    /// Task goal or intent.
    Goal,
    /// External source activity.
    Source,
    /// Task or scheduler lifecycle transition.
    Lifecycle,
    /// Workflow cycle boundary.
    Cycle,
    /// Workflow step execution.
    Step,
    /// Agent tool invocation or result.
    Tool,
    /// Test, QA, lint, or validation execution.
    Test,
    /// Produced artifact or structured evidence.
    Artifact,
    /// Execution failure or policy denial.
    Failure,
    /// Retry, rollback, restart, or recovery activity.
    Recovery,
    /// Human-originated control-plane action.
    HumanAction,
    /// Interactive agent-session activity.
    Session,
    /// Successful terminal completion.
    Completion,
}

impl TimelineCategory {
    /// Returns the stable wire-format label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Source => "source",
            Self::Lifecycle => "lifecycle",
            Self::Cycle => "cycle",
            Self::Step => "step",
            Self::Tool => "tool",
            Self::Test => "test",
            Self::Artifact => "artifact",
            Self::Failure => "failure",
            Self::Recovery => "recovery",
            Self::HumanAction => "human_action",
            Self::Session => "session",
            Self::Completion => "completion",
        }
    }

    /// Parses a wire-format category label.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "goal" => Some(Self::Goal),
            "source" => Some(Self::Source),
            "lifecycle" => Some(Self::Lifecycle),
            "cycle" => Some(Self::Cycle),
            "step" => Some(Self::Step),
            "tool" => Some(Self::Tool),
            "test" => Some(Self::Test),
            "artifact" => Some(Self::Artifact),
            "failure" => Some(Self::Failure),
            "recovery" => Some(Self::Recovery),
            "human_action" => Some(Self::HumanAction),
            "session" => Some(Self::Session),
            "completion" => Some(Self::Completion),
            _ => None,
        }
    }
}

/// Actor associated with one semantic timeline entry.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineActorRef {
    /// Actor kind such as `agent`, `system`, or `human`.
    pub actor_type: String,
    /// Stable actor identifier when known.
    pub actor_id: String,
}

/// Bounded evidence reference attached to a timeline entry.
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceRef {
    /// Evidence kind such as `command_run`, `test`, or `artifact`.
    pub kind: String,
    /// Human-readable redacted label.
    pub label: String,
    /// Optional daemon-owned URI; never a raw filesystem path.
    pub uri: Option<String>,
    /// Optional MIME-style content type.
    pub content_type: Option<String>,
    /// Optional digest for immutable content.
    pub digest: Option<String>,
    /// Whether redaction affected the underlying evidence.
    pub redacted: bool,
}

/// One semantic entry in a task/process timeline.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    /// Deterministic projection identifier.
    pub id: String,
    /// Owning task identifier.
    pub task_id: String,
    /// Display timestamp for the entry.
    pub occurred_at: String,
    /// Stable semantic category.
    pub category: TimelineCategory,
    /// Concise redacted title.
    pub title: String,
    /// Concise redacted explanation.
    pub summary: String,
    /// Optional status label such as `running`, `failed`, or `completed`.
    pub status: Option<String>,
    /// Optional actor reference.
    pub actor: Option<TimelineActorRef>,
    /// Optional workflow step identifier.
    pub step_id: Option<String>,
    /// Optional task-item identifier.
    pub task_item_id: Option<String>,
    /// Optional command-run identifier.
    pub command_run_id: Option<String>,
    /// Optional interactive session identifier.
    pub session_id: Option<String>,
    /// Optional logical checkpoint identifier.
    pub checkpoint_id: Option<String>,
    /// Optional external source-event identifier.
    pub source_event_id: Option<String>,
    /// Evidence references associated with the entry.
    pub evidence: Vec<EvidenceRef>,
    /// Database event identifiers consumed by the projection.
    pub raw_event_ids: Vec<i64>,
    /// Projection contract version.
    pub projection_version: u32,
}

/// Cursor-paginated semantic timeline response.
#[derive(Debug, Clone, Serialize)]
pub struct TimelinePage {
    /// Entries in stable source order.
    pub entries: Vec<TimelineEntry>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Whether another page exists in the fixed snapshot.
    pub has_more: bool,
    /// Maximum source event ID included in this snapshot.
    pub snapshot_max_event_id: i64,
    /// Projection contract version.
    pub projection_version: u32,
}

/// Kind of incremental timeline stream update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineDeltaKind {
    /// Insert or replace an entry by stable ID.
    Upsert,
    /// Client should reload the authoritative snapshot.
    ResetRequired,
}

/// Incremental update emitted by timeline follow streams.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineDelta {
    /// Delta operation kind.
    pub kind: TimelineDeltaKind,
    /// Entry to insert or replace for `upsert` updates.
    pub entry: Option<TimelineEntry>,
    /// Latest source event watermark observed by the stream.
    pub snapshot_max_event_id: i64,
}

/// Timeline entry plus its internal stable pagination key.
#[derive(Debug, Clone)]
pub(crate) struct ProjectedTimelineEntry {
    pub(crate) source_order: u64,
    pub(crate) entry: TimelineEntry,
}
