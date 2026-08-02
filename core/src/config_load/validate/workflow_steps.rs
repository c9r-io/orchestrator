use super::common::AgentLookup;
use crate::cli_types::WorkflowStepSpec;
use crate::config::{
    CancelSemantics, DriverTransport, PostAction, SideEffectClass, StepSemanticKind, ToolHosting,
    WorkflowStepConfig, WorkspaceAccess, resolve_step_semantic_kind,
};
use anyhow::Result;
use std::collections::HashSet;

/// Validate the step loop: duplicate IDs, semantic kind, agent capability, prehook.
pub(super) fn validate_workflow_steps<A: AgentLookup>(
    steps: &[WorkflowStepConfig],
    workflow_id: &str,
    agents: &A,
) -> Result<usize> {
    let mut enabled_count = 0usize;
    let mut seen_ids: HashSet<String> = HashSet::new();
    for step in steps {
        if !seen_ids.insert(step.id.clone()) {
            anyhow::bail!(
                "workflow '{}' has duplicate step id '{}'",
                workflow_id,
                step.id
            );
        }
        let key = step
            .builtin
            .as_deref()
            .or(step.required_capability.as_deref())
            .unwrap_or(&step.id);
        if !step.enabled {
            continue;
        }
        enabled_count += 1;
        let semantic = resolve_step_semantic_kind(step).map_err(anyhow::Error::msg)?;
        if matches!(
            semantic,
            StepSemanticKind::Builtin { ref name } if name == "ticket_scan"
        ) {
            if let Some(prehook) = step.prehook.as_ref() {
                crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
            }
            continue;
        }
        let is_self_contained = matches!(
            semantic,
            StepSemanticKind::Builtin { .. } | StepSemanticKind::Command | StepSemanticKind::Chain
        );
        if !is_self_contained && !agents.has_capability(key) {
            anyhow::bail!(
                "no agent supports capability for step '{key}' used by workflow '{workflow_id}'"
            );
        }
        if !is_self_contained {
            validate_driver_candidates(step, workflow_id, key, agents)?;
        }
        reject_retired_authoring(step, workflow_id)?;
        if let Some(prehook) = step.prehook.as_ref() {
            crate::prehook::validate_step_prehook(prehook, workflow_id, key)?;
        }
    }
    if enabled_count == 0 {
        anyhow::bail!("workflow '{workflow_id}' has no enabled steps");
    }
    Ok(enabled_count)
}

/// Reject every retired step-level authoring construct, at any nesting depth.
///
/// The recursion is the point. `chain_steps` children are dispatched through
/// the same `execute_step` path as top-level steps, so a retired field one
/// level down runs exactly as it always did — while a validator that walks only
/// `spec.steps` reports the workflow clean. That is a guard covering the shapes
/// its author had in mind and silently missing the next one, and it applied to
/// the two pre-existing checks here as much as to the FR-156 one they now sit
/// beside.
fn reject_retired_authoring(step: &WorkflowStepConfig, workflow_id: &str) -> Result<()> {
    if !step.behavior.captures.is_empty() {
        anyhow::bail!(
            "[legacy_coordination_removed] workflow '{}' step '{}' uses behavior.captures; use typed driver/tool results",
            workflow_id,
            step.id
        );
    }
    if step.behavior.post_actions.iter().any(|action| {
        matches!(
            action,
            PostAction::SpawnTasks(_) | PostAction::GenerateItems(_)
        )
    }) {
        anyhow::bail!(
            "[legacy_json_path_removed] workflow '{}' step '{}' uses a JSONPath-backed post-action; use typed daemon tools",
            workflow_id,
            step.id
        );
    }
    reject_pipeline_variable_authoring(step, workflow_id)?;
    for child in &step.chain_steps {
        reject_retired_authoring(child, workflow_id)?;
    }
    Ok(())
}

