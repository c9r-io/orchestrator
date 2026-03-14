/// Preflight config and workflow checks.
pub mod check;
/// Runtime invariant enforcement helpers.
pub mod invariant;
mod item_executor;
/// Dynamic task-item generation helpers.
pub mod item_generate;
/// Task-item selection helpers.
pub mod item_select;
mod loop_engine;
mod phase_runner;
mod query;
mod runtime;
/// Safety features such as checkpoints and binary snapshots.
pub mod safety;
/// Child-task spawning helpers.
pub mod spawn;
mod task_state;
/// Task trace construction and rendering.
pub mod trace;

pub use agent_orchestrator::state::RunningTask;
pub use item_executor::{execute_guard_step, process_item, GuardResult};
pub use loop_engine::{evaluate_loop_guard_rules, run_task_loop};
pub use phase_runner::{run_phase, run_phase_with_rotation};
pub use query::{
    delete_task_impl, follow_task_logs, get_task_details_impl, list_tasks_impl, load_task_summary,
    resolve_task_id, stream_task_logs_impl, watch_task,
};
pub use runtime::{
    kill_current_child, load_task_runtime_context, register_running_task, shutdown_running_tasks,
    spawn_task_runner, stop_task_runtime, stop_task_runtime_for_delete, unregister_running_task,
};
pub use safety::{
    create_checkpoint, execute_self_test_step, restore_binary_snapshot, rollback_to_checkpoint,
    snapshot_binary,
};
pub use task_state::{
    count_unresolved_items, find_latest_resumable_task_id, first_task_item_id,
    prepare_task_for_start, reset_blocked_items, set_task_status, update_task_cycle_state,
};
