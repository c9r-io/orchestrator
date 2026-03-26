mod build;
mod normalize;
mod persist;
mod self_heal;
mod state;
mod validate;
mod workspace;

pub use build::*;
pub use normalize::*;
pub use persist::*;
pub use self_heal::*;
pub use state::*;
pub use validate::*;
pub use workspace::*;

pub(crate) use normalize::{normalize_config, normalize_step_execution_mode_recursive};
pub(crate) use persist::serialize_config_snapshot;
pub(crate) use self_heal::apply_self_heal_pass;
pub(crate) use validate::validate_workflow_config_with_agents;

use std::path::PathBuf;

/// Returns the current UTC timestamp encoded as RFC 3339.
pub fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Returns the daemon data directory (`~/.orchestratord` by default).
///
/// Override with the `ORCHESTRATORD_DATA_DIR` environment variable.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ORCHESTRATORD_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .expect("unable to determine home directory")
        .join(".orchestratord")
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::config::{
        LoopMode, OrchestratorConfig, StepBehavior, WorkflowConfig, WorkflowFinalizeConfig,
        WorkflowLoopConfig, WorkflowLoopGuardConfig, WorkflowStepConfig,
    };
    #[allow(unused_imports)]
    use std::collections::HashMap;

    pub fn make_step(id: &str, enabled: bool) -> WorkflowStepConfig {
        WorkflowStepConfig {
            id: id.to_string(),
            description: None,
            builtin: None,
            required_capability: None,
            execution_profile: None,
            enabled,
            repeatable: true,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            template: None,
            outputs: vec![],
            pipe_to: None,
            command: None,
            chain_steps: vec![],
            scope: None,
            behavior: StepBehavior::default(),
            max_parallel: None,
            stagger_delay_ms: None,
            timeout_secs: None,
            stall_timeout_secs: None,
            item_select_config: None,
            store_inputs: vec![],
            store_outputs: vec![],
            step_vars: None,
        }
    }

    pub fn make_builtin_step(id: &str, builtin: &str, enabled: bool) -> WorkflowStepConfig {
        WorkflowStepConfig {
            builtin: Some(builtin.to_string()),
            ..make_step(id, enabled)
        }
    }

    pub fn make_command_step(id: &str, cmd: &str) -> WorkflowStepConfig {
        WorkflowStepConfig {
            command: Some(cmd.to_string()),
            ..make_step(id, true)
        }
    }

    pub fn make_workflow(steps: Vec<WorkflowStepConfig>) -> WorkflowConfig {
        WorkflowConfig {
            steps,
            execution: Default::default(),
            loop_policy: WorkflowLoopConfig {
                mode: LoopMode::Once,
                guard: WorkflowLoopGuardConfig {
                    enabled: false,
                    ..WorkflowLoopGuardConfig::default()
                },
                convergence_expr: None,
            },
            finalize: WorkflowFinalizeConfig { rules: vec![] },
            qa: None,
            fix: None,
            retest: None,
            dynamic_steps: vec![],
            adaptive: None,
            safety: crate::config::SafetyConfig::default(),
            max_parallel: None,
            stagger_delay_ms: None,
            item_isolation: None,
        }
    }

    pub fn make_config_with_agent(capability: &str, _template: &str) -> OrchestratorConfig {
        use crate::config::AgentConfig;
        let mut agents = HashMap::new();
        agents.insert(
            "test-agent".to_string(),
            AgentConfig {
                enabled: true,
                capabilities: vec![capability.to_string()],
                command: "echo test".to_string(),
                ..AgentConfig::default()
            },
        );
        let mut config = OrchestratorConfig::default();
        config
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default()
            .agents = agents;
        config
    }

    pub fn make_config_with_default_project() -> OrchestratorConfig {
        let mut config = OrchestratorConfig::default();
        config
            .projects
            .entry(crate::config::DEFAULT_PROJECT_ID.to_string())
            .or_default();
        config
    }

    pub fn make_minimal_buildable_config() -> OrchestratorConfig {
        let mut config = make_config_with_default_project();
        let project = config
            .projects
            .get_mut(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project");
        project.agents = make_config_with_agent("qa", "echo qa")
            .projects
            .remove(crate::config::DEFAULT_PROJECT_ID)
            .expect("default project")
            .agents;
        project.workspaces.insert(
            "default".to_string(),
            crate::config::WorkspaceConfig {
                root_path: ".".to_string(),
                qa_targets: vec!["fixtures/qa-probe-targets".to_string()],
                ticket_dir: "fixtures/ticket".to_string(),
                self_referential: false,
                health_policy: Default::default(),
            },
        );
        project.workflows.insert(
            "basic".to_string(),
            make_workflow(vec![make_builtin_step("self_test", "self_test", true)]),
        );
        super::normalize_config(config)
    }

    pub fn make_test_db() -> (std::path::PathBuf, std::path::PathBuf) {
        let temp_dir =
            std::env::temp_dir().join(format!("config-load-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("create config-load temp dir");
        let db_path = temp_dir.join("agent_orchestrator.db");
        crate::db::init_schema(&db_path).expect("initialize test schema");
        (temp_dir, db_path)
    }

    #[test]
    fn now_ts_returns_rfc3339_string() {
        let ts = super::now_ts();
        assert!(!ts.is_empty());
        let parsed = chrono::DateTime::parse_from_rfc3339(&ts);
        assert!(parsed.is_ok(), "now_ts should return valid RFC3339: {}", ts);
    }

    #[test]
    fn now_ts_returns_recent_timestamp() {
        let before = chrono::Utc::now();
        let ts = super::now_ts();
        let after = chrono::Utc::now();
        let parsed =
            chrono::DateTime::parse_from_rfc3339(&ts).expect("timestamp should parse as RFC3339");
        assert!(parsed >= before, "timestamp should be >= before");
        assert!(parsed <= after, "timestamp should be <= after");
    }
}
