use crate::dto::{CommandRunDto, EventDto};
use serde::Serialize;
use std::collections::HashMap;

// ── Data structures ──────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TaskTrace {
    pub task_id: String,
    pub status: String,
    pub cycles: Vec<CycleTrace>,
    pub anomalies: Vec<Anomaly>,
    pub summary: TraceSummary,
}

#[derive(Debug, Serialize)]
pub struct CycleTrace {
    pub cycle: u32,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub steps: Vec<StepTrace>,
}

#[derive(Debug, Serialize)]
pub struct StepTrace {
    pub step_id: String,
    pub scope: String,
    pub item_id: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub exit_code: Option<i64>,
    pub agent_id: Option<String>,
    pub duration_secs: Option<f64>,
    pub skipped: bool,
    pub skip_reason: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Anomaly {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub at: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Serialize)]
pub struct TraceSummary {
    pub total_cycles: u32,
    pub total_steps: u32,
    pub total_commands: u32,
    pub failed_commands: u32,
    pub anomaly_counts: HashMap<String, u32>,
    pub wall_time_secs: Option<f64>,
}

// ── Pure trace builder ───────────────────────────────────────────────

pub fn build_trace(
    task_id: &str,
    status: &str,
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

    let cycles = build_cycles(events, command_runs);
    let mut anomalies = Vec::new();

    detect_duplicate_runner(events, &mut anomalies);
    detect_overlapping_cycles(events, &mut anomalies);
    detect_overlapping_steps(events, &mut anomalies);
    detect_missing_step_end(events, &mut anomalies);
    detect_empty_cycles(events, &mut anomalies);
    detect_orphan_commands(events, command_runs, &mut anomalies);
    detect_nonzero_exit(command_runs, &mut anomalies);
    detect_unexpanded_template_var(command_runs, &mut anomalies);
    detect_long_running_steps(&cycles, &mut anomalies);

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

    let wall_time_secs = compute_wall_time(events);

    let summary = TraceSummary {
        total_cycles: cycles.len() as u32,
        total_steps,
        total_commands,
        failed_commands,
        anomaly_counts,
        wall_time_secs,
    };

    TaskTrace {
        task_id: task_id.to_string(),
        status: status.to_string(),
        cycles,
        anomalies,
        summary,
    }
}

// ── Timeline reconstruction ──────────────────────────────────────────

fn build_cycles(events: &[EventDto], command_runs: &[CommandRunDto]) -> Vec<CycleTrace> {
    let mut cycles: Vec<CycleTrace> = Vec::new();
    let mut current_cycle: Option<u32> = None;

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
                let cycle_num = event
                    .payload
                    .get("cycle")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                current_cycle = Some(cycle_num);
                cycles.push(CycleTrace {
                    cycle: cycle_num,
                    started_at: Some(event.created_at.clone()),
                    ended_at: None,
                    steps: Vec::new(),
                });
            }
            "step_started" | "chain_step_started" | "dynamic_step_started" => {
                let step_id = event
                    .payload
                    .get("step")
                    .or_else(|| event.payload.get("step_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let scope = if event.task_item_id.is_some() {
                    "item"
                } else {
                    "task"
                };

                let step = StepTrace {
                    step_id,
                    scope: scope.to_string(),
                    item_id: event.task_item_id.clone(),
                    started_at: Some(event.created_at.clone()),
                    ended_at: None,
                    exit_code: None,
                    agent_id: None,
                    duration_secs: None,
                    skipped: false,
                    skip_reason: None,
                };

                if let Some(cycle) = cycles.last_mut() {
                    cycle.steps.push(step);
                } else {
                    // Step before any cycle — create an implicit cycle 0
                    if current_cycle.is_none() {
                        current_cycle = Some(0);
                        cycles.push(CycleTrace {
                            cycle: 0,
                            started_at: Some(event.created_at.clone()),
                            ended_at: None,
                            steps: Vec::new(),
                        });
                    }
                    cycles.last_mut().unwrap().steps.push(step);
                }
            }
            "step_finished" => {
                let step_id = event
                    .payload
                    .get("step")
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
                if let Some(cycle) = cycles.last_mut() {
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
                        if let Some(item_id) = &step.item_id {
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

                let step = StepTrace {
                    step_id,
                    scope: if event.task_item_id.is_some() {
                        "item"
                    } else {
                        "task"
                    }
                    .to_string(),
                    item_id: event.task_item_id.clone(),
                    started_at: Some(event.created_at.clone()),
                    ended_at: Some(event.created_at.clone()),
                    exit_code: None,
                    agent_id: None,
                    duration_secs: Some(0.0),
                    skipped: true,
                    skip_reason: reason,
                };

                if let Some(cycle) = cycles.last_mut() {
                    cycle.steps.push(step);
                }
            }
            "task_completed" | "task_failed" => {
                // Close the last cycle
                if let Some(cycle) = cycles.last_mut() {
                    if cycle.ended_at.is_none() {
                        cycle.ended_at = Some(event.created_at.clone());
                    }
                }
            }
            _ => {}
        }
    }

    cycles
}

