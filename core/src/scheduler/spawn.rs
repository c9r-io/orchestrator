use crate::config::{SpawnTaskAction, SpawnTasksAction};
use crate::dto::CreateTaskPayload;
use crate::json_extract::{extract_field, extract_json_array};
use crate::state::InnerState;
use crate::task_ops::create_task_impl;
use anyhow::{Context, Result};
use std::collections::HashMap;
use tracing::{info, warn};

/// Execute a single task spawn from a post-action.
pub fn execute_spawn_task(
    state: &InnerState,
    parent_task_id: &str,
    parent_project_id: &str,
    parent_workspace_id: &str,
    parent_workflow_id: &str,
    parent_spawn_depth: i64,
    pipeline_vars: &HashMap<String, String>,
    action: &SpawnTaskAction,
) -> Result<String> {
    // Resolve goal template
    let goal = resolve_template(&action.goal, pipeline_vars);

    let workflow_id = action
        .workflow
        .clone()
        .unwrap_or_else(|| parent_workflow_id.to_string());

    let payload = CreateTaskPayload {
        name: Some(format!("spawn:{}", truncate_goal(&goal, 40))),
        goal: Some(goal),
        project_id: if action.inherit.project {
            Some(parent_project_id.to_string())
        } else {
            None
        },
        workspace_id: if action.inherit.workspace {
            Some(parent_workspace_id.to_string())
        } else {
            None
        },
        workflow_id: Some(workflow_id),
        target_files: None,
        parent_task_id: Some(parent_task_id.to_string()),
        spawn_reason: Some("spawn_task".to_string()),
    };

    let summary = create_task_impl(state, payload)?;

    // Update spawn_depth in DB
    let conn = crate::db::open_conn(&state.db_path)?;
    conn.execute(
        "UPDATE tasks SET spawn_depth = ?1 WHERE id = ?2",
        rusqlite::params![parent_spawn_depth + 1, summary.id],
    )?;

    info!(
        parent = parent_task_id,
        child = summary.id,
        "spawned child task"
    );

    Ok(summary.id)
}

/// Execute batch task spawning from a JSON pipeline variable.
pub fn execute_spawn_tasks(
    state: &InnerState,
    parent_task_id: &str,
    parent_project_id: &str,
    parent_workspace_id: &str,
    parent_workflow_id: &str,
    parent_spawn_depth: i64,
    pipeline_vars: &HashMap<String, String>,
    action: &SpawnTasksAction,
) -> Result<Vec<String>> {
    let json_str = pipeline_vars
        .get(&action.from_var)
        .with_context(|| format!("pipeline variable '{}' not found for spawn_tasks", action.from_var))?;

    let items = extract_json_array(json_str, &action.json_path)?;
    let max = action.max_tasks.min(items.len());

    let mut spawned_ids = Vec::new();
    for item in items.iter().take(max) {
        let goal = match extract_field(item, &action.mapping.goal) {
            Some(g) => g,
            None => {
                warn!("skipping spawn item: no goal field at {}", action.mapping.goal);
                continue;
            }
        };

        let workflow_id = action
            .mapping
            .workflow
            .as_ref()
            .and_then(|path| extract_field(item, path))
            .unwrap_or_else(|| parent_workflow_id.to_string());

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
                Some(parent_project_id.to_string())
            } else {
                None
            },
            workspace_id: if action.inherit.workspace {
                Some(parent_workspace_id.to_string())
            } else {
                None
            },
            workflow_id: Some(workflow_id),
            target_files: None,
            parent_task_id: Some(parent_task_id.to_string()),
            spawn_reason: Some("spawn_tasks".to_string()),
        };

        match create_task_impl(state, payload) {
            Ok(summary) => {
                let conn = crate::db::open_conn(&state.db_path)?;
                conn.execute(
                    "UPDATE tasks SET spawn_depth = ?1 WHERE id = ?2",
                    rusqlite::params![parent_spawn_depth + 1, summary.id],
                )?;
                info!(
                    parent = parent_task_id,
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
pub fn validate_spawn_depth(
    current_depth: i64,
    max_depth: Option<usize>,
) -> Result<()> {
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
}
