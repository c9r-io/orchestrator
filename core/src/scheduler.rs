#![allow(dead_code)]
#![allow(unused_imports)]

mod item_executor;
mod loop_engine;
mod phase_runner;
mod query;
mod runtime;
pub mod safety;
mod task_state;

pub use crate::state::RunningTask;
pub use item_executor::{execute_builtin_step, execute_guard_step, process_item, GuardResult};
pub use loop_engine::{evaluate_loop_guard_rules, run_task_loop};
pub use phase_runner::{run_phase, run_phase_with_rotation};
pub use query::{
    delete_task_impl, follow_task_logs, get_task_details_impl, list_tasks_impl, load_task_summary,
    resolve_task_id, stream_task_logs_impl, watch_task,
};
pub use runtime::{
    kill_current_child, load_task_runtime_context, shutdown_running_tasks, spawn_task_runner,
    stop_task_runtime, stop_task_runtime_for_delete,
};
pub use safety::{
    create_checkpoint, execute_self_test_step, restore_binary_snapshot, rollback_to_checkpoint,
    snapshot_binary,
};
pub use task_state::{
    count_unresolved_items, find_latest_resumable_task_id, first_task_item_id,
    prepare_task_for_start, set_task_status, update_task_cycle_state,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collab::MessageBus;
    use crate::config::{
        AgentConfig, AgentMetadata, AgentSelectionConfig, LoopMode, WorkflowConfig,
        WorkflowFinalizeConfig, WorkflowLoopConfig, WorkflowLoopGuardConfig, WorkflowStepConfig,
        WorkflowStepType,
    };
    use crate::db::open_conn;
    use crate::dto::CreateTaskPayload;
    use crate::events::NoopSink;
    use crate::task_ops::create_task_impl;
    use crate::test_utils::TestState;
    use anyhow::Context;
    use rusqlite::params;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, RwLock};

    #[test]
    fn load_task_summary_maps_created_and_updated_at_correctly() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/mapping_check.md");
        std::fs::write(&qa_file, "# mapping check\n").expect("seed qa file");
        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("mapping-check".to_string()),
                goal: Some("validate summary timestamps".to_string()),
                ..Default::default()
            },
        )
        .expect("task should be created");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (workflow_id, created_at, updated_at): (String, String, String) = conn
            .query_row(
                "SELECT workflow_id, created_at, updated_at FROM tasks WHERE id = ?1",
                params![created.id.clone()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("task row should exist");

        let summary = load_task_summary(&state, &created.id).expect("summary should load");
        assert_eq!(summary.workflow_id, workflow_id);
        assert_eq!(summary.created_at, created_at);
        assert_eq!(summary.updated_at, updated_at);
    }

    #[tokio::test]
    async fn plan_output_is_propagated_to_qa_doc_gen_template() {
        let mut fixture = TestState::new()
            .with_agent(
                "planner",
                AgentConfig {
                    metadata: AgentMetadata {
                        name: "planner".to_string(),
                        description: Some("plan propagation test agent".to_string()),
                        version: None,
                        cost: Some(1),
                    },
                    capabilities: vec!["plan".to_string(), "qa_doc_gen".to_string()],
                    templates: {
                        let mut t = HashMap::new();
                        t.insert("plan".to_string(), "echo PLAN_MARKER_SB_SMOKE".to_string());
                        t.insert(
                            "qa_doc_gen".to_string(),
                            "echo QA_DOC_FROM_PLAN:{plan_output}".to_string(),
                        );
                        t.insert(
                            "loop_guard".to_string(),
                            "echo '{\"continue\":false,\"should_stop\":true}'".to_string(),
                        );
                        t
                    },
                    selection: AgentSelectionConfig::default(),
                },
            )
            .with_workflow(
                "plan-propagation",
                WorkflowConfig {
                    steps: vec![
                        WorkflowStepConfig {
                            id: "plan".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::Plan),
                            builtin: None,
                            required_capability: Some("plan".to_string()),
                            enabled: true,
                            repeatable: false,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                            chain_steps: vec![],
                        },
                        WorkflowStepConfig {
                            id: "qa_doc_gen".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::QaDocGen),
                            builtin: None,
                            required_capability: Some("qa_doc_gen".to_string()),
                            enabled: true,
                            repeatable: false,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                            chain_steps: vec![],
                        },
                        WorkflowStepConfig {
                            id: "loop_guard".to_string(),
                            description: None,
                            step_type: Some(WorkflowStepType::LoopGuard),
                            builtin: Some("loop_guard".to_string()),
                            required_capability: None,
                            enabled: true,
                            repeatable: true,
                            is_guard: true,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                            chain_steps: vec![],
                        },
                    ],
                    loop_policy: WorkflowLoopConfig {
                        mode: LoopMode::Once,
                        guard: WorkflowLoopGuardConfig {
                            enabled: true,
                            stop_when_no_unresolved: true,
                            max_cycles: Some(1),
                            agent_template: None,
                        },
                    },
                    finalize: WorkflowFinalizeConfig { rules: vec![] },
                    qa: None,
                    fix: None,
                    retest: None,
                    dynamic_steps: vec![],
                    safety: crate::config::SafetyConfig::default(),
                },
            );
        let state = fixture.build();

        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/plan_propagation.md");
        std::fs::write(&qa_file, "# plan propagation\n").expect("seed qa file");

        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("plan-propagation".to_string()),
                goal: Some("verify plan output propagation".to_string()),
                workflow_id: Some("plan-propagation".to_string()),
                target_files: Some(vec!["docs/qa/plan_propagation.md".to_string()]),
                ..Default::default()
            },
        )
        .expect("task should be created");

        prepare_task_for_start(&state, &created.id).expect("prepare task");
        run_task_loop(state.clone(), &created.id, RunningTask::new())
            .await
            .expect("task should run");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let (qa_command, qa_stdout): (String, String) = conn
            .query_row(
                "SELECT command, json_extract(output_json, '$.stdout')
                 FROM command_runs
                 WHERE task_item_id = (
                   SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1
                 ) AND phase = 'qa_doc_gen'
                 ORDER BY started_at DESC LIMIT 1",
                params![created.id.clone()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("qa_doc_gen run should exist");

        assert!(qa_command.contains("PLAN_MARKER_SB_SMOKE"));
        assert!(!qa_command.contains("{plan_output}"));
        assert!(qa_stdout.contains("QA_DOC_FROM_PLAN:PLAN_MARKER_SB_SMOKE"));
    }

    #[tokio::test]
    async fn smoke_chain_step_type_is_parsed_correctly() {
        let step_type: WorkflowStepType = "smoke_chain".parse().expect("should parse");
        assert_eq!(step_type, WorkflowStepType::SmokeChain);
        assert_eq!(step_type.as_str(), "smoke_chain");
        assert!(step_type.has_structured_output());
    }

    #[tokio::test]
    async fn smoke_chain_normalize_sets_required_capability() {
        let mut workflow = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "smoke_chain".to_string(),
                description: None,
                step_type: Some(WorkflowStepType::SmokeChain),
                builtin: None,
                required_capability: None,
                enabled: true,
                repeatable: true,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![
                    WorkflowStepConfig {
                        id: "verify".to_string(),
                        description: None,
                        step_type: Some(WorkflowStepType::Qa),
                        builtin: None,
                        required_capability: Some("qa".to_string()),
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
                    },
                ],
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

        crate::config_load::normalize_workflow_config(&mut workflow);

        let smoke_step = workflow
            .steps
            .iter()
            .find(|s| s.id == "smoke_chain")
            .expect("smoke_chain step should exist");
        assert_eq!(
            smoke_step.required_capability.as_deref(),
            Some("smoke_chain"),
            "required_capability should be set to 'smoke_chain'"
        );
    }

    // TODO: smoke_chain_propagates_plan_output_through_chain_steps
    // Removed incomplete integration test introduced by smoke-chain agent run.
    // The SmokeChain chain_steps execution path in item_executor.rs needs
    // proper wiring before this test can pass. Re-add when SmokeChain feature
    // is intentionally developed.

    #[tokio::test]
    async fn self_test_survives_smoke_test() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let project_root = manifest_dir.parent().unwrap();
        let core_dir = project_root.join("core");
        assert!(core_dir.exists());

        let state = Arc::new(crate::state::InnerState {
            app_root: project_root.to_path_buf(),
            db_path: PathBuf::new(),
            logs_dir: PathBuf::new(),
            active_config: RwLock::new(crate::config::ActiveConfig {
                config: crate::config::OrchestratorConfig::default(),
                workspaces: HashMap::new(),
                projects: HashMap::new(),
                default_project_id: String::new(),
                default_workspace_id: String::new(),
                default_workflow_id: String::new(),
            }),
            running: tokio::sync::Mutex::new(HashMap::new()),
            agent_health: RwLock::new(HashMap::new()),
            agent_metrics: RwLock::new(HashMap::new()),
            message_bus: Arc::new(MessageBus::new()),
            event_sink: RwLock::new(Arc::new(NoopSink)),
            db_writer: Arc::new(crate::db_write::DbWriteCoordinator::new(&PathBuf::new()).unwrap()),
        });

        state.emit_event(
            "test-task",
            Some("test-item"),
            "self_test_phase",
            serde_json::json!({"phase": "cargo_check"}),
        );

        let check_output = tokio::process::Command::new("cargo")
            .args(["check", "--message-format=short"])
            .current_dir(&core_dir)
            .output()
            .await
            .context("failed to run cargo check")
            .expect("cargo check should execute");

        let exit_code = check_output.status.code().unwrap_or(1) as i64;
        assert_eq!(exit_code, 0);
    }
}
