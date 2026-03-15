use super::accumulator::StepExecutionAccumulator;
use super::dispatch::is_execution_hard_failure;
use super::spill::{spill_large_var, spill_to_file};
use crate::scheduler::item_select::{execute_item_select, ItemEvalState};
use agent_orchestrator::config::PIPELINE_VAR_INLINE_LIMIT;
use agent_orchestrator::config::{
    CaptureDecl, CaptureSource, ExecutionMode, ItemSelectConfig, PipelineVariables,
    SelectionStrategy, StepBehavior, TieBreak,
};
use std::collections::HashMap;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("item-exec-test-{}-{}", name, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create item executor temp dir");
    dir
}

fn empty_pipeline() -> PipelineVariables {
    PipelineVariables {
        prev_stdout: String::new(),
        prev_stderr: String::new(),
        diff: String::new(),
        build_errors: Vec::new(),
        test_failures: Vec::new(),
        vars: HashMap::new(),
    }
}

#[test]
fn execution_hard_failure_detects_failed_validation_status() {
    let result = agent_orchestrator::dto::RunResult {
        success: false,
        exit_code: -6,
        stdout_path: String::new(),
        stderr_path: String::new(),
        timed_out: false,
        duration_ms: None,
        output: None,
        validation_status: "failed".to_string(),
        agent_id: "agent".to_string(),
        run_id: "run".to_string(),
        execution_profile: "host".to_string(),
        execution_mode: "host".to_string(),
        sandbox_denied: false,
        sandbox_denial_reason: None,
        sandbox_violation_kind: None,
        sandbox_resource_kind: None,
        sandbox_network_target: None,
    };

    assert!(is_execution_hard_failure(&result));
}

#[test]
fn execution_hard_failure_ignores_non_validation_failures() {
    let result = agent_orchestrator::dto::RunResult {
        success: false,
        exit_code: 1,
        stdout_path: String::new(),
        stderr_path: String::new(),
        timed_out: false,
        duration_ms: None,
        output: None,
        validation_status: "passed".to_string(),
        agent_id: "agent".to_string(),
        run_id: "run".to_string(),
        execution_profile: "host".to_string(),
        execution_mode: "host".to_string(),
        sandbox_denied: false,
        sandbox_denial_reason: None,
        sandbox_violation_kind: None,
        sandbox_resource_kind: None,
        sandbox_network_target: None,
    };

    assert!(!is_execution_hard_failure(&result));
}

// ── spill_large_var tests ────────────────────────────────────────

