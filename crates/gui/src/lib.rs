pub mod client;
pub mod commands;
pub mod state;

use state::AppState;

/// Build and configure the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
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
            commands::task::task_recover,
            commands::task::task_delete_bulk,
            // streaming
            commands::stream::start_task_follow,
            commands::stream::stop_task_follow,
            commands::stream::start_task_watch,
            commands::stream::stop_task_watch,
            commands::stream::task_logs,
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
