use crate::anomaly::Severity;
use crate::dto::{CommandRunDto, EventDto};
use crate::events::{
    observed_step_scope_from_payload, observed_step_scope_label, ObservedStepScope,
};
use std::collections::HashMap;

use super::anomaly::*;
use super::model::*;
use super::time::compute_wall_time;

fn get_build_version() -> Option<BuildVersion> {
    Some(BuildVersion {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_hash: env!("BUILD_GIT_HASH").to_string(),
        build_timestamp: env!("BUILD_TIMESTAMP").to_string(),
    })
}

pub(super) fn split_observed_item_binding(
    scope: Option<ObservedStepScope>,
    task_item_id: &Option<String>,
) -> (String, Option<String>, Option<String>) {
    match scope {
        Some(ObservedStepScope::Item) => (
            observed_step_scope_label(scope).to_string(),
            task_item_id.clone(),
            None,
        ),
        Some(ObservedStepScope::Task) => (
            observed_step_scope_label(scope).to_string(),
            None,
            task_item_id.clone(),
        ),
        None => ("unspecified".to_string(), None, task_item_id.clone()),
    }
}

pub fn build_trace(
    task_id: &str,
    status: &str,
    events: &[EventDto],
    command_runs: &[CommandRunDto],
) -> TaskTrace {
    let first_event_at = events.first().map(|e| e.created_at.as_str()).unwrap_or("");
    let last_event_at = events
        .last()
        .map(|e| e.created_at.as_str())
        .unwrap_or(first_event_at);
    build_trace_with_meta(
        TraceTaskMeta {
            task_id,
            status,
            created_at: first_event_at,
            started_at: None,
            completed_at: None,
            updated_at: last_event_at,
        },
        events,
        command_runs,
    )
}

pub fn build_trace_with_meta(
    task_meta: TraceTaskMeta<'_>,
    events: &[EventDto],
    command_runs: &[CommandRunDto],
) -> TaskTrace {
    let mut sorted_events: Vec<&EventDto> = events.iter().collect();
    sorted_events.sort_by_key(|e| e.id);
    let sorted_refs: Vec<EventDto> = sorted_events
        .into_iter()
        .map(|e| EventDto {
            id: e.id,
            task_id: e.task_id.clone(),
            task_item_id: e.task_item_id.clone(),
            event_type: e.event_type.clone(),
            payload: e.payload.clone(),
            created_at: e.created_at.clone(),
        })
        .collect();
    let events = &sorted_refs;

    let cycles = build_cycles(&task_meta, events, command_runs);
    let graph_runs = build_graph_runs(events);
    let mut anomalies = Vec::new();

    detect_duplicate_runner(events, &mut anomalies);
    detect_overlapping_cycles(&cycles, &mut anomalies);
    detect_overlapping_steps(events, &mut anomalies);
    detect_missing_step_end(events, &mut anomalies);
    detect_empty_cycles(events, &mut anomalies);
    detect_orphan_commands(events, command_runs, &mut anomalies);
    detect_nonzero_exit(command_runs, &mut anomalies);
    detect_unexpanded_template_var(command_runs, &mut anomalies);
    detect_long_running_steps(&cycles, &mut anomalies);
    detect_low_output_steps(events, &mut anomalies);

    let total_steps: u32 = cycles.iter().map(|c| c.steps.len() as u32).sum();
    let total_commands = command_runs.len() as u32;
    let failed_commands = command_runs
        .iter()
        .filter(|r| r.exit_code.is_some_and(|c| c != 0 && c != -1))
        .count() as u32;

    let mut anomaly_counts = HashMap::new();
    for a in &anomalies {
        let key = match a.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        *anomaly_counts.entry(key.to_string()).or_insert(0) += 1;
    }

    let wall_time_secs = compute_wall_time(&task_meta, events);

    let summary = TraceSummary {
        total_cycles: cycles.len() as u32,
        total_steps,
        total_commands,
        failed_commands,
        anomaly_counts,
        wall_time_secs,
    };

    TaskTrace {
        task_id: task_meta.task_id.to_string(),
        status: task_meta.status.to_string(),
        cycles,
        graph_runs,
        anomalies,
        summary,
        build_version: get_build_version(),
    }
}

