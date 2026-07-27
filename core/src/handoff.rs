//! Immutable task handoffs and fail-closed logical resume planning.
//!
//! A handoff is a deterministic projection of persisted orchestration state. A resume plan is a
//! separate, expiring intent record: callers must reserve execution with the same state version
//! before any scheduler or workspace mutation is attempted.
//!
//! The three tables live in `orchestrator_persistence::handoff_store` (FR-130
//! B14). What stays here is the projection: what a briefing contains, how a
//! workspace is digested, how a state version is hashed, which resume modes a
//! boundary supports, and what an expired or stale plan means.

use crate::async_database::AsyncDatabase;
use crate::config::SideEffectClass;
use crate::config_load::now_ts;
use anyhow::{Context, Result, anyhow, bail};
use chrono::{Duration, Utc};
use orchestrator_persistence::handoff_store as store;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

const PROJECTION_VERSION: i64 = 1;
const MAX_LIST_ENTRIES: usize = 50;
const MAX_TEXT_CHARS: usize = 2_000;

/// Deterministic, bounded briefing content suitable for a handoff panel or a new agent prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffBriefing {
    /// Original task goal.
    pub goal: String,
    /// Current persisted task status and cycle.
    pub current_state: Value,
    /// Most recent successful semantic event, if present.
    pub last_success: Option<Value>,
    /// Most recent failure evidence, if present.
    pub failure: Option<Value>,
    /// Bounded structured test evidence.
    pub test_evidence: Vec<Value>,
    /// Changed file paths found in structured event evidence.
    pub changed_files: Vec<String>,
    /// Persisted constraints safe to expose to another session.
    pub constraints: Vec<String>,
    /// Persisted decisions safe to expose to another session.
    pub decisions: Vec<String>,
    /// Open questions extracted from structured evidence.
    pub open_questions: Vec<String>,
    /// Deterministic next-action recommendations.
    pub recommendations: Vec<String>,
}

/// Immutable persisted handoff snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffSnapshot {
    /// Opaque snapshot identifier.
    pub id: String,
    /// Owning project.
    pub project_id: String,
    /// Source task.
    pub task_id: String,
    /// Highest included event identifier.
    pub source_event_cursor: i64,
    /// Projection schema version.
    pub projection_version: i64,
    /// Deterministic structured briefing.
    pub briefing: HandoffBriefing,
    /// SHA-256 of canonical structured content and cursor.
    pub content_hash: String,
    /// Optimistic task-state fingerprint captured at generation.
    pub state_version: String,
    /// Trusted actor that generated the snapshot.
    pub generated_by: String,
    /// Persistence timestamp; excluded from the content hash.
    pub created_at: String,
}

/// A logical execution boundary. Checkpoint identifiers are references only and never imply a
/// destructive source-control rollback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumeBoundary {
    /// Stable boundary identifier derived from task, cycle, step and item.
    pub id: String,
    /// Source task.
    pub task_id: String,
    /// Loop cycle at which replay would start.
    pub cycle: i64,
    /// Step at which replay would start.
    pub step_id: Option<String>,
    /// Optional task-item scope.
    pub task_item_id: Option<String>,
    /// Optional latest command-run reference.
    pub command_run_id: Option<String>,
    /// Optional provider session presence. The opaque token is never returned.
    pub provider_session_available: bool,
    /// Optional source-control checkpoint reference. This is not a rollback instruction.
    pub checkpoint_id: Option<String>,
    /// Strongest declared side effect at the boundary.
    pub side_effect_class: SideEffectClass,
    /// Whether replay is safe without elevated confirmation.
    pub replay_safe: bool,
    /// Human-readable reason for offering the boundary.
    pub reason: String,
    /// Captured optimistic task-state fingerprint.
    pub state_version: String,
}

/// Explicit resume operation selected during planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeMode {
    /// Continue normal scheduling from the persisted task state.
    ContinueTask,
    /// Retry one failed item at its current logical boundary.
    RetryItem,
    /// Create a correlated child task beginning at a logical step boundary.
    RestartFromBoundary,
    /// Ask the provider adapter to reuse an existing provider session.
    ResumeProviderSession,
}

