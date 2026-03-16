//! Trigger engine: cron scheduler and event-driven task creation.
//!
//! The `TriggerEngine` runs as a long-lived tokio task inside `orchestratord`,
//! watching for cron ticks and task-lifecycle events and creating tasks when
//! trigger conditions are met.

use crate::config::{TriggerConfig, TriggerCronConfig};
use crate::dto::CreateTaskPayload;
use crate::events::insert_event;
use crate::state::InnerState;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Cancels a running task for the Replace trigger policy.
///
/// This is a simplified version of `scheduler::stop_task_runtime` that avoids
/// a dependency on the scheduler crate.
async fn cancel_task_for_trigger(state: &InnerState, task_id: &str) -> Result<()> {
    // Signal the in-process running task to stop, if present.
    let runtime = {
        let running = state.running.lock().await;
        running.get(task_id).cloned()
    };
    if let Some(rt) = runtime {
        rt.stop_flag.store(true, Ordering::SeqCst);
    }
    // Update task status to cancelled.
    state
        .db_writer
        .set_task_status(task_id, "cancelled", false)
        .await?;
    insert_event(
        state,
        task_id,
        None,
        "task_control",
        serde_json::json!({"status": "cancelled"}),
    )
    .await?;
    Ok(())
}

// ── Public types ─────────────────────────────────────────────────────────────

/// Payload broadcast when a task_completed or task_failed event fires.
#[derive(Debug, Clone)]
pub struct TriggerEventPayload {
    /// Event type: "task_completed" or "task_failed".
    pub event_type: String,
    /// Source task ID.
    pub task_id: String,
}

/// Notification sent to the engine when trigger configuration changes.
#[derive(Debug)]
pub enum TriggerReloadEvent {
    /// Re-read all triggers from the current config snapshot.
    Reload,
}

/// Handle used by the rest of the system to communicate with a running engine.
#[derive(Clone)]
pub struct TriggerEngineHandle {
    reload_tx: mpsc::Sender<TriggerReloadEvent>,
}

impl TriggerEngineHandle {
    /// Notify the engine to reload trigger configuration (async).
    pub async fn reload(&self) {
        let _ = self.reload_tx.send(TriggerReloadEvent::Reload).await;
    }

    /// Notify the engine to reload trigger configuration (sync-safe).
    ///
    /// Uses `try_send` so it can be called from synchronous code paths
    /// like `apply_manifests`. Returns `true` if the notification was sent.
    pub fn reload_sync(&self) -> bool {
        self.reload_tx.try_send(TriggerReloadEvent::Reload).is_ok()
    }
}

/// The trigger engine itself. Constructed via [`TriggerEngine::new`] and
/// driven via [`TriggerEngine::run`].
pub struct TriggerEngine {
    state: Arc<InnerState>,
    reload_rx: mpsc::Receiver<TriggerReloadEvent>,
    trigger_event_rx: tokio::sync::broadcast::Receiver<TriggerEventPayload>,
    /// Triggers that have been present for at least one config reload cycle.
    /// A freshly-created trigger (from an agent `apply`) must survive one
    /// reload before it is eligible to fire — this prevents agent-applied
    /// triggers from immediately spawning parasitic tasks.
    stabilized_triggers: HashSet<(String, String)>,
}

impl TriggerEngine {
    /// Create a new engine and its control handle.
    pub fn new(state: Arc<InnerState>) -> (Self, TriggerEngineHandle) {
        let (reload_tx, reload_rx) = mpsc::channel(16);
        let trigger_event_rx = state.trigger_event_tx.subscribe();
        let engine = Self {
            state,
            reload_rx,
            trigger_event_rx,
            stabilized_triggers: HashSet::new(),
        };
        let handle = TriggerEngineHandle { reload_tx };
        (engine, handle)
    }

