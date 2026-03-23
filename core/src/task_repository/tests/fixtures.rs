use crate::db::open_conn;
use crate::dto::CreateTaskPayload;
use crate::task_ops::create_task_impl;
use crate::test_utils::TestState;
use rusqlite::params;

pub fn seed_task(fixture: &mut TestState) -> (std::sync::Arc<crate::state::InnerState>, String) {
    let state = fixture.build();
    let qa_file = state
        .data_dir
        .join("workspace/default/docs/qa/repo_test.md");
    std::fs::write(&qa_file, "# repository test\n").expect("seed qa file");
    let created = create_task_impl(
        &state,
        CreateTaskPayload {
            name: Some("repo-test".to_string()),
            goal: Some("repo-test-goal".to_string()),
            ..Default::default()
        },
    )
    .expect("task should be created");
    (state, created.id)
}

pub fn get_item_id(state: &crate::state::InnerState, task_id: &str) -> String {
    let conn = open_conn(&state.db_path).expect("open sqlite");
    conn.query_row(
        "SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1",
        params![task_id],
        |row| row.get(0),
    )
    .expect("task item exists")
}