#[test]
fn spill_large_var_small_value_inserts_inline() {
    let dir = temp_dir("slv-small");
    let mut pipeline = empty_pipeline();
    let value = "hello world".to_string();

    spill_large_var(&dir, "task1", "stdout", value.clone(), &mut pipeline);

    assert_eq!(
        pipeline.vars.get("stdout").expect("stdout should be set"),
        "hello world"
    );
    // _path is always set now (even for small values)
    let p = pipeline
        .vars
        .get("stdout_path")
        .expect("stdout_path must be set");
    assert_eq!(
        std::fs::read_to_string(p).expect("read stdout spill file"),
        "hello world"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spill_large_var_exactly_at_limit_inserts_inline() {
    let dir = temp_dir("slv-exact");
    let mut pipeline = empty_pipeline();
    let value = "x".repeat(PIPELINE_VAR_INLINE_LIMIT);

    spill_large_var(&dir, "task1", "out", value.clone(), &mut pipeline);

    assert_eq!(pipeline.vars.get("out").expect("out should be set"), &value);
    // _path is always set now (even for small values)
    let p = pipeline.vars.get("out_path").expect("out_path must be set");
    assert_eq!(
        std::fs::read_to_string(p).expect("read out spill file"),
        value
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spill_large_var_one_byte_over_limit_spills_to_file() {
    let dir = temp_dir("slv-over");
    let mut pipeline = empty_pipeline();
    let value = "x".repeat(PIPELINE_VAR_INLINE_LIMIT + 1);

    spill_large_var(&dir, "task1", "big", value.clone(), &mut pipeline);

    // Inline value should be truncated with the marker
    let inline = pipeline.vars.get("big").expect("big should be set");
    assert!(inline.contains("...\n[truncated — full content at "));
    // The inline prefix (before the marker) should be at most PIPELINE_VAR_INLINE_LIMIT bytes
    let prefix_end = inline
        .find("...\n[truncated")
        .expect("truncation marker should exist");
    assert!(prefix_end <= PIPELINE_VAR_INLINE_LIMIT);

    // Companion path variable should exist
    let path_str = pipeline
        .vars
        .get("big_path")
        .expect("big_path should be set");
    let spill_path = std::path::Path::new(path_str);
    assert!(spill_path.exists());

    // File should contain the full original value
    let on_disk = std::fs::read_to_string(spill_path).expect("read spilled big value");
    assert_eq!(on_disk, value);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spill_large_var_large_value_sets_correct_path_key() {
    let dir = temp_dir("slv-pathkey");
    let mut pipeline = empty_pipeline();
    let value = "y".repeat(PIPELINE_VAR_INLINE_LIMIT + 100);

    spill_large_var(&dir, "t42", "my_key", value, &mut pipeline);

    let path_str = pipeline
        .vars
        .get("my_key_path")
        .expect("my_key_path should be set");
    assert!(path_str.contains("t42"));
    assert!(path_str.ends_with("my_key.txt"));
}

#[test]
fn spill_large_var_multibyte_boundary() {
    let dir = temp_dir("slv-mb");
    let mut pipeline = empty_pipeline();
    // Build a string that puts a multi-byte char right at the 4096 boundary.
    // Chinese chars are 3 bytes each. Fill up to just before the limit, then
    // add a char whose encoding would straddle the boundary.
    let prefix_len = PIPELINE_VAR_INLINE_LIMIT - 1; // 4095 ASCII bytes
    let mut value = "a".repeat(prefix_len);
    // Append multi-byte chars so total exceeds limit
    value.push_str("你好世界"); // 12 bytes of UTF-8
    assert!(value.len() > PIPELINE_VAR_INLINE_LIMIT);

    spill_large_var(&dir, "task1", "mb", value.clone(), &mut pipeline);

    let inline = pipeline.vars.get("mb").expect("mb should be set");
    // The truncated portion must be valid UTF-8 (guaranteed by safe_end logic)
    assert!(inline.contains("...\n[truncated"));

    // Verify the full file content is intact
    let path_str = pipeline.vars.get("mb_path").expect("mb_path should be set");
    let on_disk = std::fs::read_to_string(path_str).expect("read multibyte spill file");
    assert_eq!(on_disk, value);

    std::fs::remove_dir_all(&dir).ok();
}

// ── spill_to_file tests ──────────────────────────────────────────

#[test]
fn spill_to_file_small_value_returns_none() {
    let dir = temp_dir("stf-small");
    let value = "short string";

    let result = spill_to_file(&dir, "task1", "key", value);
    assert!(result.is_none());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spill_to_file_exactly_at_limit_returns_none() {
    let dir = temp_dir("stf-exact");
    let value = "z".repeat(PIPELINE_VAR_INLINE_LIMIT);

    let result = spill_to_file(&dir, "task1", "key", &value);
    assert!(result.is_none());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spill_to_file_one_byte_over_returns_some() {
    let dir = temp_dir("stf-over");
    let value = "z".repeat(PIPELINE_VAR_INLINE_LIMIT + 1);

    let result = spill_to_file(&dir, "task1", "key", &value);
    assert!(result.is_some());

    let (truncated, path_str) = result.expect("spill should occur");
    assert!(truncated.starts_with("zzzz"));
    assert!(truncated.contains("...\n[truncated — full content at "));
    assert!(path_str.ends_with("key.txt"));

    // Verify file on disk
    let on_disk = std::fs::read_to_string(&path_str).expect("read spilled file");
    assert_eq!(on_disk, value);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spill_to_file_large_value_truncated_format() {
    let dir = temp_dir("stf-fmt");
    let value = "A".repeat(PIPELINE_VAR_INLINE_LIMIT + 500);

    let (truncated, path_str) =
        spill_to_file(&dir, "task1", "output", &value).expect("spill should occur");

    // The truncated string should contain the marker text
    assert!(truncated.contains("...\n[truncated — full content at "));
    // The path in the truncated message should match the returned path
    assert!(truncated.contains(&path_str));
    // The truncated prefix should be exactly PIPELINE_VAR_INLINE_LIMIT bytes of 'A'
    let prefix = &truncated[..PIPELINE_VAR_INLINE_LIMIT];
    assert!(prefix.chars().all(|c| c == 'A'));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spill_to_file_multibyte_at_boundary() {
    let dir = temp_dir("stf-mb");
    // Create a value where a 3-byte UTF-8 char straddles the 4096 boundary.
    // 4095 ASCII bytes + "你好" (6 bytes) = 4101 total, exceeding the limit.
    // The char "你" starts at byte 4095 and ends at 4097, straddling the boundary.
    let mut value = "b".repeat(PIPELINE_VAR_INLINE_LIMIT - 1);
    value.push_str("你好世界你好世界"); // 24 more bytes

    let result = spill_to_file(&dir, "task1", "key", &value);
    assert!(result.is_some());

    let (truncated, _path_str) = result.expect("spill should occur");
    // The truncated text should be valid UTF-8 (it is a String, so guaranteed)
    // and should NOT split a multi-byte character
    let prefix_end = truncated
        .find("...\n[truncated")
        .expect("truncation marker should exist");
    let prefix = &truncated[..prefix_end];
    // The prefix should end before the multi-byte char since it can't fit
    // within the limit without splitting
    assert_eq!(prefix.len(), PIPELINE_VAR_INLINE_LIMIT - 1);
    assert!(prefix.chars().all(|c| c == 'b'));

    // Full content on disk should be intact
    let on_disk = std::fs::read_to_string(&_path_str).expect("read spilled multibyte file");
    assert_eq!(on_disk, value);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spill_to_file_multibyte_fully_within_limit() {
    let dir = temp_dir("stf-mb2");
    // 4094 ASCII bytes + "你" (3 bytes) = 4097, just over the limit.
    // But the char boundary at 4094+3=4097 > 4096, so safe_end backs down to 4094.
    let mut value = "c".repeat(PIPELINE_VAR_INLINE_LIMIT - 2);
    value.push_str("你好世界"); // 12 bytes, total = 4094 + 12 = 4106

    let (truncated, _) = spill_to_file(&dir, "task1", "k", &value).expect("spill should occur");
    let prefix_end = truncated
        .find("...\n[truncated")
        .expect("truncation marker should exist");
    let prefix = &truncated[..prefix_end];
    // safe_end should back up to the start of the multibyte char
    // 4094 bytes of 'c', then "你" starts at 4094 and needs bytes 4094..4097
    // which exceeds the 4096 limit, so safe_end = 4094
    assert_eq!(prefix.len(), PIPELINE_VAR_INLINE_LIMIT - 2);

    std::fs::remove_dir_all(&dir).ok();
}

// ── Layer-2 dispatch guard tests ─────────────────────────────────

fn make_step(
    id: &str,
    builtin: Option<&str>,
    execution: ExecutionMode,
) -> agent_orchestrator::config::TaskExecutionStep {
    agent_orchestrator::config::TaskExecutionStep {
        id: id.to_string(),
        builtin: builtin.map(|s| s.to_string()),
        required_capability: None,
        execution_profile: None,
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
        behavior: StepBehavior {
            execution,
            ..StepBehavior::default()
        },
        max_parallel: None,
        timeout_secs: None,
        item_select_config: None,
        store_inputs: vec![],
        store_outputs: vec![],
    }
}

#[test]
fn builtin_guard_routes_self_test_regardless_of_execution_mode() {
    // Step has stale Agent execution mode but builtin field is authoritative.
    let step = make_step("self_test", Some("self_test"), ExecutionMode::Agent);
    assert_eq!(
        step.effective_execution_mode().as_ref(),
        &ExecutionMode::Builtin {
            name: "self_test".to_string()
        },
        "dispatch guard must resolve self_test builtin even when behavior.execution is Agent"
    );
}

#[test]
fn builtin_guard_noop_for_agent_step() {
    // Pure agent step (no builtin field) stays as Agent.
    let step = make_step("plan", None, ExecutionMode::Agent);
    assert_eq!(
        step.effective_execution_mode().as_ref(),
        &ExecutionMode::Agent
    );
}

#[test]
fn builtin_guard_noop_when_already_correct() {
    // Step already has correct Builtin execution mode — guard is a no-op.
    let step = make_step(
        "self_test",
        Some("self_test"),
        ExecutionMode::Builtin {
            name: "self_test".to_string(),
        },
    );
    assert_eq!(
        step.effective_execution_mode().as_ref(),
        &ExecutionMode::Builtin {
            name: "self_test".to_string()
        }
    );
}

// ── StepExecutionAccumulator tests ──────────────────────────────

fn make_task_ctx(
    steps: Vec<agent_orchestrator::config::TaskExecutionStep>,
    max_cycles: Option<u32>,
    current_cycle: u32,
) -> agent_orchestrator::config::TaskRuntimeContext {
    agent_orchestrator::config::TaskRuntimeContext {
        workspace_id: "default".to_string(),
        workspace_root: std::path::PathBuf::from("/tmp/test"),
        ticket_dir: "/tmp/test/docs/ticket".to_string(),
        execution_plan: std::sync::Arc::new(agent_orchestrator::config::TaskExecutionPlan {
            steps,
            loop_policy: agent_orchestrator::config::WorkflowLoopConfig {
                mode: agent_orchestrator::config::LoopMode::Fixed,
                guard: agent_orchestrator::config::WorkflowLoopGuardConfig {
                    enabled: true,
                    stop_when_no_unresolved: true,
                    max_cycles,
                    agent_template: None,
                },
                convergence_expr: None,
            },
            finalize: Default::default(),
            max_parallel: None,
            item_isolation: None,
        }),
        execution: Default::default(),
        current_cycle,
        init_done: false,
        dynamic_steps: std::sync::Arc::new(vec![]),
        adaptive: std::sync::Arc::new(None),
        pipeline_vars: empty_pipeline(),
        safety: std::sync::Arc::new(Default::default()),
        self_referential: false,
        consecutive_failures: 0,
        project_id: String::new(),
        pinned_invariants: std::sync::Arc::new(vec![]),
        workflow_id: String::new(),
        spawn_depth: 0,
        item_step_failures: HashMap::new(),
        item_retry_after: HashMap::new(),
        restart_completed_steps: std::collections::HashSet::new(),
    }
}

fn make_item(id: &str, qa_file: &str) -> agent_orchestrator::dto::TaskItemRow {
    agent_orchestrator::dto::TaskItemRow {
        id: id.to_string(),
        qa_file_path: qa_file.to_string(),
        dynamic_vars_json: None,
        label: None,
        source: "static".to_string(),
    }
}

fn make_run_result(
    exit_code: i64,
    success: bool,
    output: Option<agent_orchestrator::collab::AgentOutput>,
) -> agent_orchestrator::dto::RunResult {
    agent_orchestrator::dto::RunResult {
        success,
        exit_code,
        stdout_path: String::new(),
        stderr_path: String::new(),
        timed_out: false,
        duration_ms: None,
        output,
        validation_status: if success { "passed" } else { "failed" }.to_string(),
        agent_id: "test-agent".to_string(),
        run_id: "run-1".to_string(),
        execution_profile: "host".to_string(),
        execution_mode: "host".to_string(),
        sandbox_denied: false,
        sandbox_denial_reason: None,
        sandbox_violation_kind: None,
        sandbox_resource_kind: None,
        sandbox_network_target: None,
    }
}

// ── new() ────────────────────────────────

#[test]
fn accumulator_new_initializes_with_pending_status() {
    let acc = StepExecutionAccumulator::new(empty_pipeline());
    assert_eq!(acc.item_status, "pending");
    assert!(acc.active_tickets.is_empty());
    assert!(acc.flags.is_empty());
    assert!(acc.exit_codes.is_empty());
    assert!(!acc.terminal);
    assert_eq!(acc.new_ticket_count, 0);
    assert!(acc.qa_confidence.is_none());
}

// ── merge_task_pipeline_vars() ───────────

#[test]
fn merge_task_pipeline_vars_does_not_overwrite_existing() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.pipeline_vars
        .vars
        .insert("key".to_string(), "item_value".to_string());

    let mut task_vars = empty_pipeline();
    task_vars
        .vars
        .insert("key".to_string(), "task_value".to_string());
    task_vars
        .vars
        .insert("new_key".to_string(), "new_value".to_string());

    acc.merge_task_pipeline_vars(&task_vars);

    assert_eq!(acc.pipeline_vars.vars.get("key").unwrap(), "item_value");
    assert_eq!(acc.pipeline_vars.vars.get("new_key").unwrap(), "new_value");
}

#[test]
fn merge_task_pipeline_vars_copies_build_errors_when_empty() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let mut task_vars = empty_pipeline();
    task_vars.build_errors = vec![agent_orchestrator::config::BuildError {
        file: Some("main.rs".to_string()),
        line: Some(10),
        column: None,
        message: "error".to_string(),
        level: agent_orchestrator::config::BuildErrorLevel::Error,
    }];
    task_vars.test_failures = vec![agent_orchestrator::config::TestFailure {
        test_name: "test1".to_string(),
        message: "failed".to_string(),
        file: None,
        line: None,
        stdout: None,
    }];

    acc.merge_task_pipeline_vars(&task_vars);

    assert_eq!(acc.pipeline_vars.build_errors.len(), 1);
    assert_eq!(acc.pipeline_vars.test_failures.len(), 1);
}

#[test]
fn merge_task_pipeline_vars_preserves_existing_build_errors() {
    let mut pipeline = empty_pipeline();
    pipeline.build_errors = vec![agent_orchestrator::config::BuildError {
        file: Some("existing.rs".to_string()),
        line: None,
        column: None,
        message: "existing error".to_string(),
        level: agent_orchestrator::config::BuildErrorLevel::Error,
    }];
    let mut acc = StepExecutionAccumulator::new(pipeline);

    let mut task_vars = empty_pipeline();
    task_vars.build_errors = vec![agent_orchestrator::config::BuildError {
        file: Some("new.rs".to_string()),
        line: None,
        column: None,
        message: "new error".to_string(),
        level: agent_orchestrator::config::BuildErrorLevel::Error,
    }];

    acc.merge_task_pipeline_vars(&task_vars);

    assert_eq!(acc.pipeline_vars.build_errors.len(), 1);
    assert_eq!(
        acc.pipeline_vars.build_errors[0].file,
        Some("existing.rs".to_string())
    );
}

// ── apply_captures() ─────────────────────

#[test]
fn apply_captures_exit_code() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "qa_exit".to_string(),
        source: CaptureSource::ExitCode,
        json_path: None,
    }];
    let result = make_run_result(42, false, None);

    acc.apply_captures(&captures, temp.path(), "task-1", "qa_testing", &result);

    assert_eq!(*acc.exit_codes.get("qa_testing").unwrap(), 42);
    assert_eq!(acc.pipeline_vars.vars.get("qa_exit").unwrap(), "42");
}

#[test]
fn apply_captures_failed_flag() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "qa_failed".to_string(),
        source: CaptureSource::FailedFlag,
        json_path: None,
    }];
    let result = make_run_result(1, false, None);

    acc.apply_captures(&captures, temp.path(), "task-1", "qa", &result);

    assert!(*acc.flags.get("qa_failed").unwrap());
    assert_eq!(acc.pipeline_vars.vars.get("qa_failed").unwrap(), "true");
}

#[test]
fn apply_captures_success_flag() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "fix_success".to_string(),
        source: CaptureSource::SuccessFlag,
        json_path: None,
    }];
    let result = make_run_result(0, true, None);

    acc.apply_captures(&captures, temp.path(), "task-1", "fix", &result);

    assert!(*acc.flags.get("fix_success").unwrap());
    assert_eq!(acc.pipeline_vars.vars.get("fix_success").unwrap(), "true");
}

