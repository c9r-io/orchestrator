use crate::collab::MessageBus;
use crate::config::{
    AgentConfig, AgentMetadata, AgentSelectionConfig, LoopMode, OrchestratorConfig, ProjectConfig,
    PromptDelivery, ResourceMetadataStore, ResumeConfig, RunnerConfig, SafetyConfig,
    StepBehavior, WorkflowConfig, WorkflowFinalizeConfig, WorkflowLoopConfig,
    WorkflowLoopGuardConfig, WorkflowStepConfig, WorkspaceConfig,
};
use crate::config_load::{
    build_active_config, load_raw_config_from_db, persist_raw_config, read_active_config,
};
use crate::db::init_schema;
use crate::events::NoopSink;
use crate::state::InnerState;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::config::ResolvedWorkspace;

fn backfill_default_scope_data(
    _db_path: &Path,
    _workspace_id: &str,
    _workflow_id: &str,
    _workspace: &ResolvedWorkspace,
) -> anyhow::Result<()> {
    Ok(())
}

fn create_minimal_test_config() -> OrchestratorConfig {
    OrchestratorConfig {
        runner: RunnerConfig {
            shell: "/bin/bash".to_string(),
            shell_arg: "-lc".to_string(),
            ..RunnerConfig::default()
        },
        resume: ResumeConfig { auto: false },
        observability: crate::config::ObservabilityConfig::default(),
        projects: {
            let mut projects = HashMap::new();
            projects.insert(
                crate::config::DEFAULT_PROJECT_ID.to_string(),
                ProjectConfig {
                    description: Some("Built-in default project".to_string()),
                    workspaces: {
                        let mut ws = HashMap::new();
                        ws.insert(
                            "default".to_string(),
                            WorkspaceConfig {
                                root_path: "workspace/default".to_string(),
                                qa_targets: vec!["docs/qa".to_string()],
                                ticket_dir: "docs/ticket".to_string(),
                                self_referential: false,
                            },
                        );
                        ws
                    },
                    agents: {
                        let mut agents = HashMap::new();
                        agents.insert(
                            "echo".to_string(),
                            AgentConfig {
                                metadata: AgentMetadata {
                                    name: "echo".to_string(),
                                    description: Some("Echo agent for testing".to_string()),
                                    version: None,
                                    cost: Some(1),
                                },
                                capabilities: vec!["qa".to_string()],
                                command: "echo 'qa: {rel_path}'".to_string(),
                                selection: AgentSelectionConfig::default(),
                                env: None,
                                prompt_delivery: PromptDelivery::default(),
                            },
                        );
                        agents
                    },
                    workflows: {
                        let mut workflows = HashMap::new();
                        workflows.insert(
                            "basic".to_string(),
                            WorkflowConfig {
                                steps: vec![WorkflowStepConfig {
                                    id: "qa".to_string(),
                                    description: None,
                                    builtin: None,
                                    required_capability: None,
                                    enabled: true,
                                    repeatable: false,
                                    is_guard: false,
                                    cost_preference: None,
                                    prehook: None,
                                    tty: false,
                                    template: None,
                                    outputs: Vec::new(),
                                    pipe_to: None,
                                    command: None,
                                    chain_steps: vec![],
                                    scope: None,
                                    behavior: StepBehavior::default(),
                                    max_parallel: None,
                                    timeout_secs: None,
                                    item_select_config: None,
                                    store_inputs: vec![],
                                    store_outputs: vec![],
                                }],
                                loop_policy: WorkflowLoopConfig {
                                    mode: LoopMode::Once,
                                    guard: WorkflowLoopGuardConfig {
                                        enabled: false,
                                        stop_when_no_unresolved: false,
                                        max_cycles: None,
                                        agent_template: None,
                                    },
                                },
                                finalize: WorkflowFinalizeConfig { rules: vec![] },
                                qa: None,
                                fix: None,
                                retest: None,
                                dynamic_steps: vec![],
                                adaptive: None,
                                safety: SafetyConfig::default(),
                                max_parallel: None,
                            },
                        );
                        workflows
                    },
                    step_templates: HashMap::new(),
                    env_stores: HashMap::new(),
                },
            );
            projects
        },
        resource_meta: ResourceMetadataStore::default(),
        custom_resource_definitions: HashMap::new(),
        custom_resources: HashMap::new(),
        resource_store: Default::default(),
    }
}

pub(crate) struct TestState {
    temp_root: PathBuf,
    config: OrchestratorConfig,
    state: Option<Arc<InnerState>>,
}

impl TestState {
    pub(crate) fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!(
            "agent-orchestrator-test-{}-{}",
            nonce,
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp_root).expect("failed to create test temp root");