impl ResumeMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ContinueTask => "continue_task",
            Self::RetryItem => "retry_item",
            Self::RestartFromBoundary => "restart_from_boundary",
            Self::ResumeProviderSession => "resume_provider_session",
        }
    }

    /// Parses the stable API/storage label.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "continue_task" => Ok(Self::ContinueTask),
            "retry_item" => Ok(Self::RetryItem),
            "restart_from_boundary" => Ok(Self::RestartFromBoundary),
            "resume_provider_session" => Ok(Self::ResumeProviderSession),
            _ => bail!("unsupported resume mode: {value}"),
        }
    }

    /// Returns the stable API/storage label.
    pub fn label(self) -> &'static str {
        self.as_str()
    }
}

/// Persisted consequence preview produced before mutating execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumePlan {
    /// Opaque plan identifier required by execute.
    pub id: String,
    /// Source task.
    pub task_id: String,
    /// Selected boundary.
    pub boundary: ResumeBoundary,
    /// Selected operation.
    pub mode: ResumeMode,
    /// Expected state fingerprint checked again during execute.
    pub expected_state_version: String,
    /// Deterministic consequence preview.
    pub consequence: Value,
    /// Whether elevated confirmation is required.
    pub elevated_confirmation_required: bool,
    /// Plan expiration timestamp.
    pub expires_at: String,
    /// Plan lifecycle status.
    pub status: String,
}

/// Result of atomically reserving a plan execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumeExecutionReservation {
    /// Durable execution identifier.
    pub id: String,
    /// Plan being executed.
    pub plan_id: String,
    /// True only for the caller that owns subsequent scheduler side effects.
    pub should_execute: bool,
    /// Current durable execution status.
    pub status: String,
}

/// Trusted and operator-supplied fields required to reserve a resume execution.
#[derive(Debug, Clone)]
pub struct ResumeExecutionRequest {
    /// State version returned by the reviewed plan.
    pub expected_state_version: String,
    /// Retry-safe caller key.
    pub idempotency_key: String,
    /// Trusted control-plane actor.
    pub actor: String,
    /// Required audit reason.
    pub operator_reason: String,
    /// Explicit acknowledgement for non-idempotent replay.
    pub elevated_confirmation: bool,
    /// Whether project policy permits elevated replay.
    pub elevated_policy_enabled: bool,
}

/// SQLite-backed handoff and resume repository.
#[derive(Clone)]
pub struct AsyncHandoffRepository {
    db: Arc<AsyncDatabase>,
}

impl AsyncHandoffRepository {
    /// Creates a repository sharing the daemon database connections.
    pub fn new(db: Arc<AsyncDatabase>) -> Self {
        Self { db }
    }

    /// Generates or returns an immutable deterministic snapshot at the requested event cursor.
    pub async fn generate_snapshot(
        &self,
        task_id: &str,
        requested_cursor: Option<i64>,
        actor: &str,
    ) -> Result<HandoffSnapshot> {
        let inputs = store::snapshot_inputs(&self.db, task_id.to_string())
            .await?
            .ok_or_else(|| anyhow!("task not found: {task_id}"))?;
        let cursor = requested_cursor.unwrap_or(inputs.max_cursor);
        if cursor > inputs.max_cursor || cursor < 0 {
            bail!(
                "invalid handoff cursor {cursor}; latest is {}",
                inputs.max_cursor
            );
        }
        let events: Vec<(String, Value, String)> =
            store::events_up_to(&self.db, task_id.to_string(), cursor)
                .await?
                .into_iter()
                .map(|event| {
                    (
                        event.event_type,
                        serde_json::from_str::<Value>(&event.payload_json).unwrap_or_default(),
                        event.created_at,
                    )
                })
                .collect();

        let briefing = project_briefing(
            inputs.goal.clone(),
            inputs.status.clone(),
            inputs.current_cycle,
            &events,
        );
        // Out here on purpose: this runs three `git` subprocesses and reads
        // every untracked file in the workspace. It used to happen inside the
        // SQLite writer's closure.
        let state_version = task_state_version(&inputs.state_version)?;
        let content_hash = hash_value(&json!({
            "task_id": task_id,
            "cursor": cursor,
            "projection_version": PROJECTION_VERSION,
            "briefing": briefing,
        }))?;

        let row = store::find_or_insert_snapshot(
            &self.db,
            store::NewSnapshot {
                id: Uuid::new_v4().to_string(),
                project_id: inputs.project_id,
                task_id: task_id.to_string(),
                source_event_cursor: cursor,
                projection_version: PROJECTION_VERSION,
                briefing_json: serde_json::to_string(&briefing)?,
                content_hash,
                state_version,
                generated_by: actor.to_string(),
                created_at: now_ts(),
            },
        )
        .await?;
        snapshot_from_row(row)
    }

