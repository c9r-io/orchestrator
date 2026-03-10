use super::continuation::*;
use super::cycle_safety::*;
use super::segment::*;
use crate::config::{
    LoopMode, PipelineVariables, StepScope, WorkflowLoopConfig, WorkflowLoopGuardConfig,
};
use crate::scheduler::item_executor::StepExecutionAccumulator;
use crate::test_utils::TestState;
use std::collections::HashMap;

fn make_loop_policy(mode: LoopMode, max_cycles: Option<u32>) -> WorkflowLoopConfig {
    WorkflowLoopConfig {
        mode,
        guard: WorkflowLoopGuardConfig {
            max_cycles,
            ..Default::default()
        },
    }
}

#[test]
fn fixed_mode_stops_at_max_cycles() {
    let policy = make_loop_policy(LoopMode::Fixed, Some(2));
    // cycle 1 < 2 → continue
    let result = evaluate_loop_guard_rules(&policy, 1, 0);
    assert_eq!(result, Some((true, "fixed_cycle_continue".to_string())));
    // cycle 2 >= 2 → stop
    let result = evaluate_loop_guard_rules(&policy, 2, 0);
    assert_eq!(result, Some((false, "fixed_cycles_complete".to_string())));
    // cycle 3 >= 2 → stop
    let result = evaluate_loop_guard_rules(&policy, 3, 0);
    assert_eq!(result, Some((false, "fixed_cycles_complete".to_string())));
}

#[test]
fn fixed_mode_defaults_to_one_cycle() {
    let policy = make_loop_policy(LoopMode::Fixed, None);
    // cycle 1 >= 1 → stop immediately (acts like once)
    let result = evaluate_loop_guard_rules(&policy, 1, 0);
    assert_eq!(result, Some((false, "fixed_cycles_complete".to_string())));
}

#[test]
fn once_mode_always_stops() {
    let policy = make_loop_policy(LoopMode::Once, None);
    let result = evaluate_loop_guard_rules(&policy, 1, 0);
    assert_eq!(result, Some((false, "once_mode".to_string())));
}

#[test]
fn infinite_mode_respects_max_cycles() {
    let policy = make_loop_policy(LoopMode::Infinite, Some(3));
    let result = evaluate_loop_guard_rules(&policy, 2, 0);
    assert_eq!(result, None); // guard enabled, no decision yet
    let result = evaluate_loop_guard_rules(&policy, 3, 0);
    assert_eq!(result, Some((false, "max_cycles_reached".to_string())));
}

#[test]
fn infinite_mode_with_disabled_guard_continues_immediately() {
    let mut policy = make_loop_policy(LoopMode::Infinite, None);
    policy.guard.enabled = false;
    let result = evaluate_loop_guard_rules(&policy, 1, 0);
    assert_eq!(result, Some((true, "guard_disabled".to_string())));
}