#[test]
fn apply_captures_success_flag_on_failure() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "fix_success".to_string(),
        source: CaptureSource::SuccessFlag,
        json_path: None,
    }];
    let result = make_run_result(1, false, None);

    acc.apply_captures(&captures, temp.path(), "task-1", "fix", &result);

    assert!(!*acc.flags.get("fix_success").unwrap());
}

#[test]
fn apply_captures_stderr() {
    let output = agent_orchestrator::collab::AgentOutput {
        run_id: uuid::Uuid::new_v4(),
        agent_id: "a".to_string(),
        phase: "qa".to_string(),
        exit_code: 0,
        stdout: "out content".to_string(),
        stderr: "err content".to_string(),
        artifacts: vec![],
        metrics: Default::default(),
        confidence: 0.0,
        quality_score: 0.0,
        created_at: chrono::Utc::now(),
        build_errors: vec![],
        test_failures: vec![],
    };
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "qa_stderr".to_string(),
        source: CaptureSource::Stderr,
        json_path: None,
    }];
    let result = make_run_result(0, true, Some(output));

    acc.apply_captures(&captures, temp.path(), "task-1", "qa", &result);

    assert_eq!(
        acc.pipeline_vars.vars.get("qa_stderr").unwrap(),
        "err content"
    );
}