    /// Returns one immutable snapshot.
    pub async fn get_snapshot(&self, id: &str) -> Result<Option<HandoffSnapshot>> {
        store::read_snapshot(&self.db, id.to_string())
            .await?
            .map(snapshot_from_row)
            .transpose()
    }

    /// Lists logical resume boundaries without changing task or workspace state.
    pub async fn list_boundaries(&self, task_id: &str) -> Result<Vec<ResumeBoundary>> {
        let inputs = store::boundary_inputs(&self.db, task_id.to_string())
            .await?
            .ok_or_else(|| anyhow!("task not found: {task_id}"))?;
        project_boundaries(task_id, &inputs)
    }

    /// Persists an expiring consequence preview. No task or workspace mutation occurs.
    pub async fn create_plan(
        &self,
        task_id: &str,
        boundary_id: &str,
        mode: ResumeMode,
        actor: &str,
        attention_item_id: Option<&str>,
    ) -> Result<ResumePlan> {
        let inputs = store::boundary_inputs(&self.db, task_id.to_string())
            .await?
            .ok_or_else(|| anyhow!("task not found: {task_id}"))?;
        let boundary = project_boundaries(task_id, &inputs)?
            .into_iter()
            .find(|boundary| boundary.id == boundary_id)
            .ok_or_else(|| anyhow!("resume boundary is stale or unknown"))?;
        if mode == ResumeMode::RetryItem && boundary.task_item_id.is_none() {
            bail!("retry_item requires an item-scoped boundary");
        }
        if mode == ResumeMode::ResumeProviderSession && !boundary.provider_session_available {
            bail!("provider session is unavailable; create a restart_from_boundary plan instead");
        }
        let plan_id = Uuid::new_v4().to_string();
        let elevated = !boundary.replay_safe;
        let consequence = json!({
            "mode": mode,
            "task_id": task_id,
            "boundary_id": boundary.id,
            "step_id": boundary.step_id,
            "task_item_id": boundary.task_item_id,
            "creates_correlated_child": mode == ResumeMode::RestartFromBoundary || mode == ResumeMode::ResumeProviderSession,
            "workspace_rollback": false,
            "provider_session_reuse": mode == ResumeMode::ResumeProviderSession,
            "fallback_mode": if mode == ResumeMode::ResumeProviderSession { Value::String("restart_from_boundary".to_string()) } else { Value::Null },
        });
        let expires_at = (Utc::now() + Duration::minutes(15)).to_rfc3339();
        store::insert_plan(
            &self.db,
            store::NewResumePlan {
                id: plan_id.clone(),
                project_id: inputs.project_id.clone(),
                task_id: task_id.to_string(),
                attention_item_id: attention_item_id.map(str::to_string),
                boundary_id: boundary.id.clone(),
                mode: mode.as_str().to_string(),
                expected_state_version: boundary.state_version.clone(),
                side_effect_class: side_effect_label(boundary.side_effect_class).to_string(),
                replay_safe: boundary.replay_safe,
                elevated_confirmation_required: elevated,
                consequence_json: serde_json::to_string(&consequence)?,
                execution_input_json: serde_json::to_string(&boundary)?,
                provider_command_run_id: boundary.command_run_id.clone(),
                expires_at: expires_at.clone(),
                created_by: actor.to_string(),
                created_at: now_ts(),
            },
        )
        .await?;
        Ok(ResumePlan {
            id: plan_id,
            task_id: task_id.to_string(),
            expected_state_version: boundary.state_version.clone(),
            boundary,
            mode,
            consequence,
            elevated_confirmation_required: elevated,
            expires_at,
            status: "planned".to_string(),
        })
    }

    /// Returns one persisted resume plan while its referenced boundary is still available.
    pub async fn get_plan(&self, id: &str) -> Result<Option<ResumePlan>> {
        store::read_plan(&self.db, id.to_string())
            .await?
            .map(|row| plan_from_row(id, row))
            .transpose()
    }