    /// Main run loop. Returns when `shutdown_rx` fires.
    pub async fn run(mut self, mut shutdown_rx: tokio::sync::watch::Receiver<bool>) {
        info!("trigger engine started");

        // Load initial trigger set.
        let mut cron_schedule = self.build_cron_schedule();

        loop {
            let sleep_duration = next_cron_sleep(&cron_schedule);
            let sleep_fut = tokio::time::sleep(sleep_duration);
            tokio::pin!(sleep_fut);

            tokio::select! {
                // ── Cron tick ───────────────────────────────────────────
                () = &mut sleep_fut => {
                    let now = Utc::now();
                    let fired = collect_due_triggers(&cron_schedule, now);
                    for (trigger_name, project) in fired {
                        self.fire_trigger(&trigger_name, &project).await;
                    }
                    // Recompute schedule after firing.
                    cron_schedule = self.build_cron_schedule();
                }

                // ── Event trigger ───────────────────────────────────────
                event_result = self.trigger_event_rx.recv() => {
                    match event_result {
                        Ok(payload) => {
                            self.handle_event_trigger(&payload).await;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!(skipped = n, "trigger event receiver lagged");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            debug!("trigger event channel closed");
                            break;
                        }
                    }
                }

                // ── Config reload ───────────────────────────────────────
                Some(_) = self.reload_rx.recv() => {
                    info!("trigger engine: reloading configuration");
                    cron_schedule = self.build_cron_schedule();
                }

                // ── Shutdown ────────────────────────────────────────────
                _ = shutdown_rx.changed() => {
                    info!("trigger engine shutting down");
                    break;
                }
            }
        }
    }

    // ── Cron helpers ─────────────────────────────────────────────────────────

    fn build_cron_schedule(&mut self) -> Vec<CronEntry> {
        let snap = self.state.config_runtime.load();
        let config = &snap.active_config.config;
        let mut entries = Vec::new();

        // Collect the current set of triggers to update stabilization tracking.
        let mut current_triggers: HashSet<(String, String)> = HashSet::new();
        for (project_id, project) in &config.projects {
            for name in project.triggers.keys() {
                current_triggers.insert((project_id.clone(), name.clone()));
            }
        }

        // Promote triggers that were already known from a prior cycle.
        // Triggers seen for the first time in THIS reload are NOT yet stabilized
        // and will only become eligible after the next reload.
        let previously_known = std::mem::take(&mut self.stabilized_triggers);
        self.stabilized_triggers = current_triggers.clone();

        for (project_id, project) in &config.projects {
            for (name, trigger) in &project.triggers {
                if trigger.suspend {
                    continue;
                }
                // Skip triggers that are new (not in the previous cycle's set).
                if !previously_known.contains(&(project_id.clone(), name.clone())) {
                    debug!(
                        trigger = name.as_str(),
                        project = project_id.as_str(),
                        "trigger not yet stabilized, skipping cron schedule"
                    );
                    continue;
                }
                if let Some(ref cron_spec) = trigger.cron {
                    match compute_next_fire(cron_spec, Utc::now()) {
                        Ok(next) => {
                            entries.push(CronEntry {
                                trigger_name: name.clone(),
                                project: project_id.clone(),
                                next_fire: next,
                            });
                        }
                        Err(e) => {
                            warn!(
                                trigger = name.as_str(),
                                project = project_id.as_str(),
                                error = %e,
                                "failed to compute next fire time"
                            );
                        }
                    }
                }
            }
        }
        entries
    }

    // ── Event trigger matching ───────────────────────────────────────────────

    async fn handle_event_trigger(&self, payload: &TriggerEventPayload) {
        // Resolve the source task's workflow from the database (the payload only
        // carries event_type + task_id to keep the broadcast lightweight).
        let source_workflow = self.lookup_task_workflow(&payload.task_id).await;

        let snap = self.state.config_runtime.load();
        let config = &snap.active_config.config;

        for (project_id, project) in &config.projects {
            for (name, trigger) in &project.triggers {
                if trigger.suspend {
                    continue;
                }
                // Skip triggers not yet stabilized (first seen in most recent reload).
                if !self
                    .stabilized_triggers
                    .contains(&(project_id.clone(), name.clone()))
                {
                    continue;
                }
                if let Some(ref event_spec) = trigger.event {
                    // Match event source type.
                    if event_spec.source != payload.event_type {
                        continue;
                    }
                    // Match optional workflow filter.
                    if let Some(ref filter) = event_spec.filter {
                        if let Some(ref filter_wf) = filter.workflow {
                            match source_workflow {
                                Some(ref sw) if sw == filter_wf => {}
                                _ => continue,
                            }
                        }
                        // CEL condition evaluation — future extension.
                        // For now, if a condition is set, log a warning and skip.
                        if filter.condition.is_some() {
                            debug!(
                                trigger = name.as_str(),
                                "CEL condition evaluation not yet implemented, skipping"
                            );
                            continue;
                        }
                    }

                    info!(
                        trigger = name.as_str(),
                        project = project_id.as_str(),
                        event_type = payload.event_type.as_str(),
                        source_task = payload.task_id.as_str(),
                        "event trigger matched"
                    );
                    self.fire_trigger_with_config(name, project_id, trigger)
                        .await;
                }
            }
        }
    }

