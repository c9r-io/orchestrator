use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use agent_orchestrator::dto::{EventDto, TaskTimelineSource, TimelineCommandRunDto};
use agent_orchestrator::runner::redact_text;
use sha2::{Digest, Sha256};

use super::model::{
    EvidenceRef, ProjectedTimelineEntry, TIMELINE_PROJECTION_VERSION, TimelineActorRef,
    TimelineCategory, TimelineEntry,
};

pub(crate) fn project_timeline(
    source: &TaskTimelineSource,
    redaction_patterns: &[String],
    categories: &HashSet<TimelineCategory>,
) -> Vec<ProjectedTimelineEntry> {
    let mut projected = Vec::new();
    let goal = redact_text(&source.task.goal, redaction_patterns);
    if categories.is_empty() || categories.contains(&TimelineCategory::Goal) {
        projected.push(ProjectedTimelineEntry {
            source_order: 0,
            entry: TimelineEntry {
                id: stable_id(&source.task.id, TimelineCategory::Goal, &[], "goal"),
                task_id: source.task.id.clone(),
                occurred_at: source.task.created_at.clone(),
                category: TimelineCategory::Goal,
                title: "Goal defined".to_string(),
                summary: if goal.is_empty() {
                    source.task.name.clone()
                } else {
                    goal
                },
                status: Some(source.task.status.clone()),
                actor: Some(TimelineActorRef {
                    actor_type: "human".to_string(),
                    actor_id: "requester".to_string(),
                }),
                step_id: None,
                task_item_id: None,
                command_run_id: None,
                session_id: None,
                checkpoint_id: None,
                source_event_id: None,
                evidence: Vec::new(),
                raw_event_ids: Vec::new(),
                projection_version: TIMELINE_PROJECTION_VERSION,
            },
        });
    }

    let mut consumed = HashSet::new();
    let mut consumed_runs = HashSet::new();
    let item_labels: HashMap<&str, &str> = source
        .items
        .iter()
        .map(|item| (item.id.as_str(), item.qa_file_path.as_str()))
        .collect();

    for (index, event) in source.events.iter().enumerate() {
        if consumed.contains(&event.id) || is_low_value_event(&event.event_type) {
            continue;
        }

        if let Some(finish_type) = matching_finish_type(&event.event_type) {
            let step_id = event_step(event);
            let finish = source.events.iter().skip(index + 1).find(|candidate| {
                !consumed.contains(&candidate.id)
                    && candidate.event_type == finish_type
                    && candidate.task_item_id == event.task_item_id
                    && event_step(candidate) == step_id
            });
            if let Some(finish_event) = finish {
                consumed.insert(finish_event.id);
            }
            let run = matching_run(
                &source.runs,
                &consumed_runs,
                event.task_item_id.as_deref(),
                step_id.as_deref(),
            );
            if let Some(run) = run {
                consumed_runs.insert(run.id.clone());
            }
            let mut entries =
                project_step(source, event, finish, run, redaction_patterns, &item_labels);
            projected.append(&mut entries);
            continue;
        }

        if is_finish_event(&event.event_type) {
            let run = matching_run(
                &source.runs,
                &consumed_runs,
                event.task_item_id.as_deref(),
                event_step(event).as_deref(),
            );
            if let Some(run) = run {
                consumed_runs.insert(run.id.clone());
            }
            let mut entries = project_step(
                source,
                event,
                Some(event),
                run,
                redaction_patterns,
                &item_labels,
            );
            projected.append(&mut entries);
            continue;
        }

        if let Some(entry) = project_event(source, event, redaction_patterns) {
            projected.push(entry);
        }
    }

    projected.retain(|entry| categories.is_empty() || categories.contains(&entry.entry.category));
    projected.sort_by(|left, right| {
        (left.source_order, left.entry.id.as_str())
            .cmp(&(right.source_order, right.entry.id.as_str()))
    });
    projected
}

