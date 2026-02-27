use crate::config::{LoopMode, WorkflowStepType};
use crate::events::insert_event;
use crate::state::InnerState;
use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::item_executor::{execute_guard_step, process_item};
use super::phase_runner::run_phase_with_rotation;
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
        if let Some(step) = task_ctx.execution_plan.step(WorkflowStepType::InitOnce) {
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
                    task_id,
                    &anchor_item_id,
                    &step.id,
                    "init_once",
                    step.tty,
                    step.required_capability.as_deref(),
                    ".",
                    &[],
                    &task_ctx.workspace_root,
                    &task_ctx.workspace_id,
                    task_ctx.current_cycle,
                    &runtime,
                    None,
                    task_ctx.safety.step_timeout_secs,
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
            record_task_execution_metric(&state, task_id, "paused", task_ctx.current_cycle, unresolved)?;
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
            record_task_execution_metric(&state, task_id, "paused", task_ctx.current_cycle, unresolved)?;
            return Ok(());
        }

        task_ctx.current_cycle += 1;
        update_task_cycle_state(&state, task_id, task_ctx.current_cycle, task_ctx.init_done)?;
        insert_event(&state, task_id, None, "cycle_started", json!({"cycle": task_ctx.current_cycle}))?;
        state.emit_event(task_id, None, "cycle_started", json!({"cycle": task_ctx.current_cycle}));

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
        let task_item_paths: Vec<String> = items.iter().map(|item| item.qa_file_path.clone()).collect();
        for item in items {
            process_item(&state, task_id, &item, &task_item_paths, &task_ctx, &runtime).await?;
            if runtime.stop_flag.load(Ordering::SeqCst) || is_task_paused_in_db(&state, task_id)? {
                continue 'cycle;
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
            let rollback_cycle = task_ctx.current_cycle.saturating_sub(task_ctx.consecutive_failures);
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
                    state.emit_event(task_id, None, "auto_rollback", json!({"rollback_to": rollback_tag}));

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

            let guard_result = execute_guard_step(&state, task_id, step, &task_ctx, &runtime).await?;
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
                state.emit_event(task_id, None, "workflow_terminated", json!({"guard_step": step.id}));
                set_task_status(&state, task_id, "completed", true)?;
                insert_event(&state, task_id, None, "task_completed", json!({}))?;
                state.emit_event(task_id, None, "task_completed", json!({}));
                let unresolved = count_unresolved_items(&state, task_id)?;
                record_task_execution_metric(&state, task_id, "completed", task_ctx.current_cycle, unresolved)?;
                return Ok(());
            }
        }

        let unresolved = count_unresolved_items(&state, task_id)?;
        let loop_mode_check =
            evaluate_loop_guard_rules(&task_ctx.execution_plan.loop_policy, task_ctx.current_cycle, unresolved);

        let should_continue = if let Some((continue_loop, _)) = loop_mode_check {
            continue_loop
        } else if task_ctx.execution_plan.loop_policy.guard.stop_when_no_unresolved {
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
        insert_event(&state, task_id, None, "task_failed", json!({"unresolved_items": unresolved}))?;
        state.emit_event(task_id, None, "task_failed", json!({"unresolved_items": unresolved}));
        record_task_execution_metric(&state, task_id, "failed", task_ctx.current_cycle, unresolved)?;
    } else {
        set_task_status(&state, task_id, "completed", true)?;
        insert_event(&state, task_id, None, "task_completed", json!({}))?;
        state.emit_event(task_id, None, "task_completed", json!({}));
        record_task_execution_metric(&state, task_id, "completed", task_ctx.current_cycle, unresolved)?;
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