    /// Atomically reserves a plan after stale-state, expiry, policy and confirmation checks.
    /// The caller may perform scheduler side effects only when `should_execute` is true.
    pub async fn reserve_execution(
        &self,
        plan_id: &str,
        request: ResumeExecutionRequest,
    ) -> Result<ResumeExecutionReservation> {
        if request.idempotency_key.trim().is_empty() || request.operator_reason.trim().is_empty() {
            bail!("idempotency_key and operator_reason are required");
        }
        let plan = store::read_plan(&self.db, plan_id.to_string())
            .await?
            .ok_or_else(|| anyhow!("resume plan not found"))?;
        if plan.status != "planned" {
            bail!("resume plan is not executable: {}", plan.status);
        }
        if chrono::DateTime::parse_from_rfc3339(&plan.expires_at)? < Utc::now() {
            bail!("resume plan has expired");
        }
        // The `git`-backed digest, computed before the reservation rather than
        // inside its transaction. What that opens — the plan moving in between —
        // is closed by the store, which re-fences on both `status='planned'` and
        // this same `expected_state_version` and writes nothing if either moved.
        let inputs = store::state_version_inputs(&self.db, plan.task_id.clone())
            .await?
            .ok_or_else(|| anyhow!("resume plan references a task that no longer exists"))?;
        let current_version = task_state_version(&inputs)?;
        if request.expected_state_version != plan.expected_state_version
            || current_version != plan.expected_state_version
        {
            bail!("stale resume plan: task state changed; generate a new consequence preview");
        }
        if plan.elevated_confirmation_required != 0
            && (!request.elevated_policy_enabled || !request.elevated_confirmation)
        {
            bail!("non-idempotent replay denied: elevated policy and confirmation are required");
        }
        let request_hash = hash_value(&json!({
            "plan_id": plan_id,
            "expected_state_version": request.expected_state_version,
            "actor": request.actor,
            "operator_reason": request.operator_reason,
            "elevated_confirmation": request.elevated_confirmation,
        }))?;
        let outcome = store::reserve_execution(
            &self.db,
            store::NewExecution {
                id: Uuid::new_v4().to_string(),
                plan_id: plan_id.to_string(),
                actor: request.actor.clone(),
                operator_reason: request.operator_reason.clone(),
                idempotency_key: request.idempotency_key.clone(),
                request_hash,
                verified_state_version: plan.expected_state_version.clone(),
                created_at: now_ts(),
            },
        )
        .await?;
        match outcome {
            store::Reservation::Existing { id, status } => Ok(ResumeExecutionReservation {
                id,
                plan_id: plan_id.to_string(),
                should_execute: false,
                status,
            }),
            store::Reservation::Reserved { id } => Ok(ResumeExecutionReservation {
                id,
                plan_id: plan_id.to_string(),
                should_execute: true,
                status: "executing".to_string(),
            }),
            store::Reservation::PlanMoved => {
                bail!("stale resume plan: task state changed; generate a new consequence preview")
            }
        }
    }

    /// Completes an owned execution reservation after the scheduler mutation finishes.
    pub async fn complete_execution(
        &self,
        execution_id: &str,
        child_task_id: Option<&str>,
        error_code: Option<&str>,
    ) -> Result<()> {
        let status = if error_code.is_some() {
            "failed"
        } else {
            "succeeded"
        };
        if !store::complete_execution(
            &self.db,
            execution_id.to_string(),
            status.to_string(),
            child_task_id.map(str::to_string),
            error_code.map(str::to_string),
            now_ts(),
        )
        .await?
        {
            bail!("resume execution reservation is missing or already terminal");
        }
        Ok(())
    }
}

/// Parses a stored snapshot row back into its typed form.
///
/// The briefing is this module's type, so the parse lives here; below the
/// boundary a `serde_json` failure would have to be reported as a
/// column-conversion failure against an invented column index.
fn snapshot_from_row(row: store::SnapshotRow) -> Result<HandoffSnapshot> {
    Ok(HandoffSnapshot {
        briefing: serde_json::from_str(&row.briefing_json)
            .with_context(|| format!("parse briefing of handoff snapshot {}", row.id))?,
        id: row.id,
        project_id: row.project_id,
        task_id: row.task_id,
        source_event_cursor: row.source_event_cursor,
        projection_version: row.projection_version,
        content_hash: row.content_hash,
        state_version: row.state_version,
        generated_by: row.generated_by,
        created_at: row.created_at,
    })
}