#[test]
fn apply_captures_stdout_no_output_is_noop() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "qa_stdout".to_string(),
        source: CaptureSource::Stdout,
        json_path: None,
    }];
    let result = make_run_result(0, true, None);

    acc.apply_captures(&captures, temp.path(), "task-1", "qa", &result);

    assert!(!acc.pipeline_vars.vars.contains_key("qa_stdout"));
}

#[test]
fn apply_captures_stderr_no_output_is_noop() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "qa_stderr".to_string(),
        source: CaptureSource::Stderr,
        json_path: None,
    }];
    let result = make_run_result(0, true, None);

    acc.apply_captures(&captures, temp.path(), "task-1", "qa", &result);

    assert!(!acc.pipeline_vars.vars.contains_key("qa_stderr"));
}

#[test]
fn apply_captures_multiple() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![
        CaptureDecl {
            var: "exit".to_string(),
            source: CaptureSource::ExitCode,
            json_path: None,
        },
        CaptureDecl {
            var: "failed".to_string(),
            source: CaptureSource::FailedFlag,
            json_path: None,
        },
        CaptureDecl {
            var: "ok".to_string(),
            source: CaptureSource::SuccessFlag,
            json_path: None,
        },
    ];
    let result = make_run_result(0, true, None);

    acc.apply_captures(&captures, temp.path(), "task-1", "step1", &result);

    assert_eq!(acc.pipeline_vars.vars.get("exit").unwrap(), "0");
    assert!(!*acc.flags.get("failed").unwrap());
    assert!(*acc.flags.get("ok").unwrap());
}

#[test]
fn apply_captures_stdout_spills_under_task_logs_dir() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let captures = vec![CaptureDecl {
        var: "plan_output".to_string(),
        source: CaptureSource::Stdout,
        json_path: None,
    }];
    let output = agent_orchestrator::collab::AgentOutput {
        run_id: uuid::Uuid::new_v4(),
        agent_id: "planner".to_string(),
        phase: "plan".to_string(),
        exit_code: 0,
        stdout: "CHAIN_PLAN".to_string(),
        stderr: String::new(),
        artifacts: vec![],
        metrics: Default::default(),
        confidence: 0.0,
        quality_score: 0.0,
        created_at: chrono::Utc::now(),
        build_errors: vec![],
        test_failures: vec![],
    };
    let result = make_run_result(0, true, Some(output));

    acc.apply_captures(&captures, temp.path(), "task-123", "plan", &result);

    let spill_path = temp.path().join("task-123").join("plan_output.txt");
    assert_eq!(
        acc.pipeline_vars
            .vars
            .get("plan_output_path")
            .expect("path var should be set"),
        &spill_path.to_string_lossy().to_string()
    );
    assert_eq!(
        std::fs::read_to_string(&spill_path).expect("read spill file"),
        "CHAIN_PLAN"
    );
}

#[test]
fn apply_captures_stdout_json_path_extracts_score() {
    let output = agent_orchestrator::collab::AgentOutput {
        run_id: uuid::Uuid::new_v4(),
        agent_id: "bench".to_string(),
        phase: "qa".to_string(),
        exit_code: 0,
        stdout: r#"{"total_score":85,"summary":"ok"}"#.to_string(),
        stderr: String::new(),
        artifacts: vec![],
        metrics: Default::default(),
        confidence: 0.0,
        quality_score: 0.0,
        created_at: chrono::Utc::now(),
        build_errors: vec![],
        test_failures: vec![],
    };
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "score".to_string(),
        source: CaptureSource::Stdout,
        json_path: Some("$.total_score".to_string()),
    }];
    let result = make_run_result(0, true, Some(output));

    acc.apply_captures(&captures, temp.path(), "task-1", "benchmark", &result);

    assert_eq!(acc.pipeline_vars.vars.get("score").unwrap(), "85");
}

