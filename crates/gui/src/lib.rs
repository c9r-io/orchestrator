#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unwrap_used, clippy::expect_used)
)]

pub mod client;
pub mod commands;
pub mod errors;
pub mod state;

use std::sync::Arc;

use state::AppState;

/// Build and configure the Tauri application.
#[allow(clippy::expect_used)] // Tauri event loop failure is unrecoverable
pub fn run() {
    let app_state = Arc::new(AppState::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(app_state.clone())
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = app_state.clone();
            tauri::async_runtime::spawn(async move {
                state.set_app_handle(handle).await;
                state.start_heartbeat();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // system
            commands::system::connect,
            commands::system::ping,
            commands::system::probe_role,
            commands::system::check,
            commands::system::worker_status,
            commands::system::db_status,
            commands::system::shutdown,
            commands::system::maintenance_mode,
            commands::process_metrics::process_metrics_get,
            commands::process_metrics::process_metric_record,
            // task
            commands::task::task_list,
            commands::task::task_info,
            commands::task::task_create,
            commands::task::task_start,
            commands::task::task_pause,
            commands::task::task_resume,
            commands::task::task_retry,
            commands::task::task_delete,
            commands::task::task_trace,
            commands::task::task_timeline,
            commands::task::task_recover,
            commands::task::task_delete_bulk,
            // attention
            commands::attention::attention_list,
            commands::attention::attention_get,
            commands::attention::attention_claim,
            commands::attention::attention_snooze,
            commands::attention::attention_resolve,
            commands::attention::attention_execute_action,
            // external sources
            commands::source::source_event_list,
            commands::source::source_event_get,
            commands::source::source_automation_route_get,
            commands::source::source_automation_catalog_get,
            commands::source::source_task_template_preview,
            commands::source::source_task_binding_simulate,
            commands::source::source_task_binding_suspend,
            commands::source::source_task_binding_resume,
            commands::source::source_automation_list,
            commands::source::source_automation_get,
            commands::source::source_automation_simulate,
            commands::source::source_automation_replay,
            commands::source::source_automation_ignore,
            commands::source::source_automation_status_get,
            commands::source::start_source_automation_watch,
            commands::source::stop_source_automation_watch,
            commands::source::source_binding_list,
            commands::source::source_replay,
            // handoff and safe resume
            commands::handoff::handoff_generate,
            commands::handoff::resume_boundary_list,
            commands::handoff::resume_plan,
            commands::handoff::resume_execute,
            // streaming
            commands::stream::start_task_follow,
            commands::stream::stop_task_follow,
            commands::stream::start_task_watch,
            commands::stream::stop_task_watch,
            commands::stream::task_logs,
            commands::stream::start_task_timeline_follow,
            commands::stream::stop_task_timeline_follow,
            commands::stream::start_attention_follow,
            commands::stream::stop_attention_follow,
            // resource
            commands::resource::resource_get,
            commands::resource::resource_describe,
            commands::resource::resource_apply,
            commands::resource::resource_delete,
            // agent
            commands::agent::agent_list,
            commands::agent::agent_cordon,
            commands::agent::agent_uncordon,
            commands::agent::agent_drain,
            commands::session::agent_session_list,
            commands::session::agent_session_attach,
            commands::session::agent_session_heartbeat,
            commands::session::agent_session_send_input,
            commands::session::agent_session_detach,
            commands::session::agent_session_close,
            commands::session::start_agent_session_read,
            commands::session::stop_agent_session_read,
            // store
            commands::store::store_list,
            commands::store::store_get,
            commands::store::store_put,
            commands::store::store_delete,
            // manifest
            commands::manifest::manifest_validate,
            commands::manifest::manifest_export,
            // secret
            commands::secret::secret_key_list,
            commands::secret::secret_key_status,
            commands::secret::secret_key_rotate,
            commands::secret::secret_key_revoke,
            // event
            commands::event::event_cleanup,
            commands::event::event_stats,
            // trigger
            commands::trigger::trigger_suspend,
            commands::trigger::trigger_resume,
            commands::trigger::trigger_fire,
        ])
        .run(tauri::generate_context!())
        .expect("error running orchestrator GUI");
}

#[cfg(test)]
mod live_bridge_tests {
    use super::*;
    use serde_json::{Value, json};
    use tauri::ipc::InvokeBody;
    use tauri::webview::InvokeRequest;

    fn invoke(
        webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
        body: Value,
    ) -> Result<Value, Value> {
        tauri::test::get_ipc_response(
            webview,
            InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().expect("url"),
                body: InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
        .map(|response| response.deserialize::<Value>().expect("response json"))
    }

    fn required_string<'a>(value: &'a Value, key: &str) -> &'a str {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("missing string field {key}: {value}"))
    }

    #[test]
    fn live_failed_process_crosses_real_tauri_handlers_and_grpc() {
        if std::env::var("FR103_LIVE_E2E").as_deref() != Ok("1") {
            return;
        }
        let project = std::env::var("FR103_PROJECT").expect("FR103_PROJECT");
        let target = std::env::var("FR103_TARGET").expect("FR103_TARGET");
        let app_state = Arc::new(AppState::new());
        let app = tauri::test::mock_builder()
            .manage(app_state)
            .invoke_handler(tauri::generate_handler![
                commands::system::connect,
                commands::task::task_create,
                commands::task::task_start,
                commands::task::task_info,
                commands::task::task_timeline,
                commands::attention::attention_list,
                commands::handoff::handoff_generate,
                commands::handoff::resume_boundary_list,
                commands::handoff::resume_plan,
                commands::handoff::resume_execute,
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock Tauri app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview");

        invoke(&webview, "connect", json!({})).expect("connect through Tauri");
        let created = invoke(
            &webview,
            "task_create",
            json!({
                "name": "FR-103 live vertical flow",
                "goal": "prove failure to reviewed resume through the desktop bridge",
                "project_id": project,
                "workspace_id": "process-console-vertical",
                "workflow_id": "process-console-vertical",
                "target_files": [target],
                "no_start": true
            }),
        )
        .expect("create through Tauri");
        let task_id = required_string(&created, "task_id").to_string();
        println!("FR103_TASK_ID={task_id}");
        invoke(
            &webview,
            "task_start",
            json!({"task_id": task_id, "latest": false}),
        )
        .expect("start through Tauri");

        let detail = (0..80)
            .find_map(|_| {
                let detail = invoke(&webview, "task_info", json!({"task_id": task_id})).ok()?;
                if matches!(
                    required_string(&detail, "status"),
                    "failed" | "completed" | "cancelled"
                ) {
                    Some(detail)
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    None
                }
            })
            .expect("terminal task");
        assert_eq!(required_string(&detail, "status"), "failed");

        let attention = (0..40)
            .find_map(|_| {
                let response = invoke(
                    &webview,
                    "attention_list",
                    json!({
                        "project_id": project,
                        "item_state": null,
                        "kind": null,
                        "severity": "intervention",
                        "assignee": null,
                        "task_id": task_id,
                        "active_only": true
                    }),
                )
                .ok()?;
                if let Some(item) = response.get("items")?.as_array()?.first() {
                    Some(item.clone())
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    None
                }
            })
            .expect("failed task attention");
        assert_eq!(required_string(&attention, "state"), "open");

        let timeline = invoke(
            &webview,
            "task_timeline",
            json!({"task_id": task_id, "cursor": null, "limit": 50, "categories": []}),
        )
        .expect("timeline through Tauri");
        assert!(timeline["entries"].as_array().is_some_and(|entries| {
            entries.iter().any(|entry| entry["category"] == "failure")
                && entries.iter().any(|entry| {
                    entry["evidence"]
                        .as_array()
                        .is_some_and(|evidence| !evidence.is_empty())
                })
        }));

        let handoff = invoke(&webview, "handoff_generate", json!({"task_id": task_id}))
            .expect("handoff through Tauri");
        assert!(!required_string(&handoff, "content_hash").is_empty());
        let boundaries = invoke(
            &webview,
            "resume_boundary_list",
            json!({"task_id": task_id}),
        )
        .expect("boundaries through Tauri");
        let boundary_id = boundaries
            .as_array()
            .and_then(|values| values.iter().find(|value| value["step_id"] == "qa"))
            .map(|value| required_string(value, "id").to_string())
            .expect("qa boundary");

        let plan_body = || {
            json!({
                "task_id": task_id,
                "boundary_id": boundary_id,
                "mode": "restart_from_boundary"
            })
        };
        let stale_plan = invoke(&webview, "resume_plan", plan_body()).expect("stale plan");
        let stale_error = invoke(
            &webview,
            "resume_execute",
            json!({
                "plan_id": required_string(&stale_plan, "id"),
                "expected_state_version": "intentionally-stale",
                "operator_reason": "verify request correlation before mutation",
                "idempotency_key": "fr103-stale",
                "elevated_confirmation": false
            }),
        )
        .expect_err("stale execution must fail");
        assert!(stale_error.to_string().contains("请求 ID"));

        let plan = invoke(&webview, "resume_plan", plan_body()).expect("fresh plan");
        let execution = invoke(
            &webview,
            "resume_execute",
            json!({
                "plan_id": required_string(&plan, "id"),
                "expected_state_version": required_string(&plan, "expected_state_version"),
                "operator_reason": "reviewed deterministic FR-103 recovery",
                "idempotency_key": "fr103-reviewed-resume",
                "elevated_confirmation": false
            }),
        )
        .expect("reviewed resume through Tauri");
        assert_eq!(required_string(&execution, "status"), "succeeded");
        assert!(
            execution["child_task_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty())
        );

        let resolved = (0..40).any(|_| {
            let response = invoke(
                &webview,
                "attention_list",
                json!({
                    "project_id": project,
                    "item_state": "resolved",
                    "kind": null,
                    "severity": null,
                    "assignee": null,
                    "task_id": task_id,
                    "active_only": false
                }),
            )
            .unwrap_or(Value::Null);
            let found = response["items"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["state"] == "resolved"));
            if !found {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            found
        });
        assert!(
            resolved,
            "source Attention should resolve after durable resume"
        );
    }

    #[test]
    fn live_source_automation_crosses_real_tauri_handlers_and_grpc() {
        if std::env::var("FR112_LIVE_E2E").as_deref() != Ok("1") {
            return;
        }
        let project = std::env::var("FR112_PROJECT").expect("FR112_PROJECT");
        let app_state = Arc::new(AppState::new());
        let app = tauri::test::mock_builder()
            .manage(app_state)
            .invoke_handler(tauri::generate_handler![
                commands::system::connect,
                commands::source::source_automation_catalog_get,
                commands::source::source_task_template_preview,
                commands::source::source_automation_simulate,
                commands::source::source_task_binding_suspend,
                commands::source::source_task_binding_resume,
                commands::resource::resource_apply,
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock Tauri app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview");

        invoke(&webview, "connect", json!({})).expect("connect through Tauri");
        let catalog = invoke(
            &webview,
            "source_automation_catalog_get",
            json!({"project_id": project}),
        )
        .expect("catalog through Tauri");
        assert!(
            catalog["templates"]
                .as_array()
                .is_some_and(|items| items.len() >= 2)
        );
        assert!(
            catalog["bindings"]
                .as_array()
                .is_some_and(|items| items.len() >= 2)
        );
        let public_catalog = catalog.to_string();
        assert!(!public_catalog.contains("qa-source-routing-signing-secret"));
        assert!(!public_catalog.contains("qa-source-routing-fake-token"));
        assert!(!public_catalog.contains("normalized_json"));

        let preview = invoke(
            &webview,
            "source_task_template_preview",
            json!({
                "name": "implement-from-slack",
                "project_id": project,
                "provider": "slack",
                "installation_id": "T_QA_ROUTING",
                "message_url": "https://qa-workspace.slack.com/archives/C_QA_ROUTING/p1234567890000100",
                "event_id": null,
                "reaction": "agent-implement",
                "target_id": "C_QA_ROUTING:1234567890.000100",
                "draft_content": null
            }),
        )
        .expect("preview through Tauri");
        assert_eq!(required_string(&preview, "skill_invocation"), "$docs");
        assert!(required_string(&preview, "goal").contains("qa-workspace.slack.com"));

        let simulation = invoke(
            &webview,
            "source_automation_simulate",
            json!({
                "project_id": project,
                "provider": "slack",
                "installation_id": "T_QA_ROUTING",
                "event_kind": "reaction_added",
                "reaction": "agent-implement",
                "target_kind": "message",
                "channel_id": "C_QA_ROUTING",
                "external_actor_id": "U_OPERATOR",
                "message_url": "https://qa-workspace.slack.com/archives/C_QA_ROUTING/p1234567890000100",
                "event_id": null,
                "target_id": "C_QA_ROUTING:1234567890.000100",
                "draft_binding_content": null
            }),
        )
        .expect("simulation through Tauri");
        assert_eq!(simulation["match_result"]["status"], "matched");
        assert_eq!(simulation["mutation_performed"], false);
        assert_eq!(simulation["network_performed"], false);

        let revision = catalog["bindings"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["name"] == "slack-implement"))
            .map(|item| required_string(item, "revision").to_string())
            .expect("binding revision");
        invoke(
            &webview,
            "source_task_binding_suspend",
            json!({"name":"slack-implement","project_id":project,"expected_revision":revision,"reason":"FR-112 live bridge suspend"}),
        )
        .expect("suspend through Tauri");
        let suspended = invoke(
            &webview,
            "source_automation_catalog_get",
            json!({"project_id": project}),
        )
        .expect("suspended catalog");
        let suspended_binding = suspended["bindings"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["name"] == "slack-implement"))
            .expect("suspended binding");
        assert_eq!(suspended_binding["suspended"], true);
        invoke(
            &webview,
            "source_task_binding_resume",
            json!({"name":"slack-implement","project_id":project,"expected_revision":required_string(suspended_binding,"revision"),"reason":"FR-112 live bridge resume"}),
        )
        .expect("resume through Tauri");

        let new_template = json!({
            "apiVersion":"orchestrator.dev/v2","kind":"SourceTaskTemplate",
            "metadata":{"name":"fr112-bridge-template"},
            "spec":{"skill":{"name":"docs","invocation":"$docs","args":[]},
              "action":{"workflow":"source-routing-fixture","workspace":"source-routing-fixture","start":false},
              "goalTemplate":"{skill_invocation}: inspect {source_message_url}",
              "allowedVariables":["skill_invocation","source_message_url"]}
        })
        .to_string();
        invoke(
            &webview,
            "resource_apply",
            json!({"content":new_template,"project_id":project,"expected_revision":null,"require_absent":true,"reason":"create bridge fixture","idempotency_key":"fr112-create"}),
        )
        .expect("create through optimistic resource apply");
        let stale = invoke(
            &webview,
            "resource_apply",
            json!({"content":new_template,"project_id":project,"expected_revision":null,"require_absent":true,"reason":"prove create CAS","idempotency_key":"fr112-stale"}),
        )
        .expect_err("second create must fail closed");
        assert!(stale.to_string().contains("重新加载"), "{stale}");
        println!("FR112_BRIDGE_OK=1");
    }

    #[test]
    fn live_slack_skill_release_crosses_tauri_provenance_boundary() {
        if std::env::var("FR113_LIVE_E2E").as_deref() != Ok("1") {
            return;
        }
        let project = std::env::var("FR113_PROJECT").expect("FR113_PROJECT");
        let route_id = std::env::var("FR113_ROUTE_ID").expect("FR113_ROUTE_ID");
        let task_id = std::env::var("FR113_TASK_ID").expect("FR113_TASK_ID");
        let app_state = Arc::new(AppState::new());
        let app = tauri::test::mock_builder()
            .manage(app_state)
            .invoke_handler(tauri::generate_handler![
                commands::system::connect,
                commands::source::source_automation_list,
                commands::source::source_automation_get,
                commands::source::source_automation_route_get,
                commands::source::source_event_get,
                commands::source::source_binding_list,
                commands::task::task_info,
                commands::task::task_timeline,
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock Tauri app");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("mock webview");

        invoke(&webview, "connect", json!({})).expect("connect through Tauri");
        let page = invoke(
            &webview,
            "source_automation_list",
            json!({
                "project_id": project,
                "route_state": "routed",
                "provider": "slack",
                "binding_name": null,
                "task_id": task_id,
                "page_token": null
            }),
        )
        .expect("route list through Tauri");
        let listed_route = page["routes"]
            .as_array()
            .and_then(|routes| routes.iter().find(|route| route["id"] == route_id))
            .expect("release route in catalog");
        assert_eq!(listed_route["task_id"], task_id);
        assert!(listed_route["permalink"].is_null());

        let detail = invoke(
            &webview,
            "source_automation_get",
            json!({"route_id": route_id}),
        )
        .expect("route detail through Tauri");
        let source_event_id = required_string(&detail["route"], "source_event_id").to_string();
        assert_eq!(detail["route"]["task_id"], task_id);
        assert_eq!(detail["route"]["status"], "routed");
        assert!(!required_string(&detail["route"], "request_id").is_empty());

        let protected_route = invoke(
            &webview,
            "source_automation_route_get",
            json!({"source_event_id": source_event_id}),
        )
        .expect("protected route through Tauri");
        assert_eq!(protected_route["id"], route_id);
        assert_eq!(protected_route["task_id"], task_id);
        assert!(
            required_string(&protected_route, "permalink")
                .starts_with("https://qa-release-workspace.slack.com/")
        );

        let source = invoke(&webview, "source_event_get", json!({"id": source_event_id}))
            .expect("source event through Tauri");
        assert_eq!(source["automation_route_id"], route_id);
        assert_eq!(source["routed_task_id"], task_id);
        assert_eq!(source["reaction_name"], "agent-implement");

        let bindings = invoke(&webview, "source_binding_list", json!({"task_id": task_id}))
            .expect("task source bindings through Tauri");
        assert!(bindings.as_array().is_some_and(|items| {
            items.iter().any(|binding| {
                binding["task_id"] == task_id && binding["binding_type"] == "automation"
            })
        }));

        let task = invoke(&webview, "task_info", json!({"task_id": task_id}))
            .expect("Process Workspace task through Tauri");
        assert_eq!(task["project_id"], project);
        assert_eq!(task["workflow_id"], "slack-release-implement");
        assert_eq!(task["status"], "completed");
        assert!(required_string(&task, "goal").starts_with("$ticket-fix https://"));

        let timeline = invoke(
            &webview,
            "task_timeline",
            json!({"task_id": task_id, "cursor": null, "limit": 50, "categories": null}),
        )
        .expect("Process Workspace timeline through Tauri");
        assert!(
            timeline["entries"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );

        let projection = json!({
            "detail": detail,
            "source": source,
            "bindings": bindings,
            "timeline": timeline
        })
        .to_string();
        for forbidden in [
            "qa-slack-release-signing-secret",
            "qa-slack-release-valid-token",
            "normalized_json",
            "private message body",
        ] {
            assert!(!projection.contains(forbidden), "leaked {forbidden}");
        }
        println!("FR113_TAURI_OK=1");
    }
}