fn compute_wall_time(events: &[EventDto]) -> Option<f64> {
    let first = events.first()?;
    let last = events.last()?;
    let start = parse_timestamp(&first.created_at)?;
    let end = parse_timestamp(&last.created_at)?;
    let duration = end.signed_duration_since(start);
    Some(duration.num_milliseconds() as f64 / 1000.0)
}

fn parse_timestamp(ts: &str) -> Option<chrono::NaiveDateTime> {
    // Try common formats
    chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f"))
        .or_else(|_| {
            // Handle timezone-aware strings by trimming tz
            let trimmed = ts.trim_end_matches('Z');
            chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S%.f")
        })
        .ok()
}

// ── Anomaly detection rules ──────────────────────────────────────────

fn detect_duplicate_runner(events: &[EventDto], anomalies: &mut Vec<Anomaly>) {
    let mut task_started_count = 0u32;
    let mut last_task_started_at: Option<String> = None;

    for event in events {
        match event.event_type.as_str() {
            "task_started" | "cycle_started"
                if event.payload.get("cycle").and_then(|v| v.as_u64()) == Some(1) =>
            {
                task_started_count += 1;
                if task_started_count > 1 {
                    anomalies.push(Anomaly {
                        rule: "duplicate_runner".to_string(),
                        severity: Severity::Error,
                        message: format!(
                            "Multiple task starts detected (#{}) at {} — previous at {}",
                            task_started_count,
                            event.created_at,
                            last_task_started_at.as_deref().unwrap_or("?"),
                        ),
                        at: Some(event.created_at.clone()),
                    });
                }
                last_task_started_at = Some(event.created_at.clone());
            }
            "task_completed" | "task_failed" => {
                // Reset — a clean stop means a subsequent start is OK
                task_started_count = 0;
                last_task_started_at = None;
            }
            _ => {}
        }
    }
}

fn detect_overlapping_cycles(events: &[EventDto], anomalies: &mut Vec<Anomaly>) {
    let mut open_cycle: Option<(u32, String)> = None; // (cycle_num, started_at)

    for event in events {
        if event.event_type == "cycle_started" {
            let cycle = event
                .payload
                .get("cycle")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            if let Some((prev_cycle, ref prev_at)) = open_cycle {
                anomalies.push(Anomaly {
                    rule: "overlapping_cycles".to_string(),
                    severity: Severity::Error,
                    message: format!(
                        "Cycle {} started at {} while Cycle {} (started {}) still running",
                        cycle, event.created_at, prev_cycle, prev_at,
                    ),
                    at: Some(event.created_at.clone()),
                });
            }
            open_cycle = Some((cycle, event.created_at.clone()));
        } else if event.event_type == "task_completed"
            || event.event_type == "task_failed"
            || event.event_type == "task_paused"
        {
            open_cycle = None;
        }
    }
}

fn detect_overlapping_steps(events: &[EventDto], anomalies: &mut Vec<Anomaly>) {
    // Track open steps by (step_id, item_id)
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
                        anomalies.push(Anomaly {
                            rule: "overlapping_steps".to_string(),
                            severity: Severity::Error,
                            message: format!(
                                "Step '{}' started at {} while previous instance (started {}) still running",
                                step, event.created_at, prev_at,
                            ),
                            at: Some(event.created_at.clone()),
                        });
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
                // New cycle clears open steps
                open.clear();
            }
            _ => {}
        }
    }
}

