use super::common::AgentLookup;
use crate::cli_types::WorkflowStepSpec;
use crate::config::{
    CancelSemantics, DriverTransport, SideEffectClass, StepSemanticKind, ToolHosting,
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
];
// `steps` was listed here until FR-173, alongside a skip for prior step ids, so
// that `steps.<id>.<captured_var>` would lint clean. `build_step_prehook_cel_context`
// binds no variable named `steps` and never has — the form fails at execution with
// "no such variable", and only `captures` made it look like a mechanism. Captured
// variables were reachable by their bare name, which `bind_compatibility_vars`
// still does for whatever populates `vars`. No tracked blueprint uses the form.

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
                warnings.push(format!(
                    "workflow '{}' step '{}' prehook references '{}', which is not a builtin CEL variable",
                    workflow_id, step.id, id
                ));
            }
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
    fn prehook_warns_on_a_variable_nothing_binds() {
        let prior_step = WorkflowStepSpec {
            id: "qa_doc_gen".to_string(),
            step_type: "qa_doc_gen".to_string(),
            behavior: crate::config::StepBehavior {
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
        let warnings = collect_step_warnings(&[prior_step, prehook_step], "test-wf");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("regression_target_ids"),
            "{warnings:?}"
        );
        assert!(
            warnings[0].contains("not a builtin CEL variable"),
            "{warnings:?}"
        );
    }

    /// The negative half. It used to be "a variable a prior step captured does
    /// not warn"; FR-173 removed captures, so the only variables that do not
    /// warn are the ones the evaluator actually binds. Without this case an
    /// implementation that warned about every identifier would satisfy the
    /// positive test above.
    #[test]
    fn prehook_does_not_warn_about_builtin_variables() {
        let prehook_step = WorkflowStepSpec {
            id: "qa_testing".to_string(),
            step_type: "qa_testing".to_string(),
            prehook: Some(crate::cli_types::WorkflowPrehookSpec {
                engine: "cel".to_string(),
                when: "qa_failed && qa_exit_code != 0 && !is_last_cycle".to_string(),
                reason: None,
                ui: None,
                extended: false,
            }),
            ..default_step_spec()
        };
        let warnings = collect_step_warnings(&[prehook_step], "test-wf");
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
        // `capture` used to earn a "did you mean 'behavior.captures'?" hint.
        // FR-173 deleted that field, so the hint would now point at nothing; the
        // unknown field is still named, and the absence of the hint is asserted
        // rather than left unstated, because a hint naming a deleted field is
        // the failure this pair exists to catch.
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("unknown field 'capture'")),
            "expected the unknown field to be named, got: {warnings:?}"
        );
        assert!(
            !warnings.iter().any(|w| w.contains("behavior.captures")),
            "no warning may suggest a field FR-173 deleted: {warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("regression_target_ids")
                    && w.contains("not a builtin CEL variable")),
            "expected unbound var warning, got: {warnings:?}"
        );
    }

    /// This asserted the opposite until FR-173: `steps.<id>.<var>` was linted
    /// clean, on the strength of `steps` sitting in BUILTIN_CEL_VARS and a skip
    /// for prior step ids. `build_step_prehook_cel_context` binds no `steps`
    /// variable, so the expression fails at execution with "no such variable" —
    /// the lint was certifying a form that does not run. Warning is the correct
    /// outcome, and the diagnostic has to name `steps` itself: naming only the
    /// trailing member would send an author looking for the wrong mistake.
    #[test]
    fn prehook_warns_on_the_steps_dot_step_id_form_the_evaluator_never_bound() {
        let prior_step = WorkflowStepSpec {
            id: "step_a".to_string(),
            step_type: "qa_doc_gen".to_string(),
            behavior: crate::config::StepBehavior {
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
        let warnings = collect_step_warnings(&[prior_step, prehook_step], "test-workflow");
        assert!(
            warnings.iter().any(|w| w.contains("'steps'")),
            "the unbound root must be named: {warnings:?}"
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
            extra: Default::default(),
        }
    }
}

/// FR-173 retired five compatibility surfaces at the v0.7 window. What replaces
/// each named rejection is asserted here, because deleting a check without
/// asserting what happens instead leaves no evidence that the retirement told
/// anyone anything.
///
/// Two mechanisms answer, and which one applies depends on the struct:
/// `WorkflowStepSpec` carries a flattened `extra` catch-all, so a retired field
/// at step level becomes a named apply-time warning; `StepBehavior` and
/// `RunnerSpec` have no catch-all, so they carry `deny_unknown_fields` and a
/// retired field there is a hard deserialisation error. Without the attribute
/// serde's default would drop the key in silence, which is worse than the named
/// rejection it replaced — that is the property these cases exist to hold.
#[cfg(test)]
mod fr173_retirement {
    use crate::cli_types::WorkflowStepSpec;

    fn warnings_for(yaml: &str) -> Vec<String> {
        let spec: WorkflowStepSpec =
            serde_yaml::from_str(yaml).expect("step-level retired fields must still deserialize");
        super::collect_step_warnings(&[spec], "wf")
    }

    #[test]
    fn step_level_retired_fields_become_named_warnings() {
        for field in ["store_inputs", "store_outputs", "step_vars"] {
            let warnings = warnings_for(&format!("id: s1\ntype: qa\n{field}: {{}}\n"));
            assert!(
                warnings.iter().any(|w| w.contains(field)),
                "retiring {field} must still name it: {warnings:?}"
            );
        }
    }

    #[test]
    fn behavior_level_retired_fields_are_a_stated_error_not_a_dropped_key() {
        // `captures` and the JSONPath post-actions lived on StepBehavior, which
        // has no catch-all. deny_unknown_fields is the only thing standing
        // between a retired field and silence.
        let err = serde_yaml::from_str::<WorkflowStepSpec>(
            "id: s1\ntype: qa\nbehavior:\n  captures:\n    - var: x\n      source: stdout\n",
        )
        .expect_err("a retired behavior field must not be silently dropped");
        assert!(
            err.to_string().contains("captures"),
            "the error must name the field: {err}"
        );
    }

    #[test]
    fn a_retired_post_action_variant_is_a_stated_error() {
        let err = serde_yaml::from_str::<WorkflowStepSpec>(
            "id: s1\ntype: qa\nbehavior:\n  post_actions:\n    - type: generate_items\n",
        )
        .expect_err("a retired post-action must not deserialize");
        assert!(
            err.to_string().contains("generate_items"),
            "the error must name the variant: {err}"
        );
    }

    #[test]
    fn a_step_using_none_of_them_still_parses_and_warns_about_nothing() {
        // The negative half. Without it, an implementation that rejected every
        // step would satisfy all three cases above.
        let warnings = warnings_for("id: s1\ntype: qa\nenabled: true\n");
        assert!(warnings.is_empty(), "clean step warned: {warnings:?}");
    }
}