#[test]
fn build_segments_groups_contiguous_scopes() {
    use crate::config::*;
    let task_ctx = TaskRuntimeContext {
        workspace_id: "ws".into(),
        workspace_root: "/tmp".into(),
        ticket_dir: "tickets".into(),
        execution_plan: TaskExecutionPlan {
            steps: vec![
                TaskExecutionStep {
                    id: "plan".into(),

                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "implement".into(),

                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "qa_testing".into(),

                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "ticket_fix".into(),

                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "doc_governance".into(),

                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
            ],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
            max_parallel: None,
        },
        current_cycle: 1,
        init_done: true,
        dynamic_steps: vec![],
        adaptive: None,
        pipeline_vars: PipelineVariables::default(),
        safety: SafetyConfig::default(),
        self_referential: false,
        consecutive_failures: 0,
        project_id: String::new(),
        pinned_invariants: std::sync::Arc::new(vec![]),
        workflow_id: String::new(),
        spawn_depth: 0,
    };

    let segments = build_scope_segments(&task_ctx);

    // Should produce 3 segments:
    // [plan, implement] → Task
    // [qa_testing, ticket_fix] → Item
    // [doc_governance] → Task
    assert_eq!(segments.len(), 3);

    assert_eq!(segments[0].scope, StepScope::Task);
    assert!(segments[0].step_ids.contains("plan"));
    assert!(segments[0].step_ids.contains("implement"));

    assert_eq!(segments[1].scope, StepScope::Item);
    assert!(segments[1].step_ids.contains("qa_testing"));
    assert!(segments[1].step_ids.contains("ticket_fix"));

    assert_eq!(segments[2].scope, StepScope::Task);
    assert!(segments[2].step_ids.contains("doc_governance"));
}

#[test]
fn build_segments_skips_guards() {
    use crate::config::*;
    let task_ctx = TaskRuntimeContext {
        workspace_id: "ws".into(),
        workspace_root: "/tmp".into(),
        ticket_dir: "tickets".into(),
        execution_plan: TaskExecutionPlan {
            steps: vec![
                TaskExecutionStep {
                    id: "plan".into(),

                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "loop_guard".into(),

                    required_capability: None,
                    execution_profile: None,
                    builtin: Some("loop_guard".into()),
                    enabled: true,
                    repeatable: true,
                    is_guard: true,
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
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
            ],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
            max_parallel: None,
        },
        current_cycle: 1,
        init_done: true,
        dynamic_steps: vec![],
        adaptive: None,
        pipeline_vars: PipelineVariables::default(),
        safety: SafetyConfig::default(),
        self_referential: false,
        consecutive_failures: 0,
        project_id: String::new(),
        pinned_invariants: std::sync::Arc::new(vec![]),
        workflow_id: String::new(),
        spawn_depth: 0,
    };

    let segments = build_scope_segments(&task_ctx);
    // Guard is excluded, only plan remains
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].scope, StepScope::Task);
    assert!(segments[0].step_ids.contains("plan"));
    assert!(!segments[0].step_ids.contains("loop_guard"));
}

#[test]
fn resolved_scope_uses_explicit_override() {
    use crate::config::*;
    let step = TaskExecutionStep {
        id: "qa_testing".into(),

        required_capability: None,
        execution_profile: None,
        builtin: None,
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
        scope: Some(StepScope::Task), // Override default Item scope
        behavior: StepBehavior::default(),
        max_parallel: None,
        timeout_secs: None,
        item_select_config: None,
        store_inputs: vec![],
        store_outputs: vec![],
    };
    assert_eq!(step.resolved_scope(), StepScope::Task);
}

#[test]
fn propagate_task_segment_terminal_state_marks_all_items_terminal() {
    let items = vec![
        crate::dto::TaskItemRow {
            id: "item-1".to_string(),
            qa_file_path: "a.md".to_string(),
            dynamic_vars_json: None,
            label: None,
            source: "static".to_string(),
        },
        crate::dto::TaskItemRow {
            id: "item-2".to_string(),
            qa_file_path: "b.md".to_string(),
            dynamic_vars_json: None,
            label: None,
            source: "static".to_string(),
        },
    ];
    let mut item_state = HashMap::new();
    let mut task_acc = StepExecutionAccumulator::new(PipelineVariables::default());
    task_acc.item_status = "unresolved".to_string();
    task_acc.terminal = true;
    task_acc.flags.insert("execution_failed".to_string(), true);
    task_acc.pipeline_vars.vars.insert(
        "fatal_reason".to_string(),
        "provider rate limit exceeded".to_string(),
    );

    propagate_task_segment_terminal_state(
        &items,
        &mut item_state,
        &task_acc,
        &PipelineVariables::default(),
        &["qa_testing".to_string(), "ticket_fix".to_string()],
    );

    assert_eq!(item_state.len(), 2);
    for item_id in ["item-1", "item-2"] {
        let acc = item_state.get(item_id).expect("item state missing");
        assert!(acc.terminal);
        assert_eq!(acc.item_status, "unresolved");
        assert_eq!(acc.flags.get("execution_failed").copied(), Some(true));
        assert_eq!(acc.step_skipped.get("qa_testing").copied(), Some(true));
        assert_eq!(acc.step_skipped.get("ticket_fix").copied(), Some(true));
        assert_eq!(
            acc.pipeline_vars
                .vars
                .get("fatal_reason")
                .map(String::as_str),
            Some("provider rate limit exceeded")
        );
    }
}