fn project_step(
    source: &TaskTimelineSource,
    start: &EventDto,
    finish: Option<&EventDto>,
    run: Option<&TimelineCommandRunDto>,
    patterns: &[String],
    item_labels: &HashMap<&str, &str>,
) -> Vec<ProjectedTimelineEntry> {
    let step_id = event_step(start).unwrap_or_else(|| "unknown".to_string());
    let category = if is_test_phase(&step_id) {
        TimelineCategory::Test
    } else {
        TimelineCategory::Step
    };
    let success = finish
        .and_then(|event| event.payload.get("success"))
        .and_then(serde_json::Value::as_bool)
        .or_else(|| run.and_then(|value| value.exit_code.map(|code| code == 0)));
    let status = match success {
        Some(true) => "completed",
        Some(false) => "failed",
        None => "running",
    };
    let raw_ids = finish
        .filter(|event| event.id != start.id)
        .map(|event| vec![start.id, event.id])
        .unwrap_or_else(|| vec![start.id]);
    let mut evidence = run.map(run_evidence).unwrap_or_default();
    if let Some(run) = run {
        evidence.extend(artifact_evidence(run, patterns));
    }
    let item_label = start
        .task_item_id
        .as_deref()
        .and_then(|id| item_labels.get(id).copied())
        .filter(|label| !label.is_empty());
    let mut summary = format!("Step {step_id} is {status}");
    if let Some(label) = item_label {
        summary.push_str(&format!(" for {label}"));
    }
    if let Some(run) = run.filter(|value| !value.agent_id.is_empty()) {
        summary.push_str(&format!(" using {}", run.agent_id));
    }
    let entry = TimelineEntry {
        id: stable_id(&source.task.id, category, &raw_ids, &step_id),
        task_id: source.task.id.clone(),
        occurred_at: finish.unwrap_or(start).created_at.clone(),
        category,
        title: if category == TimelineCategory::Test {
            format!("Test: {step_id}")
        } else {
            format!("Step: {step_id}")
        },
        summary: redact_text(&summary, patterns),
        status: Some(status.to_string()),
        actor: run.map(|value| TimelineActorRef {
            actor_type: "agent".to_string(),
            actor_id: value.agent_id.clone(),
        }),
        step_id: Some(step_id.clone()),
        task_item_id: start.task_item_id.clone(),
        command_run_id: run.map(|value| value.id.clone()),
        session_id: run.and_then(|value| value.session_id.clone()),
        checkpoint_id: payload_string(start, "checkpoint_id"),
        source_event_id: None,
        evidence: evidence.clone(),
        raw_event_ids: raw_ids.clone(),
        projection_version: TIMELINE_PROJECTION_VERSION,
    };
    let mut entries = vec![ProjectedTimelineEntry {
        source_order: event_order(start.id, 0),
        entry,
    }];
    if success == Some(false) {
        let reason = finish
            .map(event_summary)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("Step {step_id} failed"));
        entries.push(ProjectedTimelineEntry {
            source_order: event_order(finish.unwrap_or(start).id, 1),
            entry: TimelineEntry {
                id: stable_id(
                    &source.task.id,
                    TimelineCategory::Failure,
                    &raw_ids,
                    &step_id,
                ),
                task_id: source.task.id.clone(),
                occurred_at: finish.unwrap_or(start).created_at.clone(),
                category: TimelineCategory::Failure,
                title: format!("Failure: {step_id}"),
                summary: redact_text(&reason, patterns),
                status: Some("failed".to_string()),
                actor: run.map(|value| TimelineActorRef {
                    actor_type: "agent".to_string(),
                    actor_id: value.agent_id.clone(),
                }),
                step_id: Some(step_id),
                task_item_id: start.task_item_id.clone(),
                command_run_id: run.map(|value| value.id.clone()),
                session_id: run.and_then(|value| value.session_id.clone()),
                checkpoint_id: None,
                source_event_id: None,
                evidence,
                raw_event_ids: raw_ids,
                projection_version: TIMELINE_PROJECTION_VERSION,
            },
        });
    }
    entries
}

fn project_event(
    source: &TaskTimelineSource,
    event: &EventDto,
    patterns: &[String],
) -> Option<ProjectedTimelineEntry> {
    let category = event_category(&event.event_type);
    let status = event_status(event, category);
    let title = humanize_event_type(&event.event_type);
    let summary = redact_text(&event_summary(event), patterns);
    let actor = payload_string(event, "agent_id").map(|agent_id| TimelineActorRef {
        actor_type: "agent".to_string(),
        actor_id: agent_id,
    });
    let raw_ids = vec![event.id];
    Some(ProjectedTimelineEntry {
        source_order: event_order(event.id, 0),
        entry: TimelineEntry {
            id: stable_id(&source.task.id, category, &raw_ids, &event.event_type),
            task_id: source.task.id.clone(),
            occurred_at: event.created_at.clone(),
            category,
            title,
            summary,
            status,
            actor,
            step_id: event_step(event),
            task_item_id: event.task_item_id.clone(),
            command_run_id: payload_string(event, "command_run_id")
                .or_else(|| payload_string(event, "run_id")),
            session_id: payload_string(event, "session_id"),
            checkpoint_id: payload_string(event, "checkpoint_id"),
            source_event_id: payload_string(event, "source_event_id"),
            evidence: Vec::new(),
            raw_event_ids: raw_ids,
            projection_version: TIMELINE_PROJECTION_VERSION,
        },
    })
}

fn event_category(event_type: &str) -> TimelineCategory {
    if event_type.starts_with("cycle_") {
        TimelineCategory::Cycle
    } else if event_type == "task_completed" || event_type == "task_finished" {
        TimelineCategory::Completion
    } else if event_type.contains("failed")
        || event_type.contains("denied")
        || event_type.contains("blocked")
        || event_type.contains("timeout")
    {
        TimelineCategory::Failure
    } else if event_type.contains("test") || event_type.contains("validation") {
        TimelineCategory::Test
    } else if event_type.starts_with("agent_tool_") {
        TimelineCategory::Tool
    } else if event_type.contains("artifact") || event_type == "agent_run_summary" {
        TimelineCategory::Artifact
    } else if event_type.contains("retry")
        || event_type.contains("recover")
        || event_type.contains("restart")
        || event_type.contains("rollback")
        || event_type.contains("checkpoint")
    {
        TimelineCategory::Recovery
    } else if event_type.starts_with("session_") {
        TimelineCategory::Session
    } else if event_type.starts_with("human_") || event_type.starts_with("approval_") {
        TimelineCategory::HumanAction
    } else if event_type.starts_with("source_") || event_type.starts_with("trigger_") {
        TimelineCategory::Source
    } else {
        TimelineCategory::Lifecycle
    }
}