    // ── Fire logic ───────────────────────────────────────────────────────────

    async fn fire_trigger(&self, trigger_name: &str, project: &str) {
        let snap = self.state.config_runtime.load();
        let config = &snap.active_config.config;

        let trigger = config
            .projects
            .get(project)
            .and_then(|p| p.triggers.get(trigger_name));

        let Some(trigger) = trigger else {
            warn!(trigger = trigger_name, "trigger not found in config");
            return;
        };

        self.fire_trigger_with_config(trigger_name, project, trigger)
            .await;
    }

    async fn fire_trigger_with_config(
        &self,
        trigger_name: &str,
        project: &str,
        trigger: &TriggerConfig,
    ) {
        // ── Suspend check ────────────────────────────────────────────────
        if trigger.suspend {
            self.emit_trigger_event(trigger_name, "trigger_skipped", "suspended");
            return;
        }

        // ── Throttle check (event triggers only) ─────────────────────────
        if let Some(ref throttle) = trigger.throttle {
            if throttle.min_interval > 0 {
                if let Some(last) = self.load_last_fired(trigger_name, project).await {
                    let elapsed = (Utc::now() - last).num_seconds();
                    if elapsed >= 0 && (elapsed as u64) < throttle.min_interval {
                        self.emit_trigger_event(trigger_name, "trigger_skipped", "throttled");
                        return;
                    }
                }
            }
        }

        // ── Concurrency policy ───────────────────────────────────────────
        match trigger.concurrency_policy {
            crate::cli_types::ConcurrencyPolicy::Forbid => {
                if self.has_active_task(trigger_name, project).await {
                    self.emit_trigger_event(
                        trigger_name,
                        "trigger_skipped",
                        "concurrent_task_active",
                    );
                    return;
                }
            }
            crate::cli_types::ConcurrencyPolicy::Replace => {
                // Cancel active tasks created by this trigger before creating a new one.
                self.cancel_active_tasks(trigger_name, project).await;
            }
            crate::cli_types::ConcurrencyPolicy::Allow => {}
        }

        // ── Create task ──────────────────────────────────────────────────
        let target_files = trigger
            .action
            .args
            .as_ref()
            .and_then(|a| a.get("target-file"))
            .cloned();

        let task_name = format!("trigger-{trigger_name}");

        let payload = CreateTaskPayload {
            name: Some(task_name),
            goal: Some(format!("Triggered by: {trigger_name}")),
            project_id: Some(project.to_string()),
            workspace_id: Some(trigger.action.workspace.clone()),
            workflow_id: Some(trigger.action.workflow.clone()),
            target_files,
            parent_task_id: None,
            spawn_reason: None,
        };

        match crate::task_ops::create_task_as_service(&self.state, payload) {
            Ok(summary) => {
                let task_id = summary.id.clone();
                info!(
                    trigger = trigger_name,
                    task_id = task_id.as_str(),
                    "trigger fired: task created"
                );

                // Update trigger state.
                self.update_trigger_state(trigger_name, project, &task_id, "created")
                    .await;

                // Emit event.
                self.state.emit_event(
                    &task_id,
                    None,
                    "trigger_fired",
                    serde_json::json!({
                        "trigger": trigger_name,
                        "source": if trigger.cron.is_some() { "cron" } else { "event" },
                        "task_id": task_id,
                    }),
                );

                // Start the task if action.start is true.
                if trigger.action.start {
                    let state = self.state.clone();
                    let tid = task_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            crate::scheduler_service::enqueue_task_as_service(&state, &tid).await
                        {
                            error!(task_id = tid.as_str(), error = %e, "failed to enqueue triggered task");
                        } else {
                            state.worker_notify.notify_one();
                        }
                    });
                }

                // History limit cleanup (best-effort, async).
                if trigger.history_limit.is_some() {
                    let state = self.state.clone();
                    let name = trigger_name.to_string();
                    let proj = project.to_string();
                    let limit = trigger.history_limit.clone();
                    tokio::spawn(async move {
                        if let Err(e) = cleanup_history(&state, &name, &proj, limit.as_ref()).await
                        {
                            debug!(trigger = name.as_str(), error = %e, "history cleanup failed");
                        }
                    });
                }
            }
            Err(e) => {
                error!(
                    trigger = trigger_name,
                    error = %e,
                    "trigger failed to create task"
                );
                self.update_trigger_state(trigger_name, project, "", "failed_to_create")
                    .await;
                self.state.emit_event(
                    "",
                    None,
                    "trigger_error",
                    serde_json::json!({
                        "trigger": trigger_name,
                        "error": e.to_string(),
                    }),
                );
            }
        }
    }

    // ── DB helpers ───────────────────────────────────────────────────────────

    /// Look up the workflow_id for a task from the database.
    async fn lookup_task_workflow(&self, task_id: &str) -> Option<String> {
        let tid = task_id.to_owned();
        let result = self
            .state
            .async_database
            .reader()
            .call(move |conn| {
                let wf: Option<String> = conn
                    .query_row(
                        "SELECT workflow_id FROM tasks WHERE id = ?1",
                        rusqlite::params![tid],
                        |row| row.get(0),
                    )
                    .ok();
                Ok(wf)
            })
            .await;
        match result {
            Ok(wf) => wf,
            Err(e) => {
                debug!(task_id, error = %e, "failed to look up task workflow");
                None
            }
        }
    }

    async fn load_last_fired(&self, trigger_name: &str, project: &str) -> Option<DateTime<Utc>> {
        let name = trigger_name.to_owned();
        let proj = project.to_owned();
        let result = self
            .state
            .async_database
            .reader()
            .call(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT last_fired_at FROM trigger_state WHERE trigger_name = ?1 AND project = ?2",
                    )
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
                let ts: Option<String> = stmt
                    .query_row(rusqlite::params![name, proj], |row| row.get(0))
                    .ok();
                Ok(ts)
            })
            .await;

        match result {
            Ok(Some(ts)) => ts.parse::<DateTime<Utc>>().ok(),
            _ => None,
        }
    }

    async fn has_active_task(&self, trigger_name: &str, project: &str) -> bool {
        let name = trigger_name.to_owned();
        let proj = project.to_owned();
        let result = self
            .state
            .async_database
            .reader()
            .call(move |conn| {
                let last_task_id: Option<String> = conn
                    .query_row(
                        "SELECT last_task_id FROM trigger_state WHERE trigger_name = ?1 AND project = ?2",
                        rusqlite::params![name, proj],
                        |row| row.get(0),
                    )
                    .ok()
                    .flatten();

                if let Some(ref tid) = last_task_id {
                    let status: Option<String> = conn
                        .query_row(
                            "SELECT status FROM tasks WHERE id = ?1",
                            rusqlite::params![tid],
                            |row| row.get(0),
                        )
                        .ok();
                    if let Some(s) = status {
                        return Ok(matches!(
                            s.as_str(),
                            "created" | "pending" | "running" | "restart_pending"
                        ));
                    }
                }
                Ok(false)
            })
            .await;

        result.unwrap_or(false)
    }

    async fn cancel_active_tasks(&self, trigger_name: &str, project: &str) {
        let name = trigger_name.to_owned();
        let proj = project.to_owned();
        let state = self.state.clone();
        let result = state
            .async_database
            .reader()
            .call(move |conn| {
                let tid: Option<String> = conn
                    .query_row(
                        "SELECT last_task_id FROM trigger_state WHERE trigger_name = ?1 AND project = ?2",
                        rusqlite::params![name, proj],
                        |row| row.get(0),
                    )
                    .ok()
                    .flatten();
                Ok(tid)
            })
            .await;

        if let Ok(Some(task_id)) = result {
            if let Err(e) = cancel_task_for_trigger(&self.state, &task_id).await {
                warn!(
                    trigger = trigger_name,
                    task_id = task_id.as_str(),
                    error = %e,
                    "failed to cancel active task for Replace policy"
                );
            }
        }
    }

    async fn update_trigger_state(
        &self,
        trigger_name: &str,
        project: &str,
        task_id: &str,
        status: &str,
    ) {
        let name = trigger_name.to_owned();
        let proj = project.to_owned();
        let tid = task_id.to_owned();
        let st = status.to_owned();
        let now = Utc::now().to_rfc3339();
        let now2 = now.clone();

        if let Err(e) = self
            .state
            .async_database
            .writer()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO trigger_state (trigger_name, project, last_fired_at, fire_count, last_task_id, last_status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7)
                     ON CONFLICT(trigger_name, project) DO UPDATE SET
                       last_fired_at = ?3,
                       fire_count = fire_count + 1,
                       last_task_id = ?4,
                       last_status = ?5,
                       updated_at = ?7",
                    rusqlite::params![name, proj, now, tid, st, now2, now2],
                )
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
                Ok(())
            })
            .await
        {
            warn!(trigger = trigger_name, error = %e, "failed to update trigger_state");
        }
    }

    fn emit_trigger_event(&self, trigger_name: &str, event_type: &str, reason: &str) {
        debug!(trigger = trigger_name, event_type, reason, "trigger event");
        self.state.emit_event(
            "",
            None,
            event_type,
            serde_json::json!({
                "trigger": trigger_name,
                "reason": reason,
            }),
        );
    }
}

