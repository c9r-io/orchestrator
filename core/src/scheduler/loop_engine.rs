use crate::config::{LoopMode, StepScope};
use crate::events::insert_event;
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::item_executor::{execute_guard_step, process_item, process_item_filtered};
use super::phase_runner::{run_phase_with_rotation, RotatingPhaseRunRequest};
use super::runtime::load_task_runtime_context;
use super::safety::{
    create_checkpoint, restore_binary_snapshot, rollback_to_checkpoint, snapshot_binary,
};
use super::task_state::{
    count_unresolved_items, first_task_item_id, is_task_paused_in_db, list_task_items_for_cycle,
    record_task_execution_metric, set_task_status, update_task_cycle_state,
};
use super::RunningTask;

pub async fn run_task_loop(
    state: Arc<InnerState>,
    task_id: &str,
    runtime: RunningTask,
) -> Result<()> {
    set_task_status(&state, task_id, "running", false)?;
    let result = run_task_loop_core(state.clone(), task_id, runtime).await;
    if let Err(ref e) = result {
        let _ = set_task_status(&state, task_id, "failed", false);
        let _ = insert_event(
            &state,
            task_id,
            None,
            "task_failed",
            json!({"error": e.to_string()}),
        );
        state.emit_event(
            task_id,
            None,
            "task_failed",
            json!({"error": e.to_string()}),
        );
        let unresolved = count_unresolved_items(&state, task_id).unwrap_or(0);
        let _ = record_task_execution_metric(&state, task_id, "failed", 0, unresolved);
    }
    result
}

