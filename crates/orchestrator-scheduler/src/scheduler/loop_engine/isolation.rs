use agent_orchestrator::config::{
    ItemIsolationCleanup, ItemIsolationConfig, ItemIsolationStrategy, PipelineVariables,
    TaskRuntimeContext,
};
use agent_orchestrator::dto::TaskItemRow;
use agent_orchestrator::events::insert_event;
use crate::scheduler::item_executor::StepExecutionAccumulator;
use agent_orchestrator::state::InnerState;
use anyhow::{Context, Result};
use serde_json::json;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) const ITEM_WORKTREE_PATH_VAR: &str = "item_worktree_path";
pub(crate) const ITEM_BRANCH_VAR: &str = "item_branch";
const WINNER_APPLIED_VAR: &str = "item_isolation_winner_applied";

pub(crate) fn item_isolation_config(task_ctx: &TaskRuntimeContext) -> Option<&ItemIsolationConfig> {
    task_ctx
        .execution_plan
        .item_isolation
        .as_ref()
        .filter(|config| config.is_enabled())
}

pub(crate) fn step_workspace_root(
    task_ctx: &TaskRuntimeContext,
    pipeline_vars: &PipelineVariables,
    step_scope: agent_orchestrator::config::StepScope,
) -> PathBuf {
    if step_scope == agent_orchestrator::config::StepScope::Item {
        if let Some(path) = pipeline_vars.vars.get(ITEM_WORKTREE_PATH_VAR) {
            return PathBuf::from(path);
        }
    }
    task_ctx.workspace_root.clone()
}

pub(crate) async fn ensure_item_isolation(
    state: &Arc<InnerState>,
    task_id: &str,
    item: &TaskItemRow,
    task_ctx: &TaskRuntimeContext,
    acc: &mut StepExecutionAccumulator,
) -> Result<()> {
    let Some(config) = item_isolation_config(task_ctx) else {
        return Ok(());
    };
    if config.strategy != ItemIsolationStrategy::GitWorktree {
        return Ok(());
    }
    if acc.pipeline_vars.vars.contains_key(ITEM_WORKTREE_PATH_VAR) {
        return Ok(());
    }

    ensure_workspace_clean(&task_ctx.workspace_root).await?;

    let branch = branch_name(config, task_id, item);
    let path = worktree_path(state, task_id, item);
    tokio::fs::create_dir_all(task_worktree_root(state, task_id))
        .await
        .with_context(|| format!("create worktree root for task {}", task_id))?;

    let _ = run_git(
        &task_ctx.workspace_root,
        [
            "worktree",
            "remove",
            "--force",
            path.to_string_lossy().as_ref(),
        ],
    )
    .await;
    let _ = run_git(&task_ctx.workspace_root, ["branch", "-D", branch.as_str()]).await;

    run_git(
        &task_ctx.workspace_root,
        [
            "worktree",
            "add",
            "-b",
            branch.as_str(),
            path.to_string_lossy().as_ref(),
            "HEAD",
        ],
    )
    .await
    .with_context(|| format!("create git worktree for item {}", item.id))?;

    acc.pipeline_vars
        .vars
        .insert(ITEM_BRANCH_VAR.to_string(), branch.clone());
    acc.pipeline_vars.vars.insert(
        ITEM_WORKTREE_PATH_VAR.to_string(),
        path.to_string_lossy().to_string(),
    );

    insert_event(
        state,
        task_id,
        Some(item.id.as_str()),
        "item_isolation_prepared",
        json!({
            "strategy": "git_worktree",
            "branch": branch,
            "worktree_path": path,
        }),
    )
    .await?;

    Ok(())
}

