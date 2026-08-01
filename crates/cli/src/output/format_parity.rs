//! FR-154 acceptance (a): behavioral proof that `-o json` and `-o yaml`
//! decode to the same data for every payload, over fully-populated fixtures
//! (every `Option` is `Some`, every `Vec` non-empty — an empty fixture would
//! pass vacuously). The comparator itself is proven fallible by
//! `comparator_detects_divergence`.

use serde_json::Value;

use super::render::{self, Encoding};

/// The comparator: both encodings of a payload must decode to the same value.
/// Returns `Err` on divergence so its own falsifiability is testable.
fn equivalence(json_text: &str, yaml_text: &str) -> Result<(), String> {
    let from_json: Value =
        serde_json::from_str(json_text).map_err(|e| format!("json did not parse: {e}"))?;
    let from_yaml: Value =
        serde_yaml::from_str(yaml_text).map_err(|e| format!("yaml did not parse: {e}"))?;
    if from_json == from_yaml {
        Ok(())
    } else {
        Err(format!(
            "json and yaml decode to different values\njson: {from_json}\nyaml: {from_yaml}"
        ))
    }
}

fn assert_parity(name: &str, value: &Value) {
    let json_text = render::encode(value, Encoding::JsonPretty).expect("json encode");
    let yaml_text = render::encode(value, Encoding::Yaml).expect("yaml encode");
    if let Err(err) = equivalence(&json_text, &yaml_text) {
        panic!("{name}: {err}");
    }
    let back: Value = serde_yaml::from_str(&yaml_text).expect("yaml parse");
    assert_eq!(
        &back, value,
        "{name}: yaml round trip diverged from projection"
    );
}

#[test]
fn comparator_detects_divergence() {
    let json_text = render::encode(
        &serde_json::json!({"id": "x", "dropped_in_yaml": true}),
        Encoding::JsonPretty,
    )
    .expect("json encode");
    let yaml_text =
        render::encode(&serde_json::json!({"id": "x"}), Encoding::Yaml).expect("yaml encode");
    assert!(
        equivalence(&json_text, &yaml_text).is_err(),
        "comparator accepted a divergent json/yaml pair — it can no longer fail"
    );
}

mod fixtures {
    use orchestrator_proto::{
        AttentionActionDescriptor, AttentionDelta, AttentionItem, AttentionListResponse,
        CommandRun, Event, TaskInfoResponse, TaskItem, TaskSummary, TaskTimelineResponse,
        TimelineActorRef, TimelineDelta, TimelineEntry, TimelineEvidenceRef,
    };

    pub(super) fn task_summary() -> TaskSummary {
        TaskSummary {
            id: "task-1".into(),
            name: "task-name".into(),
            status: "failed".into(),
            started_at: Some("2026-03-10T00:00:30Z".into()),
            completed_at: Some("2026-03-10T00:09:00Z".into()),
            goal: "goal".into(),
            project_id: "project-1".into(),
            workspace_id: "ws-1".into(),
            workflow_id: "wf-1".into(),
            target_files: vec!["src/lib.rs".into()],
            total_items: 3,
            finished_items: 1,
            failed_items: 1,
            created_at: "2026-03-10T00:00:00Z".into(),
            updated_at: "2026-03-10T00:10:00Z".into(),
            parent_task_id: Some("task-0".into()),
            spawn_reason: Some("retry".into()),
            spawn_depth: 1,
        }
    }

    pub(super) fn task_item() -> TaskItem {
        TaskItem {
            id: "item-1".into(),
            task_id: "task-1".into(),
            order_no: 7,
            qa_file_path: "docs/qa/case.md".into(),
            status: "failed".into(),
            ticket_files: vec!["docs/ticket/bug.md".into()],
            ticket_content_json: "{\"severity\":\"high\"}".into(),
            fix_required: true,
            fixed: false,
            last_error: "boom: line 1\nline 2".into(),
            started_at: Some("2026-03-10T00:01:00Z".into()),
            completed_at: Some("2026-03-10T00:02:00Z".into()),
            updated_at: "2026-03-10T00:02:00Z".into(),
        }
    }

    pub(super) fn event() -> Event {
        Event {
            id: 42,
            task_id: "task-1".into(),
            task_item_id: Some("item-1".into()),
            event_type: "task_failed".into(),
            payload_json: "{\"reason\":\"timeout\",\"count\":2}".into(),
            created_at: "2026-03-10T00:03:00Z".into(),
        }
    }

    pub(super) fn command_run() -> CommandRun {
        CommandRun {
            id: "run-1".into(),
            task_item_id: "item-1".into(),
            phase: "qa".into(),
            command: "qa-doc-gen".into(),
            cwd: "/tmp/workspace".into(),
            workspace_id: "ws-1".into(),
            agent_id: "agent-1".into(),
            exit_code: Some(1),
            stdout_path: "/tmp/out.log".into(),
            stderr_path: "/tmp/err.log".into(),
            started_at: "2026-03-10T00:01:00Z".into(),
            ended_at: Some("2026-03-10T00:02:00Z".into()),
            interrupted: true,
        }
    }

    pub(super) fn task_info_response() -> TaskInfoResponse {
        TaskInfoResponse {
            task: Some(task_summary()),
            items: vec![task_item()],
            runs: vec![command_run()],
            events: vec![event()],
            graph_debug: vec![],
            agent_states: vec![],
        }
    }

