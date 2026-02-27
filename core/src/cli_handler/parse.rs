use crate::cli_types::ResourceMetadata;
use crate::config::TaskExecutionPlan;
use crate::scheduler::resolve_task_id;
use crate::session_store;
use crate::task_repository::{SqliteTaskRepository, TaskRepository};
use anyhow::{Context, Result};

pub(super) fn parse_resource_selector(selector: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = selector.splitn(2, '/').collect();
    match parts.as_slice() {
        [kind, name] => {
            if kind.trim().is_empty() || name.trim().is_empty() {
                anyhow::bail!(
                    "invalid resource selector format: expected kind/name, got '{}'",
                    selector
                );
            }
            Ok((kind, name))
        }
        _ => anyhow::bail!(
            "invalid resource selector format: expected kind/name, got '{}'",
            selector
        ),
    }
}

#[derive(Debug)]
pub(super) enum ExecTargetRef<'a> {
    TaskStep { task_id: &'a str, step_id: &'a str },
    SessionId { session_id: &'a str },
}

pub(super) struct ResolvedExecTarget {
    pub task_id: String,
    pub step_id: String,
    pub step_tty: bool,
    pub session: Option<crate::session_store::SessionRow>,
}

pub(super) fn parse_exec_target(target: &str) -> Result<ExecTargetRef<'_>> {
    let mut parts = target.split('/');
    let kind = parts.next().unwrap_or_default();
    match kind {
        "task" => {
            let task_id = parts.next().unwrap_or_default();
            let step_keyword = parts.next().unwrap_or_default();
            let step_id = parts.next().unwrap_or_default();
            if task_id.trim().is_empty()
                || step_keyword != "step"
                || step_id.trim().is_empty()
                || parts.next().is_some()
            {
                anyhow::bail!(
                    "invalid exec target '{}': expected task/<task_id>/step/<step_id>",
                    target
                );
            }
            Ok(ExecTargetRef::TaskStep { task_id, step_id })
        }
        "session" => {
            let session_id = parts.next().unwrap_or_default();
            if session_id.trim().is_empty() || parts.next().is_some() {
                anyhow::bail!(
                    "invalid exec target '{}': expected session/<session_id>",
                    target
                );
            }
            Ok(ExecTargetRef::SessionId { session_id })
        }
        _ => anyhow::bail!(
            "invalid exec target '{}': expected task/<task_id>/step/<step_id> or session/<session_id>",
            target
        ),
    }
}

pub(super) fn resolve_exec_target(
    state: &crate::state::InnerState,
    target: &str,
) -> Result<ResolvedExecTarget> {
    match parse_exec_target(target)? {
        ExecTargetRef::SessionId { session_id } => {
            let sess = session_store::load_session(&state.db_path, session_id)?
                .with_context(|| format!("session not found: {}", session_id))?;
            Ok(ResolvedExecTarget {
                task_id: sess.task_id.clone(),
                step_id: sess.step_id.clone(),
                step_tty: true,
                session: Some(sess),
            })
        }
        ExecTargetRef::TaskStep { task_id, step_id } => {
            let task_id = resolve_task_id(state, task_id)?;
            let repo = SqliteTaskRepository::new(state.db_path.clone());
            let runtime_row = repo.load_task_runtime_row(&task_id)?;
            let plan = serde_json::from_str::<TaskExecutionPlan>(&runtime_row.execution_plan_json)
                .with_context(|| format!("failed to parse execution plan for task {}", task_id))?;
            let step = plan
                .steps
                .iter()
                .find(|s| s.id == step_id)
                .with_context(|| format!("step '{}' not found in task '{}'", step_id, task_id))?;
            let session = session_store::load_active_session_for_task_step(
                &state.db_path,
                &task_id,
                step_id,
            )?;
            Ok(ResolvedExecTarget {
                task_id,
                step_id: step_id.to_string(),
                step_tty: step.tty,
                session,
            })
        }
    }
}

pub(super) fn shell_quote(input: &str) -> String {
    let escaped = input.replace('\'', "'\"'\"'");
    format!("'{}'", escaped)
}

pub(super) fn parse_key_value_pairs(
    values: &[String],
    field_name: &str,
) -> Result<std::collections::HashMap<String, String>> {
    let mut out = std::collections::HashMap::new();
    for raw in values {
        let (key, value) = raw.split_once('=').with_context(|| {
            format!("invalid {} entry '{}': expected key=value", field_name, raw)
        })?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            anyhow::bail!(
                "invalid {} entry '{}': key/value cannot be empty",
                field_name,
                raw
            );
        }
        out.insert(key.to_string(), value.to_string());
    }
    Ok(out)
}

pub(super) fn build_resource_metadata(
    name: &str,
    labels: &[String],
    annotations: &[String],
) -> Result<ResourceMetadata> {
    let label_map = parse_key_value_pairs(labels, "label")?;
    let annotation_map = parse_key_value_pairs(annotations, "annotation")?;
    Ok(ResourceMetadata {
        name: name.to_string(),
        project: None,
        labels: if label_map.is_empty() {
            None
        } else {
            Some(label_map)
        },
        annotations: if annotation_map.is_empty() {
            None
        } else {
            Some(annotation_map)
        },
    })
}

