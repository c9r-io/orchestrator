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
            // task
            commands::task::task_list,
            commands::task::task_info,
            // streaming
            commands::stream::start_task_follow,
            commands::stream::stop_task_follow,
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
        ])
        .run(tauri::generate_context!())
        .expect("error running orchestrator GUI");
}
