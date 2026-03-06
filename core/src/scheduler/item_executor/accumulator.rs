use crate::config::{
    CaptureSource, ItemFinalizeContext, PipelineVariables, StepPrehookContext, TaskRuntimeContext,
};
use std::collections::HashMap;
use std::path::Path;

use super::spill::spill_large_var;

/// Accumulator that tracks state across steps in the unified execution loop.
pub struct StepExecutionAccumulator {
    pub item_status: String,
    pub pipeline_vars: PipelineVariables,
    pub active_tickets: Vec<String>,
    pub created_ticket_files: Vec<String>,
    pub phase_artifacts: Vec<crate::collab::Artifact>,
    pub flags: HashMap<String, bool>,
    pub exit_codes: HashMap<String, i64>,
    pub step_ran: HashMap<String, bool>,
    pub step_skipped: HashMap<String, bool>,
    pub new_ticket_count: i64,
    pub qa_confidence: Option<f32>,
    pub qa_quality_score: Option<f32>,
    pub fix_confidence: Option<f32>,
    pub fix_quality_score: Option<f32>,
    pub terminal: bool,
    /// Buffered GenerateItems action from a post-action, to be applied after segment completes.
    pub pending_generate_items: Option<crate::config::GenerateItemsAction>,
}

impl StepExecutionAccumulator {
    pub fn new(pipeline_vars: PipelineVariables) -> Self {
        Self {
            item_status: "pending".to_string(),
            pipeline_vars,
            active_tickets: Vec::new(),
            created_ticket_files: Vec::new(),
            phase_artifacts: Vec::new(),
            flags: HashMap::new(),
            exit_codes: HashMap::new(),
            step_ran: HashMap::new(),
            step_skipped: HashMap::new(),
            new_ticket_count: 0,
            qa_confidence: None,
            qa_quality_score: None,
            fix_confidence: None,
            fix_quality_score: None,
            terminal: false,
            pending_generate_items: None,
        }
    }