#[test]
fn apply_captures_stdout_json_path_extracts_stream_json_score() {
    let output = agent_orchestrator::collab::AgentOutput {
        run_id: uuid::Uuid::new_v4(),
        agent_id: "bench".to_string(),
        phase: "qa".to_string(),
        exit_code: 0,
        stdout: concat!(
            "{\"type\":\"text\",\"text\":\"running\"}\n",
            "{\"type\":\"result\",\"result\":\"{\\\"total_score\\\":91}\"}\n"
        )
        .to_string(),
        stderr: String::new(),
        artifacts: vec![],
        metrics: Default::default(),
        confidence: 0.0,
        quality_score: 0.0,
        created_at: chrono::Utc::now(),
        build_errors: vec![],
        test_failures: vec![],
    };
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "score".to_string(),
        source: CaptureSource::Stdout,
        json_path: Some("$.total_score".to_string()),
    }];
    let result = make_run_result(0, true, Some(output));

    acc.apply_captures(&captures, temp.path(), "task-1", "benchmark", &result);

    assert_eq!(acc.pipeline_vars.vars.get("score").unwrap(), "91");
}

#[test]
fn apply_captures_stdout_json_path_falls_back_to_empty_string_on_missing_field() {
    let output = agent_orchestrator::collab::AgentOutput {
        run_id: uuid::Uuid::new_v4(),
        agent_id: "bench".to_string(),
        phase: "qa".to_string(),
        exit_code: 0,
        stdout: r#"{"summary":"missing"}"#.to_string(),
        stderr: String::new(),
        artifacts: vec![],
        metrics: Default::default(),
        confidence: 0.0,
        quality_score: 0.0,
        created_at: chrono::Utc::now(),
        build_errors: vec![],
        test_failures: vec![],
    };
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "score".to_string(),
        source: CaptureSource::Stdout,
        json_path: Some("$.total_score".to_string()),
    }];
    let result = make_run_result(0, true, Some(output));

    acc.apply_captures(&captures, temp.path(), "task-1", "benchmark", &result);

    assert_eq!(acc.pipeline_vars.vars.get("score").unwrap(), "");
}

#[test]
fn benchmark_score_capture_can_drive_item_select_max() {
    let temp = tempfile::tempdir().expect("tempdir");
    let captures = vec![CaptureDecl {
        var: "score".to_string(),
        source: CaptureSource::Stdout,
        json_path: Some("$.total_score".to_string()),
    }];

    let mut first = StepExecutionAccumulator::new(empty_pipeline());
    let result_a = make_run_result(
        0,
        true,
        Some(agent_orchestrator::collab::AgentOutput {
            run_id: uuid::Uuid::new_v4(),
            agent_id: "bench".to_string(),
            phase: "qa".to_string(),
            exit_code: 0,
            stdout: r#"{"total_score":67}"#.to_string(),
            stderr: String::new(),
            artifacts: vec![],
            metrics: Default::default(),
            confidence: 0.0,
            quality_score: 0.0,
            created_at: chrono::Utc::now(),
            build_errors: vec![],
            test_failures: vec![],
        }),
    );
    first.apply_captures(&captures, temp.path(), "task-1", "benchmark", &result_a);

    let mut second = StepExecutionAccumulator::new(empty_pipeline());
    let result_b = make_run_result(
        0,
        true,
        Some(agent_orchestrator::collab::AgentOutput {
            run_id: uuid::Uuid::new_v4(),
            agent_id: "bench".to_string(),
            phase: "qa".to_string(),
            exit_code: 0,
            stdout: r#"{"total_score":85}"#.to_string(),
            stderr: String::new(),
            artifacts: vec![],
            metrics: Default::default(),
            confidence: 0.0,
            quality_score: 0.0,
            created_at: chrono::Utc::now(),
            build_errors: vec![],
            test_failures: vec![],
        }),
    );
    second.apply_captures(&captures, temp.path(), "task-1", "benchmark", &result_b);

    let items = vec![
        ItemEvalState {
            item_id: "approach-a".to_string(),
            pipeline_vars: first.pipeline_vars.vars.clone(),
        },
        ItemEvalState {
            item_id: "approach-b".to_string(),
            pipeline_vars: second.pipeline_vars.vars.clone(),
        },
    ];
    let config = ItemSelectConfig {
        strategy: SelectionStrategy::Max,
        metric_var: Some("score".to_string()),
        weights: None,
        threshold: None,
        store_result: None,
        tie_break: TieBreak::First,
    };

    let result = execute_item_select(&items, &config).expect("selection should succeed");

    assert_eq!(result.winner_id, "approach-b");
}

// ── to_prehook_context() ─────────────────

#[test]
fn to_prehook_context_basic_fields() {
    let acc = StepExecutionAccumulator::new(empty_pipeline());
    let item = make_item("item-1", "docs/qa/test.md");
    let ctx = make_task_ctx(vec![], Some(2), 1);

    let phc = acc.to_prehook_context("task-1", &item, &ctx, "qa_testing");

    assert_eq!(phc.task_id, "task-1");
    assert_eq!(phc.task_item_id, "item-1");
    assert_eq!(phc.cycle, 1);
    assert_eq!(phc.step, "qa_testing");
    assert_eq!(phc.qa_file_path, "docs/qa/test.md");
    assert_eq!(phc.item_status, "pending");
    assert_eq!(phc.task_status, "running");
    assert_eq!(phc.max_cycles, 2);
    assert!(!phc.is_last_cycle);
}

#[test]
fn to_prehook_context_is_last_cycle_when_current_equals_max() {
    let acc = StepExecutionAccumulator::new(empty_pipeline());
    let item = make_item("item-1", "docs/qa/test.md");
    let ctx = make_task_ctx(vec![], Some(2), 2);

    let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

    assert!(phc.is_last_cycle);
}

