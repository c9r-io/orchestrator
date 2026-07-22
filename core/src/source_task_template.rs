//! Validation and deterministic rendering for source-to-task templates.

use crate::config::{OrchestratorConfig, SourceTaskTemplateConfig};
use anyhow::{Result, anyhow, bail};
use std::collections::HashSet;
use std::collections::{BTreeMap, HashMap};

/// Maximum UTF-8 byte length of a goal template or rendered goal.
pub const MAX_GOAL_BYTES: usize = 16 * 1024;
/// Maximum UTF-8 byte length of a source message URL.
pub const MAX_SOURCE_URL_BYTES: usize = 2 * 1024;
/// Maximum UTF-8 byte length of a trusted skill invocation.
pub const MAX_INVOCATION_BYTES: usize = 512;
/// Maximum number of trusted skill arguments.
pub const MAX_SKILL_ARGS: usize = 16;
/// Maximum UTF-8 byte length of one trusted skill argument.
pub const MAX_SKILL_ARG_BYTES: usize = 1024;
/// Maximum number of trusted initial variables.
pub const MAX_INITIAL_VARS: usize = 32;
/// Maximum UTF-8 byte length of one trusted initial variable value.
pub const MAX_INITIAL_VAR_BYTES: usize = 2 * 1024;

/// Variables a source adapter may provide to the renderer.
pub const SUPPORTED_VARIABLES: [&str; 8] = [
    "skill_name",
    "skill_invocation",
    "source_message_url",
    "source_provider",
    "source_installation_id",
    "source_event_id",
    "source_reaction",
    "source_target_id",
];

/// Typed source evidence supplied to the shared preview/live renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTaskTemplateRenderInput {
    /// Source provider identifier, currently `slack`.
    pub provider: String,
    /// Configured provider installation identifier.
    pub installation_id: String,
    /// Canonical source message URL.
    pub message_url: String,
    /// Provider event identifier when rendering a live event.
    pub event_id: Option<String>,
    /// Reaction or badge value when available.
    pub reaction: Option<String>,
    /// Provider-neutral target identifier when available.
    pub target_id: Option<String>,
    /// Whether installation and URL ownership were verified by a live resolver.
    pub installation_verified: bool,
}

/// Trusted action output produced by rendering a source task template.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct RenderedSourceTaskAction {
    /// Referenced workflow.
    pub workflow: String,
    /// Referenced workspace.
    pub workspace: String,
    /// Whether the eventual task should start immediately.
    pub start: bool,
    /// Trusted initial task variables.
    pub initial_vars: BTreeMap<String, String>,
}

/// Deterministic output shared by preview and future live binding execution.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct RenderedSourceTaskTemplate {
    /// Stable skill name.
    pub skill_name: String,
    /// Trusted skill invocation.
    pub skill_invocation: String,
    /// Trusted ordered skill arguments.
    pub skill_args: Vec<String>,
    /// Rendered task goal.
    pub goal: String,
    /// Trusted action fields.
    pub action: RenderedSourceTaskAction,
    /// Stable hash of the normalized template content.
    pub content_hash: String,
    /// Stable revision identifier (equal to the full content hash in v1).
    pub revision: String,
    /// Non-fatal preview or verification warnings.
    pub warnings: Vec<String>,
}

/// Returns a stable SHA-256 hash over a normalized template representation.
pub fn template_content_hash(template: &SourceTaskTemplateConfig) -> Result<String> {
    let mut normalized = template.clone();
    normalized.allowed_variables.sort();
    let value = serde_json::to_value(normalized)?;
    crate::action_audit::canonical_request_hash(&value)
}

/// Renders one validated template with typed source evidence and no side effects.
pub fn render_source_task_template(
    template: &SourceTaskTemplateConfig,
    input: &SourceTaskTemplateRenderInput,
) -> Result<RenderedSourceTaskTemplate> {
    validate_template_config(template)?;
    validate_source_input(input)?;

    let values = HashMap::from([
        ("skill_name", template.skill.name.as_str()),
        ("skill_invocation", template.skill.invocation.as_str()),
        ("source_message_url", input.message_url.as_str()),
        ("source_provider", input.provider.as_str()),
        ("source_installation_id", input.installation_id.as_str()),
        ("source_event_id", input.event_id.as_deref().unwrap_or("")),
        ("source_reaction", input.reaction.as_deref().unwrap_or("")),
        ("source_target_id", input.target_id.as_deref().unwrap_or("")),
    ]);
    let goal = render_goal(&template.goal_template, &values)?;
    if goal.len() > MAX_GOAL_BYTES {
        bail!("rendered goal exceeds {MAX_GOAL_BYTES} bytes");
    }
    let content_hash = template_content_hash(template)?;
    let warnings = if input.installation_verified {
        Vec::new()
    } else {
        vec!["sample_url_not_verified_against_installation".to_string()]
    };
    Ok(RenderedSourceTaskTemplate {
        skill_name: template.skill.name.clone(),
        skill_invocation: template.skill.invocation.clone(),
        skill_args: template.skill.args.clone(),
        goal,
        action: RenderedSourceTaskAction {
            workflow: template.action.workflow.clone(),
            workspace: template.action.workspace.clone(),
            start: template.action.start,
            initial_vars: template.action.initial_vars.clone(),
        },
        revision: content_hash.clone(),
        content_hash,
        warnings,
    })
}