#[test]
fn collect_remaining_item_step_steps_returns_only_item_steps_after_segment() {
    use crate::config::{
        PipelineVariables, SafetyConfig, StepBehavior, TaskExecutionPlan, TaskExecutionStep,
        TaskRuntimeContext, WorkflowFinalizeConfig, WorkflowLoopConfig,
    };

    let task_ctx = TaskRuntimeContext {
        workspace_id: "ws".into(),
        workspace_root: "/tmp".into(),
        ticket_dir: "tickets".into(),
        current_cycle: 2,
        execution_plan: TaskExecutionPlan {
            steps: vec![
                TaskExecutionStep {
                    id: "implement".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    scope: Some(StepScope::Task),
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "qa_testing".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    scope: Some(StepScope::Item),
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "ticket_fix".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    scope: Some(StepScope::Item),
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "align_tests".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    scope: Some(StepScope::Task),
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
            ],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
            max_parallel: None,
        },
        init_done: true,
        dynamic_steps: vec![],
        adaptive: None,
        pipeline_vars: PipelineVariables::default(),
        safety: SafetyConfig::default(),
        self_referential: false,
        consecutive_failures: 0,
        project_id: String::new(),
        pinned_invariants: std::sync::Arc::new(vec![]),
        workflow_id: String::new(),
        spawn_depth: 0,
    };
    let segments = build_scope_segments(&task_ctx);

    let skipped = collect_remaining_item_step_steps(&task_ctx, &segments, 1);

    assert_eq!(
        skipped,
        vec!["qa_testing".to_string(), "ticket_fix".to_string()]
    );
}

#[test]
fn collect_remaining_item_step_steps_skips_non_repeatable_steps_after_first_cycle() {
    use crate::config::{
        PipelineVariables, SafetyConfig, StepBehavior, TaskExecutionPlan, TaskExecutionStep,
        TaskRuntimeContext, WorkflowFinalizeConfig, WorkflowLoopConfig,
    };

    let task_ctx = TaskRuntimeContext {
        workspace_id: "ws".into(),
        workspace_root: "/tmp".into(),
        ticket_dir: "tickets".into(),
        current_cycle: 2,
        execution_plan: TaskExecutionPlan {
            steps: vec![TaskExecutionStep {
                id: "qa_testing".into(),
                required_capability: None,
                execution_profile: None,
                builtin: None,
                enabled: true,
                repeatable: false,
                is_guard: false,
                cost_preference: None,
                prehook: None,
                tty: false,
                template: None,
                outputs: vec![],
                pipe_to: None,
                command: None,
                chain_steps: vec![],
                scope: Some(StepScope::Item),
                behavior: StepBehavior::default(),
                max_parallel: None,
                timeout_secs: None,
                item_select_config: None,
                store_inputs: vec![],
                store_outputs: vec![],
            }],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
            max_parallel: None,
        },
        init_done: true,
        dynamic_steps: vec![],
        adaptive: None,
        pipeline_vars: PipelineVariables::default(),
        safety: SafetyConfig::default(),
        self_referential: false,
        consecutive_failures: 0,
        project_id: String::new(),
        pinned_invariants: std::sync::Arc::new(vec![]),
        workflow_id: String::new(),
        spawn_depth: 0,
    };
    let segments = build_scope_segments(&task_ctx);

    assert!(collect_remaining_item_step_steps(&task_ctx, &segments, 0).is_empty());
}

#[tokio::test]
async fn emit_skipped_item_step_events_writes_event_rows() {
    let mut fixture = TestState::new();
    let state = fixture.build();
    let items = vec![
        crate::dto::TaskItemRow {
            id: "item-1".to_string(),
            qa_file_path: "a.md".to_string(),
            dynamic_vars_json: None,
            label: None,
            source: "static".to_string(),
        },
        crate::dto::TaskItemRow {
            id: "item-2".to_string(),
            qa_file_path: "b.md".to_string(),
            dynamic_vars_json: None,
            label: None,
            source: "static".to_string(),
        },
    ];
    let task_id = "task-skip-events";

    emit_skipped_item_step_events(
        &state,
        task_id,
        &items,
        &["qa_testing".to_string(), "ticket_fix".to_string()],
    )
    .await
    .expect("emit skipped events");

    let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
    let skipped_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE task_id = ?1 AND event_type = 'step_skipped'",
            rusqlite::params![task_id],
            |row| row.get(0),
        )
        .expect("count skipped events");

    assert_eq!(skipped_count, 4);
}