/// Reject the retired step-level pipeline-variable authoring surface (FR-156).
///
/// All four of these route an author-chosen value through
/// `PipelineVariables.vars`, the generic map the coordination collapse retired:
/// `store_inputs` reads a store into it, `store_outputs` and the `store_put`
/// post-action write a variable out of it, and `step_vars` overlays it for one
/// step. Steps address project-scoped state directly now — the CLI, or a typed
/// daemon tool — so none of them has a live consumer.
///
/// The spec types stay deserializable on purpose (DD-137): a removed field that
/// still parses can be answered with a stable retirement diagnostic naming it,
/// where a deleted field would surface as an opaque unknown-key error.
///
/// One arm per field rather than one combined predicate, so the diagnostic
/// always names the field the author actually wrote. A single message covering
/// all four would be satisfied by a validator that detected the wrong one.
fn reject_pipeline_variable_authoring(step: &WorkflowStepConfig, workflow_id: &str) -> Result<()> {
    let retired = if !step.store_inputs.is_empty() {
        Some(
            "store_inputs; read the store from the step instead, e.g. `orchestrator store get <store> <key> --project {project_id}`",
        )
    } else if !step.store_outputs.is_empty() {
        Some(
            "store_outputs; write from the step instead, e.g. `orchestrator store put <store> <key> <value> --project {project_id}`",
        )
    } else if step.step_vars.as_ref().is_some_and(|vars| !vars.is_empty()) {
        Some("step_vars; put the value in the step's own command or prompt")
    } else if step
        .behavior
        .post_actions
        .iter()
        .any(|action| matches!(action, PostAction::StorePut { .. }))
    {
        Some(
            "a store_put post-action; write from the step instead, e.g. `orchestrator store put <store> <key> <value> --project {project_id}`",
        )
    } else {
        None
    };

    if let Some(retired) = retired {
        anyhow::bail!(
            "[legacy_pipeline_variables_removed] workflow '{}' step '{}' uses {}",
            workflow_id,
            step.id,
            retired
        );
    }
    Ok(())
}

fn validate_driver_candidates<A: AgentLookup>(
    step: &WorkflowStepConfig,
    workflow_id: &str,
    capability: &str,
    agents: &A,
) -> Result<()> {
    let requirements = &step.behavior.driver_requirements;
    for (agent_id, agent) in agents.agents_with_capability(capability) {
        let Some(driver) = agent.driver.as_ref() else {
            // Legacy command Agents retain pre-FR-116 behavior.
            continue;
        };
        crate::driver::validate_driver_config(driver, &agent.command).map_err(|error| {
            anyhow::anyhow!(
                "[driver_config_invalid] workflow '{}' step '{}' agent '{}': {}",
                workflow_id,
                step.id,
                agent_id,
                error
            )
        })?;
        let capabilities = crate::driver::driver_capabilities(driver);
        let driver_id = crate::driver::driver_id(driver);
        if requirements.multi_turn && !capabilities.multi_turn {
            driver_error(
                "driver_multi_turn_required",
                workflow_id,
                step,
                agent_id,
                driver_id,
            )?;
        }
        if requirements.tool_hosting != ToolHosting::None
            && requirements.tool_hosting != capabilities.tool_hosting
        {
            driver_error(
                "driver_tool_hosting_required",
                workflow_id,
                step,
                agent_id,
                driver_id,
            )?;
        }
        if requirements.session_resume && !capabilities.session_resume {
            driver_error(
                "driver_session_resume_required",
                workflow_id,
                step,
                agent_id,
                driver_id,
            )?;
        }
        if requirements.permission_events && !capabilities.permission_events {
            driver_error(
                "driver_permission_events_required",
                workflow_id,
                step,
                agent_id,
                driver_id,
            )?;
        }
        if requirements.workspace_access != WorkspaceAccess::None && !capabilities.sandboxable {
            driver_error(
                "driver_workspace_sandbox_required",
                workflow_id,
                step,
                agent_id,
                driver_id,
            )?;
        }
        if step.behavior.side_effect_class == SideEffectClass::NonIdempotentExternal
            && capabilities.cancel != CancelSemantics::Guaranteed
        {
            driver_error(
                "driver_guaranteed_cancel_required",
                workflow_id,
                step,
                agent_id,
                driver_id,
            )?;
        }
        if driver.transport == DriverTransport::Sdk {
            driver_error(
                "driver_transport_unavailable",
                workflow_id,
                step,
                agent_id,
                driver_id,
            )?;
        }
    }
    Ok(())
}

