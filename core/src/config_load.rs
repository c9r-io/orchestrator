use crate::config::{
    ActiveConfig, OrchestratorConfig, ResolvedProject, ResolvedWorkspace, TaskExecutionPlan,
    WorkflowConfig, WorkflowStepConfig, WorkflowStepType,
};
use crate::db::{count_tasks_by_workflow, count_tasks_by_workspace, open_conn};
use crate::dto::ConfigOverview;
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn detect_app_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    if cwd.join("core").exists() {
        return cwd;
    }

    if cwd.ends_with("core") {
        let parent = cwd.parent().unwrap_or(&cwd);
        return parent.to_path_buf();
    }

    let candidate = cwd.join("tools/agent-orchestrator");
    if candidate.exists() {
        return candidate;
    }
    cwd
}

pub fn load_config(config_path: &Path) -> Result<OrchestratorConfig> {
    let content = std::fs::read_to_string(config_path)
        .with_context(|| format!("config file not found: {}", config_path.display()))?;
    serde_yaml::from_str::<OrchestratorConfig>(&content)
        .with_context(|| format!("failed to parse {}", config_path.display()))
}

pub fn normalize_workflow_config(workflow: &mut WorkflowConfig) {
    let had_ticket_scan_step = workflow
        .steps
        .iter()
        .any(|step| step.step_type.as_ref() == Some(&WorkflowStepType::TicketScan));
    if workflow.steps.is_empty() {
        workflow.steps = crate::config::default_workflow_steps(
            workflow.qa.as_deref(),
            false,
            workflow.fix.as_deref(),
            workflow.retest.as_deref(),
        );
    }
    let mut normalized: Vec<WorkflowStepConfig> = Vec::new();
    let mut by_type: HashMap<String, WorkflowStepConfig> = HashMap::new();
    for step in workflow.steps.drain(..) {
        let key = step
            .step_type
            .as_ref()
            .map(|t| t.as_str().to_string())
            .or_else(|| step.builtin.clone())
            .or_else(|| step.required_capability.clone())
            .unwrap_or(step.id.clone());
        by_type.entry(key).or_insert(step);
    }
    for step_type in [
        WorkflowStepType::InitOnce,
        WorkflowStepType::Qa,
        WorkflowStepType::TicketScan,
        WorkflowStepType::Fix,
        WorkflowStepType::Retest,
        WorkflowStepType::LoopGuard,
    ] {
        let key = step_type.as_str().to_string();
        if let Some(mut step) = by_type.remove(&key) {
            if step.step_type.is_none() {
                step.step_type = Some(step_type.clone());
            }
            if step.required_capability.is_none() && step.builtin.is_none() {
                match step_type {
                    WorkflowStepType::Qa | WorkflowStepType::Fix | WorkflowStepType::Retest => {
                        step.required_capability = Some(step_type.as_str().to_string());
                    }
                    WorkflowStepType::LoopGuard => {
                        step.builtin = Some("loop_guard".to_string());
                        step.is_guard = true;
                    }
                    _ => {}
                }
            }
            normalized.push(step);
        } else if step_type != WorkflowStepType::LoopGuard {
            normalized.push(WorkflowStepConfig {
                id: step_type.as_str().to_string(),
                description: None,
                step_type: Some(step_type),
                required_capability: None,
                builtin: None,
                enabled: false,
                repeatable: true,
                is_guard: false,
                cost_preference: None,
                prehook: None,
            });
        }
    }
    workflow.steps = normalized;
    for step in &mut workflow.steps {
        if step.id.trim().is_empty() {
            step.id = step
                .step_type
                .as_ref()
                .map(|t| t.as_str())
                .unwrap_or(&step.id)
                .to_string();
        }
    }
    let qa_enabled = workflow
        .steps
        .iter()
        .any(|step| step.step_type.as_ref() == Some(&WorkflowStepType::Qa) && step.enabled);
    let fix_enabled = workflow
        .steps
        .iter()
        .any(|step| step.step_type.as_ref() == Some(&WorkflowStepType::Fix) && step.enabled);
    let retest_enabled = workflow
        .steps
        .iter()
        .any(|step| step.step_type.as_ref() == Some(&WorkflowStepType::Retest) && step.enabled);
    if !had_ticket_scan_step && !qa_enabled && fix_enabled && !retest_enabled {
        if let Some(scan_step) = workflow
            .steps
            .iter_mut()
            .find(|step| step.step_type.as_ref() == Some(&WorkflowStepType::TicketScan))
        {
            scan_step.enabled = true;
        }
    }
    workflow.qa = None;
    workflow.fix = None;
    workflow.retest = None;
    if workflow.finalize.rules.is_empty() {
        workflow.finalize = crate::config::default_workflow_finalize_config();
    }
    workflow.loop_policy.guard.agent_template = None;
}

