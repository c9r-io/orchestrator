pub mod check;
mod item_executor;
mod loop_engine;
mod phase_runner;
mod query;
mod runtime;
pub mod safety;
mod task_state;
pub mod trace;

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
        AgentConfig, AgentMetadata, AgentSelectionConfig, LoopMode, OnFailureAction, StepBehavior,
        StepScope, WorkflowConfig, WorkflowFinalizeConfig, WorkflowLoopConfig,
        WorkflowLoopGuardConfig, WorkflowStepConfig, PIPELINE_VAR_INLINE_LIMIT,
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
                    command: "echo {prompt}".to_string(),
                    selection: AgentSelectionConfig::default(),
                    env: None,
                },
            )
            .with_step_template(
                "plan",
                crate::config::StepTemplateConfig {
                    prompt: "PLAN_MARKER_SB_SMOKE".to_string(),
                    description: None,
                },
            )
            .with_step_template(
                "qa_doc_gen",
                crate::config::StepTemplateConfig {
                    prompt: "QA_DOC_FROM_PLAN:{plan_output}".to_string(),
                    description: None,
                },
            )
            .with_workflow(
                "plan-propagation",
                WorkflowConfig {
                    steps: vec![
                        WorkflowStepConfig {
                            id: "plan".to_string(),
                            description: None,

                            builtin: None,
                            required_capability: Some("plan".to_string()),
                            enabled: true,
                            repeatable: false,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            template: Some("plan".to_string()),
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                            chain_steps: vec![],
                            scope: None,
                            behavior: StepBehavior::default(),
                        },
                        WorkflowStepConfig {
                            id: "qa_doc_gen".to_string(),
                            description: None,

                            builtin: None,
                            required_capability: Some("qa_doc_gen".to_string()),
                            enabled: true,
                            repeatable: false,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            template: Some("qa_doc_gen".to_string()),
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                            chain_steps: vec![],
                            scope: None,
                            behavior: StepBehavior::default(),
                        },
                        WorkflowStepConfig {
                            id: "loop_guard".to_string(),
                            description: None,

                            builtin: Some("loop_guard".to_string()),
                            required_capability: None,
                            enabled: true,
                            repeatable: true,
                            is_guard: true,
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
    async fn smoke_chain_normalize_sets_required_capability() {
        let mut workflow = WorkflowConfig {
            steps: vec![WorkflowStepConfig {
                id: "smoke_chain".to_string(),
                description: None,

                builtin: None,
                required_capability: None,
                enabled: true,
                repeatable: true,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                template: None,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![WorkflowStepConfig {
                    id: "verify".to_string(),
                    description: None,

                    builtin: None,
                    required_capability: Some("qa".to_string()),
                    enabled: true,
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
                }],
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
    async fn large_plan_output_spills_to_file() {
        // Generate a plan output that exceeds the inline limit
        let large_plan = "X".repeat(PIPELINE_VAR_INLINE_LIMIT + 1024);
        let _plan_echo = format!("echo '{}'", large_plan);

        let mut fixture = TestState::new()
            .with_agent(
                "planner",
                AgentConfig {
                    metadata: AgentMetadata {
                        name: "planner".to_string(),
                        description: Some("large plan spill test".to_string()),
                        version: None,
                        cost: Some(1),
                    },
                    capabilities: vec!["plan".to_string(), "qa_doc_gen".to_string()],
                    command: "echo {prompt}".to_string(),
                    selection: AgentSelectionConfig::default(),
                    env: None,
                },
            )
            .with_step_template(
                "qa_doc_gen",
                crate::config::StepTemplateConfig {
                    prompt: "QA {plan_output} {plan_output_path}".to_string(),
                    description: None,
                },
            )
            .with_workflow(
                "spill-test",
                WorkflowConfig {
                    steps: vec![
                        WorkflowStepConfig {
                            id: "plan".to_string(),
                            description: None,

                            builtin: None,
                            required_capability: Some("plan".to_string()),
                            enabled: true,
                            repeatable: false,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            template: None,
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: Some(format!(
                                "printf '{}'",
                                "X".repeat(PIPELINE_VAR_INLINE_LIMIT + 1024)
                            )),
                            chain_steps: vec![],
                            scope: None,
                            behavior: StepBehavior::default(),
                        },
                        WorkflowStepConfig {
                            id: "qa_doc_gen".to_string(),
                            description: None,

                            builtin: None,
                            required_capability: Some("qa_doc_gen".to_string()),
                            enabled: true,
                            repeatable: false,
                            is_guard: false,
                            cost_preference: None,
                            prehook: None,
                            tty: false,
                            template: Some("qa_doc_gen".to_string()),
                            outputs: Vec::new(),
                            pipe_to: None,
                            command: None,
                            chain_steps: vec![],
                            scope: None,
                            behavior: StepBehavior::default(),
                        },
                        WorkflowStepConfig {
                            id: "loop_guard".to_string(),
                            description: None,

                            builtin: Some("loop_guard".to_string()),
                            required_capability: None,
                            enabled: true,
                            repeatable: true,
                            is_guard: true,
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
            .join("workspace/default/docs/qa/spill_test.md");
        std::fs::write(&qa_file, "# spill test\n").expect("seed qa file");

        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("spill-test".to_string()),
                goal: Some("verify large plan output spills to file".to_string()),
                workflow_id: Some("spill-test".to_string()),
                target_files: Some(vec!["docs/qa/spill_test.md".to_string()]),
                ..Default::default()
            },
        )
        .expect("task should be created");

        prepare_task_for_start(&state, &created.id).expect("prepare task");
        run_task_loop(state.clone(), &created.id, RunningTask::new())
            .await
            .expect("task should run");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let qa_command: String = conn
            .query_row(
                "SELECT command
                 FROM command_runs
                 WHERE task_item_id = (
                   SELECT id FROM task_items WHERE task_id = ?1 ORDER BY order_no LIMIT 1
                 ) AND phase = 'qa_doc_gen'
                 ORDER BY started_at DESC LIMIT 1",
                params![created.id.clone()],
                |row| row.get(0),
            )
            .expect("qa_doc_gen run should exist");

        // The plan_output in the qa_doc_gen command should be truncated (not the full large output)
        assert!(
            !qa_command.contains(&large_plan),
            "full plan output should NOT appear inline in the command"
        );
        // The inline plan_output should contain the truncation marker
        assert!(
            qa_command.contains("truncated"),
            "command should contain truncation marker, actual command (first 500 chars): {}",
            &qa_command[..qa_command.len().min(500)]
        );

        // plan_output_path should be expanded in the command (not left as placeholder)
        assert!(
            qa_command.contains("plan_output.txt"),
            "command should reference the spill file path"
        );
        assert!(
            !qa_command.contains("{plan_output_path}"),
            "plan_output_path placeholder should be expanded"
        );

        // The rendered command should be within the 16KB runner limit
        assert!(
            qa_command.len() < 16384,
            "rendered command ({} bytes) should be under 16KB limit",
            qa_command.len()
        );

        // Verify the spill file exists and contains the full content
        let spill_file = state.logs_dir.join(&created.id).join("plan_output.txt");
        assert!(
            spill_file.exists(),
            "spill file should exist at {}",
            spill_file.display()
        );
        let spill_content = std::fs::read_to_string(&spill_file).expect("read spill file");
        assert!(
            spill_content.len() > PIPELINE_VAR_INLINE_LIMIT,
            "spill file should contain the full content"
        );
    }

    #[test]
    fn spill_large_var_inline_when_small() {
        let dir = std::env::temp_dir().join(format!("spill-small-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create spill-small dir");

        let mut pipeline = crate::config::PipelineVariables::default();
        let small_value = "short value".to_string();
        item_executor::spill_large_var(
            &dir,
            "task1",
            "plan_output",
            small_value.clone(),
            &mut pipeline,
        );

        assert_eq!(
            pipeline
                .vars
                .get("plan_output")
                .expect("plan_output should be set"),
            &small_value
        );
        // _path is always set now (even for small values)
        let p = pipeline
            .vars
            .get("plan_output_path")
            .expect("plan_output_path must be set");
        assert_eq!(
            std::fs::read_to_string(p).expect("read plan_output spill file"),
            small_value
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spill_large_var_spills_when_over_limit() {
        let dir = std::env::temp_dir().join(format!("spill-large-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create spill-large dir");

        let mut pipeline = crate::config::PipelineVariables::default();
        let large_value = "X".repeat(PIPELINE_VAR_INLINE_LIMIT + 500);
        item_executor::spill_large_var(
            &dir,
            "task1",
            "plan_output",
            large_value.clone(),
            &mut pipeline,
        );

        let inline = pipeline
            .vars
            .get("plan_output")
            .expect("plan_output should be set");
        assert!(inline.contains("truncated"));
        assert!(inline.len() < large_value.len());

        let path = pipeline
            .vars
            .get("plan_output_path")
            .expect("plan_output_path should be set");
        assert!(path.contains("plan_output.txt"));

        // Verify spill file has full content
        let spill_content = std::fs::read_to_string(path).expect("read spill content");
        assert_eq!(spill_content.len(), large_value.len());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spill_to_file_returns_none_when_small() {
        let dir = std::env::temp_dir().join(format!("spill-fn-small-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create spill-fn-small dir");

        let result = item_executor::spill_to_file(&dir, "task1", "key", "small value");
        assert!(result.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spill_to_file_returns_truncated_and_path_when_large() {
        let dir = std::env::temp_dir().join(format!("spill-fn-large-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create spill-fn-large dir");

        let large = "Y".repeat(PIPELINE_VAR_INLINE_LIMIT + 1000);
        let result = item_executor::spill_to_file(&dir, "task1", "output", &large);
        assert!(result.is_some());

        let (truncated, path) = result.expect("spill_to_file should return Some");
        assert!(truncated.contains("truncated"));
        assert!(path.contains("output.txt"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spill_large_var_handles_multibyte_utf8_at_boundary() {
        let dir = std::env::temp_dir().join(format!("spill-utf8-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create spill-utf8 dir");

        let mut pipeline = crate::config::PipelineVariables::default();
        // Create a string with multibyte chars near the boundary
        let mut value = "A".repeat(PIPELINE_VAR_INLINE_LIMIT - 2);
        value.push('中'); // 3-byte UTF-8 char that crosses the boundary
        value.push_str(&"B".repeat(500));

        item_executor::spill_large_var(&dir, "task1", "key", value, &mut pipeline);

        let inline = pipeline.vars.get("key").expect("key should be set");
        // Should not panic on UTF-8 boundary
        assert!(inline.contains("truncated"));
        // Truncated inline should be valid UTF-8
        assert!(inline.is_char_boundary(0));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn task_scoped_early_return_skips_downstream_item_steps() {
        let mut fixture = TestState::new().with_workflow(
            "task-early-return",
            WorkflowConfig {
                steps: vec![
                    WorkflowStepConfig {
                        id: "task_gate".to_string(),
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
                        command: Some("printf 'gated' && exit 7".to_string()),
                        chain_steps: vec![],
                        scope: Some(StepScope::Task),
                        behavior: StepBehavior {
                            on_failure: OnFailureAction::EarlyReturn {
                                status: "qa_failed".to_string(),
                            },
                            ..StepBehavior::default()
                        },
                    },
                    WorkflowStepConfig {
                        id: "item_verify".to_string(),
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
                        command: Some("echo should-not-run".to_string()),
                        chain_steps: vec![],
                        scope: Some(StepScope::Item),
                        behavior: StepBehavior::default(),
                    },
                ],
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
                safety: crate::config::SafetyConfig::default(),
            },
        );
        let state = fixture.build();

        let qa_file_a = state
            .app_root
            .join("workspace/default/docs/qa/early_return_a.md");
        let qa_file_b = state
            .app_root
            .join("workspace/default/docs/qa/early_return_b.md");
        std::fs::write(&qa_file_a, "# early return A\n").expect("seed first qa file");
        std::fs::write(&qa_file_b, "# early return B\n").expect("seed second qa file");

        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("task-early-return".to_string()),
                goal: Some("exercise task scoped early return".to_string()),
                workflow_id: Some("task-early-return".to_string()),
                target_files: Some(vec![
                    "docs/qa/early_return_a.md".to_string(),
                    "docs/qa/early_return_b.md".to_string(),
                ]),
                ..Default::default()
            },
        )
        .expect("task should be created");

        prepare_task_for_start(&state, &created.id).expect("prepare task");
        run_task_loop(state.clone(), &created.id, RunningTask::new())
            .await
            .expect("task loop should complete");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let task_gate_runs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (
                    SELECT id FROM task_items WHERE task_id = ?1
                 ) AND phase = 'task_gate'",
                params![created.id.clone()],
                |row| row.get(0),
            )
            .expect("count task step runs");
        let item_verify_runs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (
                    SELECT id FROM task_items WHERE task_id = ?1
                 ) AND phase = 'item_verify'",
                params![created.id.clone()],
                |row| row.get(0),
            )
            .expect("count item step runs");

        assert_eq!(task_gate_runs, 1);
        assert_eq!(item_verify_runs, 0);
    }

    #[tokio::test]
    async fn task_scoped_success_runs_item_step_for_each_task_item() {
        let mut fixture = TestState::new().with_workflow(
            "task-fanout",
            WorkflowConfig {
                steps: vec![
                    WorkflowStepConfig {
                        id: "task_gate".to_string(),
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
                        command: Some("echo task-ready".to_string()),
                        chain_steps: vec![],
                        scope: Some(StepScope::Task),
                        behavior: StepBehavior::default(),
                    },
                    WorkflowStepConfig {
                        id: "item_verify".to_string(),
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
                        command: Some("echo item-ok".to_string()),
                        chain_steps: vec![],
                        scope: Some(StepScope::Item),
                        behavior: StepBehavior::default(),
                    },
                ],
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
                safety: crate::config::SafetyConfig::default(),
            },
        );
        let state = fixture.build();

        let qa_file_a = state.app_root.join("workspace/default/docs/qa/fanout_a.md");
        let qa_file_b = state.app_root.join("workspace/default/docs/qa/fanout_b.md");
        std::fs::write(&qa_file_a, "# fanout A\n").expect("seed first qa file");
        std::fs::write(&qa_file_b, "# fanout B\n").expect("seed second qa file");

        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("task-fanout".to_string()),
                goal: Some("exercise task scoped fanout".to_string()),
                workflow_id: Some("task-fanout".to_string()),
                target_files: Some(vec![
                    "docs/qa/fanout_a.md".to_string(),
                    "docs/qa/fanout_b.md".to_string(),
                ]),
                ..Default::default()
            },
        )
        .expect("task should be created");

        prepare_task_for_start(&state, &created.id).expect("prepare task");
        run_task_loop(state.clone(), &created.id, RunningTask::new())
            .await
            .expect("task loop should complete");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let task_item_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_items WHERE task_id = ?1",
                params![created.id.clone()],
                |row| row.get(0),
            )
            .expect("count task items");
        let task_gate_runs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (
                    SELECT id FROM task_items WHERE task_id = ?1
                 ) AND phase = 'task_gate'",
                params![created.id.clone()],
                |row| row.get(0),
            )
            .expect("count task step runs");
        let item_verify_runs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (
                    SELECT id FROM task_items WHERE task_id = ?1
                 ) AND phase = 'item_verify'",
                params![created.id.clone()],
                |row| row.get(0),
            )
            .expect("count item step runs");

        assert!(task_item_count >= 1);
        assert_eq!(task_gate_runs, 1);
        assert_eq!(item_verify_runs, task_item_count);
    }

    #[tokio::test]
    async fn failing_cycle_records_auto_rollback_failure_when_git_checkpoint_is_unavailable() {
        let mut fixture = TestState::new().with_workflow(
            "auto-rollback-failure",
            WorkflowConfig {
                steps: vec![WorkflowStepConfig {
                    id: "qa_verify".to_string(),
                    description: None,
                    builtin: None,
                    required_capability: None,
                    enabled: true,
                    repeatable: true,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: Vec::new(),
                    pipe_to: None,
                    command: Some("exit 9".to_string()),
                    chain_steps: vec![],
                    scope: Some(StepScope::Item),
                    behavior: StepBehavior {
                        on_failure: OnFailureAction::SetStatus {
                            status: "qa_failed".to_string(),
                        },
                        ..StepBehavior::default()
                    },
                }],
                loop_policy: WorkflowLoopConfig {
                    mode: LoopMode::Fixed,
                    guard: WorkflowLoopGuardConfig {
                        enabled: false,
                        stop_when_no_unresolved: false,
                        max_cycles: Some(1),
                        agent_template: None,
                    },
                },
                finalize: WorkflowFinalizeConfig { rules: vec![] },
                qa: None,
                fix: None,
                retest: None,
                dynamic_steps: vec![],
                safety: crate::config::SafetyConfig {
                    auto_rollback: true,
                    max_consecutive_failures: 1,
                    checkpoint_strategy: crate::config::CheckpointStrategy::GitTag,
                    ..crate::config::SafetyConfig::default()
                },
            },
        );
        let state = fixture.build();

        let qa_file = state
            .app_root
            .join("workspace/default/docs/qa/auto_rollback_failure.md");
        std::fs::write(&qa_file, "# rollback failure\n").expect("seed qa file");

        let created = create_task_impl(
            &state,
            CreateTaskPayload {
                name: Some("auto-rollback-failure".to_string()),
                goal: Some("exercise rollback failure branch".to_string()),
                workflow_id: Some("auto-rollback-failure".to_string()),
                target_files: Some(vec!["docs/qa/auto_rollback_failure.md".to_string()]),
                ..Default::default()
            },
        )
        .expect("task should be created");

        prepare_task_for_start(&state, &created.id).expect("prepare task");
        run_task_loop(state.clone(), &created.id, RunningTask::new())
            .await
            .expect("task loop should complete");

        let conn = open_conn(&state.db_path).expect("open sqlite");
        let failed_runs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM command_runs WHERE task_item_id IN (
                    SELECT id FROM task_items WHERE task_id = ?1
                 ) AND phase = 'qa_verify' AND exit_code = 9",
                params![created.id.clone()],
                |row| row.get(0),
            )
            .expect("count failing command runs");

        assert_eq!(failed_runs, 1);
    }

    #[tokio::test]
    async fn self_test_survives_smoke_test() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let project_root = manifest_dir
            .parent()
            .expect("manifest dir should have parent");
        let core_dir = project_root.join("core");
        assert!(core_dir.exists());

        let target_dir = project_root.join("target");
        std::fs::create_dir_all(&target_dir).expect("create target dir");
        let db_path = target_dir.join("scheduler_self_test.db");
        crate::db::init_schema(&db_path).expect("init self test db");
        let database =
            Arc::new(crate::database::Database::new(db_path.clone()).expect("create db pool"));
        let state = Arc::new(crate::state::InnerState {
            app_root: project_root.to_path_buf(),
            db_path,
            database: database.clone(),
            logs_dir: PathBuf::new(),
            active_config: RwLock::new(crate::config::ActiveConfig {
                config: crate::config::OrchestratorConfig::default(),
                workspaces: HashMap::new(),
                projects: HashMap::new(),
                default_project_id: String::new(),
                default_workspace_id: String::new(),
                default_workflow_id: String::new(),
            }),
            active_config_error: RwLock::new(None),
            active_config_notice: RwLock::new(None),
            running: tokio::sync::Mutex::new(HashMap::new()),
            agent_health: RwLock::new(HashMap::new()),
            agent_metrics: RwLock::new(HashMap::new()),
            message_bus: Arc::new(MessageBus::new()),
            event_sink: RwLock::new(Arc::new(NoopSink)),
            db_writer: Arc::new(crate::db_write::DbWriteCoordinator::new(database)),
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
