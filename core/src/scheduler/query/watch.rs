//! Real-time task monitoring (watch command).

use crate::anomaly::AnomalyRule;
use crate::dto::TaskSummary;
use crate::state::InnerState;
use anyhow::Result;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::format::{colorize_status, format_bytes, format_duration};
use super::is_transient_query_error;
use super::task_queries::load_task_summary;
use crate::events::{
    observed_step_scope_label, query_step_events_async, ObservedStepScope, StepEvent,
};

/// Watch a task in real-time, updating the display at the specified interval.
///
/// When `timeout_secs` is `Some(n)` and `n > 0`, the watch loop exits after
/// `n` seconds with a final status snapshot printed to stderr.
pub async fn watch_task(
    state: &InnerState,
    task_id: &str,
    interval_secs: u64,
    timeout_secs: Option<u64>,
) -> Result<()> {
    let interval = Duration::from_secs(interval_secs);
    let deadline = timeout_secs
        .filter(|&t| t > 0)
        .map(|t| Instant::now() + Duration::from_secs(t));
    let mut last_warning: Option<String> = None;

    loop {
        if let Some(dl) = deadline {
            if Instant::now() >= dl {
                eprintln!("watch: timeout after {}s", timeout_secs.unwrap_or(0));
                return Ok(());
            }
        }
        let task = match load_task_summary(state, task_id).await {
            Ok(task) => task,
            Err(err) if is_transient_query_error(&err) => {
                let rule = AnomalyRule::TransientReadError;
                let warning = format!(
                    "[{}: {}] {err}",
                    rule.escalation().label(),
                    rule.canonical_name(),
                );
                if last_warning.as_deref() != Some(&warning) {
                    eprintln!("{warning}");
                    last_warning = Some(warning);
                }
                tokio::time::sleep(interval).await;
                continue;
            }
            Err(err) => return Err(err),
        };
        let events = match query_step_events_async(state, task_id).await {
            Ok(events) => events,
            Err(err) if is_transient_query_error(&err) => {
                let rule = AnomalyRule::TransientReadError;
                let warning = format!(
                    "[{}: {}] {err}",
                    rule.escalation().label(),
                    rule.canonical_name(),
                );
                if last_warning.as_deref() != Some(&warning) {
                    eprintln!("{warning}");
                    last_warning = Some(warning);
                }
                tokio::time::sleep(interval).await;
                continue;
            }
            Err(err) => return Err(err),
        };

        let frame = render_watch_frame(&task, &events, task_id);
        print!("\x1b[2J\x1b[H{frame}");
        last_warning = None;

        if task.status == "completed" || task.status == "failed" {
            return Ok(());
        }

        tokio::time::sleep(interval).await;
    }
}

/// Internal struct for tracking step state during watch rendering.
struct StepWatchInfo {
    step: String,
    scope: Option<ObservedStepScope>,
    binding_item_id: Option<String>,
    agent_id: String,
    status: String,
    duration_ms: Option<u64>,
    details: String,
    started_at: Option<String>,
}

#[derive(Default)]
struct WatchAnomalyCounts {
    intervene: u32,
    attention: u32,
    notice: u32,
}

impl WatchAnomalyCounts {
    fn total(&self) -> u32 {
        self.intervene + self.attention + self.notice
    }
}