// ── Cron schedule helpers ────────────────────────────────────────────────────

struct CronEntry {
    trigger_name: String,
    project: String,
    next_fire: DateTime<Utc>,
}

fn compute_next_fire(spec: &TriggerCronConfig, after: DateTime<Utc>) -> Result<DateTime<Utc>> {
    use cron::Schedule;
    use std::str::FromStr;

    let schedule = Schedule::from_str(&spec.schedule)
        .with_context(|| format!("invalid cron expression: {}", spec.schedule))?;

    // If a timezone is specified, compute in that timezone, then convert back to UTC.
    if let Some(ref tz_name) = spec.timezone {
        let tz: chrono_tz::Tz = tz_name
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid timezone: {tz_name}"))?;
        let local_after = after.with_timezone(&tz);
        let next = schedule
            .after(&local_after)
            .next()
            .ok_or_else(|| anyhow::anyhow!("no next fire time for schedule"))?;
        Ok(next.with_timezone(&Utc))
    } else {
        let next = schedule
            .after(&after)
            .next()
            .ok_or_else(|| anyhow::anyhow!("no next fire time for schedule"))?;
        Ok(next.with_timezone(&Utc))
    }
}

fn next_cron_sleep(entries: &[CronEntry]) -> std::time::Duration {
    let now = Utc::now();
    entries
        .iter()
        .map(|e| {
            let diff = e.next_fire.signed_duration_since(now);
            if diff.num_milliseconds() <= 0 {
                std::time::Duration::from_millis(100)
            } else {
                std::time::Duration::from_millis(diff.num_milliseconds() as u64)
            }
        })
        .min()
        // If no cron triggers, sleep for a long time (until event or reload wakes us).
        .unwrap_or(std::time::Duration::from_secs(3600))
}

