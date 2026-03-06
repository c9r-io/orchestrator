use super::anomaly::{detect_overlapping_cycles, find_template_vars};
use super::builder::{build_trace, build_trace_with_meta, split_observed_item_binding};
use super::model::*;
use super::render::{colorize_status, extract_time, format_duration, render_trace_terminal};
use super::time::parse_trace_timestamp;
use crate::anomaly::Severity;
use crate::dto::{CommandRunDto, EventDto};
use crate::events::ObservedStepScope;
use serde_json::{json, Value};

fn make_task_meta<'a>(
    status: &'a str,
    started_at: Option<&'a str>,
    completed_at: Option<&'a str>,
) -> TraceTaskMeta<'a> {
    TraceTaskMeta {
        task_id: "test-task",
        status,
        created_at: "2025-01-01T10:00:00+00:00",
        started_at,
        completed_at,
        updated_at: completed_at.unwrap_or("2025-01-01T10:00:00+00:00"),
    }
}

fn make_event(id: i64, event_type: &str, payload: Value, created_at: &str) -> EventDto {
    let mut payload = payload;
    if matches!(
        event_type,
        "step_started"
            | "step_finished"
            | "step_skipped"
            | "chain_step_started"
            | "chain_step_finished"
            | "dynamic_step_started"
            | "dynamic_step_finished"
            | "step_heartbeat"
    ) && payload.get("step_scope").is_none()
    {
        payload["step_scope"] = json!("task");
    }
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
    let mut payload = payload;
    if matches!(
        event_type,
        "step_started"
            | "step_finished"
            | "step_skipped"
            | "chain_step_started"
            | "chain_step_finished"
            | "dynamic_step_started"
            | "dynamic_step_finished"
            | "step_heartbeat"
    ) && payload.get("step_scope").is_none()
    {
        payload["step_scope"] = json!("item");
    }
    EventDto {
        id,
        task_id: "test-task".to_string(),
        task_item_id: Some(item_id.to_string()),
        event_type: event_type.to_string(),
        payload,
        created_at: created_at.to_string(),
    }
}

fn make_run(phase: &str, item_id: &str, exit_code: Option<i64>, agent_id: &str) -> CommandRunDto {
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
    assert_eq!(
        dup.expect("duplicate_runner anomaly should exist").severity,
        Severity::Error
    );
}

#[test]
fn detect_overlapping_cycles_anomaly() {
    let cycles = vec![
        CycleTrace {
            cycle: 1,
            started_at: Some("2025-01-01T10:00:00+00:00".to_string()),
            ended_at: Some("2025-01-01T10:00:10+00:00".to_string()),
            steps: vec![],
        },
        CycleTrace {
            cycle: 2,
            started_at: Some("2025-01-01T10:00:05+00:00".to_string()),
            ended_at: Some("2025-01-01T10:00:20+00:00".to_string()),
            steps: vec![],
        },
    ];

    let mut anomalies = Vec::new();
    detect_overlapping_cycles(&cycles, &mut anomalies);
    let overlap = anomalies.iter().find(|a| a.rule == "overlapping_cycles");
    assert!(overlap.is_some(), "should detect overlapping cycles");
    assert_eq!(
        overlap
            .expect("overlapping_cycles anomaly should exist")
            .severity,
        Severity::Error
    );
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
    assert!(tmpl
        .expect("unexpanded_template_var anomaly should exist")
        .message
        .contains("{plan_output}"));
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
    let long = trace.anomalies.iter().find(|a| a.rule == "long_running");
    assert!(long.is_some(), "should detect long running step");
}

#[test]
fn detect_low_output_step_anomaly() {
    let events = vec![
        make_item_event(
            1,
            "step_started",
            json!({"step": "plan"}),
            "2025-01-01 10:00:00",
            "item-1",
        ),
        make_item_event(
            2,
            "step_heartbeat",
            json!({
                "step": "plan",
                "output_state": "low_output",
                "pid_alive": true,
                "elapsed_secs": 120,
                "stagnant_heartbeats": 3
            }),
            "2025-01-01 10:02:00",
            "item-1",
        ),
    ];

    let trace = build_trace("test-task", "running", &events, &[]);
    let low_output = trace.anomalies.iter().find(|a| a.rule == "low_output");
    assert!(low_output.is_some(), "should detect low output step");
}