fn driver_error(
    code: &str,
    workflow_id: &str,
    step: &WorkflowStepConfig,
    agent_id: &str,
    driver_id: &str,
) -> Result<()> {
    anyhow::bail!(
        "[{code}] workflow '{}' step '{}' is incompatible with agent '{}' driver '{}'",
        workflow_id,
        step.id,
        agent_id,
        driver_id
    )
}

/// "Did you mean" suggestions for commonly misplaced step-level fields.
fn did_you_mean(key: &str) -> Option<&'static str> {
    match key {
        "capture" | "captures" => Some("behavior.captures"),
        "on_failure" => Some("behavior.on_failure"),
        "on_success" => Some("behavior.on_success"),
        "post_actions" => Some("behavior.post_actions"),
        "execution" => Some("behavior.execution"),
        "collect_artifacts" => Some("behavior.collect_artifacts"),
        _ => None,
    }
}

/// Built-in CEL prehook variable names injected by the runtime.
const BUILTIN_CEL_VARS: &[&str] = &[
    "context",
    "task_id",
    "task_item_id",
    "cycle",
    "max_cycles",
    "is_last_cycle",
    "api_publishable",
    "last_sandbox_denied",
    "sandbox_denied_count",
    "last_sandbox_denial_reason",
    "step",
    "qa_file_path",
    "item_status",
    "task_status",
    "qa_exit_code",
    "fix_exit_code",
    "retest_exit_code",
    "active_ticket_count",
    "new_ticket_count",
    "qa_failed",
    "fix_required",
    "qa_confidence",
    "qa_quality_score",
    "fix_has_changes",
    "build_errors",
    "test_failures",
    "build_exit_code",
    "test_exit_code",
    "self_test_exit_code",
    "self_referential_safe",
    "self_referential_safe_scenarios",
    "steps",
];

/// CEL keywords, literals, and built-in functions that are not variable references.
const CEL_KEYWORDS: &[&str] = &[
    "true",
    "false",
    "null",
    "in",
    "has",
    "size",
    "len",
    "type",
    "int",
    "uint",
    "double",
    "bool",
    "string",
    "bytes",
    "list",
    "map",
    "matches",
    "startsWith",
    "endsWith",
    "contains",
    "exists",
    "all",
    "filter",
    "map",
    "exists_one",
    "duration",
    "timestamp",
];

/// Extract identifiers from a CEL expression via simple lexical scan.
fn extract_cel_identifiers(expr: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut chars = expr.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;
    while let Some(&ch) = chars.peek() {
        if escaped {
            escaped = false;
            chars.next();
            continue;
        }
        if ch == '\\' && (in_single_quote || in_double_quote) {
            escaped = true;
            chars.next();
            continue;
        }
        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            chars.next();
            continue;
        }
        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            chars.next();
            continue;
        }
        if in_single_quote || in_double_quote {
            chars.next();
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let mut ident = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphanumeric() || c == '_' {
                    ident.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            ids.insert(ident);
        } else {
            chars.next();
        }
    }
    ids
}