fn plan_from_row(id: &str, row: store::ResumePlanRow) -> Result<ResumePlan> {
    Ok(ResumePlan {
        id: id.to_string(),
        task_id: row.task_id,
        boundary: serde_json::from_str(&row.execution_input_json)
            .context("resume plan boundary snapshot is invalid")?,
        mode: ResumeMode::parse(&row.mode)?,
        expected_state_version: row.expected_state_version,
        consequence: serde_json::from_str(&row.consequence_json)?,
        elevated_confirmation_required: row.elevated_confirmation_required != 0,
        expires_at: row.expires_at,
        status: row.status,
    })
}

fn project_briefing(
    goal: String,
    status: String,
    cycle: i64,
    events: &[(String, Value, String)],
) -> HandoffBriefing {
    let mut last_success = None;
    let mut failure = None;
    let mut tests = Vec::new();
    let mut changed_files = BTreeSet::new();
    let mut constraints = BTreeSet::new();
    let mut decisions = BTreeSet::new();
    let mut questions = BTreeSet::new();

    for (event_type, payload, created_at) in events {
        let normalized = event_type.to_ascii_lowercase();
        let evidence = json!({
            "event_type": event_type,
            "created_at": created_at,
            "summary": bounded_payload(payload),
        });
        if normalized.contains("success") || normalized.contains("completed") {
            last_success = Some(evidence.clone());
        }
        if normalized.contains("fail") || normalized.contains("error") {
            failure = Some(evidence.clone());
        }
        if normalized.contains("test") || normalized.contains("qa") || normalized.contains("lint") {
            tests.push(evidence);
        }
        collect_string_values(payload, &["changed_files", "files"], &mut changed_files);
        collect_string_values(payload, &["constraints"], &mut constraints);
        collect_string_values(payload, &["decisions"], &mut decisions);
        collect_string_values(payload, &["open_questions", "questions"], &mut questions);
    }

    let recommendations = if failure.is_some() {
        vec![
            "Review the failure evidence and choose a listed logical resume boundary.".to_string(),
            "Generate a consequence preview before executing any retry or restart.".to_string(),
        ]
    } else if status == "completed" {
        vec!["Review the final evidence before closing the task.".to_string()]
    } else {
        vec!["Continue from the latest persisted orchestration state.".to_string()]
    };

    HandoffBriefing {
        goal: truncate_text(&goal),
        current_state: json!({"status": status, "cycle": cycle}),
        last_success,
        failure,
        test_evidence: tests.into_iter().rev().take(MAX_LIST_ENTRIES).collect(),
        changed_files: changed_files.into_iter().take(MAX_LIST_ENTRIES).collect(),
        constraints: constraints.into_iter().take(MAX_LIST_ENTRIES).collect(),
        decisions: decisions.into_iter().take(MAX_LIST_ENTRIES).collect(),
        open_questions: questions.into_iter().take(MAX_LIST_ENTRIES).collect(),
        recommendations,
    }
}