#[test]
fn quiet_heartbeat_does_not_create_low_output_anomaly() {
    let events = vec![make_item_event(
        1,
        "step_heartbeat",
        json!({
            "step": "plan",
            "output_state": "quiet",
            "pid_alive": true,
            "elapsed_secs": 60,
            "stagnant_heartbeats": 2
        }),
        "2025-01-01 10:01:00",
        "item-1",
    )];

    let trace = build_trace("test-task", "running", &events, &[]);
    assert!(
        trace.anomalies.iter().all(|a| a.rule != "low_output"),
        "quiet heartbeat should not create low output anomaly"
    );
}

#[test]
fn multiple_low_output_heartbeats_for_same_step_deduplicate() {
    let events = vec![
        make_item_event(
            1,
            "step_heartbeat",
            json!({
                "step": "plan",
                "output_state": "low_output",
                "pid_alive": true,
                "elapsed_secs": 120,
                "stagnant_heartbeats": 3
            }),
            "2025-01-01 10:02:00",
            "item-1",
        ),
        make_item_event(
            2,
            "step_heartbeat",
            json!({
                "step": "plan",
                "output_state": "low_output",
                "pid_alive": true,
                "elapsed_secs": 150,
                "stagnant_heartbeats": 4
            }),
            "2025-01-01 10:02:30",
            "item-1",
        ),
    ];

    let trace = build_trace("test-task", "running", &events, &[]);
    let count = trace
        .anomalies
        .iter()
        .filter(|a| a.rule == "low_output")
        .count();
    assert_eq!(
        count, 1,
        "same step should only emit one low_output anomaly"
    );
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
    let wall = trace
        .summary
        .wall_time_secs
        .expect("wall time should be computed");
    assert!(
        (wall - 272.0).abs() < 1.0,
        "wall time should be ~272s, got {}",
        wall
    );
}

#[test]
fn two_cycle_completed_task_closes_first_cycle_without_overlap() {
    let events = vec![
        make_event(
            1,
            "cycle_started",
            json!({"cycle": 1}),
            "2026-03-01T04:00:00.000000+00:00",
        ),
        make_item_event(
            2,
            "step_started",
            json!({"step": "implement"}),
            "2026-03-01T04:00:01.000000+00:00",
            "item-1",
        ),
        make_item_event(
            3,
            "step_finished",
            json!({"step": "implement", "success": true}),
            "2026-03-01T04:00:10.000000+00:00",
            "item-1",
        ),
        make_item_event(
            4,
            "step_skipped",
            json!({"step": "align_tests", "reason": "prehook_false"}),
            "2026-03-01T04:00:12.000000+00:00",
            "item-1",
        ),
        make_event(
            5,
            "cycle_started",
            json!({"cycle": 2}),
            "2026-03-01T04:00:13.000000+00:00",
        ),
        make_item_event(
            6,
            "step_started",
            json!({"step": "implement"}),
            "2026-03-01T04:00:14.000000+00:00",
            "item-1",
        ),
        make_item_event(
            7,
            "step_finished",
            json!({"step": "implement", "success": true}),
            "2026-03-01T04:00:20.000000+00:00",
            "item-1",
        ),
        make_event(
            8,
            "task_completed",
            json!({}),
            "2026-03-01T04:00:21.000000+00:00",
        ),
    ];

    let trace = build_trace_with_meta(
        make_task_meta(
            "completed",
            Some("2026-03-01T04:00:00.000000+00:00"),
            Some("2026-03-01T04:00:21.000000+00:00"),
        ),
        &events,
        &[],
    );

    assert_eq!(trace.cycles.len(), 2);
    assert_eq!(
        trace.cycles[0].ended_at.as_deref(),
        Some("2026-03-01T04:00:12.000000+00:00")
    );
    assert_eq!(
        trace.cycles[1].ended_at.as_deref(),
        Some("2026-03-01T04:00:21.000000+00:00")
    );
    assert!(
        trace
            .anomalies
            .iter()
            .all(|a| a.rule != "overlapping_cycles"),
        "unexpected overlap anomaly: {:?}",
        trace.anomalies
    );
}