#[test]
fn to_prehook_context_exit_codes_from_canonical_step_ids() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.exit_codes.insert("qa_testing".to_string(), 1);
    acc.exit_codes.insert("ticket_fix".to_string(), 0);
    acc.exit_codes.insert("retest".to_string(), 2);

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], Some(1), 1);

    let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

    assert_eq!(phc.qa_exit_code, Some(1));
    assert_eq!(phc.fix_exit_code, Some(0));
    assert_eq!(phc.retest_exit_code, Some(2));
}

#[test]
fn to_prehook_context_exit_codes_use_first_alias_match() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    // "qa" is the first canonical alias for qa capability
    acc.exit_codes.insert("qa".to_string(), 5);
    acc.exit_codes.insert("qa_testing".to_string(), 10);

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], Some(1), 1);

    let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

    assert_eq!(phc.qa_exit_code, Some(5));
}

#[test]
fn to_prehook_context_qa_failed_and_fix_required() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.flags.insert("qa_failed".to_string(), true);
    acc.active_tickets.push("ticket1.md".to_string());

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], Some(1), 1);

    let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

    assert!(phc.qa_failed);
    assert!(phc.fix_required);
    assert_eq!(phc.active_ticket_count, 1);
}

#[test]
fn to_prehook_context_fix_required_from_tickets_even_without_qa_failed() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.active_tickets.push("ticket1.md".to_string());

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], Some(1), 1);

    let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

    assert!(!phc.qa_failed);
    assert!(phc.fix_required); // fix_required = qa_failed || !active_tickets.is_empty()
}

#[test]
fn to_prehook_context_self_test_vars() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.pipeline_vars
        .vars
        .insert("self_test_exit_code".to_string(), "0".to_string());
    acc.pipeline_vars
        .vars
        .insert("self_test_passed".to_string(), "true".to_string());

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], Some(1), 1);

    let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

    assert_eq!(phc.self_test_exit_code, Some(0));
    assert!(phc.self_test_passed);
}

#[test]
fn to_prehook_context_build_test_counts() {
    let mut pipeline = empty_pipeline();
    pipeline.build_errors = vec![agent_orchestrator::config::BuildError {
        file: Some("f.rs".to_string()),
        line: None,
        column: None,
        message: "err".to_string(),
        level: agent_orchestrator::config::BuildErrorLevel::Error,
    }];
    pipeline.test_failures = vec![
        agent_orchestrator::config::TestFailure {
            test_name: "t1".to_string(),
            message: "fail".to_string(),
            file: None,
            line: None,
            stdout: None,
        },
        agent_orchestrator::config::TestFailure {
            test_name: "t2".to_string(),
            message: "fail".to_string(),
            file: None,
            line: None,
            stdout: None,
        },
    ];
    let acc = StepExecutionAccumulator::new(pipeline);

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], Some(1), 1);

    let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

    assert_eq!(phc.build_error_count, 1);
    assert_eq!(phc.test_failure_count, 2);
}

#[test]
fn to_prehook_context_capability_based_step_ids() {
    let steps = vec![make_step("custom_qa", None, ExecutionMode::Agent)];
    // Give the step a qa capability
    let mut steps = steps;
    steps[0].required_capability = Some("qa".to_string());

    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.exit_codes.insert("custom_qa".to_string(), 3);

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(steps, Some(1), 1);

    let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

    // canonical aliases "qa", "qa_testing" come first, then capability match "custom_qa"
    // Since neither "qa" nor "qa_testing" have exit codes, it falls through to "custom_qa"
    assert_eq!(phc.qa_exit_code, Some(3));
}

#[test]
fn to_prehook_context_max_cycles_defaults_to_1() {
    let acc = StepExecutionAccumulator::new(empty_pipeline());
    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], None, 1);

    let phc = acc.to_prehook_context("task-1", &item, &ctx, "step");

    assert_eq!(phc.max_cycles, 1);
    assert!(phc.is_last_cycle);
}

// ── to_finalize_context() ────────────────

#[test]
fn to_finalize_context_basic_fields() {
    let acc = StepExecutionAccumulator::new(empty_pipeline());
    let item = make_item("item-1", "docs/qa/test.md");
    let ctx = make_task_ctx(vec![], Some(2), 1);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    assert_eq!(fc.task_id, "task-1");
    assert_eq!(fc.task_item_id, "item-1");
    assert_eq!(fc.cycle, 1);
    assert_eq!(fc.qa_file_path, "docs/qa/test.md");
    assert_eq!(fc.item_status, "pending");
    assert!(!fc.is_last_cycle);
}

#[test]
fn to_finalize_context_qa_ran_and_configured() {
    let steps = vec![{
        let mut s = make_step("qa_testing", None, ExecutionMode::Agent);
        s.required_capability = Some("qa".to_string());
        s
    }];

    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.step_ran.insert("qa_testing".to_string(), true);
    acc.exit_codes.insert("qa_testing".to_string(), 0);

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(steps, Some(1), 1);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    assert!(fc.qa_ran);
    assert!(fc.qa_configured);
    assert!(fc.qa_observed);
    assert!(fc.qa_enabled);
    assert!(!fc.qa_skipped);
}

#[test]
fn to_finalize_context_qa_skipped() {
    let steps = vec![{
        let mut s = make_step("qa_testing", None, ExecutionMode::Agent);
        s.required_capability = Some("qa".to_string());
        s
    }];

    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.step_skipped.insert("qa_testing".to_string(), true);

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(steps, Some(1), 1);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    assert!(!fc.qa_ran);
    assert!(fc.qa_configured);
    assert!(fc.qa_observed);
    assert!(fc.qa_skipped);
}

#[test]
fn to_finalize_context_fix_ran_and_success() {
    let steps = vec![{
        let mut s = make_step("ticket_fix", None, ExecutionMode::Agent);
        s.required_capability = Some("fix".to_string());
        s
    }];

    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.step_ran.insert("ticket_fix".to_string(), true);
    acc.flags.insert("fix_success".to_string(), true);

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(steps, Some(1), 1);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    assert!(fc.fix_ran);
    assert!(fc.fix_success);
    assert!(fc.fix_configured);
    assert!(fc.fix_enabled);
}

