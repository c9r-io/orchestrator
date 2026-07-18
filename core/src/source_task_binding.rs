//! Deterministic validation and matching for source-to-task bindings.

use crate::config::{OrchestratorConfig, SourceTaskBindingConfig};
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::HashSet;

/// Maximum number of explicitly allowed channels in one binding.
pub const MAX_CHANNELS: usize = 64;

/// Provider-neutral normalized evidence consumed by the binding matcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTaskBindingMatchInput {
    /// Authenticated provider name.
    pub provider: String,
    /// Authenticated provider installation identity.
    pub installation_id: String,
    /// Normalized event kind.
    pub event_kind: String,
    /// Exact normalized reaction name.
    pub reaction: String,
    /// Exact normalized target kind.
    pub target_kind: String,
    /// Normalized source channel identity.
    pub channel_id: String,
    /// Authenticated external actor identity.
    pub external_actor_id: String,
}

/// Safe explanation for one binding candidate.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SourceTaskBindingCandidate {
    /// Binding resource name.
    pub binding_id: String,
    /// Stable candidate reason code.
    pub reason: String,
    /// Stable binding content revision.
    pub revision: String,
}

/// Deterministic binding selection returned to simulation and live routing.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SourceTaskBindingMatchResult {
    /// `matched`, `no_match`, or `ambiguous`.
    pub status: String,
    /// Stable overall reason code.
    pub reason: String,
    /// Resolved Trigger name when the installation is unambiguous.
    pub trigger_name: Option<String>,
    /// Trusted role resolved from Trigger actorRoles.
    pub resolved_role: Option<String>,
    /// Selected binding name for exactly-one match.
    pub binding_id: Option<String>,
    /// Selected template name for exactly-one match.
    pub template_ref: Option<String>,
    /// Selected binding revision for provenance.
    pub binding_revision: Option<String>,
    /// Safe deterministic explanations ordered by binding ID.
    pub candidates: Vec<SourceTaskBindingCandidate>,
}

/// Returns a stable SHA-256 hash over normalized binding content.
pub fn binding_content_hash(binding: &SourceTaskBindingConfig) -> Result<String> {
    let mut normalized = binding.clone();
    normalized.match_rule.channels.sort();
    normalized.allowed_actor_roles.sort();
    let value = serde_json::to_value(normalized)?;
    crate::action_audit::canonical_request_hash(&value)
}

/// Validates bounded match fields independently of project references.
pub fn validate_binding_config(binding: &SourceTaskBindingConfig) -> Result<()> {
    if binding.trigger_ref.trim().is_empty() {
        bail!("source_task_binding.spec.triggerRef cannot be empty");
    }
    if binding.template_ref.trim().is_empty() {
        bail!("source_task_binding.spec.templateRef cannot be empty");
    }
    if binding.match_rule.event_kind != "reaction_added" {
        bail!("source_task_binding.spec.match.eventKind must be reaction_added");
    }
    if binding.match_rule.target_kind != "message" {
        bail!("source_task_binding.spec.match.targetKind must be message");
    }
    validate_reaction_name(&binding.match_rule.reaction)?;
    if binding.match_rule.channels.is_empty() != binding.match_rule.all_channels {
        bail!(
            "source_task_binding.spec.match requires exactly one of non-empty channels or allChannels=true"
        );
    }
    if binding.match_rule.channels.len() > MAX_CHANNELS {
        bail!(
            "source_task_binding.spec.match.channels exceeds {} entries",
            MAX_CHANNELS
        );
    }
    let mut channels = HashSet::new();
    for channel in &binding.match_rule.channels {
        if channel.is_empty()
            || channel.len() > 128
            || !channel.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
        {
            bail!(
                "source_task_binding.spec.match.channels values must contain 1-128 ASCII alphanumeric, '_', or '-' characters"
            );
        }
        if !channels.insert(channel.as_str()) {
            bail!(
                "source_task_binding.spec.match.channels contains duplicate '{}'",
                channel
            );
        }
    }
    if binding.allowed_actor_roles.is_empty() {
        bail!("source_task_binding.spec.allowedActorRoles cannot be empty");
    }
    let mut roles = HashSet::new();
    for role in &binding.allowed_actor_roles {
        if !matches!(role.as_str(), "read_only" | "operator" | "admin") {
            bail!(
                "source_task_binding.spec.allowedActorRoles values must be read_only, operator, or admin"
            );
        }
        if !roles.insert(role.as_str()) {
            bail!(
                "source_task_binding.spec.allowedActorRoles contains duplicate '{}'",
                role
            );
        }
    }
    Ok(())
}