#[test]
fn completed_task_wall_time_uses_task_meta_when_events_are_sparse() {
    let events = vec![make_event(
        1,
        "cycle_started",
        json!({"cycle": 1}),
        "2026-03-01T04:07:03.635397+00:00",
    )];

    let trace = build_trace_with_meta(
        make_task_meta(
            "completed",
            Some("2026-03-01T04:07:03.635397+00:00"),
            Some("2026-03-01T04:09:38.477325+00:00"),
        ),
        &events,
        &[],
    );

    let wall = trace
        .summary
        .wall_time_secs
        .expect("completed task should have wall time");
    assert!(
        (wall - 154.842).abs() < 0.01,
        "unexpected wall time: {}",
        wall
    );
}

#[test]
fn parse_trace_timestamp_accepts_rfc3339_offset() {
    let parsed = parse_trace_timestamp("2026-03-01T04:09:38.477325+00:00");
    assert!(parsed.is_some(), "should parse RFC3339 with offset");
}

#[test]
fn completed_task_backfills_last_cycle_end_from_completed_at() {
    let events = vec![make_event(
        1,
        "cycle_started",
        json!({"cycle": 1}),
        "2026-03-01T04:00:00.000000+00:00",
    )];

    let trace = build_trace_with_meta(
        make_task_meta(
            "completed",
            Some("2026-03-01T04:00:00.000000+00:00"),
            Some("2026-03-01T04:00:30.000000+00:00"),
        ),
        &events,
        &[],
    );

    assert_eq!(
        trace.cycles[0].ended_at.as_deref(),
        Some("2026-03-01T04:00:30.000000+00:00")
    );
}

#[test]
fn build_trace_marks_task_scoped_step_with_anchor_item() {
    let events = vec![
        make_event(
            1,
            "cycle_started",
            json!({"cycle": 1}),
            "2026-03-01T04:00:00.000000+00:00",
        ),
        make_item_event(
            2,
            "step_started",
            json!({"step": "plan", "step_scope": "task"}),
            "2026-03-01T04:00:01.000000+00:00",
            "item-1",
        ),
        make_item_event(
            3,
            "step_finished",
            json!({"step": "plan", "step_scope": "task", "success": true}),
            "2026-03-01T04:00:02.000000+00:00",
            "item-1",
        ),
        make_event(
            4,
            "task_completed",
            json!({}),
            "2026-03-01T04:00:03.000000+00:00",
        ),
    ];

    let trace = build_trace("test-task", "completed", &events, &[]);
    let step = &trace.cycles[0].steps[0];
    assert_eq!(step.scope, "task");
    assert_eq!(step.item_id, None);
    assert_eq!(step.anchor_item_id.as_deref(), Some("item-1"));
}

#[test]
fn build_trace_marks_legacy_step_scope_as_legacy() {
    let events = vec![
        make_event(
            1,
            "cycle_started",
            json!({"cycle": 1}),
            "2026-03-01T04:00:00.000000+00:00",
        ),
        EventDto {
            id: 2,
            task_id: "test-task".to_string(),
            task_item_id: Some("item-1".to_string()),
            event_type: "step_started".to_string(),
            payload: json!({"step": "plan"}),
            created_at: "2026-03-01T04:00:01.000000+00:00".to_string(),
        },
        EventDto {
            id: 3,
            task_id: "test-task".to_string(),
            task_item_id: Some("item-1".to_string()),
            event_type: "step_finished".to_string(),
            payload: json!({"step": "plan", "success": true}),
            created_at: "2026-03-01T04:00:02.000000+00:00".to_string(),
        },
        make_event(
            4,
            "task_completed",
            json!({}),
            "2026-03-01T04:00:03.000000+00:00",
        ),
    ];

    let trace = build_trace("test-task", "completed", &events, &[]);
    let step = &trace.cycles[0].steps[0];
    assert_eq!(step.scope, "legacy");
    assert_eq!(step.item_id, None);
    assert_eq!(step.anchor_item_id.as_deref(), Some("item-1"));
}

// ── Build version tests ────────────────────────────────────────

#[test]
fn build_trace_includes_build_version() {
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
    assert!(
        trace.build_version.is_some(),
        "build_version should be populated"
    );
}