fn render_watch_frame(task: &TaskSummary, events: &[StepEvent], task_id: &str) -> String {
    let mut frame = String::new();
    let _ = writeln!(
        frame,
        "Task: {}  Status: {}  Workflow: {}",
        &task_id[..8.min(task_id.len())],
        colorize_status(&task.status),
        &task.workflow_id,
    );

    let cycle_count = events
        .iter()
        .filter(|e| e.event_type == "cycle_started")
        .count();
    let _ = writeln!(frame, "Cycle: {}  Tickets: {}", cycle_count, 0);
    let _ = writeln!(frame, "{}", "━".repeat(72));
    let _ = writeln!(
        frame,
        " {:<15} {:<7} {:<12} {:<10} {:<9} Details",
        "Step", "Scope", "Agent", "Status", "Duration"
    );
    let _ = writeln!(
        frame,
        " {:<15} {:<7} {:<12} {:<10} {:<9} ─────────────",
        "───────────────", "───────", "────────────", "──────────", "─────────"
    );

    let mut step_states: Vec<StepWatchInfo> = Vec::new();
    let mut watch_anomaly_counts = WatchAnomalyCounts::default();
    for ev in events {
        match ev.event_type.as_str() {
            "step_started" => {
                let step = ev.step.clone().unwrap_or_default();
                let agent = ev.agent_id.clone().unwrap_or_default();
                if let Some(existing) = step_states.iter_mut().find(|s| s.step == step) {
                    existing.scope = ev.step_scope;
                    existing.binding_item_id = ev.task_item_id.clone();
                    existing.status = "running".to_string();
                    existing.agent_id = agent;
                    existing.started_at = Some(ev.created_at.clone());
                } else {
                    step_states.push(StepWatchInfo {
                        step,
                        scope: ev.step_scope,
                        binding_item_id: ev.task_item_id.clone(),
                        agent_id: agent,
                        status: "running".to_string(),
                        duration_ms: None,
                        details: String::new(),
                        started_at: Some(ev.created_at.clone()),
                    });
                }
            }
            "step_finished" => {
                let step = ev.step.clone().unwrap_or_default();
                if let Some(existing) = step_states.iter_mut().find(|s| s.step == step) {
                    if ev.step_scope.is_some() {
                        existing.scope = ev.step_scope;
                    }
                    if ev.task_item_id.is_some() {
                        existing.binding_item_id = ev.task_item_id.clone();
                    }
                    let success = ev.success.unwrap_or(false);
                    existing.status = if success {
                        "done".to_string()
                    } else {
                        "failed".to_string()
                    };
                    existing.duration_ms = ev.duration_ms;
                    existing.agent_id = ev.agent_id.clone().unwrap_or(existing.agent_id.clone());
                    if let Some(conf) = ev.confidence {
                        existing.details = format!("conf={:.2}", conf);
                    }
                }
            }
            "step_skipped" => {
                step_states.push(StepWatchInfo {
                    step: ev.step.clone().unwrap_or_default(),
                    scope: ev.step_scope,
                    binding_item_id: ev.task_item_id.clone(),
                    agent_id: String::new(),
                    status: "skipped".to_string(),
                    duration_ms: None,
                    details: ev.reason.clone().unwrap_or_default(),
                    started_at: None,
                });
            }
            "step_heartbeat" => {
                let step = ev.step.clone().unwrap_or_default();
                if let Some(existing) = step_states
                    .iter_mut()
                    .find(|s| s.step == step && s.status == "running")
                {
                    if ev.step_scope.is_some() {
                        existing.scope = ev.step_scope;
                    }
                    if ev.task_item_id.is_some() {
                        existing.binding_item_id = ev.task_item_id.clone();
                    }
                    let stdout_b = ev.stdout_bytes.unwrap_or(0);
                    let stderr_b = ev.stderr_bytes.unwrap_or(0);
                    let stdout_delta_b = ev.stdout_delta_bytes.unwrap_or(0);
                    let stderr_delta_b = ev.stderr_delta_bytes.unwrap_or(0);
                    let total_delta = stdout_delta_b + stderr_delta_b;
                    let pid = ev.pid.unwrap_or(0);
                    let alive = ev.pid_alive.unwrap_or(false);
                    let elapsed = ev.elapsed_secs.unwrap_or(0);

                    let lo_rule = AnomalyRule::LowOutput;
                    let lr_rule = AnomalyRule::LongRunning;

                    existing.details = match ev.output_state.as_deref() {
                        Some("low_output") => {
                            watch_anomaly_counts.intervene += 1;
                            format!(
                                "{} [{}] pid={} {} out={} err={} Δ={} quiet={}",
                                lo_rule.display_tag(),
                                lo_rule.escalation().label(),
                                pid,
                                if alive { "alive" } else { "DEAD" },
                                format_bytes(stdout_b),
                                format_bytes(stderr_b),
                                format_bytes(total_delta),
                                ev.stagnant_heartbeats.unwrap_or(0)
                            )
                        }
                        Some(state) => format!(
                            "pid={} {} out={} err={} Δ={} state={}",
                            pid,
                            if alive { "alive" } else { "DEAD" },
                            format_bytes(stdout_b),
                            format_bytes(stderr_b),
                            format_bytes(total_delta),
                            state
                        ),
                        None => format!(
                            "pid={} {} stdout={}",
                            pid,
                            if alive { "alive" } else { "DEAD" },
                            format_bytes(stdout_b)
                        ),
                    };

                    if elapsed > 600 && ev.output_state.as_deref() != Some("low_output") {
                        watch_anomaly_counts.notice += 1;
                        existing.details.push_str(&format!(
                            " {} [{}]",
                            lr_rule.display_tag(),
                            lr_rule.escalation().label(),
                        ));
                    }

                    if existing.scope == Some(ObservedStepScope::Task) {
                        if let Some(anchor_item_id) = &existing.binding_item_id {
                            existing
                                .details
                                .push_str(&format!(" anchor={anchor_item_id}"));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    for s in &step_states {
        let duration_str = match s.duration_ms {
            Some(ms) => format_duration(ms),
            None if s.status == "running" => {
                if let Some(ref ts) = s.started_at {
                    format!("{}...", ts.chars().skip(11).take(8).collect::<String>())
                } else {
                    "-".to_string()
                }
            }
            _ => "-".to_string(),
        };
        let status_icon = match s.status.as_str() {
            "done" => "\x1b[32m✓ done\x1b[0m",
            "failed" => "\x1b[31m✗ fail\x1b[0m",
            "running" => "\x1b[33m● run\x1b[0m",
            "skipped" => "\x1b[90m○ skip\x1b[0m",
            _ => &s.status,
        };
        let _ = writeln!(
            frame,
            " {:<15} {:<7} {:<12} {:<18} {:<9} {}",
            s.step,
            match s.scope {
                Some(scope) => observed_step_scope_label(Some(scope)),
                None => "~",
            },
            if s.agent_id.is_empty() {
                "-"
            } else {
                &s.agent_id
            },
            status_icon,
            duration_str,
            s.details
        );
    }

    let _ = writeln!(frame);
    if watch_anomaly_counts.total() > 0 {
        let _ = writeln!(
            frame,
            "Anomalies: {} intervene, {} attention, {} notice",
            watch_anomaly_counts.intervene,
            watch_anomaly_counts.attention,
            watch_anomaly_counts.notice,
        );
    }
    if task.status == "completed" || task.status == "failed" {
        let _ = writeln!(frame, "Task finished: {}", colorize_status(&task.status));
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_watch_frame_includes_running_step_and_cycle() {
        let task = TaskSummary {
            id: "12345678-1234-1234-1234-123456789abc".to_string(),
            name: "watch".to_string(),
            status: "running".to_string(),
            started_at: Some("2026-03-01T00:00:00Z".to_string()),
            completed_at: None,
            goal: "observe".to_string(),
            project_id: "default".to_string(),
            workspace_id: "default".to_string(),
            workflow_id: "basic".to_string(),
            target_files: vec![],
            total_items: 1,
            finished_items: 0,
            failed_items: 0,
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-01T00:00:01Z".to_string(),
            parent_task_id: None,
            spawn_reason: None,
            spawn_depth: 0,
        };
        let events = vec![
            StepEvent {
                event_type: "cycle_started".to_string(),
                step: None,
                step_scope: None,
                task_item_id: None,
                agent_id: None,
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: None,
                stdout_bytes: None,
                stderr_bytes: None,
                stdout_delta_bytes: None,
                stderr_delta_bytes: None,
                stagnant_heartbeats: None,
                pid: None,
                pid_alive: None,
                output_state: None,
                created_at: "2026-03-01T00:00:00Z".to_string(),
            },
            StepEvent {
                event_type: "step_started".to_string(),
                step: Some("plan".to_string()),
                step_scope: Some(ObservedStepScope::Task),
                task_item_id: Some("item-1".to_string()),
                agent_id: Some("echo".to_string()),
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: None,
                stdout_bytes: None,
                stderr_bytes: None,
                stdout_delta_bytes: None,
                stderr_delta_bytes: None,
                stagnant_heartbeats: None,
                pid: None,
                pid_alive: None,
                output_state: None,
                created_at: "2026-03-01T00:00:01Z".to_string(),
            },
        ];

        let frame = render_watch_frame(&task, &events, &task.id);
        assert!(frame.contains("Task: 12345678"));
        assert!(frame.contains("Cycle: 1"));
        assert!(frame.contains("Scope"));
        assert!(frame.contains("plan"));
        assert!(frame.contains(" task "));
        assert!(frame.contains("echo"));
    }

    #[test]
    fn render_watch_frame_shows_low_output_details_for_heartbeat() {
        let task = TaskSummary {
            id: "12345678-1234-1234-1234-123456789abc".to_string(),
            name: "watch".to_string(),
            status: "running".to_string(),
            started_at: Some("2026-03-01T00:00:00Z".to_string()),
            completed_at: None,
            goal: "observe".to_string(),
            project_id: "default".to_string(),
            workspace_id: "default".to_string(),
            workflow_id: "basic".to_string(),
            target_files: vec![],
            total_items: 1,
            finished_items: 0,
            failed_items: 0,
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-01T00:01:31Z".to_string(),
            parent_task_id: None,
            spawn_reason: None,
            spawn_depth: 0,
        };
        let events = vec![
            StepEvent {
                event_type: "step_started".to_string(),
                step: Some("plan".to_string()),
                step_scope: Some(ObservedStepScope::Task),
                task_item_id: Some("item-1".to_string()),
                agent_id: Some("echo".to_string()),
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: None,
                stdout_bytes: None,
                stderr_bytes: None,
                stdout_delta_bytes: None,
                stderr_delta_bytes: None,
                stagnant_heartbeats: None,
                pid: None,
                pid_alive: None,
                output_state: None,
                created_at: "2026-03-01T00:00:01Z".to_string(),
            },
            StepEvent {
                event_type: "step_heartbeat".to_string(),
                step: Some("plan".to_string()),
                step_scope: Some(ObservedStepScope::Task),
                task_item_id: Some("item-1".to_string()),
                agent_id: None,
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: Some(90),
                stdout_bytes: Some(137),
                stderr_bytes: Some(0),
                stdout_delta_bytes: Some(0),
                stderr_delta_bytes: Some(0),
                stagnant_heartbeats: Some(3),
                pid: Some(4321),
                pid_alive: Some(true),
                output_state: Some("low_output".to_string()),
                created_at: "2026-03-01T00:01:31Z".to_string(),
            },
        ];

        let frame = render_watch_frame(&task, &events, &task.id);
        assert!(
            frame.contains("LOW_OUTPUT"),
            "should contain LOW_OUTPUT tag"
        );
        assert!(
            frame.contains("[INTERVENE]"),
            "should contain escalation tag"
        );
        assert!(frame.contains("Δ=0B"));
        assert!(frame.contains("quiet=3"));
        assert!(frame.contains("anchor=item-1"));
        assert!(
            frame.contains("Anomalies: 1 intervene"),
            "should show anomaly summary"
        );
    }

    #[test]
    fn render_watch_frame_keeps_active_state_for_active_heartbeat() {
        let task = TaskSummary {
            id: "12345678-1234-1234-1234-123456789abc".to_string(),
            name: "watch".to_string(),
            status: "running".to_string(),
            started_at: Some("2026-03-01T00:00:00Z".to_string()),
            completed_at: None,
            goal: "observe".to_string(),
            project_id: "default".to_string(),
            workspace_id: "default".to_string(),
            workflow_id: "basic".to_string(),
            target_files: vec![],
            total_items: 1,
            finished_items: 0,
            failed_items: 0,
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-01T00:00:31Z".to_string(),
            parent_task_id: None,
            spawn_reason: None,
            spawn_depth: 0,
        };
        let events = vec![
            StepEvent {
                event_type: "step_started".to_string(),
                step: Some("plan".to_string()),
                step_scope: Some(ObservedStepScope::Item),
                task_item_id: Some("item-1".to_string()),
                agent_id: Some("echo".to_string()),
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: None,
                stdout_bytes: None,
                stderr_bytes: None,
                stdout_delta_bytes: None,
                stderr_delta_bytes: None,
                stagnant_heartbeats: None,
                pid: None,
                pid_alive: None,
                output_state: None,
                created_at: "2026-03-01T00:00:01Z".to_string(),
            },
            StepEvent {
                event_type: "step_heartbeat".to_string(),
                step: Some("plan".to_string()),
                step_scope: Some(ObservedStepScope::Item),
                task_item_id: Some("item-1".to_string()),
                agent_id: None,
                success: None,
                duration_ms: None,
                confidence: None,
                reason: None,
                elapsed_secs: Some(30),
                stdout_bytes: Some(256),
                stderr_bytes: Some(0),
                stdout_delta_bytes: Some(64),
                stderr_delta_bytes: Some(0),
                stagnant_heartbeats: Some(0),
                pid: Some(4321),
                pid_alive: Some(true),
                output_state: Some("active".to_string()),
                created_at: "2026-03-01T00:00:31Z".to_string(),
            },
        ];

        let frame = render_watch_frame(&task, &events, &task.id);
        assert!(frame.contains(" item "));
        assert!(frame.contains("state=active"));
        assert!(!frame.contains("LOW OUTPUT"));
    }

    #[test]
    fn render_watch_frame_shows_unspecified_scope_marker_for_missing_scope_event() {
        let task = TaskSummary {
            id: "12345678-1234-1234-1234-123456789abc".to_string(),
            name: "watch".to_string(),
            status: "running".to_string(),
            started_at: Some("2026-03-01T00:00:00Z".to_string()),
            completed_at: None,
            goal: "observe".to_string(),
            project_id: "default".to_string(),
            workspace_id: "default".to_string(),
            workflow_id: "basic".to_string(),
            target_files: vec![],
            total_items: 1,
            finished_items: 0,
            failed_items: 0,
            created_at: "2026-03-01T00:00:00Z".to_string(),
            updated_at: "2026-03-01T00:00:01Z".to_string(),
            parent_task_id: None,
            spawn_reason: None,
            spawn_depth: 0,
        };
        let events = vec![StepEvent {
            event_type: "step_started".to_string(),
            step: Some("plan".to_string()),
            step_scope: None,
            task_item_id: Some("item-1".to_string()),
            agent_id: Some("echo".to_string()),
            success: None,
            duration_ms: None,
            confidence: None,
            reason: None,
            elapsed_secs: None,
            stdout_bytes: None,
            stderr_bytes: None,
            stdout_delta_bytes: None,
            stderr_delta_bytes: None,
            stagnant_heartbeats: None,
            pid: None,
            pid_alive: None,
            output_state: None,
            created_at: "2026-03-01T00:00:01Z".to_string(),
        }];

        let frame = render_watch_frame(&task, &events, &task.id);
        assert!(
            frame.contains(" ~ "),
            "unspecified scope should display as ~"
        );
    }
}
