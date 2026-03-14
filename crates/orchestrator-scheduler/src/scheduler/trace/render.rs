use agent_orchestrator::anomaly::{Escalation, Severity};

use super::model::TaskTrace;

/// Renders a task trace to the terminal in human-readable form.
pub fn render_trace_terminal(trace: &TaskTrace, verbose: bool) {
    // Header
    println!(
        "Task {} — status: {}",
        &trace.task_id[..trace.task_id.len().min(8)],
        colorize_status(&trace.status),
    );

    // Build version
    if let Some(ref bv) = trace.build_version {
        println!(
            "Build: {} ({}) {}",
            bv.version, bv.git_hash, bv.build_timestamp,
        );
    }

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
            let esc_tag = match a.escalation {
                Escalation::Intervene => " \x1b[41;37m[INTERVENE]\x1b[0m",
                Escalation::Attention => " \x1b[33m[ATTENTION]\x1b[0m",
                Escalation::Notice => "",
            };
            println!(
                "  {}{}\x1b[0m  {} — {}{}",
                color, label, a.rule, a.message, esc_tag
            );
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
                let scope_display = if step.scope == "unspecified" {
                    "scope=unspecified (step_scope not recorded)".to_string()
                } else {
                    format!("scope={}", step.scope)
                };
                let mut parts = vec![scope_display];
                if let Some(item_id) = &step.item_id {
                    parts.push(format!("item={item_id}"));
                }
                if let Some(anchor_item_id) = &step.anchor_item_id {
                    parts.push(format!("anchor_item={anchor_item_id}"));
                }
                println!("             {}", parts.join(" "));
            }
        }
    }
    println!();
}

pub(super) fn colorize_status(status: &str) -> String {
    match status {
        "completed" => format!("\x1b[32m{}\x1b[0m", status),
        "failed" => format!("\x1b[31m{}\x1b[0m", status),
        "running" => format!("\x1b[33m{}\x1b[0m", status),
        "paused" => format!("\x1b[90m{}\x1b[0m", status),
        _ => status.to_string(),
    }
}

pub(super) fn format_duration(secs: f64) -> String {
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

pub(super) fn extract_time(ts: &str) -> String {
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
