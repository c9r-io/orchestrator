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
        ActionAuditRecord, AgentSession, AgentStatus, AttentionActionDescriptor, AttentionDelta,
        AttentionItem, AttentionListResponse, CommandRun, Event, HandoffSnapshotResponse,
        ResumeBoundary, SecretKeyRecord, SourceAutomationRoute, SourceBinding, SourceConnection,
        SourceConnectionDedicatedLifecycleResponse, SourceConnectionDedicatedProvisioningResponse,
        SourceConnectionIntentResponse, SourceConnectionManifestDiffEntry, SourceEvent,
        TaskInfoResponse, TaskItem, TaskSummary, TaskTimelineResponse, TimelineActorRef,
        TimelineDelta, TimelineEntry, TimelineEvidenceRef,
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

    pub(super) fn agent_session() -> AgentSession {
        AgentSession {
            session_id: "session-1".into(),
            task_id: "task-1".into(),
            task_item_id: Some("item-1".into()),
            step_id: "step-1".into(),
            phase: "execute: 执行".into(),
            agent_id: "agent-1".into(),
            state: "running".into(),
            pid: 4242,
            writer_client_id: Some("client-1".into()),
            writer_actor: Some("operator: 陈瀚".into()),
            writer_lease_expires_at: Some("2026-03-10T00:15:00Z".into()),
            writer_fencing_token: 7,
            state_version: 3,
            created_at: "2026-03-10T00:00:00Z".into(),
            updated_at: "2026-03-10T00:10:00Z".into(),
            ended_at: Some("2026-03-10T00:20:00Z".into()),
            exit_code: Some(1),
        }
    }

    pub(super) fn agent_status() -> AgentStatus {
        AgentStatus {
            name: "agent: 执行者".into(),
            enabled: true,
            lifecycle_state: "draining".into(),
            in_flight_items: 2,
            capabilities: vec!["qa".into(), "note: 备注".into()],
            drain_requested_at: Some("2026-03-10T00:07:00Z".into()),
            is_healthy: false,
            diseased_until: Some("2026-03-10T01:00:00Z".into()),
            consecutive_errors: 3,
        }
    }

    pub(super) fn handoff_snapshot() -> HandoffSnapshotResponse {
        HandoffSnapshotResponse {
            id: "handoff-1".into(),
            task_id: "task-1".into(),
            source_event_cursor: 42,
            projection_version: 3,
            briefing_json: "{\"summary\":\"context: 交接摘要\",\"open_items\":[\"item-1\"]}".into(),
            content_hash: "sha256:abc".into(),
            state_version: "sv-9".into(),
            generated_by: "orchestratord".into(),
            created_at: "2026-03-10T00:08:00Z".into(),
        }
    }

    pub(super) fn resume_boundary() -> ResumeBoundary {
        ResumeBoundary {
            id: "boundary-1".into(),
            task_id: "task-1".into(),
            cycle: 2,
            step_id: Some("step-1".into()),
            task_item_id: Some("item-1".into()),
            command_run_id: Some("run-1".into()),
            provider_session_available: true,
            checkpoint_id: Some("ckpt-1".into()),
            side_effect_class: "idempotent".into(),
            replay_safe: true,
            reason: "boundary reason: 可恢复边界".into(),
            state_version: "sv-9".into(),
        }
    }

    pub(super) fn audit_record() -> ActionAuditRecord {
        ActionAuditRecord {
            request_id: "req-1".into(),
            schema_version: 1,
            project_id: "project-1".into(),
            actor: Some("operator: 陈瀚".into()),
            resolved_role: Some("operator".into()),
            transport: "grpc".into(),
            target_type: "task".into(),
            target_id: "task-1".into(),
            action: "pause".into(),
            reason_code: "manual".into(),
            operator_reason: Some("reason: 手动暂停".into()),
            idempotency_key: Some("idem-1".into()),
            expected_version: Some("5".into()),
            fencing_token: Some(7),
            request_hash: "sha256:def".into(),
            status: "completed".into(),
            error_code: Some("none".into()),
            result_type: Some("task".into()),
            result_id: Some("task-1".into()),
            created_at: "2026-03-10T00:00:00Z".into(),
            updated_at: "2026-03-10T00:01:00Z".into(),
            completed_at: Some("2026-03-10T00:01:00Z".into()),
        }
    }

    pub(super) fn secret_key_record() -> SecretKeyRecord {
        SecretKeyRecord {
            key_id: "key-1".into(),
            state: "active: 使用中".into(),
            fingerprint: "fp: sha256:abc".into(),
            file_path: "/tmp/keys/key-1.pem".into(),
            created_at: "2026-03-10T00:00:00Z".into(),
            activated_at: Some("2026-03-10T00:01:00Z".into()),
            rotated_out_at: Some("2026-03-11T00:00:00Z".into()),
            retired_at: Some("2026-03-12T00:00:00Z".into()),
            revoked_at: Some("2026-03-13T00:00:00Z".into()),
        }
    }

    pub(super) fn source_connection() -> SourceConnection {
        SourceConnection {
            id: "conn-1".into(),
            project_id: "project-1".into(),
            provider: "slack".into(),
            display_label: "workspace: 工作区".into(),
            provisioning_mode: "dedicated".into(),
            installation_id: "install-1".into(),
            installation_id_digest: "digest-1".into(),
            enterprise_id_digest: Some("ent-digest-1".into()),
            owner_daemon_id: "daemon-1".into(),
            generation: 2,
            version: 5,
            state: "connected".into(),
            capabilities: vec!["events".into(), "note: 能力".into()],
            scopes: vec!["chat:write".into()],
            trigger_name: Some("slack-trigger".into()),
            last_delivery_at: Some("2026-03-10T00:05:00Z".into()),
            last_acked_cursor: 41,
            delivery_lag: 1,
            last_error_code: Some("none".into()),
            created_at: "2026-03-10T00:00:00Z".into(),
            updated_at: "2026-03-10T00:06:00Z".into(),
            reauthorized_at: Some("2026-03-10T00:03:00Z".into()),
            disconnected_at: Some("2026-03-10T00:04:00Z".into()),
            app_ownership: "platform".into(),
            app_id_digest: Some("app-digest-1".into()),
            manifest_version: Some("mv-1".into()),
            provision_state: Some("ready".into()),
            provision_error_code: Some("none".into()),
        }
    }

    pub(super) fn source_binding() -> SourceBinding {
        SourceBinding {
            id: "binding-1".into(),
            project_id: "project-1".into(),
            task_id: "task-1".into(),
            provider: "slack".into(),
            installation_id: "install-1".into(),
            conversation_id: Some("channel: 频道".into()),
            thread_id: Some("1710000000.000100".into()),
            binding_type: "thread".into(),
            created_by_event_id: "evt-1".into(),
            created_at: "2026-03-10T00:02:00Z".into(),
        }
    }

    pub(super) fn source_event() -> SourceEvent {
        SourceEvent {
            id: "evt-1".into(),
            project_id: "project-1".into(),
            provider: "slack".into(),
            installation_id: "install-1".into(),
            external_event_id: "Ev123".into(),
            event_type: "message".into(),
            external_actor_id: Some("U123".into()),
            conversation_id: Some("C123".into()),
            thread_id: Some("1710000000.000100".into()),
            occurred_at: "2026-03-10T00:01:00Z".into(),
            received_at: "2026-03-10T00:01:01Z".into(),
            normalized_json: "{\"text\":\"message: 请修复\"}".into(),
            payload_hash: "sha256:abc".into(),
            routing_state: "routed".into(),
            routing_attempts: 2,
            routed_task_id: Some("task-1".into()),
            last_error_code: Some("none".into()),
            automation_route_id: Some("route-1".into()),
            automation_status: Some("completed".into()),
            automation_binding_name: Some("binding: 自动化".into()),
            automation_template_name: Some("template-1".into()),
            automation_template_hash: Some("sha256:tpl".into()),
        }
    }

    pub(super) fn automation_route() -> SourceAutomationRoute {
        SourceAutomationRoute {
            id: "route-1".into(),
            project_id: "project-1".into(),
            source_event_id: "evt-1".into(),
            provider: "slack".into(),
            reaction: "eyes".into(),
            binding_name: "binding: 自动化".into(),
            binding_revision: "rev-1".into(),
            template_name: "template-1".into(),
            template_hash: "sha256:tpl".into(),
            status: "failed".into(),
            error_code: Some("timeout".into()),
            task_id: Some("task-1".into()),
            permalink: Some("https://example.com/p/1".into()),
            request_id: "req-1".into(),
            created_at: "2026-03-10T00:01:00Z".into(),
            completed_at: Some("2026-03-10T00:02:00Z".into()),
            error_category: Some("retryable: 可重试".into()),
            generation: 2,
            version: 5,
            attempt_count: 3,
            max_attempts: 5,
            next_attempt_at: Some("2026-03-10T00:03:00Z".into()),
            lease_expires_at: Some("2026-03-10T00:04:00Z".into()),
            suspended_scope: Some("binding".into()),
            last_attempt_at: Some("2026-03-10T00:02:00Z".into()),
            updated_at: "2026-03-10T00:02:00Z".into(),
        }
    }

    pub(super) fn intent_response() -> SourceConnectionIntentResponse {
        SourceConnectionIntentResponse {
            id: "intent-1".into(),
            project_id: "project-1".into(),
            provider: "slack".into(),
            provisioning_mode: "managed".into(),
            status: "pending: 等待授权".into(),
            connection_id: Some("conn-1".into()),
            error_code: Some("none".into()),
            expires_at: "2026-03-10T01:00:00Z".into(),
            authorize_url: Some("https://slack.com/oauth/v2/authorize?state=opaque".into()),
            connection: Some(source_connection()),
        }
    }

    pub(super) fn manifest_diff_entry() -> SourceConnectionManifestDiffEntry {
        SourceConnectionManifestDiffEntry {
            field: "oauth_config.scopes".into(),
            change: "added: 新增".into(),
            before: vec!["chat:write".into()],
            after: vec!["chat:write".into(), "reactions:write".into()],
            permission_expansion: true,
        }
    }

    pub(super) fn dedicated_provisioning() -> SourceConnectionDedicatedProvisioningResponse {
        SourceConnectionDedicatedProvisioningResponse {
            id: "prov-1".into(),
            project_id: "project-1".into(),
            status: "previewed: 已预览".into(),
            manifest_version: "mv-1".into(),
            manifest_digest: "sha256:manifest".into(),
            diff: vec![manifest_diff_entry()],
            app_id_digest: Some("app-digest-1".into()),
            oauth_intent_id: Some("intent-1".into()),
            authorize_url: Some("https://slack.com/oauth/v2/authorize?state=opaque".into()),
            error_code: Some("none".into()),
            expires_at: "2026-03-10T01:00:00Z".into(),
            target_connection_id: Some("conn-1".into()),
        }
    }

    pub(super) fn dedicated_lifecycle() -> SourceConnectionDedicatedLifecycleResponse {
        SourceConnectionDedicatedLifecycleResponse {
            lifecycle_id: "lifecycle-1".into(),
            connection_id: "conn-1".into(),
            status: "upgrade_previewed: 升级预览".into(),
            manifest_version: "mv-2".into(),
            manifest_digest: "sha256:manifest2".into(),
            diff: vec![manifest_diff_entry()],
            permission_expansion: true,
            expires_at: "2026-03-10T01:00:00Z".into(),
            oauth_intent_id: Some("intent-1".into()),
            authorize_url: Some("https://slack.com/oauth/v2/authorize?state=opaque".into()),
            connection: Some(source_connection()),
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

#[test]
fn agent_session_json_yaml_equivalent() {
    let value = Value::Array(vec![crate::commands::agent::session_value(
        &fixtures::agent_session(),
    )]);
    assert_parity("agent session list", &value);
}

#[test]
fn agent_status_json_yaml_equivalent() {
    let value = Value::Array(vec![super::value::agent_status_value(
        &fixtures::agent_status(),
    )]);
    assert_parity("agent list", &value);
}

#[test]
fn handoff_snapshot_json_yaml_equivalent() {
    assert_parity(
        "handoff get",
        &crate::commands::handoff::snapshot_value(&fixtures::handoff_snapshot()),
    );
}

#[test]
fn resume_boundary_json_yaml_equivalent() {
    let value = Value::Array(vec![crate::commands::handoff::boundary_value(
        &fixtures::resume_boundary(),
    )]);
    assert_parity("resume boundaries", &value);
}

#[test]
fn audit_record_json_yaml_equivalent() {
    let value = Value::Array(vec![crate::commands::audit::record_value(
        &fixtures::audit_record(),
    )]);
    assert_parity("audit list", &value);
}

#[test]
fn secret_key_status_json_yaml_equivalent() {
    let key = fixtures::secret_key_record();
    let value = serde_json::json!({
        "active_key": crate::commands::secret::key_record_to_json(&key),
        "all_keys": [crate::commands::secret::key_record_to_json(&key)],
    });
    assert_parity("secret key status", &value);
}

#[test]
fn source_connection_json_yaml_equivalent() {
    let value = Value::Array(vec![crate::commands::source::connection_value(
        &fixtures::source_connection(),
    )]);
    assert_parity("source connection list", &value);
}

#[test]
fn source_binding_json_yaml_equivalent() {
    let value = Value::Array(vec![crate::commands::source::binding_value(
        &fixtures::source_binding(),
    )]);
    assert_parity("source binding list", &value);
}

#[test]
fn source_event_json_yaml_equivalent() {
    let value = Value::Array(vec![crate::commands::source::event_value(
        &fixtures::source_event(),
    )]);
    assert_parity("source event list", &value);
}

#[test]
fn source_automation_route_json_yaml_equivalent() {
    let value = Value::Array(vec![crate::commands::source::automation_route_value(
        &fixtures::automation_route(),
    )]);
    assert_parity("source automation list", &value);
}

#[test]
fn source_intent_json_yaml_equivalent() {
    assert_parity(
        "source connection intent",
        &crate::commands::source::intent_value(&fixtures::intent_response()),
    );
}

#[test]
fn source_dedicated_provisioning_json_yaml_equivalent() {
    assert_parity(
        "source connection dedicated preview",
        &crate::commands::source::dedicated_value(&fixtures::dedicated_provisioning()),
    );
}

#[test]
fn source_dedicated_lifecycle_json_yaml_equivalent() {
    assert_parity(
        "source connection dedicated upgrade",
        &crate::commands::source::dedicated_lifecycle_value(&fixtures::dedicated_lifecycle()),
    );
}