fn validate_reaction_name(reaction: &str) -> Result<()> {
    if reaction.is_empty()
        || reaction.len() > 128
        || !reaction.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '+' | '-')
        })
    {
        bail!(
            "source_task_binding.spec.match.reaction must contain 1-128 ASCII alphanumeric, '_', '+', or '-' characters"
        );
    }
    Ok(())
}

/// Returns whether two enabled rules can match the same normalized event.
pub fn bindings_overlap(left: &SourceTaskBindingConfig, right: &SourceTaskBindingConfig) -> bool {
    if left.suspend
        || right.suspend
        || left.trigger_ref != right.trigger_ref
        || left.match_rule.event_kind != right.match_rule.event_kind
        || left.match_rule.reaction != right.match_rule.reaction
        || left.match_rule.target_kind != right.match_rule.target_kind
    {
        return false;
    }
    let channels_overlap = left.match_rule.all_channels
        || right.match_rule.all_channels
        || left
            .match_rule
            .channels
            .iter()
            .any(|channel| right.match_rule.channels.contains(channel));
    let roles_overlap = left
        .allowed_actor_roles
        .iter()
        .any(|role| right.allowed_actor_roles.contains(role));
    channels_overlap && roles_overlap
}

/// Resolves one authenticated installation and deterministically matches its bindings.
pub fn match_source_task_binding(
    config: &OrchestratorConfig,
    project_id: &str,
    input: &SourceTaskBindingMatchInput,
) -> Result<SourceTaskBindingMatchResult> {
    let Some(project) = config.projects.get(project_id) else {
        return Ok(empty_result("trigger_not_found"));
    };
    let mut triggers = project
        .triggers
        .iter()
        .filter(|(_, trigger)| {
            trigger
                .event
                .as_ref()
                .and_then(|event| event.webhook.as_ref())
                .is_some_and(|webhook| {
                    webhook.provider.as_deref() == Some(input.provider.as_str())
                        && webhook.installation_id.as_deref()
                            == Some(input.installation_id.as_str())
                })
        })
        .collect::<Vec<_>>();
    triggers.sort_by(|left, right| left.0.cmp(right.0));
    if triggers.is_empty() {
        return Ok(empty_result("trigger_not_found"));
    }
    if triggers.len() > 1 {
        let mut result = empty_result("trigger_ambiguous");
        result.status = "ambiguous".to_string();
        return Ok(result);
    }
    let (trigger_name, trigger) = triggers[0];
    let webhook = trigger
        .event
        .as_ref()
        .and_then(|event| event.webhook.as_ref())
        .ok_or_else(|| anyhow::anyhow!("source trigger webhook config missing"))?;
    if trigger.suspend {
        let mut result = empty_result("trigger_suspended");
        result.trigger_name = Some(trigger_name.clone());
        return Ok(result);
    }
    if webhook.reaction_routing != "bindings" {
        let mut result = empty_result("reaction_automation_disabled");
        result.trigger_name = Some(trigger_name.clone());
        return Ok(result);
    }
    let resolved_role = webhook.actor_roles.get(&input.external_actor_id).cloned();
    let mut bindings = project
        .source_task_bindings
        .iter()
        .filter(|(_, binding)| binding.trigger_ref == *trigger_name)
        .collect::<Vec<_>>();
    bindings.sort_by(|left, right| left.0.cmp(right.0));
    let mut candidates = Vec::with_capacity(bindings.len());
    let mut matches = Vec::new();
    for (binding_id, binding) in bindings {
        let reason = candidate_reason(binding, input, resolved_role.as_deref());
        let revision = binding_content_hash(binding)?;
        if reason == "matched" {
            matches.push((binding_id, binding, revision.clone()));
        }
        candidates.push(SourceTaskBindingCandidate {
            binding_id: binding_id.clone(),
            reason: reason.to_string(),
            revision,
        });
    }
    let base = SourceTaskBindingMatchResult {
        status: "no_match".to_string(),
        reason: if candidates.len() == 1 {
            candidates[0].reason.clone()
        } else if candidates.is_empty() {
            "binding_not_found".to_string()
        } else {
            "binding_no_match".to_string()
        },
        trigger_name: Some(trigger_name.clone()),
        resolved_role,
        binding_id: None,
        template_ref: None,
        binding_revision: None,
        candidates,
    };
    match matches.as_slice() {
        [] => Ok(base),
        [(binding_id, binding, revision)] => Ok(SourceTaskBindingMatchResult {
            status: "matched".to_string(),
            reason: "binding_matched".to_string(),
            binding_id: Some((*binding_id).clone()),
            template_ref: Some(binding.template_ref.clone()),
            binding_revision: Some(revision.clone()),
            ..base
        }),
        _ => Ok(SourceTaskBindingMatchResult {
            status: "ambiguous".to_string(),
            reason: "binding_ambiguous".to_string(),
            ..base
        }),
    }
}

