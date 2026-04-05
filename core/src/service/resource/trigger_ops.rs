use crate::config_load::{persist_config_and_reload, read_active_config};
use crate::error::{Result, classify_resource_error};
use crate::state::InnerState;
use anyhow::Context;

/// Suspend a trigger by name, setting its `suspend` flag to `true`.
pub fn suspend_trigger(
    state: &InnerState,
    trigger_name: &str,
    project: Option<&str>,
) -> Result<()> {
    set_trigger_suspend(state, trigger_name, project, true)
}

/// Resume a suspended trigger by clearing its `suspend` flag.
pub fn resume_trigger(state: &InnerState, trigger_name: &str, project: Option<&str>) -> Result<()> {
    set_trigger_suspend(state, trigger_name, project, false)
}

/// Manually fire a trigger once, creating (and optionally starting) the task
/// described by the trigger's action configuration.  Uses the canonical engine
/// path so all semantics (suspend, throttle, concurrency, goal, target-file,
/// trigger-state, action.start, history-limit) are applied.
///
/// Returns the new task ID.
pub async fn fire_trigger(
    state: &InnerState,
    trigger_name: &str,
    project: Option<&str>,
) -> Result<String> {
    let project_id = project
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(crate::config::DEFAULT_PROJECT_ID);

    let active =
        read_active_config(state).map_err(|err| classify_resource_error("trigger.fire", err))?;
    let proj_cfg = active.config.projects.get(project_id).ok_or_else(|| {
        classify_resource_error(
            "trigger.fire",
            anyhow::anyhow!("project not found: {}", project_id),
        )
    })?;
    let trigger_cfg = proj_cfg.triggers.get(trigger_name).ok_or_else(|| {
        classify_resource_error(
            "trigger.fire",
            anyhow::anyhow!(
                "trigger '{}' not found in project '{}'",
                trigger_name,
                project_id
            ),
        )
    })?;

    crate::trigger_engine::fire_trigger_canonical(state, trigger_name, project_id, trigger_cfg, None)
        .await
        .map_err(|err| classify_resource_error("trigger.fire", err))
}

fn set_trigger_suspend(
    state: &InnerState,
    trigger_name: &str,
    project: Option<&str>,
    suspend: bool,
) -> Result<()> {
    let op = if suspend {
        "trigger.suspend"
    } else {
        "trigger.resume"
    };
    let project_id = project
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(crate::config::DEFAULT_PROJECT_ID);

    let mut config = {
        let active = read_active_config(state).map_err(|err| classify_resource_error(op, err))?;
        active.config.clone()
    };

    let proj_cfg = config.projects.get_mut(project_id).ok_or_else(|| {
        classify_resource_error(op, anyhow::anyhow!("project not found: {}", project_id))
    })?;
    let trigger_cfg = proj_cfg.triggers.get_mut(trigger_name).ok_or_else(|| {
        classify_resource_error(
            op,
            anyhow::anyhow!(
                "trigger '{}' not found in project '{}'",
                trigger_name,
                project_id
            ),
        )
    })?;

    trigger_cfg.suspend = suspend;

    let yaml = serde_yaml::to_string(&config)
        .context("failed to serialize config after trigger update")
        .map_err(|err| classify_resource_error(op, err))?;
    persist_config_and_reload(state, config, yaml, op, Some(project_id), &[])
        .map_err(|err| classify_resource_error(op, err))?;
    crate::trigger_engine::notify_trigger_reload(state);
    Ok(())
}
