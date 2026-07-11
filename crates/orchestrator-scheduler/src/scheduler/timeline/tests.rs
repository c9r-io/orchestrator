use agent_orchestrator::dto::{
    EventDto, TaskItemDto, TaskSummary, TaskTimelineSource, TimelineCommandRunDto,
};
use serde_json::json;

use super::*;

fn source() -> TaskTimelineSource {
    TaskTimelineSource {
        task: TaskSummary {
            id: "task-1".to_string(),
            name: "timeline fixture".to_string(),
            status: "failed".to_string(),
            started_at: Some("2026-01-01T00:00:01Z".to_string()),
            completed_at: Some("2026-01-01T00:00:05Z".to_string()),
            goal: "repair the failing test".to_string(),
            project_id: "default".to_string(),
            workspace_id: "default".to_string(),
            workflow_id: "qa".to_string(),
            target_files: Vec::new(),
            total_items: 1,
            finished_items: 0,
            failed_items: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:05Z".to_string(),
            parent_task_id: None,
            spawn_reason: None,
            spawn_depth: 0,
        },
        items: vec![TaskItemDto {
            id: "item-1".to_string(),
            task_id: "task-1".to_string(),
            order_no: 1,
            qa_file_path: "docs/qa/example.md".to_string(),
            status: "qa_failed".to_string(),
            ticket_files: Vec::new(),
            ticket_content: Vec::new(),
            fix_required: true,
            fixed: false,
            last_error: "tests failed".to_string(),
            started_at: None,
            completed_at: None,
            updated_at: "2026-01-01T00:00:05Z".to_string(),
        }],
        runs: vec![TimelineCommandRunDto {
            id: "run-1".to_string(),
            task_item_id: "item-1".to_string(),
            phase: "self_test".to_string(),
            agent_id: "tester".to_string(),
            exit_code: Some(1),
            started_at: "2026-01-01T00:00:02Z".to_string(),
            ended_at: Some("2026-01-01T00:00:04Z".to_string()),
            interrupted: false,
            validation_status: "valid".to_string(),
            artifacts: vec![json!({"kind":"test_report","path":"secret/path"})],
            session_id: Some("session-1".to_string()),
            machine_output_source: "shell".to_string(),
            output_json_path: None,
        }],
        events: vec![
            EventDto {
                id: 1,
                task_id: "task-1".to_string(),
                task_item_id: None,
                event_type: "cycle_started".to_string(),
                payload: json!({"cycle":1}),
                created_at: "2026-01-01T00:00:01Z".to_string(),
            },
            EventDto {
                id: 2,
                task_id: "task-1".to_string(),
                task_item_id: Some("item-1".to_string()),
                event_type: "step_started".to_string(),
                payload: json!({"step":"self_test"}),
                created_at: "2026-01-01T00:00:02Z".to_string(),
            },
            EventDto {
                id: 3,
                task_id: "task-1".to_string(),
                task_item_id: Some("item-1".to_string()),
                event_type: "step_finished".to_string(),
                payload: json!({"step":"self_test","success":false,"error":"token=secret-value"}),
                created_at: "2026-01-01T00:00:04Z".to_string(),
            },
            EventDto {
                id: 4,
                task_id: "task-1".to_string(),
                task_item_id: None,
                event_type: "task_failed".to_string(),
                payload: json!({"reason":"self test failed"}),
                created_at: "2026-01-01T00:00:05Z".to_string(),
            },
        ],
        snapshot_max_event_id: 4,
    }
}

#[test]
fn failed_workflow_projects_goal_test_failure_and_lifecycle() {
    let page = build_timeline_page(
        &source(),
        &TimelineQuery {
            cursor: None,
            limit: 50,
            categories: Vec::new(),
        },
        &["secret-value".to_string()],
    )
    .unwrap();
    let categories = page
        .entries
        .iter()
        .map(|entry| entry.category)
        .collect::<Vec<_>>();
    assert!(categories.contains(&TimelineCategory::Goal));
    assert!(categories.contains(&TimelineCategory::Cycle));
    assert!(categories.contains(&TimelineCategory::Test));
    assert!(categories.contains(&TimelineCategory::Failure));
    let test = page
        .entries
        .iter()
        .find(|entry| entry.category == TimelineCategory::Test)
        .unwrap();
    assert_eq!(test.command_run_id.as_deref(), Some("run-1"));
    assert!(!test.evidence.is_empty());
}

#[test]
fn projection_is_deterministic_and_redacted() {
    let query = TimelineQuery {
        cursor: None,
        limit: 50,
        categories: Vec::new(),
    };
    let first = build_timeline_page(&source(), &query, &["secret-value".to_string()]).unwrap();
    let second = build_timeline_page(&source(), &query, &["secret-value".to_string()]).unwrap();
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    assert!(
        !serde_json::to_string(&first)
            .unwrap()
            .contains("secret-value")
    );
    assert!(
        !serde_json::to_string(&first)
            .unwrap()
            .contains("secret/path")
    );
}

#[test]
fn cursor_pagination_has_no_duplicates_or_omissions() {
    let first = build_timeline_page(
        &source(),
        &TimelineQuery {
            cursor: None,
            limit: 2,
            categories: Vec::new(),
        },
        &[],
    )
    .unwrap();
    assert!(first.has_more);
    let second = build_timeline_page(
        &source(),
        &TimelineQuery {
            cursor: first.next_cursor.clone(),
            limit: 20,
            categories: Vec::new(),
        },
        &[],
    )
    .unwrap();
    let mut ids = first
        .entries
        .iter()
        .chain(second.entries.iter())
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), total);
    assert!(!second.has_more);
}

#[test]
fn legacy_missing_optional_fields_remains_projectable() {
    let mut legacy = source();
    legacy.events[1].payload = json!({});
    let page = build_timeline_page(
        &legacy,
        &TimelineQuery {
            cursor: None,
            limit: 50,
            categories: Vec::new(),
        },
        &[],
    )
    .unwrap();
    assert!(!page.entries.is_empty());
}

#[test]
fn category_filter_is_validated() {
    let err = build_timeline_page(
        &source(),
        &TimelineQuery {
            cursor: None,
            limit: 50,
            categories: vec!["nope".to_string()],
        },
        &[],
    )
    .unwrap_err();
    assert!(err.to_string().contains("unknown timeline category"));
}