fn detect_missing_step_end(events: &[EventDto], anomalies: &mut Vec<Anomaly>) {
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
                // At task end, any open steps are missing their end event
                for ((step, _item_id), started_at) in open.drain() {
                    anomalies.push(Anomaly {
                        rule: "missing_step_end".to_string(),
                        severity: Severity::Warning,
                        message: format!(
                            "Step '{}' started at {} has no corresponding step_finished/step_skipped",
                            step, started_at,
                        ),
                        at: Some(started_at),
                    });
                }
            }
            _ => {}
        }
    }

    // If no terminal event, anything still open is missing
    for ((step, _item_id), started_at) in open.drain() {
        anomalies.push(Anomaly {
            rule: "missing_step_end".to_string(),
            severity: Severity::Warning,
            message: format!(
                "Step '{}' started at {} has no corresponding step_finished/step_skipped",
                step, started_at,
            ),
            at: Some(started_at),
        });
    }
}

fn detect_empty_cycles(events: &[EventDto], anomalies: &mut Vec<Anomaly>) {
    let mut cycle_start: Option<(u32, String)> = None;
    let mut has_steps = false;

    for event in events {
        match event.event_type.as_str() {
            "cycle_started" => {
                if let Some((prev_cycle, ref prev_at)) = cycle_start {
                    if !has_steps {
                        anomalies.push(Anomaly {
                            rule: "empty_cycle".to_string(),
                            severity: Severity::Warning,
                            message: format!(
                                "Cycle {} (started {}) completed with no steps",
                                prev_cycle, prev_at,
                            ),
                            at: Some(prev_at.clone()),
                        });
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
                        anomalies.push(Anomaly {
                            rule: "empty_cycle".to_string(),
                            severity: Severity::Warning,
                            message: format!(
                                "Cycle {} (started {}) completed with no steps",
                                prev_cycle, prev_at,
                            ),
                            at: Some(prev_at.clone()),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

fn detect_orphan_commands(
    events: &[EventDto],
    command_runs: &[CommandRunDto],
    anomalies: &mut Vec<Anomaly>,
) {
    // Collect all (item_id, phase) from step_started events
    let mut known_steps: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
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
            anomalies.push(Anomaly {
                rule: "orphan_command".to_string(),
                severity: Severity::Warning,
                message: format!(
                    "Command run '{}' (phase={}, item={}) has no matching step_started event",
                    run.id, run.phase, run.task_item_id,
                ),
                at: Some(run.started_at.clone()),
            });
        }
    }
}

fn detect_nonzero_exit(command_runs: &[CommandRunDto], anomalies: &mut Vec<Anomaly>) {
    for run in command_runs {
        if let Some(code) = run.exit_code {
            if code != 0 && code != -1 {
                anomalies.push(Anomaly {
                    rule: "nonzero_exit".to_string(),
                    severity: Severity::Warning,
                    message: format!(
                        "Command '{}' (phase={}) exited with code {}",
                        &run.id, run.phase, code,
                    ),
                    at: Some(run.started_at.clone()),
                });
            }
        }
    }
}

fn detect_unexpanded_template_var(command_runs: &[CommandRunDto], anomalies: &mut Vec<Anomaly>) {
    for run in command_runs {
        for var in find_template_vars(&run.command) {
            anomalies.push(Anomaly {
                rule: "unexpanded_template_var".to_string(),
                severity: Severity::Warning,
                message: format!("Command contains literal {} (phase={})", var, run.phase,),
                at: Some(run.started_at.clone()),
            });
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

fn detect_long_running_steps(cycles: &[CycleTrace], anomalies: &mut Vec<Anomaly>) {
    for cycle in cycles {
        for step in &cycle.steps {
            if let Some(secs) = step.duration_secs {
                if secs > 600.0 {
                    anomalies.push(Anomaly {
                        rule: "long_running_step".to_string(),
                        severity: Severity::Info,
                        message: format!(
                            "Step '{}' took {:.0}s (>{} min)",
                            step.step_id,
                            secs,
                            (secs / 60.0).ceil() as u32,
                        ),
                        at: step.started_at.clone(),
                    });
                }
            }
        }
    }
}

// ── Terminal rendering ───────────────────────────────────────────────

pub fn render_trace_terminal(trace: &TaskTrace, verbose: bool) {
    // Header
    println!(
        "Task {} — status: {}",
        &trace.task_id[..trace.task_id.len().min(8)],
        colorize_status(&trace.status),
    );

    let wall = trace
        .summary
        .wall_time_secs
        .map(format_duration)
        .unwrap_or_else(|| "?".to_string());
    println!(
        "Wall time: {} | {} cycle{} | {} step{} | {} command{} ({} failed)",
        wall,
        trace.summary.total_cycles,
        if trace.summary.total_cycles == 1 {
            ""
        } else {
            "s"
        },
        trace.summary.total_steps,
        if trace.summary.total_steps == 1 {
            ""
        } else {
            "s"
        },
        trace.summary.total_commands,
        if trace.summary.total_commands == 1 {
            ""
        } else {
            "s"
        },
        trace.summary.failed_commands,
    );

    // Anomalies
    if !trace.anomalies.is_empty() {
        println!();
        let total: u32 = trace.summary.anomaly_counts.values().sum();
        println!(
            "\x1b[33m⚠ {} anomal{} detected:\x1b[0m",
            total,
            if total == 1 { "y" } else { "ies" },
        );
        for a in &trace.anomalies {
            let (color, label) = match a.severity {
                Severity::Error => ("\x1b[31m", "ERROR"),
                Severity::Warning => ("\x1b[33m", " WARN"),
                Severity::Info => ("\x1b[36m", " INFO"),
            };
            println!("  {}{}\x1b[0m  {} — {}", color, label, a.rule, a.message);
        }
    }

    // Cycles
    for cycle in &trace.cycles {
        println!();
        println!("── Cycle {} ─────────────────────────────────", cycle.cycle,);
        for step in &cycle.steps {
            let time = step
                .started_at
                .as_ref()
                .map(|t| extract_time(t))
                .unwrap_or_else(|| "??:??:??".to_string());

            let (icon, color) = if step.skipped {
                ("⊘", "\x1b[90m")
            } else {
                match step.exit_code {
                    Some(0) => ("✓", "\x1b[32m"),
                    Some(_) => ("✗", "\x1b[31m"),
                    None => ("●", "\x1b[33m"),
                }
            };

            let dur = step
                .duration_secs
                .map(|s| format!("{:>4}s", s as u64))
                .unwrap_or_else(|| "   - ".to_string());

            let agent = step
                .agent_id
                .as_ref()
                .map(|a| format!("  agent={}", a))
                .unwrap_or_default();

            let exit = match step.exit_code {
                Some(c) if c != 0 => format!("  exit={}", c),
                _ => String::new(),
            };

            let skip_info = step
                .skip_reason
                .as_ref()
                .map(|r| format!(" ({})", r))
                .unwrap_or_default();

            println!(
                "  {}  {}{} {:<14}\x1b[0m  {}{}{}{}",
                time, color, icon, step.step_id, dur, agent, exit, skip_info,
            );

            if verbose {
                if let Some(item_id) = &step.item_id {
                    println!("             item={}", item_id);
                }
            }
        }
    }
    println!();
}

fn colorize_status(status: &str) -> String {
    match status {
        "completed" => format!("\x1b[32m{}\x1b[0m", status),
        "failed" => format!("\x1b[31m{}\x1b[0m", status),
        "running" => format!("\x1b[33m{}\x1b[0m", status),
        "paused" => format!("\x1b[90m{}\x1b[0m", status),
        _ => status.to_string(),
    }
}

fn format_duration(secs: f64) -> String {
    let total = secs as u64;
    if total >= 3600 {
        format!(
            "{}h {:02}m {:02}s",
            total / 3600,
            (total % 3600) / 60,
            total % 60
        )
    } else if total >= 60 {
        format!("{}m {:02}s", total / 60, total % 60)
    } else {
        format!("{}s", total)
    }
}

fn extract_time(ts: &str) -> String {
    // Extract HH:MM:SS from various timestamp formats
    if let Some(t_pos) = ts.find('T') {
        let time_part = &ts[t_pos + 1..];
        time_part[..time_part.len().min(8)].to_string()
    } else if let Some(space_pos) = ts.find(' ') {
        let time_part = &ts[space_pos + 1..];
        time_part[..time_part.len().min(8)].to_string()
    } else {
        ts.to_string()
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn make_event(id: i64, event_type: &str, payload: Value, created_at: &str) -> EventDto {
        EventDto {
            id,
            task_id: "test-task".to_string(),
            task_item_id: None,
            event_type: event_type.to_string(),
            payload,
            created_at: created_at.to_string(),
        }
    }

    fn make_item_event(
        id: i64,
        event_type: &str,
        payload: Value,
        created_at: &str,
        item_id: &str,
    ) -> EventDto {
        EventDto {
            id,
            task_id: "test-task".to_string(),
            task_item_id: Some(item_id.to_string()),
            event_type: event_type.to_string(),
            payload,
            created_at: created_at.to_string(),
        }
    }

    fn make_run(
        phase: &str,
        item_id: &str,
        exit_code: Option<i64>,
        agent_id: &str,
    ) -> CommandRunDto {
        CommandRunDto {
            id: format!("run-{}-{}", phase, item_id),
            task_item_id: item_id.to_string(),
            phase: phase.to_string(),
            command: format!("echo {}", phase),
            cwd: "/tmp".to_string(),
            workspace_id: "ws".to_string(),
            agent_id: agent_id.to_string(),
            exit_code,
            stdout_path: String::new(),
            stderr_path: String::new(),
            started_at: "2025-01-01 10:00:00".to_string(),
            ended_at: Some("2025-01-01 10:00:10".to_string()),
            interrupted: false,
        }
    }

    // ── Timeline reconstruction tests ─────────────────────

    #[test]
    fn single_cycle_with_steps() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_item_event(
                2,
                "step_started",
                json!({"step": "plan"}),
                "2025-01-01 10:00:01",
                "item-1",
            ),
            make_item_event(
                3,
                "step_finished",
                json!({"step": "plan", "success": true, "duration_ms": 5000}),
                "2025-01-01 10:00:06",
                "item-1",
            ),
            make_item_event(
                4,
                "step_started",
                json!({"step": "implement"}),
                "2025-01-01 10:00:07",
                "item-1",
            ),
            make_item_event(
                5,
                "step_finished",
                json!({"step": "implement", "success": true, "duration_ms": 12000}),
                "2025-01-01 10:00:19",
                "item-1",
            ),
            make_event(6, "task_completed", json!({}), "2025-01-01 10:00:20"),
        ];

        let trace = build_trace("test-task", "completed", &events, &[]);
        assert_eq!(trace.cycles.len(), 1);
        assert_eq!(trace.cycles[0].cycle, 1);
        assert_eq!(trace.cycles[0].steps.len(), 2);
        assert_eq!(trace.cycles[0].steps[0].step_id, "plan");
        assert_eq!(trace.cycles[0].steps[0].duration_secs, Some(5.0));
        assert_eq!(trace.cycles[0].steps[1].step_id, "implement");
        assert_eq!(trace.summary.total_steps, 2);
        assert_eq!(trace.summary.total_cycles, 1);
    }

    #[test]
    fn multi_cycle_trace() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_item_event(
                2,
                "step_started",
                json!({"step": "plan"}),
                "2025-01-01 10:00:01",
                "item-1",
            ),
            make_item_event(
                3,
                "step_finished",
                json!({"step": "plan", "success": true, "duration_ms": 3000}),
                "2025-01-01 10:00:04",
                "item-1",
            ),
            make_event(
                4,
                "cycle_started",
                json!({"cycle": 2}),
                "2025-01-01 10:01:00",
            ),
            make_item_event(
                5,
                "step_started",
                json!({"step": "plan"}),
                "2025-01-01 10:01:01",
                "item-1",
            ),
            make_item_event(
                6,
                "step_finished",
                json!({"step": "plan", "success": true, "duration_ms": 2000}),
                "2025-01-01 10:01:03",
                "item-1",
            ),
            make_event(7, "task_completed", json!({}), "2025-01-01 10:01:04"),
        ];

        let trace = build_trace("test-task", "completed", &events, &[]);
        assert_eq!(trace.cycles.len(), 2);
        assert_eq!(trace.cycles[0].cycle, 1);
        assert_eq!(trace.cycles[1].cycle, 2);
        assert_eq!(trace.summary.total_cycles, 2);
    }

    #[test]
    fn skipped_step_recorded() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_item_event(
                2,
                "step_skipped",
                json!({"step": "qa", "reason": "prehook: build_failed"}),
                "2025-01-01 10:00:05",
                "item-1",
            ),
            make_event(3, "task_completed", json!({}), "2025-01-01 10:00:06"),
        ];

        let trace = build_trace("test-task", "completed", &events, &[]);
        assert_eq!(trace.cycles[0].steps.len(), 1);
        assert!(trace.cycles[0].steps[0].skipped);
        assert_eq!(
            trace.cycles[0].steps[0].skip_reason.as_deref(),
            Some("prehook: build_failed"),
        );
    }

    #[test]
    fn command_run_enriches_step() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_item_event(
                2,
                "step_started",
                json!({"step": "plan"}),
                "2025-01-01 10:00:01",
                "item-1",
            ),
            make_item_event(
                3,
                "step_finished",
                json!({"step": "plan", "success": true, "duration_ms": 5000}),
                "2025-01-01 10:00:06",
                "item-1",
            ),
        ];
        let runs = vec![make_run("plan", "item-1", Some(0), "agent-minimax")];

        let trace = build_trace("test-task", "completed", &events, &runs);
        assert_eq!(
            trace.cycles[0].steps[0].agent_id.as_deref(),
            Some("agent-minimax")
        );
        assert_eq!(trace.cycles[0].steps[0].exit_code, Some(0));
    }

    // ── Anomaly detection tests ───────────────────────────

    #[test]
    fn detect_duplicate_runner_anomaly() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_event(
                2,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:05",
            ),
        ];

        let trace = build_trace("test-task", "running", &events, &[]);
        let dup = trace
            .anomalies
            .iter()
            .find(|a| a.rule == "duplicate_runner");
        assert!(dup.is_some(), "should detect duplicate runner");
        assert_eq!(dup.unwrap().severity, Severity::Error);
    }

    #[test]
    fn detect_overlapping_cycles_anomaly() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_event(
                2,
                "cycle_started",
                json!({"cycle": 2}),
                "2025-01-01 10:00:05",
            ),
        ];

        let trace = build_trace("test-task", "running", &events, &[]);
        let overlap = trace
            .anomalies
            .iter()
            .find(|a| a.rule == "overlapping_cycles");
        assert!(overlap.is_some(), "should detect overlapping cycles");
        assert_eq!(overlap.unwrap().severity, Severity::Error);
    }

    #[test]
    fn detect_overlapping_steps_anomaly() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_item_event(
                2,
                "step_started",
                json!({"step": "plan"}),
                "2025-01-01 10:00:01",
                "item-1",
            ),
            make_item_event(
                3,
                "step_started",
                json!({"step": "plan"}),
                "2025-01-01 10:00:02",
                "item-1",
            ),
        ];

        let trace = build_trace("test-task", "running", &events, &[]);
        let overlap = trace
            .anomalies
            .iter()
            .find(|a| a.rule == "overlapping_steps");
        assert!(overlap.is_some(), "should detect overlapping steps");
    }

    #[test]
    fn detect_unexpanded_template_var_anomaly() {
        let runs = vec![CommandRunDto {
            id: "run-1".to_string(),
            task_item_id: "item-1".to_string(),
            phase: "qa_doc_gen".to_string(),
            command: "echo {plan_output}".to_string(),
            cwd: "/tmp".to_string(),
            workspace_id: "ws".to_string(),
            agent_id: "agent-1".to_string(),
            exit_code: Some(0),
            stdout_path: String::new(),
            stderr_path: String::new(),
            started_at: "2025-01-01 10:00:00".to_string(),
            ended_at: None,
            interrupted: false,
        }];

        let trace = build_trace("test-task", "completed", &[], &runs);
        let tmpl = trace
            .anomalies
            .iter()
            .find(|a| a.rule == "unexpanded_template_var");
        assert!(tmpl.is_some(), "should detect unexpanded template var");
        assert!(tmpl.unwrap().message.contains("{plan_output}"));
    }

    #[test]
    fn detect_nonzero_exit_anomaly() {
        let runs = vec![make_run("implement", "item-1", Some(1), "agent-1")];

        let trace = build_trace("test-task", "failed", &[], &runs);
        let nz = trace.anomalies.iter().find(|a| a.rule == "nonzero_exit");
        assert!(nz.is_some(), "should detect nonzero exit");
    }

    #[test]
    fn detect_orphan_command_anomaly() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            // No step_started for "plan" on "item-1"
        ];
        let runs = vec![make_run("plan", "item-1", Some(0), "agent-1")];

        let trace = build_trace("test-task", "completed", &events, &runs);
        let orphan = trace.anomalies.iter().find(|a| a.rule == "orphan_command");
        assert!(orphan.is_some(), "should detect orphan command");
    }

    #[test]
    fn detect_missing_step_end_anomaly() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_item_event(
                2,
                "step_started",
                json!({"step": "plan"}),
                "2025-01-01 10:00:01",
                "item-1",
            ),
            // No step_finished for "plan"
            make_event(3, "task_completed", json!({}), "2025-01-01 10:00:10"),
        ];

        let trace = build_trace("test-task", "completed", &events, &[]);
        let missing = trace
            .anomalies
            .iter()
            .find(|a| a.rule == "missing_step_end");
        assert!(missing.is_some(), "should detect missing step end");
    }

    #[test]
    fn detect_empty_cycle_anomaly() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_event(2, "task_completed", json!({}), "2025-01-01 10:00:05"),
        ];

        let trace = build_trace("test-task", "completed", &events, &[]);
        let empty = trace.anomalies.iter().find(|a| a.rule == "empty_cycle");
        assert!(empty.is_some(), "should detect empty cycle");
    }

    #[test]
    fn detect_long_running_step_anomaly() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_item_event(
                2,
                "step_started",
                json!({"step": "implement"}),
                "2025-01-01 10:00:01",
                "item-1",
            ),
            make_item_event(
                3,
                "step_finished",
                json!({"step": "implement", "success": true, "duration_ms": 700000}),
                "2025-01-01 10:11:41",
                "item-1",
            ),
        ];

        let trace = build_trace("test-task", "completed", &events, &[]);
        let long = trace
            .anomalies
            .iter()
            .find(|a| a.rule == "long_running_step");
        assert!(long.is_some(), "should detect long running step");
    }

    // ── Edge cases ────────────────────────────────────────

    #[test]
    fn empty_events_produces_empty_trace() {
        let trace = build_trace("test-task", "pending", &[], &[]);
        assert!(trace.cycles.is_empty());
        assert!(trace.anomalies.is_empty());
        assert_eq!(trace.summary.total_cycles, 0);
        assert_eq!(trace.summary.total_steps, 0);
    }

    #[test]
    fn clean_sequence_no_anomalies() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_item_event(
                2,
                "step_started",
                json!({"step": "plan"}),
                "2025-01-01 10:00:01",
                "item-1",
            ),
            make_item_event(
                3,
                "step_finished",
                json!({"step": "plan", "success": true, "duration_ms": 5000}),
                "2025-01-01 10:00:06",
                "item-1",
            ),
            make_event(4, "task_completed", json!({}), "2025-01-01 10:00:07"),
        ];
        let runs = vec![make_run("plan", "item-1", Some(0), "agent-1")];

        let trace = build_trace("test-task", "completed", &events, &runs);
        assert!(
            trace.anomalies.is_empty(),
            "clean sequence should have no anomalies, got: {:?}",
            trace.anomalies,
        );
    }

    #[test]
    fn json_serialization_roundtrip() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_item_event(
                2,
                "step_started",
                json!({"step": "plan"}),
                "2025-01-01 10:00:01",
                "item-1",
            ),
            make_item_event(
                3,
                "step_finished",
                json!({"step": "plan", "success": true, "duration_ms": 3000}),
                "2025-01-01 10:00:04",
                "item-1",
            ),
            make_event(4, "task_completed", json!({}), "2025-01-01 10:00:05"),
        ];

        let trace = build_trace("test-task", "completed", &events, &[]);
        let json_str = serde_json::to_string(&trace).expect("should serialize");
        let parsed: Value = serde_json::from_str(&json_str).expect("should parse");
        assert_eq!(parsed["task_id"], "test-task");
        assert_eq!(parsed["status"], "completed");
        assert!(parsed["cycles"].is_array());
        assert!(parsed["anomalies"].is_array());
        assert!(parsed["summary"].is_object());
    }

    #[test]
    fn wall_time_calculated() {
        let events = vec![
            make_event(
                1,
                "cycle_started",
                json!({"cycle": 1}),
                "2025-01-01 10:00:00",
            ),
            make_event(2, "task_completed", json!({}), "2025-01-01 10:04:32"),
        ];

        let trace = build_trace("test-task", "completed", &events, &[]);
        assert!(trace.summary.wall_time_secs.is_some());
        let wall = trace.summary.wall_time_secs.unwrap();
        assert!(
            (wall - 272.0).abs() < 1.0,
            "wall time should be ~272s, got {}",
            wall
        );
    }
}