/// Collect apply-time warnings for workflow step definitions.
///
/// Checks for:
/// 1. Unknown YAML fields on step specs (with "did you mean" hints)
/// 2. Prehook CEL expressions referencing variables not captured by prior steps
pub fn collect_step_warnings(steps: &[WorkflowStepSpec], workflow_id: &str) -> Vec<String> {
    let builtin_vars: HashSet<&str> = BUILTIN_CEL_VARS.iter().copied().collect();
    let cel_keywords: HashSet<&str> = CEL_KEYWORDS.iter().copied().collect();
    let mut warnings = Vec::new();
    let mut captured_vars: HashSet<String> = HashSet::new();
    let mut prior_step_ids: HashSet<String> = HashSet::new();

    for step in steps {
        // 1. Unknown field detection
        for key in step.extra.keys() {
            if let Some(suggestion) = did_you_mean(key) {
                warnings.push(format!(
                    "workflow '{}' step '{}' contains unknown field '{}' (did you mean '{}'?)",
                    workflow_id, step.id, key, suggestion
                ));
            } else {
                warnings.push(format!(
                    "workflow '{}' step '{}' contains unknown field '{}'",
                    workflow_id, step.id, key
                ));
            }
        }

        // 2. CEL prehook cross-check: does it reference uncaptured vars?
        if let Some(prehook) = &step.prehook {
            let ids = extract_cel_identifiers(&prehook.when);
            for id in &ids {
                if builtin_vars.contains(id.as_str()) {
                    continue;
                }
                if cel_keywords.contains(id.as_str()) {
                    continue;
                }
                // Skip numeric-looking identifiers or single chars that are likely operators
                if id.len() <= 1 {
                    continue;
                }
                // Skip prior step IDs (used in `steps.<step_id>.<var>` access)
                if prior_step_ids.contains(id) {
                    continue;
                }
                if !captured_vars.contains(id) {
                    warnings.push(format!(
                        "workflow '{}' step '{}' prehook references '{}' but no prior step captures this variable",
                        workflow_id, step.id, id
                    ));
                }
            }
        }

        // Track step ID for subsequent steps (for `steps.<id>.<var>` access)
        prior_step_ids.insert(step.id.clone());

        // Accumulate captured vars for subsequent steps
        for capture in &step.behavior.captures {
            captured_vars.insert(capture.var.clone());
        }

        // Recurse into chain_steps
        if !step.chain_steps.is_empty() {
            let chain_warnings = collect_step_warnings(&step.chain_steps, workflow_id);
            warnings.extend(chain_warnings);
        }
    }

    warnings
}

#[cfg(test)]
mod retirement_tests {
    use super::*;
    use crate::config::{StoreInputConfig, StoreOutputConfig};
    use crate::config_load::tests::make_step;
    use std::collections::HashMap;

    fn reject(step: WorkflowStepConfig) -> String {
        reject_retired_authoring(&step, "wf")
            .expect_err("retired construct must be rejected")
            .to_string()
    }

    fn with_store_inputs() -> WorkflowStepConfig {
        let mut step = make_step("plan", true);
        step.store_inputs = vec![StoreInputConfig {
            store: "promotion".to_string(),
            key: "last_published_sha".to_string(),
            as_var: "last_published_sha".to_string(),
            required: false,
        }];
        step
    }

    #[test]
    fn store_inputs_is_rejected_and_the_diagnostic_names_it() {
        let error = reject(with_store_inputs());
        assert!(
            error.contains("[legacy_pipeline_variables_removed]"),
            "{error}"
        );
        assert!(error.contains("step 'plan' uses store_inputs"), "{error}");
    }

    #[test]
    fn store_outputs_is_rejected_and_the_diagnostic_names_it() {
        let mut step = make_step("plan", true);
        step.store_outputs = vec![StoreOutputConfig {
            store: "promotion".to_string(),
            key: "recorded".to_string(),
            from_var: "sha".to_string(),
        }];
        let error = reject(step);
        assert!(error.contains("step 'plan' uses store_outputs"), "{error}");
    }

    #[test]
    fn step_vars_is_rejected_and_the_diagnostic_names_it() {
        let mut step = make_step("plan", true);
        step.step_vars = Some(HashMap::from([("depth".to_string(), "deep".to_string())]));
        let error = reject(step);
        assert!(error.contains("step 'plan' uses step_vars"), "{error}");
    }

    #[test]
    fn store_put_post_action_is_rejected_and_the_diagnostic_names_it() {
        let mut step = make_step("plan", true);
        step.behavior.post_actions = vec![PostAction::StorePut {
            store: "promotion".to_string(),
            key: "recorded".to_string(),
            from_var: "sha".to_string(),
        }];
        let error = reject(step);
        assert!(
            error.contains("step 'plan' uses a store_put post-action"),
            "{error}"
        );
    }