/// Resolves and renders a template from one immutable configuration snapshot.
pub fn render_source_task_template_from_config(
    config: &OrchestratorConfig,
    project_id: &str,
    template_name: &str,
    input: &SourceTaskTemplateRenderInput,
) -> Result<RenderedSourceTaskTemplate> {
    let project = config
        .projects
        .get(project_id)
        .ok_or_else(|| anyhow!("project not found: {project_id}"))?;
    let template = project
        .source_task_templates
        .get(template_name)
        .ok_or_else(|| anyhow!("source task template not found: {template_name}"))?;
    render_source_task_template(template, input)
}

/// Redacts all public text fields using the effective runtime policy patterns.
pub fn redact_rendered_source_task_template(
    rendered: &RenderedSourceTaskTemplate,
    patterns: &[String],
) -> RenderedSourceTaskTemplate {
    let redact = |value: &str| orchestrator_runner::runner::redact_text(value, patterns);
    let mut output = rendered.clone();
    output.skill_name = redact(&output.skill_name);
    output.skill_invocation = redact(&output.skill_invocation);
    output.skill_args = output
        .skill_args
        .iter()
        .map(|value| redact(value))
        .collect();
    output.goal = redact(&output.goal);
    output.action.initial_vars = output
        .action
        .initial_vars
        .iter()
        .map(|(key, value)| (key.clone(), redact(value)))
        .collect();
    output
}

fn validate_source_input(input: &SourceTaskTemplateRenderInput) -> Result<()> {
    if input.provider.trim().is_empty() {
        bail!("source provider cannot be empty");
    }
    if input.installation_id.trim().is_empty() {
        bail!("source installation id cannot be empty");
    }
    if input.message_url.len() > MAX_SOURCE_URL_BYTES {
        bail!("source message URL exceeds {MAX_SOURCE_URL_BYTES} bytes");
    }
    let parsed = url::Url::parse(&input.message_url)
        .map_err(|error| anyhow!("invalid source message URL: {error}"))?;
    if parsed.scheme() != "https" {
        bail!("source message URL must use https");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("source message URL cannot contain credentials");
    }
    if input.provider == "slack" {
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow!("Slack message URL must include a host"))?;
        if host != "slack.com" && !host.ends_with(".slack.com") {
            bail!("Slack message URL host is not allowed: {host}");
        }
        if !parsed.path().starts_with("/archives/") {
            bail!("Slack message URL must use an /archives/ permalink");
        }
    } else {
        bail!("unsupported source provider: {}", input.provider);
    }
    Ok(())
}

fn render_goal(template: &str, values: &HashMap<&str, &str>) -> Result<String> {
    let bytes = template.as_bytes();
    let mut output = String::with_capacity(template.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' if bytes.get(index + 1) == Some(&b'{') => {
                output.push('{');
                index += 2;
            }
            b'}' if bytes.get(index + 1) == Some(&b'}') => {
                output.push('}');
                index += 2;
            }
            b'{' => {
                let relative_end = bytes[index + 1..]
                    .iter()
                    .position(|byte| *byte == b'}')
                    .ok_or_else(|| anyhow!("goalTemplate contains an unclosed variable token"))?;
                let end = index + 1 + relative_end;
                let variable = &template[index + 1..end];
                let value = values.get(variable).ok_or_else(|| {
                    anyhow!("goalTemplate variable '{variable}' has no typed value")
                })?;
                output.push_str(value);
                index = end + 1;
            }
            b'}' => bail!("goalTemplate contains an unmatched closing brace"),
            _ => {
                let character = template[index..]
                    .chars()
                    .next()
                    .ok_or_else(|| anyhow!("invalid goalTemplate character boundary"))?;
                output.push(character);
                index += character.len_utf8();
            }
        }
    }
    Ok(output)
}