#[test]
fn build_version_fields_populated() {
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
    let bv = trace.build_version.expect("should have build version");
    assert!(
        !bv.version.is_empty(),
        "version should be populated"
    );
    assert!(
        !bv.git_hash.is_empty(),
        "git_hash should be populated"
    );
    assert!(
        !bv.build_timestamp.is_empty(),
        "build_timestamp should be populated"
    );
}

#[test]
fn json_serialization_includes_build_version() {
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
    let json_str = serde_json::to_string(&trace).expect("should serialize");
    let parsed: Value = serde_json::from_str(&json_str).expect("should parse");

    assert!(
        parsed.get("build_version").is_some(),
        "JSON should contain build_version"
    );
    let bv = parsed.get("build_version").expect("build_version should exist");
    assert!(
        bv.get("version").is_some() && !bv.get("version").unwrap().is_null(),
        "version should be in JSON"
    );
    assert!(
        bv.get("git_hash").is_some() && !bv.get("git_hash").unwrap().is_null(),
        "git_hash should be in JSON"
    );
    assert!(
        bv.get("build_timestamp").is_some()
            && !bv.get("build_timestamp").unwrap().is_null(),
        "build_timestamp should be in JSON"
    );
}

#[test]
fn render_trace_terminal_shows_build_version() {
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
    // Just ensure it doesn't panic - output goes to stdout
    render_trace_terminal(&trace, false);
}

#[test]
fn build_version_optional_backward_compat() {
    // Test that the trace works even when build version might be absent
    // (simulating older versions or different build configs)
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

    let trace = build_trace("test-task", "completed", &events, &[]);
    // Trace should still have valid cycles and summary
    assert_eq!(trace.cycles.len(), 1);
    assert_eq!(trace.cycles[0].steps.len(), 1);
    assert_eq!(trace.summary.total_steps, 1);
    // Build version should still be populated
    assert!(trace.build_version.is_some());
}

// ── Utility function tests ────────────────────────────

#[test]
fn format_duration_seconds_only() {
    assert_eq!(format_duration(42.0), "42s");
    assert_eq!(format_duration(0.0), "0s");
    assert_eq!(format_duration(59.0), "59s");
}

#[test]
fn format_duration_minutes_and_seconds() {
    assert_eq!(format_duration(60.0), "1m 00s");
    assert_eq!(format_duration(125.0), "2m 05s");
    assert_eq!(format_duration(3599.0), "59m 59s");
}

#[test]
fn format_duration_hours() {
    assert_eq!(format_duration(3600.0), "1h 00m 00s");
    assert_eq!(format_duration(3661.0), "1h 01m 01s");
    assert_eq!(format_duration(7200.0), "2h 00m 00s");
}

#[test]
fn extract_time_from_rfc3339() {
    assert_eq!(extract_time("2025-01-01T10:30:45+00:00"), "10:30:45");
}

#[test]
fn extract_time_from_space_separated() {
    assert_eq!(extract_time("2025-01-01 10:30:45"), "10:30:45");
}

#[test]
fn extract_time_from_no_separator() {
    assert_eq!(extract_time("10:30:45"), "10:30:45");
}

#[test]
fn colorize_status_known_values() {
    assert!(colorize_status("completed").contains("32m")); // green
    assert!(colorize_status("failed").contains("31m")); // red
    assert!(colorize_status("running").contains("33m")); // yellow
    assert!(colorize_status("paused").contains("90m")); // gray
}

#[test]
fn colorize_status_unknown_returns_plain() {
    assert_eq!(colorize_status("pending"), "pending");
}

#[test]
fn find_template_vars_basic() {
    let vars = find_template_vars("echo {rel_path} --out {output}");
    assert_eq!(vars, vec!["{rel_path}", "{output}"]);
}

#[test]
fn find_template_vars_no_matches() {
    let vars = find_template_vars("echo hello world");
    assert!(vars.is_empty());
}

#[test]
fn find_template_vars_ignores_uppercase_and_numbers() {
    let vars = find_template_vars("{OK} {test123} {valid_one}");
    assert_eq!(vars, vec!["{valid_one}"]);
}

#[test]
fn find_template_vars_empty_braces_ignored() {
    let vars = find_template_vars("echo {} test");
    assert!(vars.is_empty());
}

#[test]
fn parse_trace_timestamp_naive_datetime_format() {
    let parsed = parse_trace_timestamp("2025-01-01 10:00:00");
    assert!(parsed.is_some(), "should parse space-separated datetime");
}