    #[test]
    fn a_retired_field_nested_in_chain_steps_is_rejected_too() {
        // The parent is clean, so a validator walking only spec.steps reports
        // this workflow valid -- while execute_step dispatches chain children
        // through the same path and runs the binding. This is the case the
        // recursion exists for.
        let mut parent = make_step("chain", true);
        parent.chain_steps = vec![with_store_inputs()];

        let error = reject(parent);
        assert!(
            error.contains("[legacy_pipeline_variables_removed]"),
            "{error}"
        );
        assert!(error.contains("step 'plan' uses store_inputs"), "{error}");
    }

    #[test]
    fn an_empty_step_vars_map_is_not_a_retired_construct() {
        // `step_vars: {}` deserializes to Some(empty), not None. Rejecting on
        // Some alone would fail a manifest that authors nothing, which is the
        // false-positive half of this check.
        let mut step = make_step("plan", true);
        step.step_vars = Some(HashMap::new());
        assert!(reject_retired_authoring(&step, "wf").is_ok());
    }

    #[test]
    fn a_step_authoring_none_of_them_is_accepted() {
        let mut parent = make_step("chain", true);
        parent.chain_steps = vec![make_step("child", true)];
        assert!(reject_retired_authoring(&parent, "wf").is_ok());
    }
}

#[cfg(test)]
mod driver_tests {
    use super::*;
    use crate::config::{
        AgentConfig, AgentDriverConfig, DriverOptions, DriverProvider, DriverRequirements,
    };
    use crate::config_load::tests::make_step;
    use std::collections::HashMap;

    fn agent(provider: DriverProvider, transport: DriverTransport) -> AgentConfig {
        AgentConfig {
            capabilities: vec!["implement".to_string()],
            driver: Some(AgentDriverConfig {
                provider,
                transport,
                binary: None,
                options: DriverOptions::default(),
                claude: None,
                codex: None,
                shell: None,
                raw_args: Vec::new(),
                unsafe_raw_args: false,
            }),
            command: if provider == DriverProvider::Shell {
                "echo {prompt}".to_string()
            } else {
                String::new()
            },
            ..AgentConfig::default()
        }
    }

    fn validate(
        provider: DriverProvider,
        transport: DriverTransport,
        requirements: DriverRequirements,
        side_effect_class: SideEffectClass,
    ) -> String {
        let agents = HashMap::from([("agent".to_string(), agent(provider, transport))]);
        let mut step = make_step("implement", true);
        step.required_capability = Some("implement".to_string());
        step.behavior.driver_requirements = requirements;
        step.behavior.side_effect_class = side_effect_class;
        validate_driver_candidates(&step, "workflow", "implement", &agents)
            .expect_err("incompatible driver")
            .to_string()
    }

    #[test]
    fn apply_validation_rejects_multi_turn_shell_driver() {
        let error = validate(
            DriverProvider::Shell,
            DriverTransport::Cli,
            DriverRequirements {
                multi_turn: true,
                ..DriverRequirements::default()
            },
            SideEffectClass::WorkspaceOnly,
        );
        assert!(error.contains("driver_multi_turn_required"));
    }

    #[test]
    fn apply_validation_rejects_missing_tool_hosting() {
        let error = validate(
            DriverProvider::Codex,
            DriverTransport::Cli,
            DriverRequirements {
                tool_hosting: ToolHosting::Stdio,
                ..DriverRequirements::default()
            },
            SideEffectClass::WorkspaceOnly,
        );
        assert!(error.contains("driver_tool_hosting_required"));
    }

    #[test]
    fn apply_validation_rejects_non_guaranteed_cancel_for_external_step() {
        let error = validate(
            DriverProvider::Codex,
            DriverTransport::Sdk,
            DriverRequirements {
                workspace_access: WorkspaceAccess::None,
                ..DriverRequirements::default()
            },
            SideEffectClass::NonIdempotentExternal,
        );
        assert!(error.contains("driver_guaranteed_cancel_required"));
    }

    #[test]
    fn apply_validation_rejects_sdk_workspace_access_before_runtime() {
        let error = validate(
            DriverProvider::Codex,
            DriverTransport::Sdk,
            DriverRequirements::default(),
            SideEffectClass::WorkspaceOnly,
        );
        assert!(error.contains("driver_workspace_sandbox_required"));
    }