/// Validates bounded trusted fields and the explicit goal-variable allowlist.
pub fn validate_template_config(template: &SourceTaskTemplateConfig) -> Result<()> {
    if template.skill.name.trim().is_empty() {
        bail!("source_task_template.spec.skill.name cannot be empty");
    }
    if template.skill.invocation.trim().is_empty() {
        bail!("source_task_template.spec.skill.invocation cannot be empty");
    }
    if template.skill.invocation.len() > MAX_INVOCATION_BYTES {
        bail!("source_task_template.spec.skill.invocation exceeds {MAX_INVOCATION_BYTES} bytes");
    }
    if template.skill.invocation.contains(['\n', '\r', '\0']) {
        bail!("source_task_template.spec.skill.invocation cannot contain control line breaks");
    }
    if template.skill.args.len() > MAX_SKILL_ARGS {
        bail!("source_task_template.spec.skill.args exceeds {MAX_SKILL_ARGS} entries");
    }
    for (index, arg) in template.skill.args.iter().enumerate() {
        if arg.len() > MAX_SKILL_ARG_BYTES {
            bail!(
                "source_task_template.spec.skill.args[{index}] exceeds {MAX_SKILL_ARG_BYTES} bytes"
            );
        }
        if arg.contains('\0') {
            bail!("source_task_template.spec.skill.args[{index}] contains NUL");
        }
    }
    if template.action.workflow.trim().is_empty() {
        bail!("source_task_template.spec.action.workflow cannot be empty");
    }
    if template.action.workspace.trim().is_empty() {
        bail!("source_task_template.spec.action.workspace cannot be empty");
    }
    if template.action.initial_vars.len() > MAX_INITIAL_VARS {
        bail!("source_task_template.spec.action.initialVars exceeds {MAX_INITIAL_VARS} entries");
    }
    for (key, value) in &template.action.initial_vars {
        if key.trim().is_empty() {
            bail!("source_task_template.spec.action.initialVars contains an empty key");
        }
        if value.len() > MAX_INITIAL_VAR_BYTES {
            bail!(
                "source_task_template.spec.action.initialVars['{key}'] exceeds {MAX_INITIAL_VAR_BYTES} bytes"
            );
        }
    }
    if template.goal_template.trim().is_empty() {
        bail!("source_task_template.spec.goalTemplate cannot be empty");
    }
    if template.goal_template.len() > MAX_GOAL_BYTES {
        bail!("source_task_template.spec.goalTemplate exceeds {MAX_GOAL_BYTES} bytes");
    }

    let supported: HashSet<&str> = SUPPORTED_VARIABLES.into_iter().collect();
    let mut allowed = HashSet::new();
    for variable in &template.allowed_variables {
        if !supported.contains(variable.as_str()) {
            bail!(
                "source_task_template.spec.allowedVariables contains unsupported variable '{variable}'"
            );
        }
        if !allowed.insert(variable.as_str()) {
            bail!("source_task_template.spec.allowedVariables contains duplicate '{variable}'");
        }
    }
    for variable in template_variables(&template.goal_template)? {
        if !allowed.contains(variable.as_str()) {
            bail!("goalTemplate variable '{variable}' is not declared in allowedVariables");
        }
    }
    Ok(())
}