fn collect_string_values(value: &Value, keys: &[&str], output: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                if keys.contains(&key.as_str()) {
                    match nested {
                        Value::String(text) => {
                            output.insert(truncate_text(text));
                        }
                        Value::Array(values) => {
                            for value in values {
                                if let Some(text) = value.as_str() {
                                    output.insert(truncate_text(text));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                collect_string_values(nested, keys, output);
            }
        }
        Value::Array(values) => {
            for nested in values {
                collect_string_values(nested, keys, output);
            }
        }
        _ => {}
    }
}

fn bounded_payload(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(truncate_text(text)),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(MAX_LIST_ENTRIES)
                .map(bounded_payload)
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .filter(|(key, _)| !is_sensitive_key(key))
                .take(MAX_LIST_ENTRIES)
                .map(|(key, value)| (key.clone(), bounded_payload(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["token", "secret", "password", "authorization", "cookie"]
        .iter()
        .any(|pattern| key.contains(pattern))
}

fn truncate_text(value: &str) -> String {
    value.chars().take(MAX_TEXT_CHARS).collect()
}

/// Hashes the task columns that define a resume-relevant state version,
/// together with a digest of the workspace on disk.
///
/// Takes the columns rather than a connection: [`workspace_state_digest`] runs
/// three `git` subprocesses and reads every untracked file in the workspace,
/// and this used to run inside the SQLite writer's closure — and, in
/// `reserve_execution`, inside its transaction.
fn task_state_version(inputs: &store::StateVersionInputs) -> Result<String> {
    let mut value = json!({
        "status": inputs.status,
        "current_cycle": inputs.current_cycle,
        "init_done": inputs.init_done,
        "pipeline_vars": inputs
            .pipeline_vars_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .unwrap_or_default(),
        "execution_plan": inputs
            .execution_plan_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .unwrap_or_default(),
        "updated_at": inputs.updated_at,
        "event_cursor": inputs.event_cursor,
    });
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "workspace_state".to_string(),
            Value::String(workspace_state_digest(Path::new(&inputs.workspace_root))),
        );
    }
    hash_value(&value)
}

fn workspace_state_digest(root: &Path) -> String {
    let mut digest = Sha256::new();
    let head = std::process::Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "rev-parse",
            "--verify",
            "HEAD",
        ])
        .output();
    let Ok(head) = head else {
        return "workspace-unavailable".to_string();
    };
    if !head.status.success() {
        return "workspace-non-git".to_string();
    }
    digest.update(&head.stdout);

    let diff = std::process::Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "diff",
            "--no-ext-diff",
            "--binary",
            "HEAD",
            "--",
        ])
        .output();
    let Ok(diff) = diff else {
        return "workspace-unavailable".to_string();
    };
    if !diff.status.success() {
        return "workspace-unavailable".to_string();
    }
    digest.update(&diff.stdout);

    let untracked = std::process::Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output();
    let Ok(untracked) = untracked else {
        return "workspace-unavailable".to_string();
    };
    if !untracked.status.success() {
        return "workspace-unavailable".to_string();
    }
    for raw_path in untracked
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        digest.update(raw_path);
        let path = root.join(String::from_utf8_lossy(raw_path).as_ref());
        if let Ok(mut file) = std::fs::File::open(path) {
            let mut buffer = [0_u8; 8192];
            loop {
                match file.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => digest.update(&buffer[..read]),
                }
            }
        }
    }
    encode_hex(&digest.finalize())
}

/// Projects the resume boundaries a task offers, from rows already read.
fn project_boundaries(
    task_id: &str,
    inputs: &store::BoundaryInputs,
) -> Result<Vec<ResumeBoundary>> {
    let cycle = inputs.current_cycle;
    let state_version = task_state_version(&inputs.state_version)?;
    let plan: Value = serde_json::from_str(&inputs.execution_plan_json).unwrap_or_default();
    let steps = plan
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let failed_item = inputs.failed_item_id.clone();
    let latest_run = inputs.latest_run.clone();

    let mut boundaries = Vec::new();
    for step in steps {
        let Some(step_id) = step.get("id").and_then(Value::as_str) else {
            continue;
        };
        let side_effect_class = step
            .pointer("/behavior/side_effect_class")
            .and_then(Value::as_str)
            .and_then(parse_side_effect)
            .unwrap_or_else(|| infer_side_effect(&step));
        let item = failed_item.clone();
        let id = stable_boundary_id(task_id, cycle, Some(step_id), item.as_deref());
        boundaries.push(ResumeBoundary {
            id,
            task_id: task_id.to_string(),
            cycle,
            step_id: Some(step_id.to_string()),
            task_item_id: item,
            command_run_id: latest_run.as_ref().map(|run| run.0.clone()),
            provider_session_available: latest_run
                .as_ref()
                .and_then(|run| run.1.as_ref())
                .is_some(),
            checkpoint_id: None,
            side_effect_class,
            replay_safe: side_effect_class.replay_safe(),
            reason: format!("Restart orchestration at logical step {step_id}"),
            state_version: state_version.clone(),
        });
    }
    if boundaries.is_empty() {
        let side_effect_class = SideEffectClass::NonIdempotentExternal;
        boundaries.push(ResumeBoundary {
            id: stable_boundary_id(task_id, cycle, None, failed_item.as_deref()),
            task_id: task_id.to_string(),
            cycle,
            step_id: None,
            task_item_id: failed_item,
            command_run_id: latest_run.as_ref().map(|run| run.0.clone()),
            provider_session_available: latest_run
                .as_ref()
                .and_then(|run| run.1.as_ref())
                .is_some(),
            checkpoint_id: None,
            side_effect_class,
            replay_safe: false,
            reason: "Continue from the persisted task state; workflow side effects are undeclared"
                .to_string(),
            state_version,
        });
    }
    Ok(boundaries)
}