    #[test]
    fn apply_validation_rejects_driver_without_permission_events() {
        let error = validate(
            DriverProvider::Codex,
            DriverTransport::Cli,
            DriverRequirements {
                permission_events: true,
                ..DriverRequirements::default()
            },
            SideEffectClass::WorkspaceOnly,
        );
        assert!(error.contains("driver_permission_events_required"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_field_detected_with_suggestion() {
        let step = WorkflowStepSpec {
            id: "qa_doc_gen".to_string(),
            step_type: "qa_doc_gen".to_string(),
            extra: [("capture".to_string(), serde_yaml::Value::Null)]
                .into_iter()
                .collect(),
            ..default_step_spec()
        };
        let warnings = collect_step_warnings(&[step], "test-wf");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("did you mean 'behavior.captures'"));
        assert!(warnings[0].contains("workflow 'test-wf'"));
    }

    #[test]
    fn extract_cel_identifiers_ignores_string_literals() {
        let ids = extract_cel_identifiers(
            r#"qa_file_path.startsWith("docs/qa/") && qa_file_path.endsWith('.md')"#,
        );
        assert!(ids.contains("qa_file_path"));
        assert!(ids.contains("startsWith"));
        assert!(ids.contains("endsWith"));
        assert!(!ids.contains("docs"));
        assert!(!ids.contains("qa"));
        assert!(!ids.contains("md"));
    }

    #[test]
    fn collect_step_warnings_accepts_full_qa_self_referential_vars() {
        let step = WorkflowStepSpec {
            id: "qa_testing".to_string(),
            step_type: "qa_testing".to_string(),
            prehook: Some(crate::cli_types::WorkflowPrehookSpec {
                when: r#"qa_file_path.startsWith("docs/qa/") && qa_file_path.endsWith(".md") && (self_referential_safe || size(self_referential_safe_scenarios) > 0)"#.to_string(),
                reason: Some("safe qa docs only".to_string()),
                engine: "cel".to_string(),
                ui: None,
                extended: false,
            }),
            ..default_step_spec()
        };
        let warnings = collect_step_warnings(&[step], "full-qa");
        assert!(
            warnings.is_empty(),
            "full-qa prehook should not warn, got: {warnings:?}"
        );
    }

    #[test]
    fn unknown_field_detected_without_suggestion() {
        let step = WorkflowStepSpec {
            id: "step1".to_string(),
            step_type: "qa".to_string(),
            extra: [("foobar".to_string(), serde_yaml::Value::Null)]
                .into_iter()
                .collect(),
            ..default_step_spec()
        };
        let warnings = collect_step_warnings(&[step], "test-wf");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown field 'foobar'"));
        assert!(warnings[0].contains("workflow 'test-wf'"));
        assert!(!warnings[0].contains("did you mean"));
    }

    #[test]
    fn no_warnings_for_clean_step() {
        let step = default_step_spec();
        let warnings = collect_step_warnings(&[step], "test-wf");
        assert!(warnings.is_empty());
    }

    #[test]
    fn prehook_warns_on_uncaptured_variable() {
        let capture_step = WorkflowStepSpec {
            id: "qa_doc_gen".to_string(),
            step_type: "qa_doc_gen".to_string(),
            behavior: crate::config::StepBehavior {
                captures: vec![crate::config::CaptureDecl {
                    var: "other_var".to_string(),
                    source: crate::config::CaptureSource::Stdout,
                    json_path: None,
                }],
                ..Default::default()
            },
            ..default_step_spec()
        };
        let prehook_step = WorkflowStepSpec {
            id: "qa_testing".to_string(),
            step_type: "qa_testing".to_string(),
            prehook: Some(crate::cli_types::WorkflowPrehookSpec {
                engine: "cel".to_string(),
                when: "regression_target_ids != ''".to_string(),
                reason: None,
                ui: None,
                extended: false,
            }),
            ..default_step_spec()
        };
        let warnings = collect_step_warnings(&[capture_step, prehook_step], "test-wf");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("regression_target_ids"));
        assert!(warnings[0].contains("no prior step captures"));
    }

    #[test]
    fn prehook_no_warning_when_variable_captured() {
        let capture_step = WorkflowStepSpec {
            id: "qa_doc_gen".to_string(),
            step_type: "qa_doc_gen".to_string(),
            behavior: crate::config::StepBehavior {
                captures: vec![crate::config::CaptureDecl {
                    var: "regression_target_ids".to_string(),
                    source: crate::config::CaptureSource::Stdout,
                    json_path: None,
                }],
                ..Default::default()
            },
            ..default_step_spec()
        };
        let prehook_step = WorkflowStepSpec {
            id: "qa_testing".to_string(),
            step_type: "qa_testing".to_string(),
            prehook: Some(crate::cli_types::WorkflowPrehookSpec {
                engine: "cel".to_string(),
                when: "qa_file_path in regression_target_ids".to_string(),
                reason: None,
                ui: None,
                extended: false,
            }),
            ..default_step_spec()
        };
        let warnings = collect_step_warnings(&[capture_step, prehook_step], "test-wf");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn yaml_round_trip_captures_unknown_fields() {
        let yaml = r#"
- id: qa_doc_gen
  type: qa_doc_gen
  capture:
    - var: regression_target_ids
      source: stdout
- id: qa_testing
  type: qa_testing
  prehook:
    engine: cel
    when: "qa_file_path in regression_target_ids"
"#;
        let steps: Vec<WorkflowStepSpec> = serde_yaml::from_str(yaml).expect("parse");
        assert_eq!(steps.len(), 2);
        // "capture" is unknown → should be in extra
        assert!(
            steps[0].extra.contains_key("capture"),
            "unknown field 'capture' should be captured in extra, got: {:?}",
            steps[0].extra.keys().collect::<Vec<_>>()
        );
        let warnings = collect_step_warnings(&steps, "test-wf");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("capture") && w.contains("behavior.captures")),
            "expected 'did you mean' warning, got: {warnings:?}"
        );
        assert!(
            warnings.iter().any(
                |w| w.contains("regression_target_ids") && w.contains("no prior step captures")
            ),
            "expected uncaptured var warning, got: {warnings:?}"
        );
    }

