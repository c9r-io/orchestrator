use crate::dto::{
    CommandRunDto, EventDto, TaskGraphDebugBundle, TaskItemDto, TaskItemRow, TaskSummary,
};
use anyhow::Result;

use super::command_run::NewCommandRun;
use super::types::{TaskLogRunRow, TaskRuntimeRow};

pub trait TaskRepository {
    fn resolve_task_id(&self, task_id_or_prefix: &str) -> Result<String>;
    fn load_task_summary(&self, task_id: &str) -> Result<TaskSummary>;
    fn load_task_detail_rows(
        &self,
        task_id: &str,
    ) -> Result<(
        Vec<TaskItemDto>,
        Vec<CommandRunDto>,
        Vec<EventDto>,
        Vec<TaskGraphDebugBundle>,
    )>;
    fn load_task_item_counts(&self, task_id: &str) -> Result<(i64, i64, i64)>;
    fn list_task_ids_ordered_by_created_desc(&self) -> Result<Vec<String>>;
    fn find_latest_resumable_task_id(&self, include_pending: bool) -> Result<Option<String>>;
    fn load_task_runtime_row(&self, task_id: &str) -> Result<TaskRuntimeRow>;
    fn first_task_item_id(&self, task_id: &str) -> Result<Option<String>>;
    fn count_unresolved_items(&self, task_id: &str) -> Result<i64>;
    fn list_task_items_for_cycle(&self, task_id: &str) -> Result<Vec<TaskItemRow>>;
    fn load_task_status(&self, task_id: &str) -> Result<Option<String>>;
    fn set_task_status(&self, task_id: &str, status: &str, set_completed: bool) -> Result<()>;
    fn prepare_task_for_start_batch(&self, task_id: &str) -> Result<()>;
    fn update_task_cycle_state(
        &self,
        task_id: &str,
        current_cycle: u32,
        init_done: bool,
    ) -> Result<()>;
    fn mark_task_item_running(&self, task_item_id: &str) -> Result<()>;
    fn set_task_item_terminal_status(&self, task_item_id: &str, status: &str) -> Result<()>;
    fn update_task_item_status(&self, task_item_id: &str, status: &str) -> Result<()>;
    fn load_task_name(&self, task_id: &str) -> Result<Option<String>>;
    fn list_task_log_runs(&self, task_id: &str, limit: usize) -> Result<Vec<TaskLogRunRow>>;
    fn insert_task_graph_run(&self, run: &super::types::NewTaskGraphRun) -> Result<()>;
    fn update_task_graph_run_status(&self, graph_run_id: &str, status: &str) -> Result<()>;
    fn insert_task_graph_snapshot(
        &self,
        snapshot: &super::types::NewTaskGraphSnapshot,
    ) -> Result<()>;
    fn load_task_graph_debug_bundles(&self, task_id: &str) -> Result<Vec<TaskGraphDebugBundle>>;
    fn delete_task_and_collect_log_paths(&self, task_id: &str) -> Result<Vec<String>>;
    fn insert_command_run(&self, run: &NewCommandRun) -> Result<()>;
}
