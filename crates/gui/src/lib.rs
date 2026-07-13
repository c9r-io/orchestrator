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