fn normalize_config(mut config: OrchestratorConfig) -> OrchestratorConfig {
    for workflow in config.workflows.values_mut() {
        normalize_workflow_config(workflow);
    }
    config
}

pub fn validate_workflow_config(
    config: &OrchestratorConfig,
    workflow: &WorkflowConfig,
    workflow_id: &str,
) -> Result<()> {
    if workflow.steps.is_empty() {
        anyhow::bail!("workflow '{}' must define at least one step", workflow_id);
    }

    let mut enabled_count = 0usize;
    let mut seen: HashMap<String, bool> = HashMap::new();
    for step in &workflow.steps {
        let key = step
            .step_type
            .as_ref()
            .map(|t| t.as_str())
            .or(step.builtin.as_deref())
            .or(step.required_capability.as_deref())
            .unwrap_or(&step.id);
        if seen.insert(key.to_string(), true).is_some() {
            anyhow::bail!(
                "workflow '{}' has duplicate step type '{}'",
                workflow_id,
                key
            );
        }
        if !step.enabled {
            continue;
        }
        enabled_count += 1;
        if step.step_type.as_ref() == Some(&WorkflowStepType::TicketScan) {
            if let Some(prehook) = step.prehook.as_ref() {
                crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
            }
            continue;
        }
        let has_agent = config
            .agents
            .values()
            .any(|a| a.get_template(key).is_some());
        if !has_agent {
            anyhow::bail!(
                "no agent has template for step '{}' used by workflow '{}'",
                key,
                workflow_id
            );
        }
        if let Some(prehook) = step.prehook.as_ref() {
            crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
        }
    }
    if enabled_count == 0 {
        anyhow::bail!("workflow '{}' has no enabled steps", workflow_id);
    }
    for rule in &workflow.finalize.rules {
        crate::prehook::validate_workflow_finalize_rule(rule, workflow_id)?;
    }
    if let Some(max_cycles) = workflow.loop_policy.guard.max_cycles {
        if max_cycles == 0 {
            anyhow::bail!(
                "workflow '{}' loop.guard.max_cycles must be > 0",
                workflow_id
            );
        }
    }
    if workflow.loop_policy.guard.enabled {
        let has_loop_guard = config
            .agents
            .values()
            .any(|a| a.get_template("loop_guard").is_some());
        if !has_loop_guard {
            anyhow::bail!(
                "workflow '{}' loop.guard enabled but no agent has loop_guard template",
                workflow_id
            );
        }
    }
    Ok(())
}