async fn run_task_loop_core(
    state: Arc<InnerState>,
    task_id: &str,
    runtime: RunningTask,
) -> Result<()> {
    let mut task_ctx = load_task_runtime_context(&state, task_id)?;

    if !task_ctx.init_done {
        if let Some(step) = task_ctx.execution_plan.step_by_id("init_once") {
            if let Some(anchor_item_id) = first_task_item_id(&state, task_id)? {
                insert_event(
                    &state,
                    task_id,
                    Some(&anchor_item_id),
                    "step_started",
                    json!({"step":"init_once"}),
                )?;
                let init_result = run_phase_with_rotation(
                    &state,
                    RotatingPhaseRunRequest {
                        task_id,
                        item_id: &anchor_item_id,
                        step_id: &step.id,
                        phase: "init_once",
                        tty: step.tty,
                        capability: step.required_capability.as_deref(),
                        rel_path: ".",
                        ticket_paths: &[],
                        workspace_root: &task_ctx.workspace_root,
                        workspace_id: &task_ctx.workspace_id,
                        cycle: task_ctx.current_cycle,
                        runtime: &runtime,
                        pipeline_vars: None,
                        step_timeout_secs: task_ctx.safety.step_timeout_secs,
                    },
                )
                .await?;
                if !init_result.is_success() {
                    anyhow::bail!("init_once failed: exit={}", init_result.exit_code);
                }
                insert_event(
                    &state,
                    task_id,
                    Some(&anchor_item_id),
                    "step_finished",
                    json!({"step":"init_once","exit_code":init_result.exit_code}),
                )?;
            }
        }
        task_ctx.init_done = true;
        update_task_cycle_state(&state, task_id, task_ctx.current_cycle, true)?;
    }

    'cycle: loop {
        if is_task_paused_in_db(&state, task_id)? {
            let unresolved = count_unresolved_items(&state, task_id)?;
            record_task_execution_metric(
                &state,
                task_id,
                "paused",
                task_ctx.current_cycle,
                unresolved,
            )?;
            return Ok(());
        }

        if runtime.stop_flag.load(Ordering::SeqCst) {
            set_task_status(&state, task_id, "paused", false)?;
            insert_event(
                &state,
                task_id,
                None,
                "task_paused",
                json!({"reason":"stop_flag"}),
            )?;
            state.emit_event(task_id, None, "task_paused", json!({}));
            let unresolved = count_unresolved_items(&state, task_id)?;
            record_task_execution_metric(
                &state,
                task_id,
                "paused",
                task_ctx.current_cycle,
                unresolved,
            )?;
            return Ok(());
        }

        task_ctx.current_cycle += 1;
        update_task_cycle_state(&state, task_id, task_ctx.current_cycle, task_ctx.init_done)?;
        let max_cycles = task_ctx.execution_plan.loop_policy.guard.max_cycles;
        insert_event(
            &state,
            task_id,
            None,
            "cycle_started",
            json!({"cycle": task_ctx.current_cycle, "max_cycles": max_cycles}),
        )?;
        state.emit_event(
            task_id,
            None,
            "cycle_started",
            json!({"cycle": task_ctx.current_cycle, "max_cycles": max_cycles}),
        );

        if matches!(
            task_ctx.safety.checkpoint_strategy,
            crate::config::CheckpointStrategy::GitTag
        ) {
            let ws_path = Path::new(&task_ctx.workspace_root);
            match create_checkpoint(ws_path, task_id, task_ctx.current_cycle).await {
                Ok(tag) => {
                    insert_event(
                        &state,
                        task_id,
                        None,
                        "checkpoint_created",
                        json!({"cycle": task_ctx.current_cycle, "tag": tag}),
                    )?;

                    if task_ctx.safety.binary_snapshot && task_ctx.self_referential {
                        match snapshot_binary(&task_ctx.workspace_root).await {
                            Ok(path) => {
                                insert_event(
                                    &state,
                                    task_id,
                                    None,
                                    "binary_snapshot_created",
                                    json!({"cycle": task_ctx.current_cycle, "path": path.display().to_string()}),
                                )?;
                            }
                            Err(e) => {
                                eprintln!(
                                    "[warn] failed to create binary snapshot for cycle {}: {}",
                                    task_ctx.current_cycle, e
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[warn] failed to create checkpoint for cycle {}: {}",
                        task_ctx.current_cycle, e
                    );
                }
            }
        }

        let items = list_task_items_for_cycle(&state, task_id)?;
        let task_item_paths: Vec<String> =
            items.iter().map(|item| item.qa_file_path.clone()).collect();

        // Segment-based execution: group steps by scope and dispatch accordingly.
        // Task-scoped steps run once (using first item as context anchor).
        // Item-scoped steps fan out across all items.
        let segments = build_scope_segments(&task_ctx);
        if segments.is_empty() {
            // Fallback: no steps in execution plan, run legacy path
            for item in &items {
                process_item(&state, task_id, item, &task_item_paths, &task_ctx, &runtime).await?;
                if runtime.stop_flag.load(Ordering::SeqCst)
                    || is_task_paused_in_db(&state, task_id)?
                {
                    continue 'cycle;
                }
            }
        } else {
            for segment in &segments {
                match segment.scope {
                    StepScope::Task => {
                        // Run task-scoped steps once using first item as anchor
                        if let Some(anchor_item) = items.first() {
                            let updated_vars = process_item_filtered(
                                &state,
                                task_id,
                                anchor_item,
                                &task_item_paths,
                                &task_ctx,
                                &runtime,
                                Some(&segment.step_ids),
                            )
                            .await?;
                            // Propagate task-scoped pipeline vars to subsequent segments
                            task_ctx.pipeline_vars = updated_vars;
                        }
                    }
                    StepScope::Item => {
                        // Fan out item-scoped steps across all items
                        for item in &items {
                            let _item_vars = process_item_filtered(
                                &state,
                                task_id,
                                item,
                                &task_item_paths,
                                &task_ctx,
                                &runtime,
                                Some(&segment.step_ids),
                            )
                            .await?;
                            // Item-scoped vars do NOT propagate back to task scope
                        }
                    }
                }
                if runtime.stop_flag.load(Ordering::SeqCst)
                    || is_task_paused_in_db(&state, task_id)?
                {
                    continue 'cycle;
                }
            }
        }

        let cycle_unresolved = count_unresolved_items(&state, task_id)?;
        if cycle_unresolved > 0 {
            task_ctx.consecutive_failures += 1;
        } else {
            task_ctx.consecutive_failures = 0;
        }

        if task_ctx.safety.auto_rollback
            && task_ctx.consecutive_failures >= task_ctx.safety.max_consecutive_failures
            && matches!(
                task_ctx.safety.checkpoint_strategy,
                crate::config::CheckpointStrategy::GitTag
            )
        {
            let rollback_cycle = task_ctx
                .current_cycle
                .saturating_sub(task_ctx.consecutive_failures);
            let rollback_tag = format!("checkpoint/{}/{}", task_id, rollback_cycle.max(1));
            let ws_path = Path::new(&task_ctx.workspace_root);
            match rollback_to_checkpoint(ws_path, &rollback_tag).await {
                Ok(()) => {
                    insert_event(
                        &state,
                        task_id,
                        None,
                        "auto_rollback",
                        json!({
                            "cycle": task_ctx.current_cycle,
                            "rollback_to": rollback_tag,
                            "consecutive_failures": task_ctx.consecutive_failures,
                        }),
                    )?;
                    state.emit_event(
                        task_id,
                        None,
                        "auto_rollback",
                        json!({"rollback_to": rollback_tag}),
                    );

                    if task_ctx.safety.binary_snapshot && task_ctx.self_referential {
                        match restore_binary_snapshot(&task_ctx.workspace_root).await {
                            Ok(()) => {
                                insert_event(
                                    &state,
                                    task_id,
                                    None,
                                    "binary_snapshot_restored",
                                    json!({"cycle": task_ctx.current_cycle}),
                                )?;
                            }
                            Err(e) => eprintln!("[warn] failed to restore binary snapshot: {}", e),
                        }
                    }

                    task_ctx.consecutive_failures = 0;
                }
                Err(e) => {
                    eprintln!("[warn] auto-rollback failed: {}", e);
                    insert_event(
                        &state,
                        task_id,
                        None,
                        "auto_rollback_failed",
                        json!({"error": e.to_string()}),
                    )?;
                }
            }
        }

        for step in &task_ctx.execution_plan.steps {
            if !step.is_guard {
                continue;
            }
            if !step.repeatable && task_ctx.current_cycle > 1 {
                continue;
            }

            let guard_result =
                execute_guard_step(&state, task_id, step, &task_ctx, &runtime).await?;
            if guard_result.should_stop {
                insert_event(
                    &state,
                    task_id,
                    None,
                    "workflow_terminated",
                    json!({
                        "cycle": task_ctx.current_cycle,
                        "guard_step": step.id,
                        "reason": guard_result.reason
                    }),
                )?;
                state.emit_event(
                    task_id,
                    None,
                    "workflow_terminated",
                    json!({"guard_step": step.id}),
                );
                set_task_status(&state, task_id, "completed", true)?;
                insert_event(&state, task_id, None, "task_completed", json!({}))?;
                state.emit_event(task_id, None, "task_completed", json!({}));
                let unresolved = count_unresolved_items(&state, task_id)?;
                record_task_execution_metric(
                    &state,
                    task_id,
                    "completed",
                    task_ctx.current_cycle,
                    unresolved,
                )?;
                return Ok(());
            }
        }

        let unresolved = count_unresolved_items(&state, task_id)?;
        let loop_mode_check = evaluate_loop_guard_rules(
            &task_ctx.execution_plan.loop_policy,
            task_ctx.current_cycle,
            unresolved,
        );

        let should_continue = if let Some((continue_loop, _)) = loop_mode_check {
            continue_loop
        } else if task_ctx
            .execution_plan
            .loop_policy
            .guard
            .stop_when_no_unresolved
        {
            unresolved > 0
        } else {
            true
        };

        let reason = if let Some((_, reason)) = loop_mode_check {
            reason
        } else if !should_continue {
            "no_unresolved_items".to_string()
        } else {
            "continue".to_string()
        };
        insert_event(
            &state,
            task_id,
            None,
            "loop_guard_decision",
            json!({
                "cycle": task_ctx.current_cycle,
                "continue": should_continue,
                "reason": reason,
                "unresolved_items": unresolved
            }),
        )?;
        state.emit_event(
            task_id,
            None,
            "loop_guard_decision",
            json!({
                "cycle": task_ctx.current_cycle,
                "continue": should_continue,
                "reason": reason,
                "unresolved_items": unresolved
            }),
        );
        if !should_continue {
            break;
        }
    }

    let unresolved = count_unresolved_items(&state, task_id)?;
    if is_task_paused_in_db(&state, task_id)? {
        return Ok(());
    }

    if unresolved > 0 {
        set_task_status(&state, task_id, "failed", true)?;
        insert_event(
            &state,
            task_id,
            None,
            "task_failed",
            json!({"unresolved_items": unresolved}),
        )?;
        state.emit_event(
            task_id,
            None,
            "task_failed",
            json!({"unresolved_items": unresolved}),
        );
        record_task_execution_metric(
            &state,
            task_id,
            "failed",
            task_ctx.current_cycle,
            unresolved,
        )?;
    } else {
        set_task_status(&state, task_id, "completed", true)?;
        insert_event(&state, task_id, None, "task_completed", json!({}))?;
        state.emit_event(task_id, None, "task_completed", json!({}));
        record_task_execution_metric(
            &state,
            task_id,
            "completed",
            task_ctx.current_cycle,
            unresolved,
        )?;
    }

    Ok(())
}

pub fn evaluate_loop_guard_rules(
    loop_policy: &crate::config::WorkflowLoopConfig,
    current_cycle: u32,
    _unresolved: i64,
) -> Option<(bool, String)> {
    match loop_policy.mode {
        LoopMode::Once => Some((false, "once_mode".to_string())),
        LoopMode::Fixed => {
            let max = loop_policy.guard.max_cycles.unwrap_or(1);
            if current_cycle >= max {
                Some((false, "fixed_cycles_complete".to_string()))
            } else {
                Some((true, "fixed_cycle_continue".to_string()))
            }
        }
        LoopMode::Infinite => {
            if let Some(max_cycles) = loop_policy.guard.max_cycles {
                if current_cycle >= max_cycles {
                    return Some((false, "max_cycles_reached".to_string()));
                }
            }
            if !loop_policy.guard.enabled {
                return Some((true, "guard_disabled".to_string()));
            }
            None
        }
    }
}

/// A contiguous group of steps with the same execution scope.
struct ScopeSegment {
    scope: StepScope,
    step_ids: HashSet<String>,
}

/// Group execution plan steps into contiguous segments of the same scope.
/// Guard steps are excluded; they run separately after items.
fn build_scope_segments(task_ctx: &crate::config::TaskRuntimeContext) -> Vec<ScopeSegment> {
    let mut segments: Vec<ScopeSegment> = Vec::new();
    for step in &task_ctx.execution_plan.steps {
        if step.is_guard || !step.enabled {
            continue;
        }
        let scope = step.resolved_scope();
        if let Some(last) = segments.last_mut() {
            if last.scope == scope {
                last.step_ids.insert(step.id.clone());
                continue;
            }
        }
        let mut ids = HashSet::new();
        ids.insert(step.id.clone());
        segments.push(ScopeSegment {
            scope,
            step_ids: ids,
        });
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{WorkflowLoopConfig, WorkflowLoopGuardConfig};

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
                    },
                    TaskExecutionStep {
                        id: "implement".into(),

                        required_capability: None,
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
                    },
                    TaskExecutionStep {
                        id: "qa_testing".into(),

                        required_capability: None,
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
                    },
                    TaskExecutionStep {
                        id: "ticket_fix".into(),

                        required_capability: None,
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
                    },
                    TaskExecutionStep {
                        id: "doc_governance".into(),

                        required_capability: None,
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
                    },
                ],
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig::default(),
            },
            current_cycle: 1,
            init_done: true,
            dynamic_steps: vec![],
            pipeline_vars: PipelineVariables::default(),
            safety: SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
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
                    },
                    TaskExecutionStep {
                        id: "loop_guard".into(),

                        required_capability: None,
                        builtin: Some("loop_guard".into()),
                        enabled: true,
                        repeatable: true,
                        is_guard: true,
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
                loop_policy: WorkflowLoopConfig::default(),
                finalize: WorkflowFinalizeConfig::default(),
            },
            current_cycle: 1,
            init_done: true,
            dynamic_steps: vec![],
            pipeline_vars: PipelineVariables::default(),
            safety: SafetyConfig::default(),
            self_referential: false,
            consecutive_failures: 0,
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
            scope: Some(StepScope::Task), // Override default Item scope
            behavior: StepBehavior::default(),
        };
        assert_eq!(step.resolved_scope(), StepScope::Task);
    }
}
