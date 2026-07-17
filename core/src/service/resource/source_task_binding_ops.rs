use crate::config_load::{
    persist_config_and_reload, read_active_config, validate_source_task_bindings_for_project,
};
use crate::error::{Result, classify_resource_error};
use crate::state::InnerState;
use anyhow::Context;

/// Result of a SourceTaskBinding lifecycle mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTaskBindingMutation {
    /// Resource name.
    pub name: String,
    /// Current suspended state.
    pub suspend: bool,
    /// Stable normalized content revision.
    pub revision: String,
}

/// Suspends a source task binding and atomically reloads active configuration.
pub fn suspend_source_task_binding(
    state: &InnerState,
    name: &str,
    project: Option<&str>,
) -> Result<SourceTaskBindingMutation> {
    set_source_task_binding_suspend(state, name, project, true)
}

/// Resumes a source task binding after validating the complete project candidate config.
pub fn resume_source_task_binding(
    state: &InnerState,
    name: &str,
    project: Option<&str>,
) -> Result<SourceTaskBindingMutation> {
    set_source_task_binding_suspend(state, name, project, false)
}

fn set_source_task_binding_suspend(
    state: &InnerState,
    name: &str,
    project: Option<&str>,
    suspend: bool,
) -> Result<SourceTaskBindingMutation> {
    let op = if suspend {
        "source.binding.suspend"
    } else {
        "source.binding.resume"
    };
    let project_id = project
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let mut config = {
        let active =
            read_active_config(state).map_err(|error| classify_resource_error(op, error))?;
        active.config.clone()
    };
    let binding = config
        .projects
        .get_mut(project_id)
        .ok_or_else(|| {
            classify_resource_error(op, anyhow::anyhow!("project not found: {}", project_id))
        })?
        .source_task_bindings
        .get_mut(name)
        .ok_or_else(|| {
            classify_resource_error(
                op,
                anyhow::anyhow!(
                    "SourceTaskBinding '{}' not found in project '{}'",
                    name,
                    project_id
                ),
            )
        })?;
    binding.suspend = suspend;
    let revision = crate::source_task_binding::binding_content_hash(binding)
        .map_err(|error| classify_resource_error(op, error))?;

    validate_source_task_bindings_for_project(&config, project_id)
        .map_err(|error| classify_resource_error(op, error))?;
    let yaml = serde_yaml::to_string(&config)
        .context("failed to serialize config after SourceTaskBinding update")
        .map_err(|error| classify_resource_error(op, error))?;
    persist_config_and_reload(state, config, yaml, op, Some(project_id), &[])
        .map_err(|error| classify_resource_error(op, error))?;
    crate::trigger_engine::notify_trigger_reload(state);
    Ok(SourceTaskBindingMutation {
        name: name.to_string(),
        suspend,
        revision,
    })
}
