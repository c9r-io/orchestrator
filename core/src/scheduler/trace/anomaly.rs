use crate::anomaly::{Anomaly, AnomalyRule};
use crate::dto::{CommandRunDto, EventDto};
use std::collections::{HashMap, HashSet};

use super::model::CycleTrace;
use super::time::parse_trace_timestamp;

pub(super) fn detect_duplicate_runner(events: &[&EventDto], anomalies: &mut Vec<Anomaly>) {
    let mut task_started_count = 0u32;
    let mut last_task_started_at: Option<String> = None;

    for event in events {
        match event.event_type.as_str() {
            "task_started" | "cycle_started"
                if event.payload.get("cycle").and_then(|v| v.as_u64()) == Some(1) =>
            {
                task_started_count += 1;
                if task_started_count > 1 {
                    anomalies.push(Anomaly::new(
                        AnomalyRule::DuplicateRunner,
                        format!(
                            "Multiple task starts detected (#{}) at {} — previous at {}",
                            task_started_count,
                            event.created_at,
                            last_task_started_at.as_deref().unwrap_or("?"),
                        ),
                        Some(event.created_at.clone()),
                    ));
                }
                last_task_started_at = Some(event.created_at.clone());
            }
            "task_completed" | "task_failed" => {
                task_started_count = 0;
                last_task_started_at = None;
            }
            _ => {}
        }
    }
}

pub(super) fn detect_overlapping_cycles(cycles: &[CycleTrace], anomalies: &mut Vec<Anomaly>) {
    for pair in cycles.windows(2) {
        let prev = &pair[0];
        let next = &pair[1];
        let (Some(prev_end), Some(next_start)) =
            (prev.ended_at.as_deref(), next.started_at.as_deref())
        else {
            continue;
        };
        let (Some(prev_end_dt), Some(next_start_dt)) = (
            parse_trace_timestamp(prev_end),
            parse_trace_timestamp(next_start),
        ) else {
            continue;
        };

        if prev_end_dt > next_start_dt {
            anomalies.push(Anomaly::new(
                AnomalyRule::OverlappingCycles,
                format!(
                    "Cycle {} ended at {} after Cycle {} started at {}",
                    prev.cycle, prev_end, next.cycle, next_start,
                ),
                Some(next_start.to_string()),
            ));
        }
    }
}

pub(super) fn detect_overlapping_steps(events: &[&EventDto], anomalies: &mut Vec<Anomaly>) {
    let mut open: HashMap<(String, Option<String>), String> = HashMap::new();

    for event in events {
        let step_id = event
            .payload
            .get("step")
            .or_else(|| event.payload.get("step_id"))
            .and_then(|v| v.as_str());

        match event.event_type.as_str() {
            "step_started" | "chain_step_started" | "dynamic_step_started" => {
                if let Some(step) = step_id {
                    let key = (step.to_string(), event.task_item_id.clone());
                    if let Some(prev_at) = open.get(&key) {
                        anomalies.push(Anomaly::new(
                            AnomalyRule::OverlappingSteps,
                            format!(
                                "Step '{}' started at {} while previous instance (started {}) still running",
                                step, event.created_at, prev_at,
                            ),
                            Some(event.created_at.clone()),
                        ));
                    }
                    open.insert(key, event.created_at.clone());
                }
            }
            "step_finished" | "step_skipped" => {
                if let Some(step) = step_id {
                    let key = (step.to_string(), event.task_item_id.clone());
                    open.remove(&key);
                }
            }
            "cycle_started" => {
                open.clear();
            }
            _ => {}
        }
    }
}

pub(super) fn detect_missing_step_end(events: &[&EventDto], anomalies: &mut Vec<Anomaly>) {
    let mut open: HashMap<(String, Option<String>), String> = HashMap::new();

    for event in events {
        let step_id = event
            .payload
            .get("step")
            .or_else(|| event.payload.get("step_id"))
            .and_then(|v| v.as_str());

        match event.event_type.as_str() {
            "step_started" | "chain_step_started" | "dynamic_step_started" => {
                if let Some(step) = step_id {
                    let key = (step.to_string(), event.task_item_id.clone());
                    open.insert(key, event.created_at.clone());
                }
            }
            "step_finished" | "step_skipped" | "chain_step_finished" | "dynamic_step_finished" => {
                if let Some(step) = step_id {
                    let key = (step.to_string(), event.task_item_id.clone());
                    open.remove(&key);
                }
            }
            "task_completed" | "task_failed" => {
                for ((step, _item_id), started_at) in open.drain() {
                    anomalies.push(Anomaly::new(
                        AnomalyRule::MissingStepEnd,
                        format!(
                            "Step '{}' started at {} has no corresponding step_finished/step_skipped",
                            step, started_at,
                        ),
                        Some(started_at),
                    ));
                }
            }
            _ => {}
        }
    }

    for ((step, _item_id), started_at) in open.drain() {
        anomalies.push(Anomaly::new(
            AnomalyRule::MissingStepEnd,
            format!(
                "Step '{}' started at {} has no corresponding step_finished/step_skipped",
                step, started_at,
            ),
            Some(started_at),
        ));
    }
}