fn validate_workflow_config_with_agents(
    all_agents: &HashMap<String, &crate::config::AgentConfig>,
    workflow: &WorkflowConfig,
    workflow_id: &str,
) -> Result<()> {
    if workflow.steps.is_empty() {
        anyhow::bail!("workflow '{}' must define at least one step", workflow_id);
    }

    let mut enabled_count = 0usize;
    let mut seen: HashMap<String, bool> = HashMap::new();
    for step in &workflow.steps {
        let key = step
            .step_type
            .as_ref()
            .map(|t| t.as_str())
            .or(step.builtin.as_deref())
            .or(step.required_capability.as_deref())
            .unwrap_or(&step.id);
        if seen.insert(key.to_string(), true).is_some() {
            anyhow::bail!(
                "workflow '{}' has duplicate step type '{}'",
                workflow_id,
                key
            );
        }
        if !step.enabled {
            continue;
        }
        enabled_count += 1;
        if step.step_type.as_ref() == Some(&WorkflowStepType::TicketScan) {
            if let Some(prehook) = step.prehook.as_ref() {
                crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
            }
            continue;
        }
        let has_agent = all_agents.values().any(|a| a.get_template(key).is_some());
        if !has_agent {
            anyhow::bail!(
                "no agent has template for step '{}' used by workflow '{}'",
                key,
                workflow_id
            );
        }
        if let Some(prehook) = step.prehook.as_ref() {
            crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
        }
    }
    if enabled_count == 0 {
        anyhow::bail!("workflow '{}' has no enabled steps", workflow_id);
    }
    for rule in &workflow.finalize.rules {
        crate::prehook::validate_workflow_finalize_rule(rule, workflow_id)?;
    }
    if let Some(max_cycles) = workflow.loop_policy.guard.max_cycles {
        if max_cycles == 0 {
            anyhow::bail!(
                "workflow '{}' loop.guard.max_cycles must be > 0",
                workflow_id
            );
        }
    }
    if workflow.loop_policy.guard.enabled {
        let has_loop_guard = all_agents
            .values()
            .any(|a| a.get_template("loop_guard").is_some());
        if !has_loop_guard {
            anyhow::bail!(
                "workflow '{}' loop.guard enabled but no agent has loop_guard template",
                workflow_id
            );
        }
    }
    Ok(())
}

pub fn ensure_within_root(root: &Path, target: &Path, field: &str) -> Result<()> {
    let root_canon = root
        .canonicalize()
        .with_context(|| format!("failed to canonicalize workspace root {}", root.display()))?;
    let target_canon = target.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize path {} for {}",
            target.display(),
            field
        )
    })?;
    if !target_canon.starts_with(&root_canon) {
        anyhow::bail!(
            "{} resolves outside workspace root: {}",
            field,
            target_canon.display()
        );
    }
    Ok(())
}

pub fn resolve_workspace_path(
    workspace_root: &Path,
    rel_path: &str,
    field: &str,
) -> Result<PathBuf> {
    crate::qa_utils::validate_workspace_rel_path(rel_path, field)?;
    let joined = workspace_root.join(rel_path);
    if joined.exists() {
        ensure_within_root(workspace_root, &joined, field)?;
    } else if let Some(parent) = joined.parent() {
        if parent.exists() {
            ensure_within_root(workspace_root, parent, field)?;
        }
    }
    Ok(joined)
}

