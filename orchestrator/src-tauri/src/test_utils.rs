use super::*;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn create_minimal_test_config() -> OrchestratorConfig {
    OrchestratorConfig {
        runner: RunnerConfig {
            shell: "/bin/bash".to_string(),
            shell_arg: "-lc".to_string(),
        },
        resume: ResumeConfig { auto: false },
        defaults: ConfigDefaults {
            project: String::new(),
            workspace: "default".to_string(),
            workflow: "basic".to_string(),
        },
        projects: HashMap::new(),
        workspaces: {
            let mut ws = HashMap::new();
            ws.insert(
                "default".to_string(),
                WorkspaceConfig {
                    root_path: "workspace/default".to_string(),
                    qa_targets: vec!["docs/qa".to_string()],
                    ticket_dir: "docs/ticket".to_string(),
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
                        description: "Echo agent for testing".to_string(),
                        version: None,
                        cost: Some(1),
                    },
                    capabilities: vec!["qa".to_string()],
                    templates: {
                        let mut t = HashMap::new();
                        t.insert("qa".to_string(), "echo 'qa: {rel_path}'".to_string());
                        t
                    },
                    selection: AgentSelectionConfig::default(),
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
                        id: "run_qa".to_string(),
                        description: None,
                        step_type: Some(WorkflowStepType::Qa),
                        builtin: None,
                        required_capability: None,
                        enabled: true,
                        repeatable: false,
                        is_guard: false,
                        cost_preference: None,
                        prehook: None,
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
                },
            );
            workflows
        },
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
        self.config.workspaces.insert(
            workspace_id.clone(),
            WorkspaceConfig {
                root_path: path.into(),
                qa_targets: vec!["docs/qa".to_string(), "docs/security".to_string()],
                ticket_dir: "docs/ticket".to_string(),
            },
        );
        if !self
            .config
            .workspaces
            .contains_key(&self.config.defaults.workspace)
        {
            self.config.defaults.workspace = workspace_id;
        }
        self
    }

    pub(crate) fn with_agent(mut self, name: impl Into<String>, config: AgentConfig) -> Self {
        let agent_id = name.into();
        self.config.agents.insert(agent_id, config);
        self
    }

    pub(crate) fn with_workflow(mut self, name: impl Into<String>, config: WorkflowConfig) -> Self {
        let workflow_id = name.into();
        self.config.workflows.insert(workflow_id.clone(), config);
        if !self
            .config
            .workflows
            .contains_key(&self.config.defaults.workflow)
        {
            self.config.defaults.workflow = workflow_id;
        }
        self
    }

    pub(crate) fn build(&mut self) -> Arc<InnerState> {
        if let Some(existing) = &self.state {
            return existing.clone();
        }

        self.ensure_workspace_dirs();

        let config_dir = self.temp_root.join("config");
        let data_dir = self.temp_root.join("data");
        let logs_dir = data_dir.join("logs");
        std::fs::create_dir_all(&config_dir).expect("failed to create temp config dir");
        std::fs::create_dir_all(&logs_dir).expect("failed to create temp logs dir");

        let config_path = config_dir.join("default.yaml");
        let yaml = serde_yaml::to_string(&self.config).expect("failed to serialize test config");
        std::fs::write(&config_path, yaml).expect("failed to write temp config");

        let db_path = data_dir.join("agent_orchestrator.db");
        init_schema(&db_path).expect("failed to initialize test schema");

        let (config, _yaml, _version, _updated_at) =
            load_or_seed_config(&db_path, &config_path).expect("failed to load test config");
        let active =
            build_active_config(&self.temp_root, config).expect("failed to build active config");

        let default_workspace = active
            .workspaces
            .get(&active.default_workspace_id)
            .expect("default workspace missing in test config");

        backfill_legacy_data(
            &db_path,
            &active.default_workspace_id,
            &active.default_workflow_id,
            default_workspace,
        )
        .expect("failed to backfill test data");

        let state = Arc::new(InnerState {
            app_root: self.temp_root.clone(),
            db_path,
            logs_dir,
            config_path,
            active_config: RwLock::new(active),
            running: Mutex::new(HashMap::new()),
            agent_health: std::sync::RwLock::new(HashMap::new()),
            agent_metrics: std::sync::RwLock::new(HashMap::new()),
        });
        self.state = Some(state.clone());
        state
    }

    pub(crate) fn temp_root(&self) -> &Path {
        &self.temp_root
    }

    fn ensure_workspace_dirs(&self) {
        for workspace in self.config.workspaces.values() {
            let root_path = self.temp_root.join(&workspace.root_path);
            std::fs::create_dir_all(&root_path).unwrap_or_else(|_| {
                panic!("failed to create workspace root {}", root_path.display())
            });

            for target in &workspace.qa_targets {
                let target_path = root_path.join(target);
                std::fs::create_dir_all(&target_path).unwrap_or_else(|_| {
                    panic!(
                        "failed to create workspace qa_target {}",
                        target_path.display()
                    )
                });
            }

            let ticket_dir = root_path.join(&workspace.ticket_dir);
            std::fs::create_dir_all(&ticket_dir).unwrap_or_else(|_| {
                panic!(
                    "failed to create workspace ticket_dir {}",
                    ticket_dir.display()
                )
            });
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
        assert!(state.config_path.exists());
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