fn candidate_reason(
    binding: &SourceTaskBindingConfig,
    input: &SourceTaskBindingMatchInput,
    resolved_role: Option<&str>,
) -> &'static str {
    if binding.suspend {
        "binding_suspended"
    } else if binding.match_rule.event_kind != input.event_kind {
        "event_kind_mismatch"
    } else if binding.match_rule.reaction != input.reaction {
        "reaction_mismatch"
    } else if binding.match_rule.target_kind != input.target_kind {
        "target_kind_mismatch"
    } else if !binding.match_rule.all_channels
        && !binding.match_rule.channels.contains(&input.channel_id)
    {
        "channel_not_allowed"
    } else if resolved_role.is_none() {
        "actor_unknown"
    } else if !binding
        .allowed_actor_roles
        .iter()
        .any(|role| Some(role.as_str()) == resolved_role)
    {
        "actor_role_not_allowed"
    } else {
        "matched"
    }
}

fn empty_result(reason: &str) -> SourceTaskBindingMatchResult {
    SourceTaskBindingMatchResult {
        status: "no_match".to_string(),
        reason: reason.to_string(),
        trigger_name: None,
        resolved_role: None,
        binding_id: None,
        template_ref: None,
        binding_revision: None,
        candidates: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::ConcurrencyPolicy;
    use crate::config::{
        SourceTaskBindingMatchConfig, TriggerActionConfig, TriggerConfig, TriggerEventConfig,
        TriggerWebhookConfig,
    };
    use std::collections::HashMap;

    fn binding() -> SourceTaskBindingConfig {
        SourceTaskBindingConfig {
            trigger_ref: "slack-main".to_string(),
            match_rule: SourceTaskBindingMatchConfig {
                event_kind: "reaction_added".to_string(),
                reaction: "agent-analyze".to_string(),
                target_kind: "message".to_string(),
                channels: vec!["C01234567".to_string()],
                all_channels: false,
            },
            template_ref: "analyze-from-slack".to_string(),
            allowed_actor_roles: vec!["operator".to_string(), "admin".to_string()],
            suspend: false,
        }
    }

    fn input() -> SourceTaskBindingMatchInput {
        SourceTaskBindingMatchInput {
            provider: "slack".to_string(),
            installation_id: "T012".to_string(),
            event_kind: "reaction_added".to_string(),
            reaction: "agent-analyze".to_string(),
            target_kind: "message".to_string(),
            channel_id: "C01234567".to_string(),
            external_actor_id: "U_OPERATOR".to_string(),
        }
    }

    fn config(reaction_routing: &str) -> OrchestratorConfig {
        let mut config = OrchestratorConfig::default();
        let project = config.ensure_project(Some("demo"));
        project.triggers.insert(
            "slack-main".to_string(),
            TriggerConfig {
                cron: None,
                event: Some(TriggerEventConfig {
                    source: "webhook".to_string(),
                    filter: None,
                    webhook: Some(TriggerWebhookConfig {
                        connection_ref: None,
                        secret: None,
                        outbound_credential: None,
                        signature_header: None,
                        crd_ref: None,
                        provider: Some("slack".to_string()),
                        installation_id: Some("T012".to_string()),
                        actor_roles: HashMap::from([
                            ("U_OPERATOR".to_string(), "operator".to_string()),
                            ("U_READER".to_string(), "read_only".to_string()),
                        ]),
                        reaction_routing: reaction_routing.to_string(),
                        timestamp_tolerance_secs: 300,
                    }),
                    filesystem: None,
                }),
                action: TriggerActionConfig {
                    workflow: "noop".to_string(),
                    workspace: "noop".to_string(),
                    args: None,
                    start: false,
                },
                concurrency_policy: ConcurrencyPolicy::Allow,
                suspend: false,
                history_limit: None,
                throttle: None,
            },
        );
        project
            .source_task_bindings
            .insert("analyze".to_string(), binding());
        config
    }

    #[test]
    fn selects_exactly_one_binding_and_reports_revision() {
        let result = match_source_task_binding(&config("bindings"), "demo", &input()).unwrap();
        assert_eq!(result.status, "matched");
        assert_eq!(result.reason, "binding_matched");
        assert_eq!(result.binding_id.as_deref(), Some("analyze"));
        assert_eq!(result.template_ref.as_deref(), Some("analyze-from-slack"));
        assert_eq!(result.resolved_role.as_deref(), Some("operator"));
        assert_eq!(result.candidates[0].reason, "matched");
        assert_eq!(result.binding_revision.as_deref().map(str::len), Some(64));
    }

    #[test]
    fn secure_default_keeps_reaction_automation_disabled() {
        let result = match_source_task_binding(&config("disabled"), "demo", &input()).unwrap();
        assert_eq!(result.status, "no_match");
        assert_eq!(result.reason, "reaction_automation_disabled");
        assert!(result.binding_id.is_none());
    }

    #[test]
    fn rejects_untrusted_or_unknown_actor_without_accepting_a_caller_role() {
        let mut event = input();
        event.external_actor_id = "U_READER".to_string();
        let result = match_source_task_binding(&config("bindings"), "demo", &event).unwrap();
        assert_eq!(result.reason, "actor_role_not_allowed");
        assert_eq!(result.resolved_role.as_deref(), Some("read_only"));

        event.external_actor_id = "U_UNKNOWN".to_string();
        let result = match_source_task_binding(&config("bindings"), "demo", &event).unwrap();
        assert_eq!(result.reason, "actor_unknown");
        assert!(result.resolved_role.is_none());
    }

    #[test]
    fn mismatch_reasons_are_stable_and_safe() {
        let cases = [
            ("event_kind", "reaction_removed", "event_kind_mismatch"),
            ("reaction", "other", "reaction_mismatch"),
            ("target", "file", "target_kind_mismatch"),
            ("channel", "C_OTHER", "channel_not_allowed"),
        ];
        for (field, value, expected) in cases {
            let mut event = input();
            match field {
                "event_kind" => event.event_kind = value.to_string(),
                "reaction" => event.reaction = value.to_string(),
                "target" => event.target_kind = value.to_string(),
                "channel" => event.channel_id = value.to_string(),
                _ => unreachable!(),
            }
            let result = match_source_task_binding(&config("bindings"), "demo", &event).unwrap();
            assert_eq!(result.status, "no_match");
            assert_eq!(result.reason, expected);
        }
    }

    #[test]
    fn ambiguity_fails_closed_even_if_invalid_config_reaches_runtime() {
        let mut config = config("bindings");
        config
            .project_mut(Some("demo"))
            .unwrap()
            .source_task_bindings
            .insert("duplicate".to_string(), binding());
        let result = match_source_task_binding(&config, "demo", &input()).unwrap();
        assert_eq!(result.status, "ambiguous");
        assert_eq!(result.reason, "binding_ambiguous");
        assert!(result.binding_id.is_none());
        assert_eq!(result.candidates.len(), 2);
    }

    #[test]
    fn content_hash_is_stable_for_set_like_field_order() {
        let original = binding();
        let mut reordered = original.clone();
        reordered.allowed_actor_roles.reverse();
        reordered.match_rule.channels = vec!["C2".to_string(), "C1".to_string()];
        let mut equivalent = original;
        equivalent.match_rule.channels = vec!["C1".to_string(), "C2".to_string()];
        assert_eq!(
            binding_content_hash(&reordered).unwrap(),
            binding_content_hash(&equivalent).unwrap()
        );
    }

    #[test]
    fn concurrent_config_swap_never_mixes_binding_generations() {
        use arc_swap::ArcSwap;
        use std::sync::Arc;

        fn versioned_config(template_ref: &str) -> OrchestratorConfig {
            let mut config = config("bindings");
            config
                .project_mut(Some("demo"))
                .expect("demo project")
                .source_task_bindings
                .get_mut("analyze")
                .expect("binding")
                .template_ref = template_ref.to_string();
            config
        }

        let first = Arc::new(versioned_config("template-v1"));
        let second = Arc::new(versioned_config("template-v2"));
        let first_revision = binding_content_hash(
            first.projects["demo"]
                .source_task_bindings
                .get("analyze")
                .expect("first binding"),
        )
        .expect("first revision");
        let second_revision = binding_content_hash(
            second.projects["demo"]
                .source_task_bindings
                .get("analyze")
                .expect("second binding"),
        )
        .expect("second revision");
        let snapshot = Arc::new(ArcSwap::from(Arc::clone(&first)));

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
                    let matched = match_source_task_binding(&active, "demo", &input())
                        .expect("match immutable snapshot");
                    let pair = (
                        matched.template_ref.as_deref(),
                        matched.binding_revision.as_deref(),
                    );
                    assert!(
                        pair == (Some("template-v1"), Some(first_revision.as_str()))
                            || pair == (Some("template-v2"), Some(second_revision.as_str()))
                    );
                }
            });
        });
    }

    #[test]
    fn overlap_accounts_for_channel_role_and_suspend_boundaries() {
        let left = binding();
        let mut right = binding();
        assert!(bindings_overlap(&left, &right));
        right.match_rule.channels = vec!["C_OTHER".to_string()];
        assert!(!bindings_overlap(&left, &right));
        right.match_rule.all_channels = true;
        right.match_rule.channels.clear();
        assert!(bindings_overlap(&left, &right));
        right.suspend = true;
        assert!(!bindings_overlap(&left, &right));
    }

    #[test]
    fn validation_requires_explicit_channels_and_roles() {
        let mut value = binding();
        value.match_rule.channels.clear();
        assert!(validate_binding_config(&value).is_err());
        value.match_rule.all_channels = true;
        value.allowed_actor_roles.clear();
        assert!(validate_binding_config(&value).is_err());
        value.allowed_actor_roles.push("operator".to_string());
        assert!(validate_binding_config(&value).is_ok());
    }
}
