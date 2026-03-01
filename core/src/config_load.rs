use crate::config::{
    ActiveConfig, CaptureDecl, CaptureSource, OrchestratorConfig, PostAction, ResolvedProject,
    ResolvedWorkspace, StepBehavior, TaskExecutionPlan, WorkflowConfig, WorkflowStepConfig,
};
use crate::db::{count_tasks_by_workflow, count_tasks_by_workspace, open_conn};
use crate::dto::ConfigOverview;
use crate::resource::export_manifest_resources;
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use std::collections::{HashMap, HashSet};
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

pub fn normalize_workflow_config(workflow: &mut WorkflowConfig) {
    let had_ticket_scan_step = workflow
        .steps
        .iter()
        .any(|step| step.id == "ticket_scan");
    if workflow.steps.is_empty() {
        workflow.steps = crate::config::default_workflow_steps(
            workflow.qa.as_deref(),
            false,
            workflow.fix.as_deref(),
            workflow.retest.as_deref(),
        );
    }
    let mut normalized: Vec<WorkflowStepConfig> = Vec::new();

    // Preserve original YAML order: apply defaults to each step in place,
    // then add missing standard steps as disabled placeholders.
    let mut seen_ids: HashSet<String> = HashSet::new();
    for mut step in workflow.steps.drain(..) {
        let key = step
            .builtin
            .clone()
            .or_else(|| step.required_capability.clone())
            .unwrap_or(step.id.clone());

        seen_ids.insert(step.id.clone());

        // Apply defaults based on step id
        if step.required_capability.is_none()
            && step.builtin.is_none()
            && step.command.is_none()
        {
            match key.as_str() {
                "qa" | "fix" | "retest" | "plan" | "build" | "test" | "lint"
                | "implement" | "review" | "git_ops" | "qa_doc_gen" | "qa_testing"
                | "ticket_fix" | "doc_governance" | "align_tests" | "smoke_chain" => {
                    step.required_capability = Some(key.clone());
                }
                "loop_guard" => {
                    step.builtin = Some("loop_guard".to_string());
                    step.is_guard = true;
                }
                "self_test" => {
                    step.builtin = Some("self_test".to_string());
                }
                "init_once" | "ticket_scan" => {
                    step.builtin = Some(key.clone());
                }
                _ => {}
            }
        }

        apply_default_step_behavior(&mut step);
        normalized.push(step);
    }

    // Add missing standard steps as disabled placeholders (except LoopGuard)
    let standard_step_ids = [
        "init_once", "plan", "qa", "ticket_scan", "fix", "retest",
    ];
    for step_id in &standard_step_ids {
        if !seen_ids.contains(*step_id) {
            let mut placeholder = WorkflowStepConfig {
                id: step_id.to_string(),
                description: None,
                required_capability: None,
                builtin: None,
                enabled: false,
                repeatable: true,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: Vec::new(),
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
            };
            apply_default_step_behavior(&mut placeholder);
            normalized.push(placeholder);
        }
    }
    workflow.steps = normalized;
    let qa_enabled = workflow
        .steps
        .iter()
        .any(|step| step.id == "qa" && step.enabled);
    let fix_enabled = workflow
        .steps
        .iter()
        .any(|step| step.id == "fix" && step.enabled);
    let retest_enabled = workflow
        .steps
        .iter()
        .any(|step| step.id == "retest" && step.enabled);
    if !had_ticket_scan_step && !qa_enabled && fix_enabled && !retest_enabled {
        if let Some(scan_step) = workflow
            .steps
            .iter_mut()
            .find(|step| step.id == "ticket_scan")
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

/// Apply sensible default behavior to well-known step types when the user
/// hasn't configured explicit captures or collect_artifacts.
fn apply_default_step_behavior(step: &mut WorkflowStepConfig) {
    let key = step
        .builtin
        .as_deref()
        .or(step.required_capability.as_deref())
        .unwrap_or(&step.id);

    let has_capture = |var: &str| step.behavior.captures.iter().any(|c| c.var == var);

    let has_post_action = |pa: &PostAction| step.behavior.post_actions.iter().any(|a| a == pa);

    match key {
        "qa" | "qa_testing" => {
            step.behavior.collect_artifacts = true;
            if !has_capture("qa_failed") {
                step.behavior.captures.push(CaptureDecl {
                    var: "qa_failed".to_string(),
                    source: CaptureSource::FailedFlag,
                });
            }
            if !has_post_action(&PostAction::CreateTicket) {
                step.behavior.post_actions.push(PostAction::CreateTicket);
            }
        }
        "fix" | "ticket_fix" => {
            if !has_capture("fix_success") {
                step.behavior.captures.push(CaptureDecl {
                    var: "fix_success".to_string(),
                    source: CaptureSource::SuccessFlag,
                });
            }
        }
        "retest" => {
            step.behavior.collect_artifacts = true;
            if !has_capture("retest_success") {
                step.behavior.captures.push(CaptureDecl {
                    var: "retest_success".to_string(),
                    source: CaptureSource::SuccessFlag,
                });
            }
        }
        _ => {}
    }
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
    let mut seen_ids: HashSet<String> = HashSet::new();
    for step in &workflow.steps {
        if !seen_ids.insert(step.id.clone()) {
            anyhow::bail!(
                "workflow '{}' has duplicate step id '{}'",
                workflow_id,
                step.id
            );
        }
        let key = step
            .builtin
            .as_deref()
            .or(step.required_capability.as_deref())
            .unwrap_or(&step.id);
        if !step.enabled {
            continue;
        }
        enabled_count += 1;
        if step.id == "ticket_scan" {
            if let Some(prehook) = step.prehook.as_ref() {
                crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
            }
            continue;
        }
        // Steps with a builtin, command, or chain_steps are self-contained
        let is_self_contained =
            step.builtin.is_some() || step.command.is_some() || !step.chain_steps.is_empty();
        if !is_self_contained {
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
    if matches!(workflow.loop_policy.mode, crate::config::LoopMode::Fixed)
        && workflow.loop_policy.guard.max_cycles.is_none()
    {
        anyhow::bail!(
            "workflow '{}' loop.mode=fixed requires guard.max_cycles > 0",
            workflow_id
        );
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
    let mut seen_ids: HashSet<String> = HashSet::new();
    for step in &workflow.steps {
        if !seen_ids.insert(step.id.clone()) {
            anyhow::bail!(
                "workflow '{}' has duplicate step id '{}'",
                workflow_id,
                step.id
            );
        }
        let key = step
            .builtin
            .as_deref()
            .or(step.required_capability.as_deref())
            .unwrap_or(&step.id);
        if !step.enabled {
            continue;
        }
        enabled_count += 1;
        if step.id == "ticket_scan" {
            if let Some(prehook) = step.prehook.as_ref() {
                crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
            }
            continue;
        }
        let is_self_contained =
            step.builtin.is_some() || step.command.is_some() || !step.chain_steps.is_empty();
        if !is_self_contained {
            let has_agent = all_agents.values().any(|a| a.get_template(key).is_some());
            if !has_agent {
                anyhow::bail!(
                    "no agent has template for step '{}' used by workflow '{}'",
                    key,
                    workflow_id
                );
            }
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
    if matches!(workflow.loop_policy.mode, crate::config::LoopMode::Fixed)
        && workflow.loop_policy.guard.max_cycles.is_none()
    {
        anyhow::bail!(
            "workflow '{}' loop.mode=fixed requires guard.max_cycles > 0",
            workflow_id
        );
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

    if let Some((_yaml, json_raw, version, updated_at)) = row {
        let config = serde_json::from_str::<OrchestratorConfig>(&json_raw)
            .context("failed to parse config_json from sqlite")?;
        let config = normalize_config(config);
        let yaml = export_manifest_resources(&config)
            .iter()
            .map(crate::resource::Resource::to_yaml)
            .collect::<Result<Vec<_>>>()?
            .join("---\n");
        return Ok((config, yaml, version, updated_at));
    }

    anyhow::bail!(
        "[CONFIG_NOT_INITIALIZED] orchestrator manifest is not initialized in sqlite\n  category: validation\n  suggested_fix: run 'orchestrator apply -f <manifest.yaml>' first"
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
    let yaml = export_manifest_resources(&normalized)
        .iter()
        .map(crate::resource::Resource::to_yaml)
        .collect::<Result<Vec<_>>>()?
        .join("---\n");
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

    let Some((_yaml, json_raw, version, updated_at)) = row else {
        return Ok(None);
    };

    let config = serde_json::from_str::<OrchestratorConfig>(&json_raw)
        .context("failed to parse config_json from sqlite")?;
    Ok(Some((normalize_config(config), version, updated_at)))
}

pub fn persist_raw_config(
    db_path: &Path,
    config: OrchestratorConfig,
    author: &str,
) -> Result<ConfigOverview> {
    let normalized = normalize_config(config);
    let yaml = export_manifest_resources(&normalized)
        .iter()
        .map(crate::resource::Resource::to_yaml)
        .collect::<Result<Vec<_>>>()?
        .join("---\n");
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
            required_capability: step.required_capability.clone(),
            builtin: step.builtin.clone(),
            enabled: step.enabled,
            repeatable: step.repeatable,
            is_guard: step.is_guard,
            cost_preference: step.cost_preference.clone(),
            prehook: step.prehook.clone(),
            tty: step.tty,
            outputs: step.outputs.clone(),
            pipe_to: step.pipe_to.clone(),
            command: step.command.clone(),
            chain_steps: step
                .chain_steps
                .iter()
                .map(|cs| crate::config::TaskExecutionStep {
                    id: cs.id.clone(),
                    required_capability: cs.required_capability.clone(),
                    builtin: cs.builtin.clone(),
                    enabled: cs.enabled,
                    repeatable: cs.repeatable,
                    is_guard: cs.is_guard,
                    cost_preference: cs.cost_preference.clone(),
                    prehook: cs.prehook.clone(),
                    tty: cs.tty,
                    outputs: cs.outputs.clone(),
                    pipe_to: cs.pipe_to.clone(),
                    command: cs.command.clone(),
                    chain_steps: vec![],
                    scope: cs.scope,
                    behavior: cs.behavior.clone(),
                })
                .collect(),
            scope: step.scope,
            behavior: step.behavior.clone(),
        });
    }
    let loop_policy = workflow.loop_policy.clone();
    Ok(TaskExecutionPlan {
        steps,
        loop_policy,
        finalize: workflow.finalize.clone(),
    })
}

/// Validate safety configuration for self-referential workspaces.
/// Hard-errors if checkpoint_strategy is None; warns on missing auto_rollback or self_test step.
pub fn validate_self_referential_safety(
    workflow: &WorkflowConfig,
    workspace_id: &str,
) -> Result<()> {
    // Hard error: checkpoint_strategy must not be None
    if matches!(
        workflow.safety.checkpoint_strategy,
        crate::config::CheckpointStrategy::None
    ) {
        anyhow::bail!(
            "[SELF_REF_UNSAFE] workspace '{}' is self_referential but checkpoint_strategy is 'none'. \
             Self-referential workspaces MUST have a checkpoint strategy (e.g. git_tag) to enable rollback.",
            workspace_id
        );
    }

    // Warning: auto_rollback should be enabled
    if !workflow.safety.auto_rollback {
        eprintln!(
            "[warn] workspace '{}' is self_referential but auto_rollback is disabled. \
             Consider enabling auto_rollback for self-referential workspaces.",
            workspace_id
        );
    }

    // Warning: no self_test step in workflow
    let has_self_test = workflow.steps.iter().any(|s| s.id == "self_test");
    if !has_self_test {
        eprintln!(
            "[warn] workspace '{}' is self_referential but has no 'self_test' step in its workflow. \
             Consider adding a self_test step after 'implement' to catch breaking changes early.",
            workspace_id
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        LoopMode, WorkflowFinalizeConfig, WorkflowLoopConfig, WorkflowLoopGuardConfig,
    };
    #[allow(unused_imports)]
    use std::collections::HashMap;

    #[test]
    fn normalize_workflow_sets_builtin_for_self_test() {
        let mut workflow = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "self_test".to_string(),
                description: None,
                builtin: None,
                required_capability: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
            }],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    enabled: false,
                    ..WorkflowLoopGuardConfig::default()
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
        };

        normalize_workflow_config(&mut workflow);

        let self_test_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "self_test")
            .expect("self_test step should exist");
        assert_eq!(
            self_test_step.builtin.as_deref(),
            Some("self_test"),
            "builtin should be set to 'self_test' for SelfTest step type"
        );
    }

    #[test]
    fn normalize_workflow_preserves_multiple_self_test_steps() {
        let mut workflow = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "self_test_fail".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "self_test_recover".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
            ],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    enabled: false,
                    ..WorkflowLoopGuardConfig::default()
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
        };

        normalize_workflow_config(&mut workflow);

        let self_test_ids: Vec<&str> = workflow
            .steps
            .iter()
            .filter(|s| s.builtin.as_deref() == Some("self_test"))
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(self_test_ids, vec!["self_test_fail", "self_test_recover"]);
    }

    #[test]
    fn validate_workflow_config_allows_multiple_self_test_steps() {
        let workflow = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "self_test_fail".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "self_test_recover".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
            ],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    enabled: false,
                    ..WorkflowLoopGuardConfig::default()
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
        };

        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-workflow");
        assert!(
            result.is_ok(),
            "validation should allow multiple self_test steps"
        );
    }

    #[test]
    fn validate_workflow_config_allows_multiple_implement_steps() {
        let workflow = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "implement_phase_one".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: Some("echo phase-one".to_string()),
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "implement_phase_two".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: Some("echo phase-two".to_string()),
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
            ],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    enabled: false,
                    ..WorkflowLoopGuardConfig::default()
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
        };

        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-workflow");
        assert!(
            result.is_ok(),
            "validation should allow multiple implement steps when step ids are unique"
        );
    }

    #[test]
    fn validate_workflow_config_rejects_duplicate_step_ids() {
        let workflow = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "duplicate_step".to_string(),
                    description: None,
                    builtin: Some("self_test".to_string()),
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "duplicate_step".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: Some("echo duplicate".to_string()),
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
            ],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    enabled: false,
                    ..WorkflowLoopGuardConfig::default()
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
        };

        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-workflow");
        assert!(
            result.is_err(),
            "validation should reject duplicate step ids"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("duplicate step id 'duplicate_step'"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn validate_self_referential_safety_warns_missing_self_test() {
        let workflow = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "implement".to_string(),
                description: None,
                builtin: None,
                required_capability: Some("implement".to_string()),
                enabled: true,
                repeatable: true,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: None,
                behavior: StepBehavior::default(),
            }],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig::default(),
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitTag,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
            },
        };

        let result = validate_self_referential_safety(&workflow, "test-ws");
        assert!(
            result.is_ok(),
            "validation should pass even without self_test"
        );
    }

    #[test]
    fn validate_self_referential_safety_passes_with_self_test() {
        let workflow = WorkflowConfig {
            steps: vec![
                WorkflowStepConfig {
                    id: "implement".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: Some("implement".to_string()),
                    enabled: true,
                    repeatable: true,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
                WorkflowStepConfig {
                    id: "self_test".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None,
                    behavior: StepBehavior::default(),
                },
            ],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig::default(),
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitTag,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
            },
        };

        let result = validate_self_referential_safety(&workflow, "test-ws");
        assert!(result.is_ok(), "validation should pass with self_test step");
    }

    #[test]
    fn validate_self_referential_safety_errors_without_checkpoint_strategy() {
        let workflow = WorkflowConfig {
            steps: vec![],
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig::default(),
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::None,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
            },
        };

        let result = validate_self_referential_safety(&workflow, "test-ws");
        assert!(result.is_err(), "should error without checkpoint_strategy");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("checkpoint_strategy"),
            "error should mention checkpoint_strategy"
        );
    }

    // --- Helper to build a minimal step ---
    fn make_step(id: &str, enabled: bool) -> WorkflowStepConfig {
        WorkflowStepConfig {
            id: id.to_string(),
            description: None,
            builtin: None,
            required_capability: None,
            enabled,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: None,
            behavior: StepBehavior::default(),
        }
    }

    fn make_builtin_step(id: &str, builtin: &str, enabled: bool) -> WorkflowStepConfig {
        WorkflowStepConfig {
            builtin: Some(builtin.to_string()),
            ..make_step(id, enabled)
        }
    }

    fn make_command_step(id: &str, cmd: &str) -> WorkflowStepConfig {
        WorkflowStepConfig {
            command: Some(cmd.to_string()),
            ..make_step(id, true)
        }
    }

    fn make_workflow(steps: Vec<WorkflowStepConfig>) -> WorkflowConfig {
        WorkflowConfig {
            steps,
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    enabled: false,
                    ..WorkflowLoopGuardConfig::default()
                },
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            safety: crate::config::SafetyConfig::default(),
        }
    }

    fn make_config_with_agent(capability: &str, template: &str) -> OrchestratorConfig {
        use crate::config::AgentConfig;
        let mut templates = HashMap::new();
        templates.insert(capability.to_string(), template.to_string());
        let mut agents = HashMap::new();
        agents.insert(
            "test-agent".to_string(),
            AgentConfig {
                templates,
                ..AgentConfig::default()
            },
        );
        OrchestratorConfig {
            agents,
            ..OrchestratorConfig::default()
        }
    }

    // ======= now_ts tests =======

    #[test]
    fn now_ts_returns_rfc3339_string() {
        let ts = now_ts();
        assert!(!ts.is_empty());
        // Should be parseable as RFC3339
        let parsed = chrono::DateTime::parse_from_rfc3339(&ts);
        assert!(parsed.is_ok(), "now_ts should return valid RFC3339: {}", ts);
    }

    #[test]
    fn now_ts_returns_recent_timestamp() {
        let before = chrono::Utc::now();
        let ts = now_ts();
        let after = chrono::Utc::now();
        let parsed = chrono::DateTime::parse_from_rfc3339(&ts).unwrap();
        assert!(parsed >= before, "timestamp should be >= before");
        assert!(parsed <= after, "timestamp should be <= after");
    }

    // ======= normalize_workflow_config tests =======

    #[test]
    fn normalize_empty_steps_generates_defaults() {
        let mut workflow = make_workflow(vec![]);
        normalize_workflow_config(&mut workflow);
        assert!(
            !workflow.steps.is_empty(),
            "empty steps should generate default steps"
        );
    }

    #[test]
    fn normalize_sets_required_capability_for_qa_step() {
        let mut workflow = make_workflow(vec![make_step("qa", true)]);
        normalize_workflow_config(&mut workflow);
        let qa_step = workflow.steps.iter().find(|s| s.id == "qa").unwrap();
        assert_eq!(qa_step.required_capability.as_deref(), Some("qa"));
    }

    #[test]
    fn normalize_sets_required_capability_for_fix_step() {
        let mut workflow = make_workflow(vec![make_step("fix", true)]);
        normalize_workflow_config(&mut workflow);
        let fix_step = workflow.steps.iter().find(|s| s.id == "fix").unwrap();
        assert_eq!(fix_step.required_capability.as_deref(), Some("fix"));
    }

    #[test]
    fn normalize_sets_required_capability_for_plan_step() {
        let mut workflow = make_workflow(vec![make_step("plan", true)]);
        normalize_workflow_config(&mut workflow);
        let plan_step = workflow.steps.iter().find(|s| s.id == "plan").unwrap();
        assert_eq!(plan_step.required_capability.as_deref(), Some("plan"));
    }

    #[test]
    fn normalize_sets_required_capability_for_implement_step() {
        let mut workflow = make_workflow(vec![make_step("implement", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "implement").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("implement"));
    }

    #[test]
    fn normalize_sets_required_capability_for_review_step() {
        let mut workflow = make_workflow(vec![make_step("review", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "review").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("review"));
    }

    #[test]
    fn normalize_sets_required_capability_for_build_step() {
        let mut workflow = make_workflow(vec![make_step("build", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "build").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("build"));
    }

    #[test]
    fn normalize_sets_required_capability_for_test_step() {
        let mut workflow = make_workflow(vec![make_step("test", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "test").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("test"));
    }

    #[test]
    fn normalize_sets_required_capability_for_lint_step() {
        let mut workflow = make_workflow(vec![make_step("lint", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "lint").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("lint"));
    }

    #[test]
    fn normalize_sets_required_capability_for_gitops_step() {
        let mut workflow = make_workflow(vec![make_step("git_ops", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "git_ops").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("git_ops"));
    }

    #[test]
    fn normalize_sets_required_capability_for_qa_doc_gen_step() {
        let mut workflow = make_workflow(vec![make_step("qa_doc_gen", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "qa_doc_gen").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("qa_doc_gen"));
    }

    #[test]
    fn normalize_sets_required_capability_for_qa_testing_step() {
        let mut workflow = make_workflow(vec![make_step("qa_testing", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "qa_testing").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("qa_testing"));
    }

    #[test]
    fn normalize_sets_required_capability_for_ticket_fix_step() {
        let mut workflow = make_workflow(vec![make_step("ticket_fix", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "ticket_fix").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("ticket_fix"));
    }

    #[test]
    fn normalize_sets_required_capability_for_doc_governance_step() {
        let mut workflow = make_workflow(vec![make_step("doc_governance", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "doc_governance").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("doc_governance"));
    }

    #[test]
    fn normalize_sets_required_capability_for_align_tests_step() {
        let mut workflow = make_workflow(vec![make_step("align_tests", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "align_tests").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("align_tests"));
    }

    #[test]
    fn normalize_sets_required_capability_for_retest_step() {
        let mut workflow = make_workflow(vec![make_step("retest", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "retest").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("retest"));
    }

    #[test]
    fn normalize_sets_required_capability_for_smoke_chain_step() {
        let mut workflow = make_workflow(vec![make_step("smoke_chain", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "smoke_chain").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("smoke_chain"));
    }

    #[test]
    fn normalize_sets_loop_guard_builtin_and_is_guard() {
        let mut workflow = make_workflow(vec![make_step("loop_guard", true)]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "loop_guard").unwrap();
        assert_eq!(step.builtin.as_deref(), Some("loop_guard"));
        assert!(step.is_guard, "LoopGuard step should have is_guard=true");
    }

    #[test]
    fn normalize_sets_default_behavior_for_qa_step() {
        let mut workflow = make_workflow(vec![make_step("qa", true)]);
        normalize_workflow_config(&mut workflow);
        let qa = workflow.steps.iter().find(|s| s.id == "qa").unwrap();
        assert!(
            qa.behavior.collect_artifacts,
            "qa step should have collect_artifacts=true"
        );
        assert!(
            qa.behavior.captures.iter().any(|c| c.var == "qa_failed"
                && c.source == CaptureSource::FailedFlag),
            "qa step should capture qa_failed from FailedFlag"
        );
    }

    #[test]
    fn normalize_sets_default_behavior_for_fix_step() {
        let mut workflow = make_workflow(vec![make_step("fix", true)]);
        normalize_workflow_config(&mut workflow);
        let fix = workflow.steps.iter().find(|s| s.id == "fix").unwrap();
        assert!(
            fix.behavior.captures.iter().any(|c| c.var == "fix_success"
                && c.source == CaptureSource::SuccessFlag),
            "fix step should capture fix_success from SuccessFlag"
        );
    }

    #[test]
    fn normalize_sets_default_behavior_for_retest_step() {
        let mut workflow = make_workflow(vec![make_step("retest", true)]);
        normalize_workflow_config(&mut workflow);
        let retest = workflow.steps.iter().find(|s| s.id == "retest").unwrap();
        assert!(
            retest.behavior.collect_artifacts,
            "retest step should have collect_artifacts=true"
        );
        assert!(
            retest.behavior.captures.iter().any(|c| c.var == "retest_success"
                && c.source == CaptureSource::SuccessFlag),
            "retest step should capture retest_success from SuccessFlag"
        );
    }

    #[test]
    fn normalize_does_not_duplicate_existing_captures() {
        let mut step = make_step("qa", true);
        step.behavior.captures.push(CaptureDecl {
            var: "qa_failed".to_string(),
            source: CaptureSource::FailedFlag,
        });
        let mut workflow = make_workflow(vec![step]);
        normalize_workflow_config(&mut workflow);
        let qa = workflow.steps.iter().find(|s| s.id == "qa").unwrap();
        let qa_failed_count = qa.behavior.captures.iter()
            .filter(|c| c.var == "qa_failed")
            .count();
        assert_eq!(qa_failed_count, 1, "should not duplicate existing qa_failed capture");
    }

    #[test]
    fn normalize_skips_capability_if_builtin_already_set() {
        let mut step = make_step("qa", true);
        step.builtin = Some("custom_builtin".to_string());
        let mut workflow = make_workflow(vec![step]);
        normalize_workflow_config(&mut workflow);
        let qa_step = workflow.steps.iter().find(|s| s.id == "qa").unwrap();
        // builtin was set, so required_capability should NOT be overridden
        assert_eq!(qa_step.builtin.as_deref(), Some("custom_builtin"));
        assert!(qa_step.required_capability.is_none());
    }

    #[test]
    fn normalize_skips_capability_if_command_already_set() {
        let mut step = make_step("qa", true);
        step.command = Some("echo test".to_string());
        let mut workflow = make_workflow(vec![step]);
        normalize_workflow_config(&mut workflow);
        let qa_step = workflow.steps.iter().find(|s| s.id == "qa").unwrap();
        assert!(qa_step.required_capability.is_none());
    }

    #[test]
    fn normalize_adds_missing_standard_steps_as_disabled() {
        let mut workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
        ]);
        normalize_workflow_config(&mut workflow);
        // Should add init_once, plan, qa, ticket_scan, fix, retest as disabled
        let init_step = workflow.steps.iter().find(|s| s.id == "init_once");
        assert!(init_step.is_some(), "should add init_once step");
        assert!(!init_step.unwrap().enabled, "added init_once should be disabled");

        let plan_step = workflow.steps.iter().find(|s| s.id == "plan");
        assert!(plan_step.is_some(), "should add plan step");
        assert!(!plan_step.unwrap().enabled, "added plan should be disabled");
    }

    #[test]
    fn normalize_does_not_duplicate_existing_step_types() {
        let mut workflow = make_workflow(vec![
            make_step("plan", true),
        ]);
        normalize_workflow_config(&mut workflow);
        let plan_count = workflow.steps.iter().filter(|s| s.id == "plan").count();
        assert_eq!(plan_count, 1, "should not duplicate already-present plan step");
    }

    #[test]
    fn normalize_clears_qa_fix_retest_legacy_fields() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.qa = Some("qa_template".to_string());
        workflow.fix = Some("fix_template".to_string());
        workflow.retest = Some("retest_template".to_string());
        normalize_workflow_config(&mut workflow);
        assert!(workflow.qa.is_none(), "qa should be cleared");
        assert!(workflow.fix.is_none(), "fix should be cleared");
        assert!(workflow.retest.is_none(), "retest should be cleared");
    }

    #[test]
    fn normalize_sets_default_finalize_rules_when_empty() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        assert!(workflow.finalize.rules.is_empty());
        normalize_workflow_config(&mut workflow);
        assert!(
            !workflow.finalize.rules.is_empty(),
            "should set default finalize rules when empty"
        );
    }

    #[test]
    fn normalize_clears_guard_agent_template() {
        let mut workflow = make_workflow(vec![make_builtin_step("self_test", "self_test", true)]);
        workflow.loop_policy.guard.agent_template = Some("old_template".to_string());
        normalize_workflow_config(&mut workflow);
        assert!(
            workflow.loop_policy.guard.agent_template.is_none(),
            "agent_template should be cleared"
        );
    }

    #[test]
    fn normalize_preserves_step_id() {
        let step = make_step("plan", true);
        let mut workflow = make_workflow(vec![step]);
        normalize_workflow_config(&mut workflow);
        let plan_step = workflow.steps.iter().find(|s| s.id == "plan").unwrap();
        assert_eq!(plan_step.id, "plan", "step id should be preserved");
    }

    #[test]
    fn normalize_sets_required_capability_from_id() {
        // Step with id matching a known type gets required_capability set
        let step = make_step("plan", true);
        let mut workflow = make_workflow(vec![step]);
        normalize_workflow_config(&mut workflow);
        let step = workflow.steps.iter().find(|s| s.id == "plan").unwrap();
        assert_eq!(step.required_capability.as_deref(), Some("plan"));
    }

    #[test]
    fn normalize_enables_ticket_scan_when_fix_only() {
        // fix enabled, qa disabled, retest disabled, no prior ticket_scan
        let mut workflow = make_workflow(vec![
            make_command_step("fix", "echo fix"),
        ]);
        normalize_workflow_config(&mut workflow);
        let scan = workflow.steps.iter().find(|s| s.id == "ticket_scan");
        assert!(scan.is_some(), "ticket_scan should exist");
        assert!(scan.unwrap().enabled, "ticket_scan should be enabled when fix is enabled but qa is not");
    }

    #[test]
    fn normalize_does_not_enable_ticket_scan_when_qa_also_enabled() {
        let mut workflow = make_workflow(vec![
            make_command_step("qa", "echo qa"),
            make_command_step("fix", "echo fix"),
        ]);
        normalize_workflow_config(&mut workflow);
        let scan = workflow.steps.iter().find(|s| s.id == "ticket_scan");
        // ticket_scan should still exist (as disabled placeholder) since it wasn't in steps
        if let Some(s) = scan {
            assert!(!s.enabled, "ticket_scan should NOT be auto-enabled when qa is also enabled");
        }
    }

    // ======= validate_workflow_config tests =======

    #[test]
    fn validate_workflow_rejects_empty_steps() {
        let workflow = make_workflow(vec![]);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("at least one step"));
    }

    #[test]
    fn validate_workflow_rejects_no_enabled_steps() {
        let workflow = make_workflow(vec![
            make_step("qa", false),
        ]);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no enabled steps"));
    }

    #[test]
    fn validate_workflow_rejects_missing_agent_template() {
        // Step has no builtin, command, or chain_steps, and no agent provides the template
        let workflow = make_workflow(vec![
            make_step("qa", true),
        ]);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no agent has template"));
    }

    #[test]
    fn validate_workflow_accepts_step_with_agent_template() {
        let workflow = make_workflow(vec![
            make_step("qa", true),
        ]);
        let config = make_config_with_agent("qa", "qa_template.md");
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_ok(), "should accept step when agent has template: {:?}", result.err());
    }

    #[test]
    fn validate_workflow_accepts_builtin_step_without_agent() {
        let workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
        ]);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_ok(), "builtin steps should not require agent: {:?}", result.err());
    }

    #[test]
    fn validate_workflow_accepts_command_step_without_agent() {
        let workflow = make_workflow(vec![
            make_command_step("build", "cargo build"),
        ]);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_ok(), "command steps should not require agent: {:?}", result.err());
    }

    #[test]
    fn validate_workflow_accepts_chain_steps_without_agent() {
        let mut step = make_step("smoke_chain", true);
        step.chain_steps = vec![make_command_step("sub", "echo sub")];
        let workflow = make_workflow(vec![step]);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_ok(), "chain_steps should count as self-contained: {:?}", result.err());
    }

    #[test]
    fn validate_workflow_rejects_zero_max_cycles() {
        let mut workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.loop_policy.guard.max_cycles = Some(0);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("max_cycles must be > 0"));
    }

    #[test]
    fn validate_workflow_rejects_fixed_without_max_cycles() {
        let mut workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.loop_policy.mode = LoopMode::Fixed;
        workflow.loop_policy.guard.max_cycles = None;
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("loop.mode=fixed requires guard.max_cycles"));
    }

    #[test]
    fn validate_workflow_accepts_fixed_with_max_cycles() {
        let mut workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.loop_policy.mode = LoopMode::Fixed;
        workflow.loop_policy.guard.max_cycles = Some(2);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_ok(), "fixed mode with max_cycles should pass: {:?}", result.err());
    }

    #[test]
    fn validate_workflow_rejects_guard_enabled_without_loop_guard_agent() {
        let mut workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.loop_policy.guard.enabled = true;
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no agent has loop_guard template"));
    }

    #[test]
    fn validate_workflow_accepts_guard_enabled_with_loop_guard_agent() {
        let mut workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.loop_policy.guard.enabled = true;
        let config = make_config_with_agent("loop_guard", "loop_guard_template.md");
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_ok(), "guard with agent should pass: {:?}", result.err());
    }

    #[test]
    fn validate_workflow_skips_disabled_steps() {
        // Disabled step has no agent - should be fine
        let workflow = make_workflow(vec![
            make_step("qa", false),
            make_builtin_step("self_test", "self_test", true),
        ]);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_ok(), "disabled step missing agent should not error: {:?}", result.err());
    }

    #[test]
    fn validate_workflow_allows_ticket_scan_without_agent() {
        let workflow = make_workflow(vec![
            make_step("ticket_scan", true),
            make_builtin_step("self_test", "self_test", true),
        ]);
        let config = OrchestratorConfig::default();
        let result = validate_workflow_config(&config, &workflow, "test-wf");
        assert!(result.is_ok(), "ticket_scan should not require agent: {:?}", result.err());
    }

    // ======= build_execution_plan tests =======

    #[test]
    fn build_execution_plan_returns_only_enabled_steps() {
        let workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
            make_step("qa", false),
        ]);
        let config = OrchestratorConfig::default();
        let plan = build_execution_plan(&config, &workflow, "test-wf").unwrap();
        assert_eq!(plan.steps.len(), 1, "should only contain enabled steps");
        assert_eq!(plan.steps[0].id, "self_test");
    }

    #[test]
    fn build_execution_plan_copies_step_fields() {
        let mut step = make_command_step("build", "cargo build");
        step.repeatable = false;
        step.tty = true;
        step.outputs = vec!["result".to_string()];
        step.pipe_to = Some("next_step".to_string());
        step.cost_preference = Some(crate::config::CostPreference::Quality);
        step.scope = Some(crate::config::StepScope::Task);
        let workflow = make_workflow(vec![step]);
        let config = OrchestratorConfig::default();
        let plan = build_execution_plan(&config, &workflow, "test-wf").unwrap();
        let s = &plan.steps[0];
        assert_eq!(s.id, "build");
        assert_eq!(s.command.as_deref(), Some("cargo build"));
        assert!(!s.repeatable);
        assert!(s.tty);
        assert_eq!(s.outputs, vec!["result"]);
        assert_eq!(s.pipe_to.as_deref(), Some("next_step"));
        assert_eq!(s.cost_preference, Some(crate::config::CostPreference::Quality));
        assert_eq!(s.scope, Some(crate::config::StepScope::Task));
    }

    #[test]
    fn build_execution_plan_includes_chain_steps() {
        let mut step = make_step("smoke_chain", true);
        step.chain_steps = vec![
            make_command_step("sub1", "cargo build"),
            make_command_step("sub2", "cargo test"),
        ];
        let workflow = make_workflow(vec![step]);
        let config = OrchestratorConfig::default();
        let plan = build_execution_plan(&config, &workflow, "test-wf").unwrap();
        assert_eq!(plan.steps[0].chain_steps.len(), 2);
        assert_eq!(plan.steps[0].chain_steps[0].id, "sub1");
        assert_eq!(plan.steps[0].chain_steps[1].id, "sub2");
    }

    #[test]
    fn build_execution_plan_copies_loop_policy() {
        let mut workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.loop_policy.mode = LoopMode::Fixed;
        workflow.loop_policy.guard.max_cycles = Some(3);
        let config = OrchestratorConfig::default();
        let plan = build_execution_plan(&config, &workflow, "test-wf").unwrap();
        assert!(matches!(plan.loop_policy.mode, LoopMode::Fixed));
        assert_eq!(plan.loop_policy.guard.max_cycles, Some(3));
    }

    #[test]
    fn build_execution_plan_copies_finalize_config() {
        let mut workflow = make_workflow(vec![
            make_builtin_step("self_test", "self_test", true),
        ]);
        workflow.finalize = crate::config::default_workflow_finalize_config();
        let config = OrchestratorConfig::default();
        let plan = build_execution_plan(&config, &workflow, "test-wf").unwrap();
        assert!(
            !plan.finalize.rules.is_empty(),
            "finalize rules should be copied"
        );
    }

    #[test]
    fn build_execution_plan_fails_on_invalid_workflow() {
        let workflow = make_workflow(vec![]); // empty steps
        let config = OrchestratorConfig::default();
        let result = build_execution_plan(&config, &workflow, "test-wf");
        assert!(result.is_err(), "should fail validation");
    }

    // ======= resolve_workspace_path tests =======

    #[test]
    fn resolve_workspace_path_joins_rel_path() {
        let root = std::env::temp_dir();
        let result = resolve_workspace_path(&root, "subdir/file.md", "test_field");
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.starts_with(&root));
        assert!(path.ends_with("subdir/file.md"));
    }

    #[test]
    fn resolve_workspace_path_rejects_absolute_path() {
        let root = std::env::temp_dir();
        let result = resolve_workspace_path(&root, "/etc/passwd", "test_field");
        assert!(result.is_err(), "should reject absolute path");
    }

    #[test]
    fn resolve_workspace_path_rejects_empty_path() {
        let root = std::env::temp_dir();
        let result = resolve_workspace_path(&root, "", "test_field");
        assert!(result.is_err(), "should reject empty path");
    }

    #[test]
    fn resolve_workspace_path_rejects_whitespace_path() {
        let root = std::env::temp_dir();
        let result = resolve_workspace_path(&root, "   ", "test_field");
        assert!(result.is_err(), "should reject whitespace-only path");
    }

    #[test]
    fn resolve_workspace_path_validates_existing_path_within_root() {
        // Use the temp dir itself as root, and "." as the path
        let root = std::env::temp_dir();
        // Create a subdir to test with
        let sub = root.join(format!("test-resolve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&sub).unwrap();
        let rel = sub.file_name().unwrap().to_str().unwrap();
        let result = resolve_workspace_path(&root, rel, "test_field");
        assert!(result.is_ok(), "existing subdir within root should pass: {:?}", result.err());
        std::fs::remove_dir_all(&sub).ok();
    }

    // ======= ensure_within_root tests =======

    #[test]
    fn ensure_within_root_accepts_child_path() {
        let root = std::env::temp_dir();
        let child = root.join(format!("test-within-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&child).unwrap();
        let result = ensure_within_root(&root, &child, "test");
        assert!(result.is_ok());
        std::fs::remove_dir_all(&child).ok();
    }

    #[test]
    fn ensure_within_root_rejects_nonexistent_path() {
        let root = std::env::temp_dir();
        let nonexistent = root.join("nonexistent-path-xyz-abc");
        let result = ensure_within_root(&root, &nonexistent, "test");
        assert!(result.is_err(), "should fail for nonexistent path");
    }

    // ======= validate_self_referential_safety additional tests =======

    #[test]
    fn validate_self_referential_safety_warns_disabled_auto_rollback() {
        let workflow = WorkflowConfig {
            steps: vec![make_step("implement", true)],
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitStash,
                auto_rollback: false, // should trigger warning
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
            },
            ..make_workflow(vec![])
        };
        // Should pass (warning only, not error)
        let result = validate_self_referential_safety(&workflow, "test-ws");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_self_referential_safety_passes_with_git_stash() {
        let workflow = WorkflowConfig {
            steps: vec![
                make_step("implement", true),
                make_step("self_test", true),
            ],
            safety: crate::config::SafetyConfig {
                checkpoint_strategy: crate::config::CheckpointStrategy::GitStash,
                auto_rollback: true,
                max_consecutive_failures: 3,
                step_timeout_secs: None,
                binary_snapshot: false,
            },
            ..make_workflow(vec![])
        };
        let result = validate_self_referential_safety(&workflow, "test-ws");
        assert!(result.is_ok());
    }

    // ======= normalize_config tests =======

    #[test]
    fn normalize_config_normalizes_all_workflows() {
        let mut workflows = HashMap::new();
        workflows.insert("wf1".to_string(), make_workflow(vec![]));
        workflows.insert("wf2".to_string(), make_workflow(vec![]));
        let config = OrchestratorConfig {
            workflows,
            ..OrchestratorConfig::default()
        };
        let normalized = normalize_config(config);
        for (_, wf) in &normalized.workflows {
            assert!(!wf.steps.is_empty(), "all workflows should be normalized");
        }
    }

    #[test]
    fn normalize_preserves_required_capability_on_custom_step_ids() {
        let steps = vec![WorkflowStepConfig {
            id: "run_qa".to_string(),
            description: None,
            required_capability: Some("qa".to_string()),
            builtin: None,
            enabled: true,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: None,
            behavior: StepBehavior::default(),
        }];
        let mut wf = make_workflow(steps);
        normalize_workflow_config(&mut wf);

        let run_qa = wf.steps.iter().find(|s| s.id == "run_qa").unwrap();
        assert_eq!(
            run_qa.required_capability,
            Some("qa".to_string()),
            "required_capability must survive normalization"
        );

        let json = serde_json::to_string_pretty(run_qa).unwrap();
        assert!(
            json.contains("required_capability"),
            "required_capability must appear in JSON: {}",
            json
        );
    }

    // ======= resolve_and_validate_workspaces tests =======

    #[test]
    fn resolve_and_validate_rejects_empty_workspaces() {
        let config = OrchestratorConfig::default();
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("EMPTY_WORKSPACES"));
    }

    #[test]
    fn resolve_and_validate_rejects_empty_agents() {
        use crate::config::WorkspaceConfig;
        let mut workspaces = HashMap::new();
        workspaces.insert(
            "ws1".to_string(),
            WorkspaceConfig {
                root_path: "/tmp".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        );
        let config = OrchestratorConfig {
            workspaces,
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("EMPTY_AGENTS"));
    }

    #[test]
    fn resolve_and_validate_rejects_empty_workflows() {
        use crate::config::{AgentConfig, WorkspaceConfig};
        let mut workspaces = HashMap::new();
        workspaces.insert(
            "ws1".to_string(),
            WorkspaceConfig {
                root_path: "/tmp".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        );
        let mut agents = HashMap::new();
        agents.insert("agent1".to_string(), AgentConfig::default());
        let config = OrchestratorConfig {
            workspaces,
            agents,
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("EMPTY_WORKFLOWS"));
    }

    #[test]
    fn resolve_and_validate_rejects_empty_workspace_id() {
        use crate::config::{AgentConfig, WorkspaceConfig};
        let mut workspaces = HashMap::new();
        workspaces.insert(
            "".to_string(), // empty id
            WorkspaceConfig {
                root_path: "/tmp".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        );
        let mut agents = HashMap::new();
        agents.insert("agent1".to_string(), AgentConfig::default());
        let mut workflows = HashMap::new();
        workflows.insert("wf1".to_string(), make_workflow(vec![make_builtin_step("self_test", "self_test", true)]));
        let config = OrchestratorConfig {
            workspaces,
            agents,
            workflows,
            defaults: crate::config::ConfigDefaults {
                project: "default".to_string(),
                workspace: "".to_string(),
                workflow: "wf1".to_string(),
            },
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("INVALID_WORKSPACE"));
    }

    #[test]
    fn resolve_and_validate_rejects_empty_qa_targets() {
        use crate::config::{AgentConfig, WorkspaceConfig};
        let mut workspaces = HashMap::new();
        workspaces.insert(
            "ws1".to_string(),
            WorkspaceConfig {
                root_path: "/tmp".to_string(),
                qa_targets: vec![], // empty
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        );
        let mut agents = HashMap::new();
        agents.insert("agent1".to_string(), AgentConfig::default());
        let mut workflows = HashMap::new();
        workflows.insert("wf1".to_string(), make_workflow(vec![make_builtin_step("self_test", "self_test", true)]));
        let config = OrchestratorConfig {
            workspaces,
            agents,
            workflows,
            defaults: crate::config::ConfigDefaults {
                project: "default".to_string(),
                workspace: "ws1".to_string(),
                workflow: "wf1".to_string(),
            },
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/tmp"), &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("qa_targets cannot be empty"));
    }

    #[test]
    fn resolve_and_validate_rejects_missing_default_workflow() {
        use crate::config::{AgentConfig, WorkspaceConfig};
        let ws_root = std::env::temp_dir().join(format!("test-ws-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&ws_root).unwrap();
        let qa_dir = ws_root.join("docs");
        std::fs::create_dir_all(&qa_dir).unwrap();
        let ticket_dir = ws_root.join("tickets");
        std::fs::create_dir_all(&ticket_dir).unwrap();

        let mut workspaces = HashMap::new();
        workspaces.insert(
            "ws1".to_string(),
            WorkspaceConfig {
                root_path: ws_root.to_string_lossy().to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        );
        let mut agents = HashMap::new();
        agents.insert("agent1".to_string(), AgentConfig::default());
        let mut workflows = HashMap::new();
        workflows.insert("wf1".to_string(), make_workflow(vec![make_builtin_step("self_test", "self_test", true)]));
        let config = OrchestratorConfig {
            workspaces,
            agents,
            workflows,
            defaults: crate::config::ConfigDefaults {
                project: "default".to_string(),
                workspace: "ws1".to_string(),
                workflow: "nonexistent_wf".to_string(), // does not exist
            },
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_workspaces(Path::new("/"), &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("defaults.workflow"));
        std::fs::remove_dir_all(&ws_root).ok();
    }

    // ======= resolve_and_validate_projects tests =======

    #[test]
    fn resolve_and_validate_projects_empty_config() {
        let config = OrchestratorConfig::default();
        let result = resolve_and_validate_projects(Path::new("/tmp"), &config);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn resolve_and_validate_projects_resolves_workspaces() {
        use crate::config::{ProjectConfig, WorkspaceConfig};
        let mut projects = HashMap::new();
        let mut ws = HashMap::new();
        ws.insert(
            "proj-ws".to_string(),
            WorkspaceConfig {
                root_path: "some/relative/path".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        );
        projects.insert(
            "proj1".to_string(),
            ProjectConfig {
                description: None,
                workspaces: ws,
                agents: HashMap::new(),
                workflows: HashMap::new(),
            },
        );
        let config = OrchestratorConfig {
            projects,
            ..OrchestratorConfig::default()
        };
        let result = resolve_and_validate_projects(Path::new("/app"), &config).unwrap();
        assert!(result.contains_key("proj1"));
        let proj = &result["proj1"];
        assert!(proj.workspaces.contains_key("proj-ws"));
        assert!(proj.workspaces["proj-ws"].root_path.starts_with("/app"));
    }

    // ======= enforce_deletion_guards tests =======

    #[test]
    fn enforce_deletion_guards_allows_no_removals() {
        let db_path = std::env::temp_dir().join(format!("test-guard-{}.db", uuid::Uuid::new_v4()));
        crate::db::init_schema(&db_path).unwrap();
        let conn = crate::db::open_conn(&db_path).unwrap();
        let config = OrchestratorConfig::default();
        let result = enforce_deletion_guards(&conn, &config, &config);
        assert!(result.is_ok());
        std::fs::remove_file(&db_path).ok();
    }

    #[test]
    fn enforce_deletion_guards_allows_removing_unused_workspace() {
        use crate::config::WorkspaceConfig;
        let db_path = std::env::temp_dir().join(format!("test-guard-{}.db", uuid::Uuid::new_v4()));
        crate::db::init_schema(&db_path).unwrap();
        let conn = crate::db::open_conn(&db_path).unwrap();
        let mut previous_workspaces = HashMap::new();
        previous_workspaces.insert(
            "ws-to-remove".to_string(),
            WorkspaceConfig {
                root_path: "/tmp".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
                self_referential: false,
            },
        );
        let previous = OrchestratorConfig {
            workspaces: previous_workspaces,
            ..OrchestratorConfig::default()
        };
        let candidate = OrchestratorConfig::default(); // ws removed
        let result = enforce_deletion_guards(&conn, &previous, &candidate);
        assert!(result.is_ok(), "removing unused workspace should be allowed");
        std::fs::remove_file(&db_path).ok();
    }

    #[test]
    fn enforce_deletion_guards_allows_removing_unused_workflow() {
        let db_path = std::env::temp_dir().join(format!("test-guard-{}.db", uuid::Uuid::new_v4()));
        crate::db::init_schema(&db_path).unwrap();
        let conn = crate::db::open_conn(&db_path).unwrap();
        let mut previous_workflows = HashMap::new();
        previous_workflows.insert("wf-to-remove".to_string(), make_workflow(vec![]));
        let previous = OrchestratorConfig {
            workflows: previous_workflows,
            ..OrchestratorConfig::default()
        };
        let candidate = OrchestratorConfig::default();
        let result = enforce_deletion_guards(&conn, &previous, &candidate);
        assert!(result.is_ok(), "removing unused workflow should be allowed");
        std::fs::remove_file(&db_path).ok();
    }
}