fn event_status(event: &EventDto, category: TimelineCategory) -> Option<String> {
    payload_string(event, "status").or_else(|| match category {
        TimelineCategory::Failure => Some("failed".to_string()),
        TimelineCategory::Completion => Some("completed".to_string()),
        _ => None,
    })
}

fn event_summary(event: &EventDto) -> String {
    const KEYS: &[&str] = &[
        "message",
        "reason",
        "error",
        "step",
        "step_id",
        "phase",
        "status",
        "success",
        "agent_id",
        "tool_name",
        "duration_ms",
    ];
    let mut parts = Vec::new();
    for key in KEYS {
        if let Some(value) = event.payload.get(*key) {
            if let Some(text) = value.as_str() {
                if !text.is_empty() {
                    parts.push(format!("{key}={text}"));
                }
            } else if value.is_boolean() || value.is_number() {
                parts.push(format!("{key}={value}"));
            }
        }
    }
    if parts.is_empty() {
        humanize_event_type(&event.event_type)
    } else {
        truncate(&parts.join(", "), 320)
    }
}

fn run_evidence(run: &TimelineCommandRunDto) -> Vec<EvidenceRef> {
    vec![EvidenceRef {
        kind: if is_test_phase(&run.phase) {
            "test".to_string()
        } else {
            "command_run".to_string()
        },
        label: format!(
            "{} ({})",
            run.phase,
            run.exit_code
                .map(|value| format!("exit {value}"))
                .unwrap_or_else(|| "running".to_string())
        ),
        uri: Some(format!("orchestrator://runs/{}", run.id)),
        content_type: Some("application/vnd.orchestrator.command-run+json".to_string()),
        digest: None,
        redacted: true,
    }]
}

fn artifact_evidence(run: &TimelineCommandRunDto, patterns: &[String]) -> Vec<EvidenceRef> {
    run.artifacts
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let kind = value
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("artifact");
            EvidenceRef {
                kind: "artifact".to_string(),
                label: redact_text(&format!("{kind} artifact {}", index + 1), patterns),
                uri: None,
                content_type: None,
                digest: None,
                redacted: true,
            }
        })
        .collect()
}

fn matching_run<'a>(
    runs: &'a [TimelineCommandRunDto],
    consumed: &HashSet<String>,
    item_id: Option<&str>,
    step_id: Option<&str>,
) -> Option<&'a TimelineCommandRunDto> {
    runs.iter().find(|run| {
        !consumed.contains(&run.id)
            && item_id == Some(run.task_item_id.as_str())
            && step_id == Some(run.phase.as_str())
    })
}

fn matching_finish_type(start_type: &str) -> Option<&'static str> {
    match start_type {
        "step_started" => Some("step_finished"),
        "chain_step_started" => Some("chain_step_finished"),
        "dynamic_step_started" => Some("dynamic_step_finished"),
        _ => None,
    }
}

fn is_finish_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "step_finished" | "chain_step_finished" | "dynamic_step_finished"
    )
}

fn is_low_value_event(event_type: &str) -> bool {
    event_type.contains("heartbeat")
        || event_type == "step_spawned"
        || event_type == "execution_profile_applied"
}

fn is_test_phase(phase: &str) -> bool {
    let lower = phase.to_ascii_lowercase();
    lower.contains("test")
        || lower.contains("qa")
        || lower.contains("lint")
        || lower.contains("check")
        || lower.contains("retest")
}

fn event_step(event: &EventDto) -> Option<String> {
    payload_string(event, "step").or_else(|| payload_string(event, "step_id"))
}

fn payload_string(event: &EventDto, key: &str) -> Option<String> {
    event
        .payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn humanize_event_type(event_type: &str) -> String {
    let mut words = event_type
        .split('_')
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(first) = words.first_mut()
        && let Some(initial) = first.get_mut(0..1)
    {
        initial.make_ascii_uppercase();
    }
    words.join(" ")
}

fn stable_id(
    task_id: &str,
    category: TimelineCategory,
    raw_event_ids: &[i64],
    discriminator: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!(
        "timeline:v{TIMELINE_PROJECTION_VERSION}:{task_id}:{}:{raw_event_ids:?}:{discriminator}",
        category.as_str()
    ));
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut encoded, byte| {
            let _ = write!(encoded, "{byte:02x}");
            encoded
        })
}

fn event_order(event_id: i64, suborder: u64) -> u64 {
    (event_id.max(0) as u64)
        .saturating_mul(10)
        .saturating_add(suborder)
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut result = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    result.push('…');
    result
}
