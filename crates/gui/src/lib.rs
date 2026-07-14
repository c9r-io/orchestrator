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
}