fn build_graph_runs(events: &[EventDto]) -> Vec<GraphTrace> {
    let mut by_cycle: HashMap<u32, GraphTrace> = HashMap::new();
    for event in events {
        if !event.event_type.starts_with("dynamic_") {
            continue;
        }
        let cycle = event
            .payload
            .get("cycle")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32;
        let entry = by_cycle.entry(cycle).or_insert_with(|| GraphTrace {
            cycle,
            source: None,
            node_count: 0,
            edge_count: 0,
            events: Vec::new(),
        });
        if event.event_type == "dynamic_plan_materialized" {
            entry.source = event
                .payload
                .get("source")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string());
            entry.node_count = event
                .payload
                .get("node_count")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32;
            entry.edge_count = event
                .payload
                .get("edge_count")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32;
        }
        entry.events.push(GraphEventTrace {
            event_type: event.event_type.clone(),
            node_id: event
                .payload
                .get("node_id")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            from: event
                .payload
                .get("from")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            to: event
                .payload
                .get("to")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            taken: event.payload.get("taken").and_then(|value| value.as_bool()),
            created_at: event.created_at.clone(),
        });
    }
    let mut runs: Vec<GraphTrace> = by_cycle.into_values().collect();
    runs.sort_by_key(|run| run.cycle);
    runs
}

// ── Timeline reconstruction ──────────────────────────────────────────