        let config = create_minimal_test_config();
        Self {
            temp_root,
            config,
            state: None,
        }
    }

    pub(crate) fn with_workspace(
        mut self,
        name: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        let workspace_id = name.into();
        self.config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .workspaces
            .insert(
            workspace_id.clone(),
            WorkspaceConfig {
                root_path: path.into(),
                qa_targets: vec!["docs/qa".to_string(), "docs/security".to_string()],
                ticket_dir: "docs/ticket".to_string(),
                self_referential: false,
            },
        );
        self
    }

    #[allow(dead_code)] // test builder helper
    pub(crate) fn with_agent(mut self, name: impl Into<String>, config: AgentConfig) -> Self {
        let agent_id = name.into();
        self.config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .agents
            .insert(agent_id, config);
        self
    }

    #[allow(dead_code)] // test builder helper
    pub(crate) fn with_step_template(
        mut self,
        name: impl Into<String>,
        config: crate::config::StepTemplateConfig,
    ) -> Self {
        self.config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .step_templates
            .insert(name.into(), config);
        self
    }

    #[allow(dead_code)] // test builder helper
    pub(crate) fn with_workflow(mut self, name: impl Into<String>, config: WorkflowConfig) -> Self {
        let workflow_id = name.into();
        self.config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .workflows
            .insert(workflow_id.clone(), config);
        self
    }

    pub(crate) fn build(&mut self) -> Arc<InnerState> {
        if let Some(existing) = &self.state {
            return existing.clone();
        }

        self.ensure_workspace_dirs();

        let data_dir = self.temp_root.join("data");
        let logs_dir = data_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).expect("failed to create temp logs dir");

        let db_path = data_dir.join("agent_orchestrator.db");
        init_schema(&db_path).expect("failed to initialize test schema");
        persist_raw_config(&db_path, self.config.clone(), "test-seed")
            .expect("failed to persist test config");

        let (config, _version, _updated_at) = load_raw_config_from_db(&db_path)
            .expect("failed to load raw config from sqlite")
            .expect("missing test config in sqlite");
        let active =
            build_active_config(&self.temp_root, config).expect("failed to build active config");

        let default_workspace = active
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .and_then(|p| p.workspaces.get("default"))
            .expect("default workspace missing in test config");

        backfill_default_scope_data(
            &db_path,
            "default",
            "basic",
            default_workspace,
        )
        .expect("failed to backfill test data");

        let async_database = Arc::new({
            let db_p = db_path.clone();
            // Always create a fresh runtime on a separate thread to avoid
            // "cannot start a runtime from within a runtime" when called from
            // #[tokio::test] (which uses current_thread runtime).
            let result = std::thread::spawn(move || {
                tokio::runtime::Runtime::new()
                    .expect("failed to create tokio runtime for async_database init")
                    .block_on(crate::async_database::AsyncDatabase::open(&db_p))
            })
            .join()
            .expect("async_database init thread panicked");
            result.expect("failed to init async database")
        });
        let writer = Arc::new(crate::db_write::DbWriteCoordinator::new(
            async_database.clone(),
        ));
        let session_store = Arc::new(crate::session_store::AsyncSessionStore::new(
            async_database.clone(),
        ));
        let task_repo = Arc::new(crate::task_repository::AsyncSqliteTaskRepository::new(
            async_database.clone(),
        ));
        let store_manager =
            crate::store::StoreManager::new(async_database.clone(), self.temp_root.clone());
        let state = Arc::new(InnerState {
            app_root: self.temp_root.clone(),
            db_path,
            unsafe_mode: false,
            async_database,
            logs_dir,
            active_config: RwLock::new(active),
            active_config_error: RwLock::new(None),
            active_config_notice: RwLock::new(None),
            running: Mutex::new(HashMap::new()),
            agent_health: std::sync::RwLock::new(HashMap::new()),
            agent_metrics: std::sync::RwLock::new(HashMap::new()),
            message_bus: Arc::new(MessageBus::new()),
            event_sink: std::sync::RwLock::new(Arc::new(NoopSink)),
            db_writer: writer,
            session_store,
            task_repo,
            store_manager,
        });
        self.state = Some(state.clone());
        state
    }

    pub(crate) fn temp_root(&self) -> &Path {
        &self.temp_root
    }

    fn ensure_workspace_dirs(&self) {
        let project = self
            .config
            .projects
            .get(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project");
        for workspace in project.workspaces.values() {
            let root_path = self.temp_root.join(&workspace.root_path);
            let root_result = std::fs::create_dir_all(&root_path);
            assert!(
                root_result.is_ok(),
                "failed to create workspace root {}",
                root_path.display()
            );

            for target in &workspace.qa_targets {
                let target_path = root_path.join(target);
                let target_result = std::fs::create_dir_all(&target_path);
                assert!(
                    target_result.is_ok(),
                    "failed to create workspace qa_target {}",
                    target_path.display()
                );
            }

            let ticket_dir = root_path.join(&workspace.ticket_dir);
            let ticket_result = std::fs::create_dir_all(&ticket_dir);
            assert!(
                ticket_result.is_ok(),
                "failed to create workspace ticket_dir {}",
                ticket_dir.display()
            );
        }
    }
}

impl Drop for TestState {
    fn drop(&mut self) {
        if self.temp_root.exists() {
            let _ = std::fs::remove_dir_all(&self.temp_root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_compiles() {
        let mut fixture = TestState::new();
        let state = fixture.build();

        assert!(state.db_path.exists());
        assert!(state.logs_dir.exists());
    }

    #[test]
    fn test_state_creates_workspace() {
        let mut fixture = TestState::new().with_workspace("qa-workspace", "workspace/qa-workspace");
        let state = fixture.build();
        let active = read_active_config(&state).expect("active config should be readable");

        let workspace = active
            .workspaces
            .get("qa-workspace")
            .expect("seeded workspace missing");
        assert!(workspace.root_path.exists());
        assert!(workspace.root_path.join("docs/qa").exists());
    }

    #[test]
    fn test_state_cleanup() {
        let temp_root = {
            let mut fixture = TestState::new();
            let temp_root = fixture.temp_root().to_path_buf();
            let _state = fixture.build();
            assert!(temp_root.exists());
            temp_root
        };

        assert!(!temp_root.exists());
    }
}