fn collect_due_triggers(entries: &[CronEntry], now: DateTime<Utc>) -> Vec<(String, String)> {
    entries
        .iter()
        .filter(|e| e.next_fire <= now)
        .map(|e| (e.trigger_name.clone(), e.project.clone()))
        .collect()
}

async fn cleanup_history(
    state: &InnerState,
    trigger_name: &str,
    project: &str,
    limit: Option<&crate::config::TriggerHistoryLimitConfig>,
) -> Result<()> {
    let limit = match limit {
        Some(l) => l,
        None => return Ok(()),
    };

    let task_name_pattern = format!("trigger-{trigger_name}");
    let proj = project.to_owned();

    // For each status category, collect IDs of tasks beyond the retention limit.
    let mut ids_to_delete: Vec<String> = Vec::new();

    if let Some(max_successful) = limit.successful {
        let pattern = task_name_pattern.clone();
        let p = proj.clone();
        let max = max_successful as usize;
        let ids = state
            .async_database
            .reader()
            .call(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT id FROM tasks \
                         WHERE name = ?1 AND project_id = ?2 AND status = 'completed' \
                         ORDER BY created_at DESC",
                    )
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
                let rows = stmt
                    .query_map(rusqlite::params![pattern, p], |row| row.get::<_, String>(0))
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
                let all: Vec<String> = rows.filter_map(|r| r.ok()).collect();
                Ok(all.into_iter().skip(max).collect::<Vec<String>>())
            })
            .await
            .context("query completed tasks for history cleanup")?;
        ids_to_delete.extend(ids);
    }

    if let Some(max_failed) = limit.failed {
        let pattern = task_name_pattern.clone();
        let p = proj.clone();
        let max = max_failed as usize;
        let ids = state
            .async_database
            .reader()
            .call(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT id FROM tasks \
                         WHERE name = ?1 AND project_id = ?2 AND status = 'failed' \
                         ORDER BY created_at DESC",
                    )
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
                let rows = stmt
                    .query_map(rusqlite::params![pattern, p], |row| row.get::<_, String>(0))
                    .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
                let all: Vec<String> = rows.filter_map(|r| r.ok()).collect();
                Ok(all.into_iter().skip(max).collect::<Vec<String>>())
            })
            .await
            .context("query failed tasks for history cleanup")?;
        ids_to_delete.extend(ids);
    }

    if ids_to_delete.is_empty() {
        return Ok(());
    }

    state
        .async_database
        .writer()
        .call(move |conn| {
            let placeholders: Vec<String> =
                (1..=ids_to_delete.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "DELETE FROM tasks WHERE id IN ({})",
                placeholders.join(", ")
            );
            let params: Vec<Box<dyn rusqlite::types::ToSql>> = ids_to_delete
                .iter()
                .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::types::ToSql>)
                .collect();
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            conn.execute(&sql, param_refs.as_slice())
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))?;
            Ok(())
        })
        .await
        .context("delete excess trigger history tasks")?;

    Ok(())
}