#[test]
fn to_finalize_context_fix_skipped() {
    let steps = vec![{
        let mut s = make_step("ticket_fix", None, ExecutionMode::Agent);
        s.required_capability = Some("fix".to_string());
        s
    }];

    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.step_skipped.insert("ticket_fix".to_string(), true);

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(steps, Some(1), 1);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    assert!(!fc.fix_ran);
    assert!(fc.fix_skipped);
    assert!(fc.fix_enabled);
}

#[test]
fn to_finalize_context_retest() {
    let steps = vec![{
        let mut s = make_step("retest", None, ExecutionMode::Agent);
        s.required_capability = Some("retest".to_string());
        s
    }];

    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.step_ran.insert("retest".to_string(), true);
    acc.flags.insert("retest_success".to_string(), true);

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(steps, Some(1), 1);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    assert!(fc.retest_ran);
    assert!(fc.retest_success);
    assert!(fc.retest_enabled);
}

#[test]
fn to_finalize_context_not_repeatable_in_cycle_2() {
    let steps = vec![{
        let mut s = make_step("qa_testing", None, ExecutionMode::Agent);
        s.required_capability = Some("qa".to_string());
        s.repeatable = false;
        s
    }];

    let acc = StepExecutionAccumulator::new(empty_pipeline());
    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(steps, Some(2), 2);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    // Non-repeatable step in cycle 2 is not qa_configured
    assert!(!fc.qa_configured);
}

#[test]
fn to_finalize_context_disabled_step_not_configured() {
    let steps = vec![{
        let mut s = make_step("qa_testing", None, ExecutionMode::Agent);
        s.required_capability = Some("qa".to_string());
        s.enabled = false;
        s
    }];

    let acc = StepExecutionAccumulator::new(empty_pipeline());
    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(steps, Some(1), 1);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    assert!(!fc.qa_configured);
}

#[test]
fn to_finalize_context_tickets_set_fix_required() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.active_tickets = vec!["ticket1.md".to_string(), "ticket2.md".to_string()];
    acc.new_ticket_count = 2;

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], Some(1), 1);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    assert!(fc.fix_required);
    assert_eq!(fc.active_ticket_count, 2);
    assert_eq!(fc.new_ticket_count, 2);
}

#[test]
fn to_finalize_context_confidence_and_quality() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.qa_confidence = Some(0.85);
    acc.qa_quality_score = Some(0.9);
    acc.fix_confidence = Some(0.7);
    acc.fix_quality_score = Some(0.8);

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], Some(1), 1);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    assert_eq!(fc.qa_confidence, Some(0.85));
    assert_eq!(fc.qa_quality_score, Some(0.9));
    assert_eq!(fc.fix_confidence, Some(0.7));
    assert_eq!(fc.fix_quality_score, Some(0.8));
}

#[test]
fn to_finalize_context_artifacts() {
    let mut acc = StepExecutionAccumulator::new(empty_pipeline());
    acc.phase_artifacts
        .push(agent_orchestrator::collab::Artifact::new(
            agent_orchestrator::collab::ArtifactKind::Ticket {
                severity: agent_orchestrator::collab::artifact::Severity::Medium,
                category: "qa".to_string(),
            },
        ));
    acc.phase_artifacts
        .push(agent_orchestrator::collab::Artifact::new(
            agent_orchestrator::collab::ArtifactKind::CodeChange {
                files: vec!["f.rs".to_string()],
            },
        ));

    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], Some(1), 1);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);

    assert_eq!(fc.total_artifacts, 2);
    assert!(fc.has_ticket_artifacts);
    assert!(fc.has_code_change_artifacts);
}

#[test]
fn to_finalize_context_is_last_cycle() {
    let acc = StepExecutionAccumulator::new(empty_pipeline());
    let item = make_item("item-1", "test.md");
    let ctx = make_task_ctx(vec![], Some(2), 2);

    let fc = acc.to_finalize_context("task-1", &item, &ctx);
    assert!(fc.is_last_cycle);

    let ctx2 = make_task_ctx(vec![], Some(2), 1);
    let fc2 = acc.to_finalize_context("task-1", &item, &ctx2);
    assert!(!fc2.is_last_cycle);
}

// ── step_ids_for_capability() ────────────

#[test]
fn step_ids_for_capability_includes_canonical_and_custom() {
    let steps = vec![
        {
            let mut s = make_step("my_qa_step", None, ExecutionMode::Agent);
            s.required_capability = Some("qa".to_string());
            s
        },
        {
            let mut s = make_step("unrelated", None, ExecutionMode::Agent);
            s.required_capability = Some("fix".to_string());
            s
        },
    ];
    let ctx = make_task_ctx(steps, Some(1), 1);

    let ids = StepExecutionAccumulator::step_ids_for_capability(&ctx, "qa", &["qa", "qa_testing"]);

    assert_eq!(ids, vec!["qa", "qa_testing", "my_qa_step"]);
}

#[test]
fn step_ids_for_capability_no_duplicates_for_canonical_names() {
    let steps = vec![{
        let mut s = make_step("qa_testing", None, ExecutionMode::Agent);
        s.required_capability = Some("qa".to_string());
        s
    }];
    let ctx = make_task_ctx(steps, Some(1), 1);

    let ids = StepExecutionAccumulator::step_ids_for_capability(&ctx, "qa", &["qa", "qa_testing"]);

    // "qa_testing" is already in canonical list, should not be duplicated
    assert_eq!(ids, vec!["qa", "qa_testing"]);
}

// ── guard pure-function tests ────────────────────────────

#[test]
fn evaluate_builtin_loop_guard_stops_when_no_unresolved() {
    let result = super::guard::evaluate_builtin_loop_guard(true, 0);
    assert!(result.should_stop);
    assert_eq!(result.reason, "no_unresolved");
}

#[test]
fn evaluate_builtin_loop_guard_continues_when_has_unresolved() {
    let result = super::guard::evaluate_builtin_loop_guard(true, 5);
    assert!(!result.should_stop);
    assert_eq!(result.reason, "has_unresolved");
}

#[test]
fn evaluate_builtin_loop_guard_continues_when_flag_disabled() {
    let result = super::guard::evaluate_builtin_loop_guard(false, 0);
    assert!(!result.should_stop);
    assert_eq!(result.reason, "has_unresolved");
}