pub(super) fn detect_empty_cycles(events: &[&EventDto], anomalies: &mut Vec<Anomaly>) {
    let mut cycle_start: Option<(u32, String)> = None;
    let mut has_steps = false;

    for event in events {
        match event.event_type.as_str() {
            "cycle_started" => {
                if let Some((prev_cycle, ref prev_at)) = cycle_start {
                    if !has_steps {
                        anomalies.push(Anomaly::new(
                            AnomalyRule::EmptyCycle,
                            format!(
                                "Cycle {} (started {}) completed with no steps",
                                prev_cycle, prev_at,
                            ),
                            Some(prev_at.clone()),
                        ));
                    }
                }
                let cycle = event
                    .payload
                    .get("cycle")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                cycle_start = Some((cycle, event.created_at.clone()));
                has_steps = false;
            }
            "step_started"
            | "step_finished"
            | "step_skipped"
            | "chain_step_started"
            | "dynamic_step_started" => {
                has_steps = true;
            }
            "task_completed" | "task_failed" => {
                if let Some((prev_cycle, ref prev_at)) = cycle_start {
                    if !has_steps {
                        anomalies.push(Anomaly::new(
                            AnomalyRule::EmptyCycle,
                            format!(
                                "Cycle {} (started {}) completed with no steps",
                                prev_cycle, prev_at,
                            ),
                            Some(prev_at.clone()),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

pub(super) fn detect_orphan_commands(
    events: &[&EventDto],
    command_runs: &[CommandRunDto],
    anomalies: &mut Vec<Anomaly>,
) {
    let mut known_steps: HashSet<(String, String)> = HashSet::new();
    for event in events {
        if matches!(
            event.event_type.as_str(),
            "step_started" | "chain_step_started" | "dynamic_step_started"
        ) {
            if let (Some(item_id), Some(step)) = (
                &event.task_item_id,
                event
                    .payload
                    .get("step")
                    .or_else(|| event.payload.get("step_id"))
                    .and_then(|v| v.as_str()),
            ) {
                known_steps.insert((item_id.clone(), step.to_string()));
            }
        }
    }

    for run in command_runs {
        let key = (run.task_item_id.clone(), run.phase.clone());
        if !known_steps.contains(&key) {
            anomalies.push(Anomaly::new(
                AnomalyRule::OrphanCommand,
                format!(
                    "Command run '{}' (phase={}, item={}) has no matching step_started event",
                    run.id, run.phase, run.task_item_id,
                ),
                Some(run.started_at.clone()),
            ));
        }
    }
}

pub(super) fn detect_nonzero_exit(command_runs: &[CommandRunDto], anomalies: &mut Vec<Anomaly>) {
    for run in command_runs {
        if let Some(code) = run.exit_code {
            if code != 0 && code != -1 {
                anomalies.push(Anomaly::new(
                    AnomalyRule::NonzeroExit,
                    format!(
                        "Command '{}' (phase={}) exited with code {}",
                        &run.id, run.phase, code,
                    ),
                    Some(run.started_at.clone()),
                ));
            }
        }
    }
}

pub(super) fn detect_unexpanded_template_var(
    command_runs: &[CommandRunDto],
    anomalies: &mut Vec<Anomaly>,
) {
    for run in command_runs {
        for var in find_template_vars(&run.command) {
            anomalies.push(Anomaly::new(
                AnomalyRule::UnexpandedTemplateVar,
                format!("Command contains literal {} (phase={})", var, run.phase),
                Some(run.started_at.clone()),
            ));
        }
    }
}

/// Find `{var_name}` patterns (lowercase + underscore) in a string.
pub fn find_template_vars(s: &str) -> Vec<String> {
    let mut results = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let start = i;
            i += 1;
            let name_start = i;
            while i < bytes.len() && (bytes[i].is_ascii_lowercase() || bytes[i] == b'_') {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'}' && i > name_start {
                results.push(s[start..=i].to_string());
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    results
}

pub(super) fn detect_long_running_steps(cycles: &[CycleTrace], anomalies: &mut Vec<Anomaly>) {
    for cycle in cycles {
        for step in &cycle.steps {
            if let Some(secs) = step.duration_secs {
                if secs > 600.0 {
                    anomalies.push(Anomaly::new(
                        AnomalyRule::LongRunning,
                        format!(
                            "Step '{}' took {:.0}s (>{} min)",
                            step.step_id,
                            secs,
                            (secs / 60.0).ceil() as u32,
                        ),
                        step.started_at.clone(),
                    ));
                }
            }
        }
    }
}

/// FR-035: Detects item-phase pairs with 3+ consecutive failures (degenerate loop pattern).
pub(super) fn detect_degenerate_loop(
    command_runs: &[CommandRunDto],
    anomalies: &mut Vec<Anomaly>,
) {
    // Group runs by (item_id, phase), ordered by started_at (already sorted by caller).
    let mut groups: HashMap<(&str, &str), Vec<&CommandRunDto>> = HashMap::new();
    for run in command_runs {
        groups
            .entry((run.task_item_id.as_str(), run.phase.as_str()))
            .or_default()
            .push(run);
    }

    for ((item_id, phase), runs) in &groups {
        // Count maximum consecutive non-zero exit codes from the end.
        let mut consecutive = 0u32;
        let mut last_exit: Option<i64> = None;
        for run in runs.iter().rev() {
            match run.exit_code {
                Some(code) if code != 0 => {
                    consecutive += 1;
                    if last_exit.is_none() {
                        last_exit = Some(code);
                    }
                }
                _ => break,
            }
        }
        if consecutive >= 3 {
            anomalies.push(Anomaly::new(
                AnomalyRule::DegenerateLoop,
                format!(
                    "Item '{}' phase '{}' failed {} times consecutively (last exit: {})",
                    item_id,
                    phase,
                    consecutive,
                    last_exit.unwrap_or(-1),
                ),
                runs.last().map(|r| r.started_at.clone()),
            ));
        }
    }
}

pub(super) fn detect_low_output_steps(events: &[&EventDto], anomalies: &mut Vec<Anomaly>) {
    let mut seen_steps = HashSet::new();

    for event in events {
        if event.event_type != "step_heartbeat" {
            continue;
        }
        let output_state = event.payload["output_state"].as_str();
        let pid_alive = event.payload["pid_alive"].as_bool().unwrap_or(false);
        if output_state != Some("low_output") || !pid_alive {
            continue;
        }

        let step = event.payload["step"]
            .as_str()
            .or_else(|| event.payload["step_id"].as_str())
            .unwrap_or("unknown");
        if !seen_steps.insert(step.to_string()) {
            continue;
        }

        let elapsed_secs = event.payload["elapsed_secs"].as_u64().unwrap_or(0);
        let stagnant_heartbeats = event.payload["stagnant_heartbeats"].as_u64().unwrap_or(0);
        anomalies.push(Anomaly::new(
            AnomalyRule::LowOutput,
            format!(
                "Step '{}' entered low-output state after {}s with {} quiet heartbeats",
                step, elapsed_secs, stagnant_heartbeats
            ),
            Some(event.created_at.clone()),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::CommandRunDto;

    fn make_run(id: &str, item_id: &str, phase: &str, exit_code: Option<i64>) -> CommandRunDto {
        CommandRunDto {
            id: id.to_string(),
            task_item_id: item_id.to_string(),
            phase: phase.to_string(),
            command: "echo test".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "ws1".to_string(),
            agent_id: "agent1".to_string(),
            exit_code,
            stdout_path: String::new(),
            stderr_path: String::new(),
            started_at: "2025-01-01T00:00:00Z".to_string(),
            ended_at: None,
            interrupted: false,
        }
    }

    #[test]
    fn degenerate_loop_emits_anomaly_on_three_consecutive_failures() {
        let runs = vec![
            make_run("r1", "item-a", "run", Some(0)),   // success — resets baseline
            make_run("r2", "item-a", "run", Some(1)),   // failure 1
            make_run("r3", "item-a", "run", Some(1)),   // failure 2
            make_run("r4", "item-a", "run", Some(2)),   // failure 3 — threshold reached
        ];
        let mut anomalies = Vec::new();
        detect_degenerate_loop(&runs, &mut anomalies);
        assert_eq!(anomalies.len(), 1, "expected exactly one DegenerateLoop anomaly");
        assert_eq!(anomalies[0].rule, "degenerate_loop");
        assert!(
            anomalies[0].message.contains("item-a"),
            "anomaly message should identify the item"
        );
        assert!(
            anomalies[0].message.contains("run"),
            "anomaly message should identify the phase"
        );
    }

    #[test]
    fn degenerate_loop_no_anomaly_when_fewer_than_three_consecutive_failures() {
        let runs = vec![
            make_run("r1", "item-b", "run", Some(1)),   // failure 1
            make_run("r2", "item-b", "run", Some(1)),   // failure 2 — below threshold
        ];
        let mut anomalies = Vec::new();
        detect_degenerate_loop(&runs, &mut anomalies);
        assert!(
            anomalies.is_empty(),
            "expected no anomaly for fewer than 3 consecutive failures"
        );
    }

    #[test]
    fn degenerate_loop_no_anomaly_when_failures_are_non_consecutive() {
        // Three failures but a success breaks the streak at the tail; the trailing
        // run window (success + 2 failures counting backwards) is only 2 failures.
        let runs = vec![
            make_run("r1", "item-c", "run", Some(1)),   // failure
            make_run("r2", "item-c", "run", Some(1)),   // failure
            make_run("r3", "item-c", "run", Some(0)),   // success — breaks streak
            make_run("r4", "item-c", "run", Some(1)),   // failure
            make_run("r5", "item-c", "run", Some(1)),   // failure — streak of 2, not 3
        ];
        let mut anomalies = Vec::new();
        detect_degenerate_loop(&runs, &mut anomalies);
        assert!(
            anomalies.is_empty(),
            "expected no anomaly when a success interrupts the failure streak"
        );
    }
}