fn template_variables(template: &str) -> Result<Vec<String>> {
    let bytes = template.as_bytes();
    let mut variables = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' if bytes.get(index + 1) == Some(&b'{') => index += 2,
            b'}' if bytes.get(index + 1) == Some(&b'}') => index += 2,
            b'{' => {
                let relative_end = bytes[index + 1..]
                    .iter()
                    .position(|byte| *byte == b'}')
                    .ok_or_else(|| anyhow!("goalTemplate contains an unclosed variable token"))?;
                let end = index + 1 + relative_end;
                let variable = &template[index + 1..end];
                if variable.is_empty()
                    || !variable.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                    })
                {
                    bail!("goalTemplate contains invalid variable token '{{{variable}}}'");
                }
                variables.push(variable.to_string());
                index = end + 1;
            }
            b'}' => bail!("goalTemplate contains an unmatched closing brace"),
            _ => index += 1,
        }
    }
    Ok(variables)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SourceTaskTemplateActionConfig, SourceTaskTemplateSkillConfig};
    use std::collections::BTreeMap;

    fn valid_template() -> SourceTaskTemplateConfig {
        SourceTaskTemplateConfig {
            skill: SourceTaskTemplateSkillConfig {
                name: "docs".to_string(),
                invocation: "$docs".to_string(),
                args: vec![],
            },
            action: SourceTaskTemplateActionConfig {
                workflow: "docs".to_string(),
                workspace: "main".to_string(),
                start: true,
                initial_vars: BTreeMap::new(),
            },
            goal_template: "{{source}} {skill_invocation}: {source_message_url}".to_string(),
            allowed_variables: vec![
                "skill_invocation".to_string(),
                "source_message_url".to_string(),
            ],
        }
    }

    #[test]
    fn validation_accepts_exact_allowlist_and_escaped_braces() {
        validate_template_config(&valid_template()).expect("valid template");
    }

    #[test]
    fn validation_rejects_undeclared_and_unknown_variables() {
        let mut template = valid_template();
        template.allowed_variables = vec!["skill_invocation".to_string()];
        assert!(validate_template_config(&template).is_err());

        template
            .allowed_variables
            .push("source_message_url".to_string());
        template.allowed_variables.push("arbitrary".to_string());
        assert!(validate_template_config(&template).is_err());
    }

    #[test]
    fn validation_rejects_unbalanced_braces_and_bounds() {
        let mut template = valid_template();
        template.goal_template = "{source_message_url".to_string();
        assert!(validate_template_config(&template).is_err());

        template = valid_template();
        template.skill.args = vec!["x".to_string(); MAX_SKILL_ARGS + 1];
        assert!(validate_template_config(&template).is_err());
    }

    #[test]
    fn rendering_is_single_pass_and_revision_is_order_stable() {
        let mut template = valid_template();
        let input = SourceTaskTemplateRenderInput {
            provider: "slack".to_string(),
            installation_id: "team-a".to_string(),
            message_url: "https://example.slack.com/archives/C1/p123{skill_name}".to_string(),
            event_id: None,
            reaction: None,
            target_id: None,
            installation_verified: false,
        };
        let first = render_source_task_template(&template, &input).expect("render");
        assert_eq!(
            first.goal,
            "{source} $docs: https://example.slack.com/archives/C1/p123{skill_name}"
        );
        assert_eq!(
            first.warnings,
            vec!["sample_url_not_verified_against_installation"]
        );

        template.allowed_variables.reverse();
        let second = render_source_task_template(&template, &input).expect("render");
        assert_eq!(first.content_hash, second.content_hash);
        assert_eq!(first.revision, second.revision);
    }

    #[test]
    fn rendering_rejects_insecure_or_cross_provider_urls() {
        let template = valid_template();
        let mut input = SourceTaskTemplateRenderInput {
            provider: "slack".to_string(),
            installation_id: "team-a".to_string(),
            message_url: "http://example.slack.com/archives/C1/p123".to_string(),
            event_id: None,
            reaction: None,
            target_id: None,
            installation_verified: true,
        };
        assert!(render_source_task_template(&template, &input).is_err());
        input.message_url = "https://evil.example/archives/C1/p123".to_string();
        assert!(render_source_task_template(&template, &input).is_err());
    }

    #[test]
    fn public_projection_redacts_goal_and_trusted_values() {
        let template = valid_template();
        let input = SourceTaskTemplateRenderInput {
            provider: "slack".to_string(),
            installation_id: "team-a".to_string(),
            message_url: "https://example.slack.com/archives/C1/secret".to_string(),
            event_id: None,
            reaction: None,
            target_id: None,
            installation_verified: true,
        };
        let rendered = render_source_task_template(&template, &input).expect("render");
        let public = redact_rendered_source_task_template(&rendered, &["secret".to_string()]);
        assert!(!public.goal.to_lowercase().contains("secret"));
    }

    #[test]
    fn concurrent_config_swap_never_mixes_template_generations() {
        use arc_swap::ArcSwap;
        use std::sync::Arc;

        fn config_with_template(invocation: &str, workflow: &str) -> OrchestratorConfig {
            let mut config = OrchestratorConfig::default();
            let mut template = valid_template();
            template.skill.invocation = invocation.to_string();
            template.action.workflow = workflow.to_string();
            config
                .ensure_project(Some("alpha"))
                .source_task_templates
                .insert("docs".to_string(), template);
            config
        }

        let first = Arc::new(config_with_template("$docs-v1", "workflow-v1"));
        let second = Arc::new(config_with_template("$docs-v2", "workflow-v2"));
        let snapshot = Arc::new(ArcSwap::from(first.clone()));
        let input = SourceTaskTemplateRenderInput {
            provider: "slack".to_string(),
            installation_id: "team-a".to_string(),
            message_url: "https://example.slack.com/archives/C1/p123".to_string(),
            event_id: None,
            reaction: None,
            target_id: None,
            installation_verified: true,
        };

        std::thread::scope(|scope| {
            let writer_snapshot = Arc::clone(&snapshot);
            scope.spawn(move || {
                for index in 0..500 {
                    writer_snapshot.store(if index % 2 == 0 {
                        Arc::clone(&first)
                    } else {
                        Arc::clone(&second)
                    });
                }
            });
            let reader_snapshot = Arc::clone(&snapshot);
            scope.spawn(move || {
                for _ in 0..500 {
                    let active = reader_snapshot.load_full();
                    let rendered =
                        render_source_task_template_from_config(&active, "alpha", "docs", &input)
                            .expect("render immutable snapshot");
                    assert!(
                        (rendered.skill_invocation == "$docs-v1"
                            && rendered.action.workflow == "workflow-v1")
                            || (rendered.skill_invocation == "$docs-v2"
                                && rendered.action.workflow == "workflow-v2")
                    );
                }
            });
        });
    }
}