pub(crate) async fn apply_winner_if_needed(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &mut TaskRuntimeContext,
) -> Result<()> {
    let Some(config) = item_isolation_config(task_ctx) else {
        return Ok(());
    };
    if config.strategy != ItemIsolationStrategy::GitWorktree {
        return Ok(());
    }
    if task_ctx
        .pipeline_vars
        .vars
        .get(WINNER_APPLIED_VAR)
        .is_some_and(|value| value == "true")
    {
        return Ok(());
    }
    let Some(branch) = task_ctx.pipeline_vars.vars.get(ITEM_BRANCH_VAR).cloned() else {
        return Ok(());
    };

    run_git(
        &task_ctx.workspace_root,
        ["merge", "--ff-only", branch.as_str()],
    )
    .await
    .with_context(|| format!("merge winner branch {}", branch))?;

    task_ctx
        .pipeline_vars
        .vars
        .insert(WINNER_APPLIED_VAR.to_string(), "true".to_string());

    insert_event(
        state,
        task_id,
        None,
        "item_isolation_winner_applied",
        json!({
            "strategy": "git_worktree",
            "branch": branch,
        }),
    )
    .await?;

    Ok(())
}

pub(crate) async fn cleanup_task_isolation(
    state: &Arc<InnerState>,
    task_id: &str,
    task_ctx: &TaskRuntimeContext,
) -> Result<()> {
    let Some(config) = item_isolation_config(task_ctx) else {
        return Ok(());
    };
    if config.strategy != ItemIsolationStrategy::GitWorktree
        || config.cleanup == ItemIsolationCleanup::Never
    {
        return Ok(());
    }

    let root = task_worktree_root(state, task_id);
    if tokio::fs::try_exists(&root).await.unwrap_or(false) {
        let mut entries = tokio::fs::read_dir(&root).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                let _ = run_git(
                    &task_ctx.workspace_root,
                    [
                        "worktree",
                        "remove",
                        "--force",
                        path.to_string_lossy().as_ref(),
                    ],
                )
                .await;
            }
        }
        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    for branch in list_task_branches(&task_ctx.workspace_root, config, task_id).await? {
        let _ = run_git(&task_ctx.workspace_root, ["branch", "-D", branch.as_str()]).await;
    }

    insert_event(
        state,
        task_id,
        None,
        "item_isolation_cleaned",
        json!({
            "strategy": "git_worktree",
            "worktree_root": root,
        }),
    )
    .await?;

    Ok(())
}

fn task_worktree_root(state: &InnerState, task_id: &str) -> PathBuf {
    state.logs_dir.join(task_id).join("item-worktrees")
}

fn worktree_path(state: &InnerState, task_id: &str, item: &TaskItemRow) -> PathBuf {
    task_worktree_root(state, task_id).join(sanitize_ref_component(&item.id))
}

fn branch_name(config: &ItemIsolationConfig, task_id: &str, item: &TaskItemRow) -> String {
    let prefix = config
        .branch_prefix
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("orchestrator-item");
    format!(
        "{}/{}/{}",
        sanitize_ref_component(prefix),
        sanitize_ref_component(task_id),
        sanitize_ref_component(&item.id)
    )
}

async fn ensure_workspace_clean(workspace_root: &Path) -> Result<()> {
    let output = run_git_output(workspace_root, ["status", "--porcelain"]).await?;
    if !String::from_utf8_lossy(&output.stdout).trim().is_empty() {
        anyhow::bail!("item isolation requires a clean git workspace before creating worktrees");
    }
    Ok(())
}

async fn list_task_branches(
    workspace_root: &Path,
    config: &ItemIsolationConfig,
    task_id: &str,
) -> Result<Vec<String>> {
    let prefix = config
        .branch_prefix
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("orchestrator-item");
    let pattern = format!(
        "{}/{}/*",
        sanitize_ref_component(prefix),
        sanitize_ref_component(task_id)
    );
    let output = run_git_output(workspace_root, ["branch", "--list", pattern.as_str()]).await?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .map(|line| line.trim_start_matches("* ").trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

async fn run_git<I, S>(workspace_root: &Path, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git_output(workspace_root, args).await?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

async fn run_git_output<I, S>(workspace_root: &Path, args: I) -> Result<std::process::Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    tokio::process::Command::new("git")
        .args(args)
        .current_dir(workspace_root)
        .output()
        .await
        .context("failed to execute git command")
}

fn sanitize_ref_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '/') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    sanitized.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_ref_component_replaces_invalid_chars() {
        assert_eq!(sanitize_ref_component("task 01:item"), "task-01-item");
    }
}
