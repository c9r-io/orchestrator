use crate::config::{SpawnTaskAction, SpawnTasksAction};
use crate::dto::CreateTaskPayload;
use crate::json_extract::{extract_field, extract_json_array};
use crate::state::InnerState;
use crate::task_ops::create_task_impl;
use anyhow::{Context, Result};
use std::collections::HashMap;
use tracing::{info, warn};

/// Bundled context for spawn operations.
pub struct SpawnContext<'a> {
    pub state: &'a InnerState,
    pub parent_task_id: &'a str,
    pub parent_project_id: &'a str,
    pub parent_workspace_id: &'a str,
    pub parent_workflow_id: &'a str,
    pub parent_spawn_depth: i64,
    pub pipeline_vars: &'a HashMap<String, String>,
}

/// Execute a single task spawn from a post-action.
pub fn execute_spawn_task(ctx: &SpawnContext<'_>, action: &SpawnTaskAction) -> Result<String> {
    let goal = resolve_template(&action.goal, ctx.pipeline_vars);

    let workflow_id = action
        .workflow
        .clone()
        .unwrap_or_else(|| ctx.parent_workflow_id.to_string());

    let payload = CreateTaskPayload {
        name: Some(format!("spawn:{}", truncate_goal(&goal, 40))),
        goal: Some(goal),
        project_id: if action.inherit.project {
            Some(ctx.parent_project_id.to_string())
        } else {
            None
        },
        workspace_id: if action.inherit.workspace {
            Some(ctx.parent_workspace_id.to_string())
        } else {
            None
        },
        workflow_id: Some(workflow_id),
        target_files: None,
        parent_task_id: Some(ctx.parent_task_id.to_string()),
        spawn_reason: Some("spawn_task".to_string()),
    };

    let summary = create_task_impl(ctx.state, payload)?;

    let conn = crate::db::open_conn(&ctx.state.db_path)?;
    conn.execute(
        "UPDATE tasks SET spawn_depth = ?1 WHERE id = ?2",
        rusqlite::params![ctx.parent_spawn_depth + 1, summary.id],
    )?;

    info!(
        parent = ctx.parent_task_id,
        child = summary.id,
        "spawned child task"
    );

    Ok(summary.id)
}

/// Execute batch task spawning from a JSON pipeline variable.
pub fn execute_spawn_tasks(
    ctx: &SpawnContext<'_>,
    action: &SpawnTasksAction,
) -> Result<Vec<String>> {
    let json_str = ctx.pipeline_vars.get(&action.from_var).with_context(|| {
        format!(
            "pipeline variable '{}' not found for spawn_tasks",
            action.from_var
        )
    })?;

    let items = extract_json_array(json_str, &action.json_path)?;
    let max = action.max_tasks.min(items.len());

    let mut spawned_ids = Vec::new();
    for item in items.iter().take(max) {
        let goal = match extract_field(item, &action.mapping.goal) {
            Some(g) => g,
            None => {
                warn!(
                    "skipping spawn item: no goal field at {}",
                    action.mapping.goal
                );
                continue;
            }
        };

        let workflow_id = action
            .mapping
            .workflow
            .as_ref()
            .and_then(|path| extract_field(item, path))
            .unwrap_or_else(|| ctx.parent_workflow_id.to_string());

        let name = action
            .mapping
            .name
            .as_ref()
            .and_then(|path| extract_field(item, path))
            .unwrap_or_else(|| format!("spawn:{}", truncate_goal(&goal, 40)));

        let payload = CreateTaskPayload {
            name: Some(name),
            goal: Some(goal),
            project_id: if action.inherit.project {
                Some(ctx.parent_project_id.to_string())
            } else {
                None
            },
            workspace_id: if action.inherit.workspace {
                Some(ctx.parent_workspace_id.to_string())
            } else {
                None
            },
            workflow_id: Some(workflow_id),
            target_files: None,
            parent_task_id: Some(ctx.parent_task_id.to_string()),
            spawn_reason: Some("spawn_tasks".to_string()),
        };

        match create_task_impl(ctx.state, payload) {
            Ok(summary) => {
                let conn = crate::db::open_conn(&ctx.state.db_path)?;
                conn.execute(
                    "UPDATE tasks SET spawn_depth = ?1 WHERE id = ?2",
                    rusqlite::params![ctx.parent_spawn_depth + 1, summary.id],
                )?;
                info!(
                    parent = ctx.parent_task_id,
                    child = summary.id,
                    "spawned child task (batch)"
                );
                spawned_ids.push(summary.id);
            }
            Err(e) => {
                warn!(error = %e, "failed to spawn child task");
            }
        }
    }

    Ok(spawned_ids)
}