pub fn resolve_and_validate_workspaces(
    app_root: &Path,
    config: &OrchestratorConfig,
) -> Result<HashMap<String, ResolvedWorkspace>> {
    let has_project_workspaces = config.projects.values().any(|p| !p.workspaces.is_empty());
    let has_project_agents = config.projects.values().any(|p| !p.agents.is_empty());

    if config.workspaces.is_empty() && !has_project_workspaces {
        anyhow::bail!("[EMPTY_WORKSPACES] config.workspaces cannot be empty\n  category: validation\n  suggested_fix: add at least one workspace with root_path and qa_targets");
    }
    if config.agents.is_empty() && !has_project_agents {
        anyhow::bail!("[EMPTY_AGENTS] config.agents cannot be empty\n  category: validation\n  suggested_fix: add at least one agent with capabilities and templates");
    }
    if config.workflows.is_empty() {
        anyhow::bail!("[EMPTY_WORKFLOWS] config.workflows cannot be empty\n  category: validation\n  suggested_fix: add at least one workflow with steps");
    }

    let mut resolved = HashMap::new();
    for (id, entry) in &config.workspaces {
        if id.trim().is_empty() {
            anyhow::bail!("[INVALID_WORKSPACE] workspace id cannot be empty\n  category: validation\n  suggested_fix: provide a non-empty workspace name");
        }
        if entry.qa_targets.is_empty() {
            anyhow::bail!("[INVALID_WORKSPACE] workspace '{}' qa_targets cannot be empty\n  category: validation\n  suggested_fix: add at least one qa_targets path (e.g. docs/qa)", id);
        }

        let root_path = app_root
            .join(&entry.root_path)
            .canonicalize()
            .with_context(|| {
                format!(
                    "workspace '{}' root_path not found: {}",
                    id, entry.root_path
                )
            })?;

        for (idx, target) in entry.qa_targets.iter().enumerate() {
            let field = format!("workspace '{}' qa_targets[{}]", id, idx);
            let resolved_target = resolve_workspace_path(&root_path, target, &field)?;
            if resolved_target.exists() && !resolved_target.is_dir() {
                anyhow::bail!(
                    "{} must be a directory: {}",
                    field,
                    resolved_target.display()
                );
            }
        }
        let ticket_field = format!("workspace '{}' ticket_dir", id);
        let resolved_ticket = resolve_workspace_path(&root_path, &entry.ticket_dir, &ticket_field)?;
        if resolved_ticket.exists() && !resolved_ticket.is_dir() {
            anyhow::bail!(
                "{} must be a directory: {}",
                ticket_field,
                resolved_ticket.display()
            );
        }

        resolved.insert(
            id.clone(),
            ResolvedWorkspace {
                root_path,
                qa_targets: entry.qa_targets.clone(),
                ticket_dir: entry.ticket_dir.clone(),
            },
        );
    }

    let default_ws = &config.defaults.workspace;
    let default_in_projects = config
        .projects
        .values()
        .any(|p| p.workspaces.contains_key(default_ws));
    if !resolved.contains_key(default_ws) && !default_in_projects {
        anyhow::bail!("defaults.workspace '{}' does not exist", default_ws);
    }
    if !config.workflows.contains_key(&config.defaults.workflow) {
        anyhow::bail!(
            "defaults.workflow '{}' does not exist",
            config.defaults.workflow
        );
    }

    let all_agents: HashMap<String, &crate::config::AgentConfig> = config
        .agents
        .iter()
        .chain(config.projects.values().flat_map(|p| p.agents.iter()))
        .map(|(k, v)| (k.clone(), v))
        .collect();

    for (workflow_id, workflow) in &config.workflows {
        validate_workflow_config_with_agents(&all_agents, workflow, workflow_id)?;
    }

    Ok(resolved)
}

pub fn build_active_config(app_root: &Path, config: OrchestratorConfig) -> Result<ActiveConfig> {
    let config = normalize_config(config);
    let workspaces = resolve_and_validate_workspaces(app_root, &config)?;
    let projects = resolve_and_validate_projects(app_root, &config)?;
    Ok(ActiveConfig {
        default_project_id: config.defaults.project.clone(),
        default_workspace_id: config.defaults.workspace.clone(),
        default_workflow_id: config.defaults.workflow.clone(),
        workspaces,
        projects,
        config,
    })
}

pub fn resolve_and_validate_projects(
    app_root: &Path,
    config: &OrchestratorConfig,
) -> Result<HashMap<String, ResolvedProject>> {
    let mut resolved = HashMap::new();
    for (project_id, project_config) in &config.projects {
        let mut workspaces = HashMap::new();
        for (workspace_id, workspace_config) in &project_config.workspaces {
            let root_path = app_root.join(&workspace_config.root_path);
            workspaces.insert(
                workspace_id.clone(),
                ResolvedWorkspace {
                    root_path,
                    qa_targets: workspace_config.qa_targets.clone(),
                    ticket_dir: workspace_config.ticket_dir.clone(),
                },
            );
        }
        resolved.insert(
            project_id.clone(),
            ResolvedProject {
                workspaces,
                agents: project_config.agents.clone(),
                workflows: project_config.workflows.clone(),
            },
        );
    }
    Ok(resolved)
}