    #[test]
    fn prehook_no_warning_for_steps_dot_step_id_access() {
        let capture_step = WorkflowStepSpec {
            id: "step_a".to_string(),
            step_type: "qa_doc_gen".to_string(),
            behavior: crate::config::StepBehavior {
                captures: vec![crate::config::CaptureDecl {
                    var: "regression_target_ids".to_string(),
                    source: crate::config::CaptureSource::Stdout,
                    json_path: None,
                }],
                ..Default::default()
            },
            ..default_step_spec()
        };
        let prehook_step = WorkflowStepSpec {
            id: "step_b".to_string(),
            step_type: "qa_testing".to_string(),
            prehook: Some(crate::cli_types::WorkflowPrehookSpec {
                engine: "cel".to_string(),
                when: "len(steps.step_a.regression_target_ids) > 0".to_string(),
                reason: None,
                ui: None,
                extended: false,
            }),
            ..default_step_spec()
        };
        let warnings = collect_step_warnings(&[capture_step, prehook_step], "test-workflow");
        assert!(
            warnings.is_empty(),
            "expected no warnings for steps.step_a.var access, got: {warnings:?}"
        );
    }

    fn default_step_spec() -> WorkflowStepSpec {
        WorkflowStepSpec {
            id: "qa".to_string(),
            step_type: "qa".to_string(),
            required_capability: None,
            template: None,
            execution_profile: None,
            builtin: None,
            enabled: true,
            repeatable: false,
            is_guard: false,
            cost_preference: None,
            prehook: None,
            tty: false,
            command: None,
            chain_steps: vec![],
            scope: None,
            max_parallel: None,
            stagger_delay_ms: None,
            timeout_secs: None,
            stall_timeout_secs: None,
            behavior: Default::default(),
            item_select_config: None,
            store_inputs: vec![],
            store_outputs: vec![],
            step_vars: None,
            extra: Default::default(),
        }
    }
}