// ── Public helper for event broadcasting ─────────────────────────────────────

/// Broadcast a trigger-relevant event (task_completed / task_failed).
/// Called from the daemon's event handling path.
pub fn broadcast_task_event(state: &InnerState, payload: TriggerEventPayload) {
    // Ignore send errors (no subscribers = no triggers configured).
    let _ = state.trigger_event_tx.send(payload);
}

/// Notify the trigger engine to reload its configuration.
/// Safe to call from sync code. No-op if no engine is running.
pub fn notify_trigger_reload(state: &InnerState) {
    if let Ok(guard) = state.trigger_engine_handle.lock() {
        if let Some(ref handle) = *guard {
            let _ = handle.reload_sync();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn compute_next_fire_utc() {
        let spec = TriggerCronConfig {
            schedule: "0 0 2 * * *".to_string(), // daily at 02:00 (cron crate uses 6 fields)
            timezone: None,
        };
        let after = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();
        let next = compute_next_fire(&spec, after).expect("should compute");
        assert!(next > after);
        assert_eq!(next.hour(), 2);
    }

    #[test]
    fn compute_next_fire_with_timezone() {
        let spec = TriggerCronConfig {
            schedule: "0 0 2 * * *".to_string(),
            timezone: Some("Asia/Shanghai".to_string()),
        };
        let after = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();
        let next = compute_next_fire(&spec, after).expect("should compute with tz");
        assert!(next > after);
        // 02:00 Shanghai = 18:00 UTC previous day
        assert_eq!(next.hour(), 18);
    }

    #[test]
    fn compute_next_fire_rejects_invalid_schedule() {
        let spec = TriggerCronConfig {
            schedule: "not a cron".to_string(),
            timezone: None,
        };
        assert!(compute_next_fire(&spec, Utc::now()).is_err());
    }

    #[test]
    fn compute_next_fire_rejects_invalid_timezone() {
        let spec = TriggerCronConfig {
            schedule: "0 0 2 * * *".to_string(),
            timezone: Some("Invalid/TZ".to_string()),
        };
        assert!(compute_next_fire(&spec, Utc::now()).is_err());
    }

    #[test]
    fn next_cron_sleep_empty_returns_1h() {
        let d = next_cron_sleep(&[]);
        assert_eq!(d, std::time::Duration::from_secs(3600));
    }

    #[test]
    fn collect_due_triggers_finds_past_entries() {
        let now = Utc::now();
        let past = now - chrono::Duration::seconds(10);
        let future = now + chrono::Duration::seconds(300);
        let entries = vec![
            CronEntry {
                trigger_name: "past".to_string(),
                project: "p".to_string(),
                next_fire: past,
            },
            CronEntry {
                trigger_name: "future".to_string(),
                project: "p".to_string(),
                next_fire: future,
            },
        ];
        let due = collect_due_triggers(&entries, now);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, "past");
    }
}
