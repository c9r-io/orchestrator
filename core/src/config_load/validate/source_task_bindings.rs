use crate::config::{OrchestratorConfig, ProjectConfig, SourceTaskBindingConfig};
use anyhow::{Result, bail};

/// Validates every source task binding and its project-local references.
pub fn validate_source_task_bindings(config: &OrchestratorConfig) -> Result<()> {
    for project_id in config.projects.keys() {
        validate_source_task_bindings_for_project(config, project_id)?;
    }
    Ok(())
}

/// Validates source task bindings belonging to one project.
pub fn validate_source_task_bindings_for_project(
    config: &OrchestratorConfig,
    project_id: &str,
) -> Result<()> {
    let Some(project) = config.projects.get(project_id) else {
        bail!("project '{project_id}' not found");
    };
    for (name, binding) in &project.source_task_bindings {
        validate_one(project_id, name, binding, project)?;
    }
    let mut enabled = project
        .source_task_bindings
        .iter()
        .filter(|(_, binding)| !binding.suspend)
        .collect::<Vec<_>>();
    enabled.sort_by(|left, right| left.0.cmp(right.0));
    for (index, (left_name, left)) in enabled.iter().enumerate() {
        for (right_name, right) in enabled.iter().skip(index + 1) {
            if crate::source_task_binding::bindings_overlap(left, right) {
                bail!(
                    "SourceTaskBinding '{project_id}/{left_name}' overlaps enabled binding '{project_id}/{right_name}'; explicit precedence is not supported"
                );
            }
        }
    }
    Ok(())
}

fn validate_one(
    project_id: &str,
    name: &str,
    binding: &SourceTaskBindingConfig,
    project: &ProjectConfig,
) -> Result<()> {
    crate::source_task_binding::validate_binding_config(binding).map_err(|error| {
        anyhow::anyhow!("SourceTaskBinding '{project_id}/{name}' is invalid: {error}")
    })?;
    let trigger = project.triggers.get(&binding.trigger_ref).ok_or_else(|| {
        anyhow::anyhow!(
            "SourceTaskBinding '{}/{}' references Trigger '{}' which does not exist in project '{}'",
            project_id,
            name,
            binding.trigger_ref,
            project_id
        )
    })?;
    let webhook = trigger
        .event
        .as_ref()
        .filter(|event| event.source == "webhook")
        .and_then(|event| event.webhook.as_ref())
        .filter(|webhook| webhook.provider.as_deref() == Some("slack"))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "SourceTaskBinding '{}/{}' triggerRef '{}' must reference a Slack webhook Trigger",
                project_id,
                name,
                binding.trigger_ref
            )
        })?;
    if webhook.installation_id.as_deref().is_none_or(str::is_empty) {
        bail!(
            "SourceTaskBinding '{}/{}' triggerRef '{}' has no installationId",
            project_id,
            name,
            binding.trigger_ref
        );
    }
    if webhook.reaction_routing == "bindings" && webhook.connection_ref.is_none() {
        let credential = webhook.outbound_credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "SourceTaskBinding '{}/{}' triggerRef '{}' has no outboundCredential",
                project_id,
                name,
                binding.trigger_ref
            )
        })?;
        let store = project.secret_stores.get(&credential.from_ref).ok_or_else(|| {
                anyhow::anyhow!(
                    "SourceTaskBinding '{}/{}' outbound credential SecretStore '{}' does not exist in project '{}'",
                    project_id,
                    name,
                    credential.from_ref,
                    project_id
                )
        })?;
        if !store.data.contains_key(&credential.key) {
            bail!(
                "SourceTaskBinding '{}/{}' outbound credential key '{}' does not exist in SecretStore '{}'",
                project_id,
                name,
                credential.key,
                credential.from_ref
            );
        }
    }
    if !project
        .source_task_templates
        .contains_key(&binding.template_ref)
    {
        bail!(
            "SourceTaskBinding '{}/{}' references SourceTaskTemplate '{}' which does not exist in project '{}'",
            project_id,
            name,
            binding.template_ref,
            project_id
        );
    }
    if !webhook
        .actor_roles
        .values()
        .any(|role| binding.allowed_actor_roles.contains(role))
    {
        bail!(
            "SourceTaskBinding '{}/{}' allowedActorRoles are unreachable from Trigger '{}' actorRoles",
            project_id,
            name,
            binding.trigger_ref
        );
    }
    Ok(())
}