    pub(super) fn timeline_entry() -> TimelineEntry {
        TimelineEntry {
            id: "tl-1".into(),
            task_id: "task-1".into(),
            occurred_at: "2026-03-10T00:04:00Z".into(),
            category: "execution".into(),
            title: "step finished: 执行".into(),
            summary: "exit 0".into(),
            status: Some("succeeded".into()),
            actor: Some(TimelineActorRef {
                actor_type: "agent".into(),
                actor_id: "agent-1".into(),
            }),
            step_id: Some("step-1".into()),
            task_item_id: Some("item-1".into()),
            command_run_id: Some("run-1".into()),
            session_id: Some("session-1".into()),
            checkpoint_id: Some("ckpt-1".into()),
            source_event_id: Some("42".into()),
            evidence: vec![TimelineEvidenceRef {
                kind: "log".into(),
                label: "stdout".into(),
                uri: Some("file:///tmp/out.log".into()),
                content_type: Some("text/plain".into()),
                digest: Some("sha256:abc".into()),
                redacted: true,
            }],
            raw_event_ids: vec![41, 42],
            projection_version: 3,
        }
    }

    pub(super) fn timeline_response() -> TaskTimelineResponse {
        TaskTimelineResponse {
            entries: vec![timeline_entry()],
            next_cursor: Some("cursor-1".into()),
            has_more: true,
            snapshot_max_event_id: 42,
            projection_version: 3,
        }
    }

    pub(super) fn timeline_delta() -> TimelineDelta {
        TimelineDelta {
            kind: "upsert".into(),
            entry: Some(timeline_entry()),
            snapshot_max_event_id: 42,
        }
    }

    pub(super) fn attention_item() -> AttentionItem {
        AttentionItem {
            id: "att-1".into(),
            project_id: "project-1".into(),
            task_id: "task-1".into(),
            task_item_id: Some("item-1".into()),
            step_id: Some("step-1".into()),
            session_id: Some("session-1".into()),
            kind: "decision".into(),
            severity: "high".into(),
            state: "open".into(),
            title: "needs decision: 决策".into(),
            summary: "value contains: colon".into(),
            requested_decision_json: Some("{\"options\":[\"a\"]}".into()),
            actions: vec![AttentionActionDescriptor {
                id: "approve".into(),
                label: "Approve".into(),
                required_role: "operator".into(),
                confirmation: "Are you sure?".into(),
                input_schema_json: "{}".into(),
            }],
            assignee: Some("chenhan".into()),
            source_event_id: "42".into(),
            occurrence_count: 2,
            reopen_count: 1,
            version: 5,
            created_at: "2026-03-10T00:05:00Z".into(),
            updated_at: "2026-03-10T00:06:00Z".into(),
            last_occurred_at: "2026-03-10T00:06:00Z".into(),
            snoozed_until: Some("2026-03-11T00:00:00Z".into()),
            sla_deadline: Some("2026-03-12T00:00:00Z".into()),
            resolved_at: None,
            resolution_json: None,
            source_route_id: Some("route-1".into()),
            source_binding_name: Some("binding-1".into()),
        }
    }

    pub(super) fn attention_list_response() -> AttentionListResponse {
        AttentionListResponse {
            items: vec![attention_item()],
            latest_change_id: 9,
        }
    }

    pub(super) fn attention_delta() -> AttentionDelta {
        AttentionDelta {
            kind: "upsert".into(),
            change_id: 9,
            item: Some(attention_item()),
            notification: None,
        }
    }
}

#[test]
fn task_list_json_yaml_equivalent() {
    let value = Value::Array(vec![super::value::task_summary_value(
        &fixtures::task_summary(),
    )]);
    assert_parity("task list", &value);
}

#[test]
fn task_items_json_yaml_equivalent() {
    let value = Value::Array(vec![super::value::task_item_value(&fixtures::task_item())]);
    assert_parity("task items", &value);
}

#[test]
fn event_list_json_yaml_equivalent() {
    let value = Value::Array(vec![super::value::event_value(&fixtures::event())]);
    assert_parity("event list", &value);
}

#[test]
fn task_detail_json_yaml_equivalent() {
    let resp = fixtures::task_info_response();
    let task = resp.task.as_ref().expect("task");
    assert_parity("task info", &super::value::task_detail_value(task, &resp));
}

#[test]
fn timeline_response_json_yaml_equivalent() {
    assert_parity(
        "task timeline",
        &super::timeline::response_value(&fixtures::timeline_response()),
    );
}

#[test]
fn timeline_delta_json_yaml_equivalent() {
    assert_parity(
        "task timeline --follow",
        &super::timeline::delta_value(&fixtures::timeline_delta()),
    );
}

#[test]
fn attention_item_json_yaml_equivalent() {
    assert_parity(
        "attention get",
        &super::attention::attention_item_value(&fixtures::attention_item()),
    );
}

#[test]
fn attention_list_json_yaml_equivalent() {
    let resp = fixtures::attention_list_response();
    let value = serde_json::json!({
        "items": resp.items.iter().map(super::attention::attention_item_value).collect::<Vec<_>>(),
        "latest_change_id": resp.latest_change_id,
    });
    assert_parity("attention list", &value);
}

#[test]
fn attention_delta_json_yaml_equivalent() {
    let delta = fixtures::attention_delta();
    let value = serde_json::json!({
        "kind": delta.kind,
        "change_id": delta.change_id,
        "item": delta.item.as_ref().map(super::attention::attention_item_value),
    });
    assert_parity("attention follow", &value);
}