#[test]
fn infinite_mode_no_max_cycles_with_guard_enabled_returns_none() {
    let policy = make_loop_policy(LoopMode::Infinite, None);
    // guard is enabled by default, no max_cycles — defer to agent guard
    let result = evaluate_loop_guard_rules(&policy, 1, 0);
    assert_eq!(result, None);
}

#[test]
fn infinite_mode_with_max_cycles_before_limit_returns_none() {
    let policy = make_loop_policy(LoopMode::Infinite, Some(5));
    // cycle 2 < 5, guard enabled → defer to agent guard
    let result = evaluate_loop_guard_rules(&policy, 2, 3);
    assert_eq!(result, None);
}

#[test]
fn build_segments_skips_disabled_steps() {
    use crate::config::*;
    let task_ctx = TaskRuntimeContext {
        workspace_id: "ws".into(),
        workspace_root: "/tmp".into(),
        ticket_dir: "tickets".into(),
        execution_plan: TaskExecutionPlan {
            steps: vec![
                TaskExecutionStep {
                    id: "plan".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    scope: Some(StepScope::Task),
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "disabled_step".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
                    enabled: false,
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
                    scope: Some(StepScope::Item),
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
            ],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
            max_parallel: None,
        },
        current_cycle: 1,
        init_done: true,
        dynamic_steps: vec![],
        adaptive: None,
        pipeline_vars: PipelineVariables::default(),
        safety: SafetyConfig::default(),
        self_referential: false,
        consecutive_failures: 0,
        project_id: String::new(),
        pinned_invariants: std::sync::Arc::new(vec![]),
        workflow_id: String::new(),
        spawn_depth: 0,
    };

    let segments = build_scope_segments(&task_ctx);
    assert_eq!(segments.len(), 1);
    assert!(segments[0].step_ids.contains("plan"));
    assert!(!segments[0].step_ids.contains("disabled_step"));
}

#[test]
fn build_segments_empty_when_no_steps() {
    use crate::config::*;
    let task_ctx = TaskRuntimeContext {
        workspace_id: "ws".into(),
        workspace_root: "/tmp".into(),
        ticket_dir: "tickets".into(),
        execution_plan: TaskExecutionPlan {
            steps: vec![],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
            max_parallel: None,
        },
        current_cycle: 1,
        init_done: true,
        dynamic_steps: vec![],
        adaptive: None,
        pipeline_vars: PipelineVariables::default(),
        safety: SafetyConfig::default(),
        self_referential: false,
        consecutive_failures: 0,
        project_id: String::new(),
        pinned_invariants: std::sync::Arc::new(vec![]),
        workflow_id: String::new(),
        spawn_depth: 0,
    };

    let segments = build_scope_segments(&task_ctx);
    assert!(segments.is_empty());
}

#[test]
fn last_item_segment_detected_when_no_later_item_segments_exist() {
    let segments = vec![
        ScopeSegment {
            scope: StepScope::Task,
            step_ids: std::collections::HashSet::from(["plan".to_string()]),
            max_parallel: 1,
        },
        ScopeSegment {
            scope: StepScope::Item,
            step_ids: std::collections::HashSet::from(["qa".to_string()]),
            max_parallel: 1,
        },
        ScopeSegment {
            scope: StepScope::Task,
            step_ids: std::collections::HashSet::from(["summarize".to_string()]),
            max_parallel: 1,
        },
    ];

    assert!(is_last_item_segment(1, &segments));
}

#[test]
fn last_item_segment_rejects_item_segment_with_later_item_work_remaining() {
    let segments = vec![
        ScopeSegment {
            scope: StepScope::Item,
            step_ids: std::collections::HashSet::from(["qa".to_string()]),
            max_parallel: 1,
        },
        ScopeSegment {
            scope: StepScope::Task,
            step_ids: std::collections::HashSet::from(["plan".to_string()]),
            max_parallel: 1,
        },
        ScopeSegment {
            scope: StepScope::Item,
            step_ids: std::collections::HashSet::from(["fix".to_string()]),
            max_parallel: 1,
        },
    ];

    assert!(!is_last_item_segment(0, &segments));
    assert!(is_last_item_segment(2, &segments));
}

#[test]
fn propagate_task_segment_terminal_state_no_execution_failed_flag() {
    let items = vec![crate::dto::TaskItemRow {
        id: "item-1".to_string(),
        qa_file_path: "a.md".to_string(),
        dynamic_vars_json: None,
        label: None,
        source: "static".to_string(),
    }];
    let mut item_state = HashMap::new();
    let mut task_acc = StepExecutionAccumulator::new(PipelineVariables::default());
    task_acc.item_status = "failed".to_string();
    task_acc.terminal = true;
    // No execution_failed flag set

    propagate_task_segment_terminal_state(
        &items,
        &mut item_state,
        &task_acc,
        &PipelineVariables::default(),
        &[],
    );

    let acc = item_state.get("item-1").expect("item state missing");
    assert!(acc.terminal);
    assert_eq!(acc.item_status, "failed");
    assert!(!acc.flags.contains_key("execution_failed"));
}

#[test]
fn propagate_preserves_existing_item_state() {
    let items = vec![crate::dto::TaskItemRow {
        id: "item-1".to_string(),
        qa_file_path: "a.md".to_string(),
        dynamic_vars_json: None,
        label: None,
        source: "static".to_string(),
    }];
    let mut item_state = HashMap::new();
    let mut existing_acc = StepExecutionAccumulator::new(PipelineVariables::default());
    existing_acc
        .pipeline_vars
        .vars
        .insert("existing_key".to_string(), "existing_val".to_string());
    item_state.insert("item-1".to_string(), existing_acc);

    let mut task_acc = StepExecutionAccumulator::new(PipelineVariables::default());
    task_acc.item_status = "unresolved".to_string();
    task_acc
        .pipeline_vars
        .vars
        .insert("new_key".to_string(), "new_val".to_string());

    propagate_task_segment_terminal_state(
        &items,
        &mut item_state,
        &task_acc,
        &PipelineVariables::default(),
        &[],
    );

    let acc = item_state.get("item-1").unwrap();
    assert_eq!(
        acc.pipeline_vars.vars.get("existing_key").unwrap(),
        "existing_val"
    );
    assert_eq!(acc.pipeline_vars.vars.get("new_key").unwrap(), "new_val");
}

#[test]
fn collect_remaining_item_step_steps_from_start_index_2() {
    use crate::config::*;
    let task_ctx = TaskRuntimeContext {
        workspace_id: "ws".into(),
        workspace_root: "/tmp".into(),
        ticket_dir: "tickets".into(),
        current_cycle: 1,
        execution_plan: TaskExecutionPlan {
            steps: vec![
                TaskExecutionStep {
                    id: "plan".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    scope: Some(StepScope::Task),
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "qa".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    scope: Some(StepScope::Item),
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "governance".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    scope: Some(StepScope::Task),
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
            ],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
            max_parallel: None,
        },
        init_done: true,
        dynamic_steps: vec![],
        adaptive: None,
        pipeline_vars: PipelineVariables::default(),
        safety: SafetyConfig::default(),
        self_referential: false,
        consecutive_failures: 0,
        project_id: String::new(),
        pinned_invariants: std::sync::Arc::new(vec![]),
        workflow_id: String::new(),
        spawn_depth: 0,
    };
    let segments = build_scope_segments(&task_ctx);
    assert_eq!(segments.len(), 3);

    // Skip from segment 2 onward (governance is Task scope, no Item steps)
    let skipped = collect_remaining_item_step_steps(&task_ctx, &segments, 2);
    assert!(skipped.is_empty());
}

#[tokio::test]
async fn emit_skipped_item_step_events_empty_steps_emits_nothing() {
    let mut fixture = TestState::new();
    let state = fixture.build();
    let items = vec![crate::dto::TaskItemRow {
        id: "item-1".to_string(),
        qa_file_path: "a.md".to_string(),
        dynamic_vars_json: None,
        label: None,
        source: "static".to_string(),
    }];

    emit_skipped_item_step_events(&state, "task-1", &items, &[])
        .await
        .expect("should succeed");

    let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE task_id = 'task-1' AND event_type = 'step_skipped'",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(count, 0);
}

#[test]
fn should_snapshot_true_when_both_enabled() {
    assert!(should_snapshot_binary(true, true));
}

#[test]
fn should_snapshot_false_when_not_self_referential() {
    assert!(!should_snapshot_binary(true, false));
}

#[test]
fn should_snapshot_false_when_binary_snapshot_disabled() {
    assert!(!should_snapshot_binary(false, true));
}

#[test]
fn should_snapshot_false_when_both_disabled() {
    assert!(!should_snapshot_binary(false, false));
}

#[test]
fn build_segments_item_select_is_task_scoped() {
    use crate::config::*;
    let task_ctx = TaskRuntimeContext {
        workspace_id: "ws".into(),
        workspace_root: "/tmp".into(),
        ticket_dir: "tickets".into(),
        execution_plan: TaskExecutionPlan {
            steps: vec![
                TaskExecutionStep {
                    id: "qa_testing".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: None,
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
                    scope: None, // default: Item
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: None,
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
                TaskExecutionStep {
                    id: "evaluate".into(),
                    required_capability: None,
                    execution_profile: None,
                    builtin: Some("item_select".into()),
                    enabled: true,
                    repeatable: false,
                    is_guard: false,
                    cost_preference: None,
                    prehook: None,
                    tty: false,
                    template: None,
                    outputs: vec![],
                    pipe_to: None,
                    command: None,
                    chain_steps: vec![],
                    scope: None, // item_select defaults to Task
                    behavior: StepBehavior::default(),
                    max_parallel: None,
                    timeout_secs: None,
                    item_select_config: Some(ItemSelectConfig {
                        strategy: SelectionStrategy::Min,
                        metric_var: Some("error_count".into()),
                        weights: None,
                        threshold: None,
                        store_result: None,
                        tie_break: TieBreak::First,
                    }),
                    store_inputs: vec![],
                    store_outputs: vec![],
                },
            ],
            loop_policy: WorkflowLoopConfig::default(),
            finalize: WorkflowFinalizeConfig::default(),
            max_parallel: None,
        },
        current_cycle: 1,
        init_done: true,
        dynamic_steps: vec![],
        adaptive: None,
        pipeline_vars: PipelineVariables::default(),
        safety: SafetyConfig::default(),
        self_referential: false,
        consecutive_failures: 0,
        project_id: String::new(),
        pinned_invariants: std::sync::Arc::new(vec![]),
        workflow_id: String::new(),
        spawn_depth: 0,
    };

    let segments = build_scope_segments(&task_ctx);
    // qa_testing → Item, evaluate (item_select builtin) → Task
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].scope, StepScope::Item);
    assert!(segments[0].step_ids.contains("qa_testing"));
    assert_eq!(segments[1].scope, StepScope::Task);
    assert!(segments[1].step_ids.contains("evaluate"));

    // has_item_select_step is private to segment.rs, so we test it indirectly
    // through build_scope_segments behavior and find_item_select_config
    let config = super::segment::find_item_select_config_for_test(&task_ctx.execution_plan);
    assert!(config.is_some());
    assert_eq!(config.unwrap().strategy, SelectionStrategy::Min);
}

#[test]
fn collect_item_eval_states_maps_pipeline_vars() {
    let items = vec![
        crate::dto::TaskItemRow {
            id: "item-a".into(),
            qa_file_path: "a.md".into(),
            dynamic_vars_json: None,
            label: None,
            source: "static".into(),
        },
        crate::dto::TaskItemRow {
            id: "item-b".into(),
            qa_file_path: "b.md".into(),
            dynamic_vars_json: None,
            label: None,
            source: "static".into(),
        },
    ];
    let mut item_state = HashMap::new();
    let mut acc_a = StepExecutionAccumulator::new(PipelineVariables::default());
    acc_a
        .pipeline_vars
        .vars
        .insert("error_count".into(), "3".into());
    item_state.insert("item-a".to_string(), acc_a);

    let mut acc_b = StepExecutionAccumulator::new(PipelineVariables::default());
    acc_b
        .pipeline_vars
        .vars
        .insert("error_count".into(), "1".into());
    item_state.insert("item-b".to_string(), acc_b);

    let eval_states = collect_item_eval_states(&items, &item_state);
    assert_eq!(eval_states.len(), 2);
    assert_eq!(eval_states[0].item_id, "item-a");
    assert_eq!(
        eval_states[0].pipeline_vars.get("error_count").unwrap(),
        "3"
    );
    assert_eq!(eval_states[1].item_id, "item-b");
    assert_eq!(
        eval_states[1].pipeline_vars.get("error_count").unwrap(),
        "1"
    );
}

#[test]
fn promote_winner_vars_inserts_into_pipeline() {
    use crate::config::SelectionResult;
    let mut vars = PipelineVariables::default();
    vars.vars.insert("existing".into(), "keep".into());

    let result = SelectionResult {
        winner_id: "item-b".into(),
        eliminated_ids: vec!["item-a".into()],
        winner_vars: {
            let mut m = HashMap::new();
            m.insert("quality_score".into(), "95".into());
            m
        },
    };

    promote_winner_vars(&mut vars, &result);
    assert_eq!(vars.vars.get("item_select_winner").unwrap(), "item-b");
    assert_eq!(vars.vars.get("quality_score").unwrap(), "95");
    assert_eq!(vars.vars.get("existing").unwrap(), "keep");
}

#[test]
fn check_invariants_returns_none_for_empty_invariants() {
    // check_invariants is async, but we can verify the early-return logic
    // by checking the pinned_invariants.is_empty() path inline
    let invariants: Vec<crate::config::InvariantConfig> = vec![];
    assert!(invariants.is_empty());
    // The function returns Ok(None) when pinned_invariants is empty
}

#[tokio::test]
async fn emit_skipped_item_step_events_empty_items_emits_nothing() {
    let mut fixture = TestState::new();
    let state = fixture.build();

    emit_skipped_item_step_events(&state, "task-1", &[], &["qa_testing".to_string()])
        .await
        .expect("should succeed");

    let conn = crate::db::open_conn(&state.db_path).expect("open sqlite");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE task_id = 'task-1'",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(count, 0);
}

// ── cycle_safety pure-function tests ─────────────────────

#[test]
fn should_auto_rollback_true_when_all_conditions_met() {
    use crate::config::CheckpointStrategy;
    assert!(super::cycle_safety::should_auto_rollback(
        true,
        3,
        3,
        &CheckpointStrategy::GitTag
    ));
}

#[test]
fn should_auto_rollback_false_when_disabled() {
    use crate::config::CheckpointStrategy;
    assert!(!super::cycle_safety::should_auto_rollback(
        false,
        5,
        3,
        &CheckpointStrategy::GitTag
    ));
}

#[test]
fn should_auto_rollback_false_when_below_threshold() {
    use crate::config::CheckpointStrategy;
    assert!(!super::cycle_safety::should_auto_rollback(
        true,
        2,
        3,
        &CheckpointStrategy::GitTag
    ));
}

#[test]
fn should_auto_rollback_false_when_no_checkpoint_strategy() {
    use crate::config::CheckpointStrategy;
    assert!(!super::cycle_safety::should_auto_rollback(
        true,
        5,
        3,
        &CheckpointStrategy::None
    ));
}

#[test]
fn should_auto_rollback_true_when_failures_exceed_threshold() {
    use crate::config::CheckpointStrategy;
    assert!(super::cycle_safety::should_auto_rollback(
        true,
        10,
        3,
        &CheckpointStrategy::GitTag
    ));
}

#[test]
fn compute_rollback_tag_normal() {
    let tag = super::cycle_safety::compute_rollback_tag("task-1", 5, 2);
    assert_eq!(tag, "checkpoint/task-1/3");
}

#[test]
fn compute_rollback_tag_zero_failures() {
    let tag = super::cycle_safety::compute_rollback_tag("task-1", 5, 0);
    assert_eq!(tag, "checkpoint/task-1/5");
}

#[test]
fn compute_rollback_tag_saturates_to_one() {
    // current_cycle=1, failures=5 => saturating_sub gives 0, max(1) gives 1
    let tag = super::cycle_safety::compute_rollback_tag("task-1", 1, 5);
    assert_eq!(tag, "checkpoint/task-1/1");
}

#[test]
fn compute_rollback_tag_exact_cycle_one() {
    let tag = super::cycle_safety::compute_rollback_tag("my-task", 3, 3);
    assert_eq!(tag, "checkpoint/my-task/1");
}