fn infer_side_effect(step: &Value) -> SideEffectClass {
    if step.get("command").and_then(Value::as_str).is_some()
        || step
            .get("required_capability")
            .and_then(Value::as_str)
            .is_some()
    {
        SideEffectClass::NonIdempotentExternal
    } else {
        SideEffectClass::None
    }
}

fn parse_side_effect(value: &str) -> Option<SideEffectClass> {
    match value {
        "none" => Some(SideEffectClass::None),
        "workspace_only" => Some(SideEffectClass::WorkspaceOnly),
        "idempotent_external" => Some(SideEffectClass::IdempotentExternal),
        "non_idempotent_external" => Some(SideEffectClass::NonIdempotentExternal),
        _ => None,
    }
}

fn stable_boundary_id(
    task_id: &str,
    cycle: i64,
    step_id: Option<&str>,
    item_id: Option<&str>,
) -> String {
    let raw = format!(
        "{task_id}:{cycle}:{}:{}",
        step_id.unwrap_or("current"),
        item_id.unwrap_or("task")
    );
    format!("rb-{}", encode_hex(&Sha256::digest(raw.as_bytes())))
}

fn side_effect_label(value: SideEffectClass) -> &'static str {
    match value {
        SideEffectClass::None => "none",
        SideEffectClass::WorkspaceOnly => "workspace_only",
        SideEffectClass::IdempotentExternal => "idempotent_external",
        SideEffectClass::NonIdempotentExternal => "non_idempotent_external",
    }
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut canonical = Map::new();
            for key in keys {
                if let Some(value) = map.get(key) {
                    canonical.insert(key.clone(), canonicalize(value));
                }
            }
            Value::Object(canonical)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