pub fn load_or_seed_config(db_path: &Path) -> Result<(OrchestratorConfig, String, i64, String)> {
    let conn = open_conn(db_path)?;
    let row: Option<(String, String, i64, String)> = conn
        .query_row(
            "SELECT config_yaml, config_json, version, updated_at FROM orchestrator_config WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;

    if let Some((yaml, json_raw, version, updated_at)) = row {
        let config = serde_json::from_str::<OrchestratorConfig>(&json_raw)
            .or_else(|_| serde_yaml::from_str::<OrchestratorConfig>(&yaml))
            .context("failed to parse config from sqlite")?;
        let config = normalize_config(config);
        let yaml = serde_yaml::to_string(&config).context("failed to normalize config yaml")?;
        return Ok((config, yaml, version, updated_at));
    }

    anyhow::bail!(
        "[CONFIG_NOT_INITIALIZED] orchestrator config is not initialized in sqlite\n  category: validation\n  suggested_fix: run 'orchestrator apply -f <manifest.yaml>' first"
    )
}

pub fn enforce_deletion_guards(
    conn: &rusqlite::Connection,
    previous: &OrchestratorConfig,
    candidate: &OrchestratorConfig,
) -> Result<()> {
    let removed_workspaces: Vec<String> = previous
        .workspaces
        .keys()
        .filter(|id| !candidate.workspaces.contains_key(*id))
        .cloned()
        .collect();
    for workspace_id in removed_workspaces {
        let task_count = count_tasks_by_workspace(conn, &workspace_id)?;
        if task_count > 0 {
            anyhow::bail!(
                "cannot delete workspace '{}' because {} tasks reference it",
                workspace_id,
                task_count
            );
        }
    }

    let removed_workflows: Vec<String> = previous
        .workflows
        .keys()
        .filter(|id| !candidate.workflows.contains_key(*id))
        .cloned()
        .collect();
    for workflow_id in removed_workflows {
        let task_count = count_tasks_by_workflow(conn, &workflow_id)?;
        if task_count > 0 {
            anyhow::bail!(
                "cannot delete workflow '{}' because {} tasks reference it",
                workflow_id,
                task_count
            );
        }
    }

    let _removed_agents: Vec<String> = previous
        .agents
        .keys()
        .filter(|id| !candidate.agents.contains_key(*id))
        .cloned()
        .collect();

    Ok(())
}

fn persist_config_versioned(
    tx: &Transaction<'_>,
    yaml: &str,
    json_raw: &str,
    author: &str,
) -> Result<(i64, String)> {
    let current_version: i64 = tx.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM orchestrator_config_versions",
        [],
        |row| row.get(0),
    )?;
    let next_version = current_version + 1;
    let now = now_ts();

    tx.execute(
        "INSERT INTO orchestrator_config (id, config_yaml, config_json, version, updated_at) VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET config_yaml=excluded.config_yaml, config_json=excluded.config_json, version=excluded.version, updated_at=excluded.updated_at",
        params![yaml, json_raw, next_version, now],
    )?;
    tx.execute(
        "INSERT INTO orchestrator_config_versions (version, config_yaml, config_json, created_at, author) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![next_version, yaml, json_raw, now, author],
    )?;

    Ok((next_version, now))
}