    pub fn merge_task_pipeline_vars(&mut self, task_pipeline_vars: &PipelineVariables) {
        for (key, value) in &task_pipeline_vars.vars {
            self.pipeline_vars
                .vars
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        if self.pipeline_vars.build_errors.is_empty() {
            self.pipeline_vars.build_errors = task_pipeline_vars.build_errors.clone();
        }
        if self.pipeline_vars.test_failures.is_empty() {
            self.pipeline_vars.test_failures = task_pipeline_vars.test_failures.clone();
        }
    }

    /// Collect all step IDs that match a capability, including canonical aliases.
    pub(super) fn step_ids_for_capability(
        task_ctx: &TaskRuntimeContext,
        capability: &str,
        canonical_ids: &[&str],
    ) -> Vec<String> {
        let mut ids: Vec<String> = canonical_ids.iter().map(|s| s.to_string()).collect();
        for step in &task_ctx.execution_plan.steps {
            if step.required_capability.as_deref() == Some(capability) && !ids.contains(&step.id) {
                ids.push(step.id.clone());
            }
        }
        ids
    }

    /// Build a StepPrehookContext from accumulated state.
    pub fn to_prehook_context(
        &self,
        task_id: &str,
        item: &crate::dto::TaskItemRow,
        task_ctx: &TaskRuntimeContext,
        step_id: &str,
    ) -> StepPrehookContext {
        let qa_step_ids = Self::step_ids_for_capability(task_ctx, "qa", &["qa", "qa_testing"]);
        let fix_step_ids = Self::step_ids_for_capability(task_ctx, "fix", &["fix", "ticket_fix"]);
        let retest_step_ids = Self::step_ids_for_capability(task_ctx, "retest", &["retest"]);
        let max_cycles = task_ctx
            .execution_plan
            .loop_policy
            .guard
            .max_cycles
            .unwrap_or(1);
        StepPrehookContext {
            task_id: task_id.to_string(),
            task_item_id: item.id.clone(),
            cycle: task_ctx.current_cycle,
            step: step_id.to_string(),
            qa_file_path: item.qa_file_path.clone(),
            item_status: self.item_status.clone(),
            task_status: "running".to_string(),
            qa_exit_code: qa_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            fix_exit_code: fix_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            retest_exit_code: retest_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            active_ticket_count: self.active_tickets.len() as i64,
            new_ticket_count: self.new_ticket_count,
            qa_failed: self.flags.get("qa_failed").copied().unwrap_or(false),
            fix_required: self.flags.get("qa_failed").copied().unwrap_or(false)
                || !self.active_tickets.is_empty(),
            qa_confidence: self.qa_confidence,
            qa_quality_score: self.qa_quality_score,
            fix_has_changes: None,
            upstream_artifacts: vec![],
            build_error_count: self.pipeline_vars.build_errors.len() as i64,
            test_failure_count: self.pipeline_vars.test_failures.len() as i64,
            build_exit_code: self.exit_codes.get("build").copied(),
            test_exit_code: self.exit_codes.get("test").copied(),
            self_test_exit_code: self
                .pipeline_vars
                .vars
                .get("self_test_exit_code")
                .and_then(|v| v.parse::<i64>().ok()),
            self_test_passed: self
                .pipeline_vars
                .vars
                .get("self_test_passed")
                .map(|v| v == "true")
                .unwrap_or(false),
            max_cycles,
            is_last_cycle: task_ctx.current_cycle >= max_cycles,
        }
    }

    /// Build an ItemFinalizeContext from accumulated state.
    pub fn to_finalize_context(
        &self,
        task_id: &str,
        item: &crate::dto::TaskItemRow,
        task_ctx: &TaskRuntimeContext,
    ) -> ItemFinalizeContext {
        let qa_step_ids = Self::step_ids_for_capability(task_ctx, "qa", &["qa", "qa_testing"]);
        let fix_step_ids = Self::step_ids_for_capability(task_ctx, "fix", &["fix", "ticket_fix"]);
        let retest_step_ids = Self::step_ids_for_capability(task_ctx, "retest", &["retest"]);

        let qa_ran = qa_step_ids
            .iter()
            .any(|id| self.step_ran.get(id.as_str()).copied().unwrap_or(false));
        let qa_skipped = qa_step_ids
            .iter()
            .any(|id| self.step_skipped.get(id.as_str()).copied().unwrap_or(false));
        let qa_configured = task_ctx.execution_plan.steps.iter().any(|s| {
            qa_step_ids.contains(&s.id)
                && s.enabled
                && (s.repeatable || task_ctx.current_cycle <= 1)
        });
        let qa_observed = qa_ran || qa_skipped;
        let qa_enabled = qa_configured;
        let fix_ran = fix_step_ids
            .iter()
            .any(|id| self.step_ran.get(id.as_str()).copied().unwrap_or(false));
        let fix_success = self.flags.get("fix_success").copied().unwrap_or(false);
        let fix_configured = task_ctx.execution_plan.steps.iter().any(|s| {
            fix_step_ids.contains(&s.id)
                && s.enabled
                && (s.repeatable || task_ctx.current_cycle <= 1)
        });
        let fix_enabled = fix_ran
            || fix_step_ids
                .iter()
                .any(|id| self.step_skipped.get(id.as_str()).copied().unwrap_or(false))
            || fix_configured;
        let retest_ran = retest_step_ids
            .iter()
            .any(|id| self.step_ran.get(id.as_str()).copied().unwrap_or(false));
        let retest_success = self.flags.get("retest_success").copied().unwrap_or(false);
        let retest_enabled = retest_ran
            || retest_step_ids
                .iter()
                .any(|id| self.step_skipped.get(id.as_str()).copied().unwrap_or(false))
            || task_ctx
                .execution_plan
                .steps
                .iter()
                .any(|s| retest_step_ids.contains(&s.id) && s.enabled);

        ItemFinalizeContext {
            task_id: task_id.to_string(),
            task_item_id: item.id.clone(),
            cycle: task_ctx.current_cycle,
            qa_file_path: item.qa_file_path.clone(),
            item_status: self.item_status.clone(),
            task_status: "running".to_string(),
            qa_exit_code: qa_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            fix_exit_code: fix_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            retest_exit_code: retest_step_ids
                .iter()
                .find_map(|id| self.exit_codes.get(id.as_str()))
                .copied(),
            active_ticket_count: self.active_tickets.len() as i64,
            new_ticket_count: self.new_ticket_count,
            retest_new_ticket_count: 0,
            qa_failed: self.flags.get("qa_failed").copied().unwrap_or(false),
            fix_required: !self.active_tickets.is_empty(),
            qa_configured,
            qa_observed,
            qa_enabled,
            qa_ran,
            qa_skipped,
            fix_configured,
            fix_enabled,
            fix_ran,
            fix_skipped: fix_step_ids
                .iter()
                .any(|id| self.step_skipped.get(id.as_str()).copied().unwrap_or(false)),
            fix_success,
            retest_enabled,
            retest_ran,
            retest_success,
            qa_confidence: self.qa_confidence,
            qa_quality_score: self.qa_quality_score,
            fix_confidence: self.fix_confidence,
            fix_quality_score: self.fix_quality_score,
            total_artifacts: self.phase_artifacts.len() as i64,
            has_ticket_artifacts: self
                .phase_artifacts
                .iter()
                .any(|a| matches!(a.kind, crate::collab::ArtifactKind::Ticket { .. })),
            has_code_change_artifacts: self
                .phase_artifacts
                .iter()
                .any(|a| matches!(a.kind, crate::collab::ArtifactKind::CodeChange { .. })),
            is_last_cycle: task_ctx.current_cycle
                >= task_ctx
                    .execution_plan
                    .loop_policy
                    .guard
                    .max_cycles
                    .unwrap_or(1),
        }
    }

    /// Apply capture declarations from a step result into the accumulator.
    pub fn apply_captures(
        &mut self,
        captures: &[crate::config::CaptureDecl],
        step_id: &str,
        result: &crate::dto::RunResult,
    ) {
        for cap in captures {
            match cap.source {
                CaptureSource::ExitCode => {
                    self.exit_codes
                        .insert(step_id.to_string(), result.exit_code);
                    self.pipeline_vars
                        .vars
                        .insert(cap.var.clone(), result.exit_code.to_string());
                }
                CaptureSource::FailedFlag => {
                    let failed = !result.is_success();
                    self.flags.insert(cap.var.clone(), failed);
                    self.pipeline_vars
                        .vars
                        .insert(cap.var.clone(), failed.to_string());
                }
                CaptureSource::SuccessFlag => {
                    let success = result.is_success();
                    self.flags.insert(cap.var.clone(), success);
                    self.pipeline_vars
                        .vars
                        .insert(cap.var.clone(), success.to_string());
                }
                CaptureSource::Stdout => {
                    if let Some(ref output) = result.output {
                        spill_large_var(
                            Path::new(""),
                            "",
                            &cap.var,
                            output.stdout.clone(),
                            &mut self.pipeline_vars,
                        );
                    }
                }
                CaptureSource::Stderr => {
                    if let Some(ref output) = result.output {
                        self.pipeline_vars
                            .vars
                            .insert(cap.var.clone(), output.stderr.clone());
                    }
                }
            }
        }
    }
}