fn hash_value(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(&canonicalize(value))?;
    Ok(encode_hex(&Sha256::digest(bytes)))
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_schema;
    use orchestrator_persistence::test_support;
    // Inside the test module on purpose: the boundary scanner strips `cfg(test)`
    // blocks, and a file-scope import would count this fixture as production.
    use rusqlite::Connection;
    use tempfile::tempdir;

    async fn repository() -> (tempfile::TempDir, AsyncHandoffRepository) {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("handoff.db");
        init_schema(&path).expect("schema");
        let conn = Connection::open(&path).expect("open seed connection");
        conn.execute(
            "INSERT INTO tasks
             (id,name,status,goal,target_files_json,mode,workspace_id,workflow_id,project_id,
              workspace_root,qa_targets_json,ticket_dir,execution_plan_json,loop_mode,
              current_cycle,init_done,created_at,updated_at,pipeline_vars_json)
             VALUES ('task-1','task','failed','Fix the failing test','[]','once','ws','wf','p1',
                     '/tmp/ws','[]','docs/ticket',?1,'once',1,1,'2026-01-01','2026-01-01','{}')",
            [json!({"steps":[{"id":"test","builtin":"self_test","behavior":{"side_effect_class":"workspace_only"}}]}).to_string()],
        )
        .expect("seed task");
        conn.execute(
            "INSERT INTO events(task_id,event_type,payload_json,created_at)
             VALUES ('task-1','step_failed',?1,'2026-01-01T00:00:00Z')",
            [json!({"message":"tests failed","changed_files":["src/lib.rs"],"token":"must-redact"}).to_string()],
        )
        .expect("seed event");
        drop(conn);
        let db = Arc::new(AsyncDatabase::open(&path).await.expect("async db"));
        (temp, AsyncHandoffRepository::new(db))
    }

    #[tokio::test]
    async fn same_cursor_returns_same_content_hash_and_snapshot() {
        let (_temp, repository) = repository().await;
        let first = repository
            .generate_snapshot("task-1", Some(1), "operator")
            .await
            .expect("first");
        let second = repository
            .generate_snapshot("task-1", Some(1), "another-operator")
            .await
            .expect("second");
        assert_eq!(first.id, second.id);
        assert_eq!(first.content_hash, second.content_hash);
        assert_eq!(first.briefing.changed_files, vec!["src/lib.rs"]);
        assert!(
            !serde_json::to_string(&first)
                .expect("json")
                .contains("must-redact")
        );
    }

    #[tokio::test]
    async fn stale_plan_is_rejected_before_execution_reservation() {
        let (_temp, repository) = repository().await;
        let boundary = repository
            .list_boundaries("task-1")
            .await
            .expect("boundaries")
            .remove(0);
        let plan = repository
            .create_plan(
                "task-1",
                &boundary.id,
                ResumeMode::RestartFromBoundary,
                "operator",
                None,
            )
            .await
            .expect("plan");
        test_support::writer(&repository.db)
            .call(|conn| {
                conn.execute(
                    "UPDATE tasks SET current_cycle=current_cycle+1, updated_at='2026-01-02' WHERE id='task-1'",
                    [],
                )?;
                Ok(())
            })
            .await
            .expect("mutate task");
        let error = repository
            .reserve_execution(
                &plan.id,
                ResumeExecutionRequest {
                    expected_state_version: plan.expected_state_version.clone(),
                    idempotency_key: "key-1".to_string(),
                    actor: "operator".to_string(),
                    operator_reason: "retry after review".to_string(),
                    elevated_confirmation: false,
                    elevated_policy_enabled: false,
                },
            )
            .await
            .expect_err("stale plan must fail");
        assert!(error.to_string().contains("stale resume plan"));
    }

    #[tokio::test]
    async fn non_idempotent_boundary_requires_policy_and_confirmation() {
        let (_temp, repository) = repository().await;
        test_support::writer(&repository.db)
            .call(|conn| {
                conn.execute(
                    "UPDATE tasks SET execution_plan_json=?1 WHERE id='task-1'",
                    [
                        json!({"steps":[{"id":"deploy","required_capability":"deploy"}]})
                            .to_string(),
                    ],
                )?;
                Ok(())
            })
            .await
            .expect("change plan");
        let boundary = repository
            .list_boundaries("task-1")
            .await
            .expect("boundaries")
            .remove(0);
        assert!(!boundary.replay_safe);
        let plan = repository
            .create_plan(
                "task-1",
                &boundary.id,
                ResumeMode::RestartFromBoundary,
                "operator",
                None,
            )
            .await
            .expect("plan");
        let error = repository
            .reserve_execution(
                &plan.id,
                ResumeExecutionRequest {
                    expected_state_version: plan.expected_state_version.clone(),
                    idempotency_key: "key-2".to_string(),
                    actor: "operator".to_string(),
                    operator_reason: "deploy again".to_string(),
                    elevated_confirmation: true,
                    elevated_policy_enabled: false,
                },
            )
            .await
            .expect_err("policy disabled");
        assert!(error.to_string().contains("non-idempotent replay denied"));
    }

    #[tokio::test]
    async fn tracked_workspace_change_invalidates_reviewed_plan() {
        let (temp, repository) = repository().await;
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        std::fs::write(workspace.join("tracked.txt"), "before\n").expect("seed file");
        let git = |arguments: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(arguments)
                .status()
                .expect("run git")
        };
        assert!(git(&["init"]).success());
        assert!(git(&["add", "tracked.txt"]).success());
        assert!(
            git(&[
                "-c",
                "user.name=QA",
                "-c",
                "user.email=qa@example.invalid",
                "commit",
                "-m",
                "seed",
            ])
            .success()
        );
        let workspace_root = workspace.to_string_lossy().into_owned();
        test_support::writer(&repository.db)
            .call(move |conn| {
                conn.execute(
                    "UPDATE tasks SET workspace_root=?1 WHERE id='task-1'",
                    [workspace_root],
                )?;
                Ok(())
            })
            .await
            .expect("set workspace");
        let boundary = repository
            .list_boundaries("task-1")
            .await
            .expect("boundaries")
            .remove(0);
        let plan = repository
            .create_plan(
                "task-1",
                &boundary.id,
                ResumeMode::RestartFromBoundary,
                "operator",
                None,
            )
            .await
            .expect("plan");

        std::fs::write(workspace.join("tracked.txt"), "after\n").expect("change file");
        let error = repository
            .reserve_execution(
                &plan.id,
                ResumeExecutionRequest {
                    expected_state_version: plan.expected_state_version,
                    idempotency_key: "workspace-key".to_string(),
                    actor: "operator".to_string(),
                    operator_reason: "reviewed before workspace changed".to_string(),
                    elevated_confirmation: false,
                    elevated_policy_enabled: false,
                },
            )
            .await
            .expect_err("workspace drift must be stale");
        assert!(error.to_string().contains("stale resume plan"));
    }
}