pub(super) fn parse_label_selector(selector: &str) -> Result<Vec<(String, String)>> {
    let mut terms = Vec::new();
    for part in selector.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            anyhow::bail!("invalid label selector '{}': empty segment", selector);
        }
        let (key, value) = trimmed
            .split_once('=')
            .with_context(|| format!("invalid label selector '{}': expected key=value", trimmed))?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            anyhow::bail!(
                "invalid label selector '{}': key/value cannot be empty",
                trimmed
            );
        }
        terms.push((key.to_string(), value.to_string()));
    }
    Ok(terms)
}

pub(super) fn matches_selector(
    labels: &Option<std::collections::HashMap<String, String>>,
    selector: &[(String, String)],
) -> bool {
    if selector.is_empty() {
        return true;
    }
    let Some(labels) = labels else {
        return false;
    };
    selector
        .iter()
        .all(|(key, expected)| labels.get(key) == Some(expected))
}

pub(super) fn string_map_to_csv(map: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut items: Vec<String> = map
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| format!("{k}={s}")))
        .collect();
    items.sort();
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(",")
    }
}

pub(super) fn normalize_loop_mode(loop_mode: &str) -> Result<String> {
    use crate::config::LoopMode;
    let parsed = loop_mode.parse::<LoopMode>().map_err(|_| {
        anyhow::anyhow!(
            "invalid --loop-mode '{}': expected one of once|fixed|infinite",
            loop_mode
        )
    })?;
    let normalized = match parsed {
        LoopMode::Once => "once",
        LoopMode::Fixed => "fixed",
        LoopMode::Infinite => "infinite",
    };
    Ok(normalized.to_string())
}

pub(super) fn validate_workflow_step_type(value: &str) -> Result<String> {
    use crate::config::WorkflowStepType;
    let parsed = value.parse::<WorkflowStepType>().map_err(|_| {
        anyhow::anyhow!(
            "invalid --step '{}': expected init_once|plan|qa|ticket_scan|fix|retest|loop_guard",
            value
        )
    })?;
    Ok(parsed.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_resource_selector_workspace_default() {
        let (kind, name) =
            parse_resource_selector("workspace/default").expect("parsing should succeed");
        assert_eq!(kind, "workspace");
        assert_eq!(name, "default");
    }

    #[test]
    fn parse_resource_selector_agent_opencode() {
        let (kind, name) =
            parse_resource_selector("agent/opencode").expect("parsing should succeed");
        assert_eq!(kind, "agent");
        assert_eq!(name, "opencode");
    }

    #[test]
    fn parse_resource_selector_with_slash_in_name_uses_first_slash_only() {
        let (kind, name) =
            parse_resource_selector("workflow/my/workflow").expect("parsing should succeed");
        assert_eq!(kind, "workflow");
        assert_eq!(name, "my/workflow");
    }

    #[test]
    fn parse_resource_selector_rejects_missing_kind() {
        let err = parse_resource_selector("/name").expect_err("should reject missing kind");
        assert!(err.to_string().contains("invalid resource selector"));
    }

    #[test]
    fn parse_resource_selector_rejects_missing_name() {
        let err = parse_resource_selector("workspace/").expect_err("should reject missing name");
        assert!(err.to_string().contains("invalid resource selector"));
    }

    #[test]
    fn parse_resource_selector_rejects_no_separator() {
        let err = parse_resource_selector("workspace-default")
            .expect_err("should reject missing separator");
        assert!(err.to_string().contains("invalid resource selector"));
    }

    #[test]
    fn parse_exec_target_task_step_valid() {
        let parsed = parse_exec_target("task/task-1/step/plan-1").expect("parsing should succeed");
        match parsed {
            ExecTargetRef::TaskStep { task_id, step_id } => {
                assert_eq!(task_id, "task-1");
                assert_eq!(step_id, "plan-1");
            }
            _ => panic!("expected task-step target"),
        }
    }

    #[test]
    fn parse_exec_target_session_valid() {
        let parsed = parse_exec_target("session/sess-1").expect("parsing should succeed");
        match parsed {
            ExecTargetRef::SessionId { session_id } => assert_eq!(session_id, "sess-1"),
            _ => panic!("expected session target"),
        }
    }

    #[test]
    fn parse_exec_target_rejects_invalid_shape() {
        let err = parse_exec_target("task/task-1").expect_err("should reject invalid target");
        assert!(err
            .to_string()
            .contains("expected task/<task_id>/step/<step_id>"));
    }

    #[test]
    fn parse_label_selector_supports_comma_separated_equals() {
        let parsed = parse_label_selector("env=prod,tier=backend").expect("should parse");
        assert_eq!(
            parsed,
            vec![
                ("env".to_string(), "prod".to_string()),
                ("tier".to_string(), "backend".to_string())
            ]
        );
    }

    #[test]
    fn parse_label_selector_rejects_invalid_term() {
        let err = parse_label_selector("env").expect_err("selector should be invalid");
        assert!(err.to_string().contains("expected key=value"));
    }
}