/// Validate spawn depth against safety limits.
pub fn validate_spawn_depth(current_depth: i64, max_depth: Option<usize>) -> Result<()> {
    if let Some(max) = max_depth {
        if current_depth as usize >= max {
            anyhow::bail!(
                "spawn depth limit reached: current={}, max={}",
                current_depth,
                max
            );
        }
    }
    Ok(())
}

/// Simple template resolution: replace `{var}` with pipeline variable values.
fn resolve_template(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("{{{}}}", key), value);
    }
    result
}

fn truncate_goal(goal: &str, max_len: usize) -> &str {
    if goal.len() <= max_len {
        goal
    } else {
        &goal[..max_len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SpawnInherit, SpawnMapping};
    use crate::db::open_conn;
    use crate::test_utils::TestState;
    use rusqlite::params;

    fn test_context<'a>(
        state: &'a InnerState,
        pipeline_vars: &'a HashMap<String, String>,
        parent_workspace_id: &'a str,
        parent_spawn_depth: i64,
    ) -> SpawnContext<'a> {
        SpawnContext {
            state,
            parent_task_id: "parent-task-123",
            parent_project_id: crate::config::DEFAULT_PROJECT_ID,
            parent_workspace_id,
            parent_workflow_id: "basic",
            parent_spawn_depth,
            pipeline_vars,
        }
    }

    fn load_spawned_task(
        state: &InnerState,
        task_id: &str,
    ) -> (String, String, String, String, String, String, i64) {
        let conn = open_conn(&state.db_path).expect("open spawn task database");
        conn.query_row(
            "SELECT name, goal, project_id, workspace_id, workflow_id, spawn_reason, spawn_depth FROM tasks WHERE id = ?1",
            params![task_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .expect("load spawned task row")
    }

    fn seed_default_qa_file(state: &InnerState, name: &str) {
        let qa_path = state
            .app_root
            .join("workspace/default/docs/qa")
            .join(format!("{name}.md"));
        std::fs::write(qa_path, "# spawn coverage\n").expect("seed default QA file");
    }

    #[test]
    fn test_resolve_template() {
        let mut vars = HashMap::new();
        vars.insert("area".to_string(), "authentication".to_string());
        vars.insert("priority".to_string(), "high".to_string());

        assert_eq!(
            resolve_template("improve {area} with {priority} priority", &vars),
            "improve authentication with high priority"
        );
    }

    #[test]
    fn test_resolve_template_no_vars() {
        let vars = HashMap::new();
        assert_eq!(resolve_template("static goal", &vars), "static goal");
    }

    #[test]
    fn test_validate_spawn_depth_within_limit() {
        assert!(validate_spawn_depth(0, Some(3)).is_ok());
        assert!(validate_spawn_depth(2, Some(3)).is_ok());
    }

    #[test]
    fn test_validate_spawn_depth_at_limit() {
        assert!(validate_spawn_depth(3, Some(3)).is_err());
    }

    #[test]
    fn test_validate_spawn_depth_no_limit() {
        assert!(validate_spawn_depth(100, None).is_ok());
    }

    #[test]
    fn test_truncate_goal() {
        assert_eq!(truncate_goal("short", 10), "short");
        assert_eq!(truncate_goal("a longer goal text", 10), "a longer g");
    }

    #[test]
    fn execute_spawn_task_creates_child_task_and_increments_depth() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        seed_default_qa_file(&state, "spawn-single");
        let mut pipeline_vars = HashMap::new();
        pipeline_vars.insert("area".to_string(), "authentication".to_string());
        let ctx = test_context(&state, &pipeline_vars, "default", 2);

        let task_id = execute_spawn_task(
            &ctx,
            &SpawnTaskAction {
                goal: "fix {area} defects".to_string(),
                workflow: None,
                inherit: SpawnInherit::default(),
            },
        )
        .expect("spawn single child task");

        let (name, goal, project_id, workspace_id, workflow_id, spawn_reason, spawn_depth) =
            load_spawned_task(&state, &task_id);
        assert_eq!(name, "spawn:fix authentication defects");
        assert_eq!(goal, "fix authentication defects");
        assert_eq!(project_id, crate::config::DEFAULT_PROJECT_ID);
        assert_eq!(workspace_id, "default");
        assert_eq!(workflow_id, "basic");
        assert_eq!(spawn_reason, "spawn_task");
        assert_eq!(spawn_depth, 3);
    }

    #[test]
    fn execute_spawn_task_without_workspace_inheritance_uses_default_workspace() {
        let mut fixture = TestState::new().with_workspace("secondary", "workspace/secondary");
        let state = fixture.build();
        seed_default_qa_file(&state, "spawn-no-inherit");
        let pipeline_vars = HashMap::new();
        let ctx = test_context(&state, &pipeline_vars, "secondary", 0);

        let task_id = execute_spawn_task(
            &ctx,
            &SpawnTaskAction {
                goal: "independent child".to_string(),
                workflow: None,
                inherit: SpawnInherit {
                    workspace: false,
                    project: true,
                    target_files: false,
                },
            },
        )
        .expect("spawn child without workspace inheritance");

        let (_, _, _, workspace_id, _, _, _) = load_spawned_task(&state, &task_id);
        assert_eq!(workspace_id, "default");
    }

    #[test]
    fn execute_spawn_tasks_creates_batch_children_skips_missing_goal_and_honors_limit() {
        let mut fixture = TestState::new().with_workspace("secondary", "workspace/secondary");
        let state = fixture.build();
        seed_default_qa_file(&state, "spawn-batch");
        let mut pipeline_vars = HashMap::new();
        pipeline_vars.insert(
            "analysis".to_string(),
            serde_json::json!({
                "tasks": [
                    { "goal": "first child", "name": "First custom", "workflow": "basic" },
                    { "name": "Missing goal" },
                    { "goal": "second child", "name": "Second custom", "workflow": "basic" }
                ]
            })
            .to_string(),
        );
        let ctx = test_context(&state, &pipeline_vars, "secondary", 4);

        let task_ids = execute_spawn_tasks(
            &ctx,
            &SpawnTasksAction {
                from_var: "analysis".to_string(),
                json_path: "$.tasks".to_string(),
                mapping: SpawnMapping {
                    goal: "$.goal".to_string(),
                    workflow: Some("$.workflow".to_string()),
                    name: Some("$.name".to_string()),
                },
                inherit: SpawnInherit {
                    workspace: false,
                    project: true,
                    target_files: false,
                },
                max_tasks: 2,
                queue: true,
            },
        )
        .expect("spawn child tasks from pipeline var");

        assert_eq!(task_ids.len(), 1);

        let (name, goal, _, workspace_id, workflow_id, spawn_reason, spawn_depth) =
            load_spawned_task(&state, &task_ids[0]);
        assert_eq!(name, "First custom");
        assert_eq!(goal, "first child");
        assert_eq!(workspace_id, "default");
        assert_eq!(workflow_id, "basic");
        assert_eq!(spawn_reason, "spawn_tasks");
        assert_eq!(spawn_depth, 5);
    }

    #[test]
    fn execute_spawn_tasks_errors_when_source_variable_is_missing() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let pipeline_vars = HashMap::new();
        let ctx = test_context(&state, &pipeline_vars, "default", 0);

        let err = execute_spawn_tasks(
            &ctx,
            &SpawnTasksAction {
                from_var: "missing".to_string(),
                json_path: "$.tasks".to_string(),
                mapping: SpawnMapping {
                    goal: "$.goal".to_string(),
                    workflow: None,
                    name: None,
                },
                inherit: SpawnInherit::default(),
                max_tasks: 5,
                queue: true,
            },
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("pipeline variable 'missing' not found for spawn_tasks"));
    }
}