#[test]
fn parse_trace_timestamp_iso_without_offset() {
    let parsed = parse_trace_timestamp("2025-01-01T10:00:00");
    assert!(parsed.is_some(), "should parse ISO datetime without offset");
}

#[test]
fn parse_trace_timestamp_with_fractional_seconds() {
    let parsed = parse_trace_timestamp("2025-01-01T10:00:00.123456");
    assert!(
        parsed.is_some(),
        "should parse datetime with fractional seconds"
    );
}

#[test]
fn parse_trace_timestamp_garbage_returns_none() {
    let parsed = parse_trace_timestamp("not-a-timestamp");
    assert!(parsed.is_none());
}

#[test]
fn split_observed_item_binding_all_variants() {
    let item = Some("item-1".to_string());

    let (scope, item_id, anchor) =
        split_observed_item_binding(Some(ObservedStepScope::Item), &item);
    assert_eq!(scope, "item");
    assert_eq!(item_id.as_deref(), Some("item-1"));
    assert!(anchor.is_none());

    let (scope, item_id, anchor) =
        split_observed_item_binding(Some(ObservedStepScope::Task), &item);
    assert_eq!(scope, "task");
    assert!(item_id.is_none());
    assert_eq!(anchor.as_deref(), Some("item-1"));

    let (scope, item_id, anchor) = split_observed_item_binding(None, &item);
    assert_eq!(scope, "legacy");
    assert!(item_id.is_none());
    assert_eq!(anchor.as_deref(), Some("item-1"));
}

#[test]
fn step_started_before_cycle_started_auto_creates_cycle_zero() {
    let events = vec![
        make_item_event(
            1,
            "step_started",
            json!({"step": "plan"}),
            "2025-01-01 10:00:01",
            "item-1",
        ),
        make_item_event(
            2,
            "step_finished",
            json!({"step": "plan", "success": true, "duration_ms": 3000}),
            "2025-01-01 10:00:04",
            "item-1",
        ),
    ];

    let trace = build_trace("test-task", "completed", &events, &[]);
    assert_eq!(trace.cycles.len(), 1);
    assert_eq!(trace.cycles[0].cycle, 0);
    assert_eq!(trace.cycles[0].steps.len(), 1);
}

#[test]
fn detect_empty_cycle_between_two_cycles() {
    let events = vec![
        make_event(
            1,
            "cycle_started",
            json!({"cycle": 1}),
            "2025-01-01 10:00:00",
        ),
        // No steps in cycle 1
        make_event(
            2,
            "cycle_started",
            json!({"cycle": 2}),
            "2025-01-01 10:01:00",
        ),
        make_item_event(
            3,
            "step_started",
            json!({"step": "plan"}),
            "2025-01-01 10:01:01",
            "item-1",
        ),
        make_item_event(
            4,
            "step_finished",
            json!({"step": "plan", "success": true}),
            "2025-01-01 10:01:05",
            "item-1",
        ),
        make_event(5, "task_completed", json!({}), "2025-01-01 10:01:06"),
    ];

    let trace = build_trace("test-task", "completed", &events, &[]);
    let empty = trace.anomalies.iter().find(|a| a.rule == "empty_cycle");
    assert!(empty.is_some(), "should detect empty cycle 1");
    assert!(empty
        .expect("empty cycle anomaly")
        .message
        .contains("Cycle 1"));
}

#[test]
fn detect_missing_step_end_when_no_terminal_event() {
    // Steps started but no task_completed/task_failed follows
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
        // No step_finished, no task_completed
    ];

    let trace = build_trace("test-task", "running", &events, &[]);
    let missing = trace
        .anomalies
        .iter()
        .find(|a| a.rule == "missing_step_end");
    assert!(
        missing.is_some(),
        "should detect missing step end without terminal event"
    );
}

#[test]
fn low_output_heartbeat_with_dead_process_not_flagged() {
    let events = vec![make_item_event(
        1,
        "step_heartbeat",
        json!({
            "step": "plan",
            "output_state": "low_output",
            "pid_alive": false,
            "elapsed_secs": 120,
            "stagnant_heartbeats": 4
        }),
        "2025-01-01 10:02:00",
        "item-1",
    )];

    let trace = build_trace("test-task", "running", &events, &[]);
    assert!(
        trace.anomalies.iter().all(|a| a.rule != "low_output"),
        "low_output should not be flagged when process is dead"
    );
}