#[test]
fn evaluate_builtin_loop_guard_continues_when_flag_disabled_and_has_items() {
    let result = super::guard::evaluate_builtin_loop_guard(false, 3);
    assert!(!result.should_stop);
    assert_eq!(result.reason, "has_unresolved");
}

#[test]
fn parse_guard_output_should_stop_true() {
    let result = super::guard::parse_guard_output(r#"{"should_stop": true, "reason": "done"}"#);
    assert!(result.should_stop);
    assert_eq!(result.reason, "done");
}

#[test]
fn parse_guard_output_should_stop_false() {
    let result = super::guard::parse_guard_output(r#"{"should_stop": false, "reason": "ongoing"}"#);
    assert!(!result.should_stop);
    assert_eq!(result.reason, "ongoing");
}

#[test]
fn parse_guard_output_continue_true_means_dont_stop() {
    let result = super::guard::parse_guard_output(r#"{"continue": true}"#);
    assert!(!result.should_stop);
    assert_eq!(result.reason, "guard_json");
}

#[test]
fn parse_guard_output_continue_false_means_stop() {
    let result = super::guard::parse_guard_output(r#"{"continue": false}"#);
    assert!(result.should_stop);
    assert_eq!(result.reason, "guard_json");
}

#[test]
fn parse_guard_output_invalid_json_defaults_to_continue() {
    let result = super::guard::parse_guard_output("not valid json");
    assert!(!result.should_stop);
    assert_eq!(result.reason, "guard_json");
}

#[test]
fn parse_guard_output_empty_string_defaults_to_continue() {
    let result = super::guard::parse_guard_output("");
    assert!(!result.should_stop);
    assert_eq!(result.reason, "guard_json");
}

#[test]
fn parse_guard_output_missing_fields_defaults_to_continue() {
    let result = super::guard::parse_guard_output(r#"{"other": 42}"#);
    assert!(!result.should_stop);
    assert_eq!(result.reason, "guard_json");
}

#[test]
fn parse_guard_output_should_stop_takes_precedence_over_continue() {
    // When both fields are present, should_stop is checked first
    let result = super::guard::parse_guard_output(
        r#"{"should_stop": true, "continue": true, "reason": "both"}"#,
    );
    assert!(result.should_stop);
    assert_eq!(result.reason, "both");
}

// ── stream-json auto-capture extraction (FR-036) ──

#[test]
fn auto_capture_extracts_stream_json_result_for_spill() {
    // Simulates the auto-capture logic in dispatch.rs: extract_stream_json_result + spill_large_var
    let temp = tempfile::tempdir().expect("tempdir");
    let mut pipeline = agent_orchestrator::config::PipelineVariables::default();

    // Build a stream-json stdout with tool calls and a final result line
    let stream_json_stdout = concat!(
        "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"thinking...\"}]}}\n",
        "{\"type\":\"tool_use\",\"tool\":\"Read\",\"input\":{\"path\":\"foo.rs\"}}\n",
        "{\"type\":\"tool_result\",\"content\":\"file contents here\"}\n",
        "{\"type\":\"result\",\"result\":\"## Plan\\n\\n1. Change foo.rs\\n2. Add tests\\n\\n## Scope\\n- foo.rs only\"}\n",
    );

    // Apply the same extraction logic as dispatch.rs auto-capture
    let effective_output =
        agent_orchestrator::json_extract::extract_stream_json_result(stream_json_stdout)
            .unwrap_or_else(|| stream_json_stdout.to_string());

    super::spill::spill_large_var(
        temp.path(),
        "task-fr036",
        "plan_output",
        effective_output.clone(),
        &mut pipeline,
    );

    // Verify extracted content is the plan text, not the full transcript
    let spill_path = temp.path().join("task-fr036").join("plan_output.txt");
    let spill_content = std::fs::read_to_string(&spill_path).expect("read spill file");
    assert_eq!(
        spill_content,
        "## Plan\n\n1. Change foo.rs\n2. Add tests\n\n## Scope\n- foo.rs only"
    );
    assert!(
        !spill_content.contains("tool_use"),
        "spill file should not contain tool_use lines from the session transcript"
    );
    assert!(
        !spill_content.contains("thinking..."),
        "spill file should not contain thinking blocks from the session transcript"
    );
}

#[test]
fn auto_capture_falls_back_to_raw_stdout_for_non_stream_json() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut pipeline = agent_orchestrator::config::PipelineVariables::default();

    let plain_stdout = "This is plain text output from a non-stream-json agent";

    let effective_output =
        agent_orchestrator::json_extract::extract_stream_json_result(plain_stdout)
            .unwrap_or_else(|| plain_stdout.to_string());

    super::spill::spill_large_var(
        temp.path(),
        "task-plain",
        "plan_output",
        effective_output,
        &mut pipeline,
    );

    let spill_path = temp.path().join("task-plain").join("plan_output.txt");
    let spill_content = std::fs::read_to_string(&spill_path).expect("read spill file");
    assert_eq!(spill_content, plain_stdout);
}

#[test]
fn auto_capture_stream_json_large_result_spills_only_extracted_text() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut pipeline = agent_orchestrator::config::PipelineVariables::default();

    // Build a large stream-json output where the raw transcript is huge
    // but the extracted result is small
    let large_tool_output = "X".repeat(100_000);
    let small_plan = "## Small Plan\n\nJust one change.";
    let stream_json_stdout = format!(
        "{{\"type\":\"tool_result\",\"content\":\"{}\"}}\n{{\"type\":\"result\",\"result\":\"{}\"}}\n",
        large_tool_output,
        small_plan.replace('\n', "\\n"),
    );

    let effective_output =
        agent_orchestrator::json_extract::extract_stream_json_result(&stream_json_stdout)
            .unwrap_or_else(|| stream_json_stdout.clone());

    super::spill::spill_large_var(
        temp.path(),
        "task-large",
        "plan_output",
        effective_output.clone(),
        &mut pipeline,
    );

    // The inline variable should contain the small plan (no truncation needed)
    let inline = pipeline
        .vars
        .get("plan_output")
        .expect("plan_output should be set");
    assert!(
        !inline.contains("truncated"),
        "extracted plan is small enough to fit inline"
    );
    assert!(
        inline.contains("Small Plan"),
        "inline var should contain the plan text"
    );
}