pub fn persist_config_and_reload(
    state: &crate::state::InnerState,
    config: OrchestratorConfig,
    _yaml: String,
    author: &str,
) -> Result<ConfigOverview> {
    let candidate = build_active_config(&state.app_root, config.clone())?;
    let normalized = candidate.config.clone();
    let yaml = serde_yaml::to_string(&normalized).context("failed to serialize config yaml")?;
    let json_raw = serde_json::to_string(&normalized).context("failed to serialize config json")?;

    let previous_config = {
        let active = read_active_config(state)?;
        active.config.clone()
    };

    let conn = open_conn(&state.db_path)?;
    let tx = conn.unchecked_transaction()?;
    enforce_deletion_guards(&tx, &previous_config, &normalized)?;
    let (next_version, now) = persist_config_versioned(&tx, &yaml, &json_raw, author)?;
    tx.commit()?;

    {
        let mut active = crate::state::write_active_config(state)?;
        *active = candidate;
    }

    Ok(ConfigOverview {
        config: normalized,
        yaml,
        version: next_version,
        updated_at: now,
    })
}

pub fn load_config_overview(state: &crate::state::InnerState) -> Result<ConfigOverview> {
    let conn = open_conn(&state.db_path)?;
    let (yaml, version, updated_at): (String, i64, String) = conn.query_row(
        "SELECT config_yaml, version, updated_at FROM orchestrator_config WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    let active = read_active_config(state)?;
    Ok(ConfigOverview {
        config: active.config.clone(),
        yaml,
        version,
        updated_at,
    })
}

pub fn load_raw_config_from_db(
    db_path: &Path,
) -> Result<Option<(OrchestratorConfig, i64, String)>> {
    let conn = open_conn(db_path)?;
    let row: Option<(String, String, i64, String)> = conn
        .query_row(
            "SELECT config_yaml, config_json, version, updated_at FROM orchestrator_config WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;

    let Some((yaml, json_raw, version, updated_at)) = row else {
        return Ok(None);
    };

    let config = serde_json::from_str::<OrchestratorConfig>(&json_raw)
        .or_else(|_| serde_yaml::from_str::<OrchestratorConfig>(&yaml))
        .context("failed to parse config from sqlite")?;
    Ok(Some((normalize_config(config), version, updated_at)))
}

pub fn persist_raw_config(
    db_path: &Path,
    config: OrchestratorConfig,
    author: &str,
) -> Result<ConfigOverview> {
    let normalized = normalize_config(config);
    let yaml = serde_yaml::to_string(&normalized).context("failed to serialize config yaml")?;
    let json_raw = serde_json::to_string(&normalized).context("failed to serialize config json")?;
    let conn = open_conn(db_path)?;
    let tx = conn.unchecked_transaction()?;
    let (next_version, now) = persist_config_versioned(&tx, &yaml, &json_raw, author)?;
    tx.commit()?;

    Ok(ConfigOverview {
        config: normalized,
        yaml,
        version: next_version,
        updated_at: now,
    })
}

pub fn read_active_config<'a>(
    state: &'a crate::state::InnerState,
) -> Result<std::sync::RwLockReadGuard<'a, ActiveConfig>> {
    state
        .active_config
        .read()
        .map_err(|_| anyhow::anyhow!("active config lock is poisoned"))
}

pub fn build_execution_plan(
    config: &OrchestratorConfig,
    workflow: &WorkflowConfig,
    workflow_id: &str,
) -> Result<TaskExecutionPlan> {
    validate_workflow_config(config, workflow, workflow_id)?;
    let mut steps = Vec::new();
    for step in &workflow.steps {
        if !step.enabled {
            continue;
        }
        steps.push(crate::config::TaskExecutionStep {
            id: step.id.clone(),
            step_type: step.step_type.clone(),
            required_capability: step.required_capability.clone(),
            builtin: step.builtin.clone(),
            enabled: step.enabled,
            repeatable: step.repeatable,
            is_guard: step.is_guard,
            cost_preference: step.cost_preference.clone(),
            prehook: step.prehook.clone(),
        });
    }
    let loop_policy = workflow.loop_policy.clone();
    Ok(TaskExecutionPlan {
        steps,
        loop_policy,
        finalize: workflow.finalize.clone(),
    })
}