#[test]
fn chain_and_dynamic_step_events_handled() {
    let events = vec![
        make_event(
            1,
            "cycle_started",
            json!({"cycle": 1}),
            "2025-01-01 10:00:00",
        ),
        make_item_event(
            2,
            "chain_step_started",
            json!({"step": "qa_chain"}),
            "2025-01-01 10:00:01",
            "item-1",
        ),
        make_item_event(
            3,
            "chain_step_finished",
            json!({"step": "qa_chain", "success": true, "duration_ms": 1000}),
            "2025-01-01 10:00:02",
            "item-1",
        ),
        make_item_event(
            4,
            "dynamic_step_started",
            json!({"step": "dynamic_fix"}),
            "2025-01-01 10:00:03",
            "item-1",
        ),
        make_item_event(
            5,
            "dynamic_step_finished",
            json!({"step": "dynamic_fix", "success": false, "duration_ms": 2000}),
            "2025-01-01 10:00:05",
            "item-1",
        ),
        make_event(6, "task_completed", json!({}), "2025-01-01 10:00:06"),
    ];

    let trace = build_trace("test-task", "completed", &events, &[]);
    assert_eq!(trace.cycles[0].steps.len(), 2);
    assert_eq!(trace.cycles[0].steps[0].step_id, "qa_chain");
    assert_eq!(trace.cycles[0].steps[0].exit_code, Some(0));
    assert_eq!(trace.cycles[0].steps[1].step_id, "dynamic_fix");
    assert_eq!(trace.cycles[0].steps[1].exit_code, Some(1));
}

#[test]
fn nonzero_exit_code_minus_one_not_flagged() {
    // exit_code -1 means "still running" and should not be flagged
    let runs = vec![make_run("implement", "item-1", Some(-1), "agent-1")];
    let trace = build_trace("test-task", "running", &[], &runs);
    assert!(
        trace.anomalies.iter().all(|a| a.rule != "nonzero_exit"),
        "-1 exit code should not be flagged as nonzero_exit"
    );
}

#[test]
fn summary_counts_failed_commands() {
    let runs = vec![
        make_run("plan", "item-1", Some(0), "agent-1"),
        make_run("implement", "item-1", Some(1), "agent-2"),
        make_run("qa", "item-1", Some(2), "agent-3"),
    ];

    let trace = build_trace("test-task", "failed", &[], &runs);
    assert_eq!(trace.summary.total_commands, 3);
    assert_eq!(trace.summary.failed_commands, 2);
}

#[test]
fn wall_time_uses_started_at_from_meta() {
    let events = vec![
        make_event(
            1,
            "cycle_started",
            json!({"cycle": 1}),
            "2025-01-01T10:00:05+00:00",
        ),
        make_event(2, "task_completed", json!({}), "2025-01-01T10:00:30+00:00"),
    ];

    let trace = build_trace_with_meta(
        make_task_meta(
            "completed",
            Some("2025-01-01T10:00:00+00:00"),
            Some("2025-01-01T10:00:30+00:00"),
        ),
        &events,
        &[],
    );
    let wall = trace.summary.wall_time_secs.expect("should have wall time");
    assert!(
        (wall - 30.0).abs() < 0.01,
        "wall time should use meta.started_at, got {}",
        wall
    );
}

#[test]
fn render_trace_terminal_does_not_panic() {
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
            json!({"step": "plan", "success": false, "duration_ms": 5000, "agent_id": "a1"}),
            "2025-01-01 10:00:06",
            "item-1",
        ),
        make_item_event(
            4,
            "step_skipped",
            json!({"step": "qa", "reason": "disabled"}),
            "2025-01-01 10:00:07",
            "item-1",
        ),
        make_event(5, "task_failed", json!({}), "2025-01-01 10:00:08"),
    ];
    let runs = vec![make_run("plan", "item-1", Some(1), "a1")];

    let trace = build_trace("test-task", "failed", &events, &runs);
    // Just ensure it doesn't panic - output goes to stdout
    render_trace_terminal(&trace, false);
    render_trace_terminal(&trace, true);
}