fn build_cycles(
    task_meta: &TraceTaskMeta<'_>,
    events: &[EventDto],
    command_runs: &[CommandRunDto],
) -> Vec<CycleTrace> {
    let mut cycles: Vec<CycleBuilder> = Vec::new();
    let mut current_cycle_idx: Option<usize> = None;

    // Index command_runs by (item_id, phase) for lookup
    let mut runs_by_item_phase: HashMap<(String, String), Vec<&CommandRunDto>> = HashMap::new();
    for run in command_runs {
        runs_by_item_phase
            .entry((run.task_item_id.clone(), run.phase.clone()))
            .or_default()
            .push(run);
    }

    for event in events {
        match event.event_type.as_str() {
            "cycle_started" => {
                if let Some(idx) = current_cycle_idx {
                    let ended_at = cycles[idx]
                        .last_seen_at
                        .clone()
                        .or_else(|| cycles[idx].started_at.clone())
                        .or_else(|| Some(event.created_at.clone()));
                    close_cycle_at(&mut cycles[idx], ended_at);
                }

                let cycle_num = event
                    .payload
                    .get("cycle")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                cycles.push(CycleBuilder {
                    cycle: cycle_num,
                    started_at: Some(event.created_at.clone()),
                    ended_at: None,
                    last_seen_at: Some(event.created_at.clone()),
                    steps: Vec::new(),
                });
                current_cycle_idx = Some(cycles.len() - 1);
            }
            "step_started" | "chain_step_started" | "dynamic_step_started" => {
                let cycle_idx = ensure_cycle_for_event(&mut cycles, &mut current_cycle_idx, event);
                let step_id = event
                    .payload
                    .get("step")
                    .or_else(|| event.payload.get("step_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let (scope, item_id, anchor_item_id) = split_observed_item_binding(
                    observed_step_scope_from_payload(&event.payload),
                    &event.task_item_id,
                );

                let step = StepTrace {
                    step_id,
                    scope,
                    item_id,
                    anchor_item_id,
                    started_at: Some(event.created_at.clone()),
                    ended_at: None,
                    exit_code: None,
                    agent_id: None,
                    duration_secs: None,
                    skipped: false,
                    skip_reason: None,
                };

                let cycle = &mut cycles[cycle_idx];
                cycle.last_seen_at = Some(event.created_at.clone());
                cycle.steps.push(step);
            }
            "step_finished" | "chain_step_finished" | "dynamic_step_finished" => {
                let step_id = event
                    .payload
                    .get("step")
                    .or_else(|| event.payload.get("step_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let success = event.payload.get("success").and_then(|v| v.as_bool());
                let duration_ms = event.payload.get("duration_ms").and_then(|v| v.as_u64());
                let agent_id = event
                    .payload
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Find matching step in current cycle (search backwards for latest match)
                if let Some(cycle) = current_cycle_idx.and_then(|idx| cycles.get_mut(idx)) {
                    cycle.last_seen_at = Some(event.created_at.clone());
                    if let Some(step) = cycle
                        .steps
                        .iter_mut()
                        .rev()
                        .find(|s| s.step_id == step_id && s.ended_at.is_none())
                    {
                        step.ended_at = Some(event.created_at.clone());
                        step.exit_code = match success {
                            Some(true) => Some(0),
                            Some(false) => Some(1),
                            None => None,
                        };
                        step.duration_secs = duration_ms.map(|ms| ms as f64 / 1000.0);
                        if agent_id.is_some() {
                            step.agent_id = agent_id;
                        }
                        // Try to get actual exit_code from command_runs
                        if let Some(item_id) =
                            step.item_id.as_ref().or(step.anchor_item_id.as_ref())
                        {
                            if let Some(runs) =
                                runs_by_item_phase.get(&(item_id.clone(), step_id.to_string()))
                            {
                                if let Some(run) = runs.last() {
                                    if run.exit_code.is_some() {
                                        step.exit_code = run.exit_code;
                                    }
                                    if step.agent_id.is_none() && !run.agent_id.is_empty() {
                                        step.agent_id = Some(run.agent_id.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "step_skipped" => {
                let cycle_idx = ensure_cycle_for_event(&mut cycles, &mut current_cycle_idx, event);
                let step_id = event
                    .payload
                    .get("step")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let reason = event
                    .payload
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let (scope, item_id, anchor_item_id) = split_observed_item_binding(
                    observed_step_scope_from_payload(&event.payload),
                    &event.task_item_id,
                );

                let step = StepTrace {
                    step_id,
                    scope,
                    item_id,
                    anchor_item_id,
                    started_at: Some(event.created_at.clone()),
                    ended_at: Some(event.created_at.clone()),
                    exit_code: None,
                    agent_id: None,
                    duration_secs: Some(0.0),
                    skipped: true,
                    skip_reason: reason,
                };

                let cycle = &mut cycles[cycle_idx];
                cycle.last_seen_at = Some(event.created_at.clone());
                cycle.steps.push(step);
            }
            "task_completed" | "task_failed" | "task_paused" => {
                if let Some(cycle) = current_cycle_idx.and_then(|idx| cycles.get_mut(idx)) {
                    cycle.last_seen_at = Some(event.created_at.clone());
                    close_cycle_at(cycle, Some(event.created_at.clone()));
                }
            }
            _ if is_cycle_activity_event(&event.event_type) => {
                if let Some(cycle) = current_cycle_idx.and_then(|idx| cycles.get_mut(idx)) {
                    cycle.last_seen_at = Some(event.created_at.clone());
                }
            }
            _ => {}
        }
    }

    finalize_cycle_boundaries(&mut cycles, task_meta, events);

    cycles
        .into_iter()
        .map(|cycle| CycleTrace {
            cycle: cycle.cycle,
            started_at: cycle.started_at,
            ended_at: cycle.ended_at,
            steps: cycle.steps,
        })
        .collect()
}

fn ensure_cycle_for_event(
    cycles: &mut Vec<CycleBuilder>,
    current_cycle_idx: &mut Option<usize>,
    event: &EventDto,
) -> usize {
    if let Some(idx) = *current_cycle_idx {
        return idx;
    }

    cycles.push(CycleBuilder {
        cycle: 0,
        started_at: Some(event.created_at.clone()),
        ended_at: None,
        last_seen_at: Some(event.created_at.clone()),
        steps: Vec::new(),
    });
    let idx = cycles.len() - 1;
    *current_cycle_idx = Some(idx);
    idx
}

fn close_cycle_at(cycle: &mut CycleBuilder, ended_at: Option<String>) {
    if cycle.ended_at.is_none() {
        cycle.ended_at = ended_at;
    }
}

fn is_cycle_activity_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "loop_guard_decision"
            | "item_finalize_evaluated"
            | "task_completed"
            | "task_failed"
            | "task_paused"
    )
}

fn finalize_cycle_boundaries(
    cycles: &mut [CycleBuilder],
    task_meta: &TraceTaskMeta<'_>,
    events: &[EventDto],
) {
    let task_terminal_at = task_meta
        .completed_at
        .map(str::to_string)
        .or_else(|| events.last().map(|e| e.created_at.clone()))
        .or_else(|| {
            if task_meta.updated_at.is_empty() {
                None
            } else {
                Some(task_meta.updated_at.to_string())
            }
        });
    let task_finished = matches!(task_meta.status, "completed" | "failed");

    for idx in 0..cycles.len() {
        if cycles[idx].ended_at.is_some() {
            continue;
        }

        if idx + 1 < cycles.len() {
            cycles[idx].ended_at = cycles[idx]
                .last_seen_at
                .clone()
                .or_else(|| cycles[idx + 1].started_at.clone());
            continue;
        }

        if task_finished {
            cycles[idx].ended_at = task_terminal_at
                .clone()
                .or_else(|| cycles[idx].last_seen_at.clone())
                .or_else(|| cycles[idx].started_at.clone());
        }
    }
}
