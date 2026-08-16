use crate::cli_types::ResourceKind;
use crate::config_load::{ResourceRemoval, persist_config_for_delete, read_active_config};
use crate::error::{OrchestratorError, Result, classify_resource_error};
use crate::state::InnerState;

/// Maps a persistence failure raised while deleting resources onto the canonical
/// error type.
///
/// `external_dependency`, deliberately, and not `classify_resource_error`.
/// Until FR-130 Phase C, a blanket `From` impl for the SQLite driver's error type
/// categorised every driver failure as `ExternalDependency`, and the category is
/// part of the gRPC and CLI contract rather than an internal detail. The
/// message-based classifier reads "not found" anywhere in the text as
/// `NotFound`, and SQLite's word for a missing table is
/// `no such table: resources` — the same failure, a different category, and no
/// compile error to notice it. `phase_c_preserves_the_external_dependency_category`
/// below pins it against exactly that message.
fn classify_resource_persistence_error(error: anyhow::Error) -> OrchestratorError {
    OrchestratorError::external_dependency("resource.delete", error)
}

/// Delete a resource by kind/name.
pub fn delete_resource(
    state: &InnerState,
    resource: &str,
    force: bool,
    project: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    delete_resource_with_references(state, resource, force, project, dry_run, false)
}

/// Deletes a resource and, when explicitly authorized by the caller, its binding references.
pub fn delete_resource_with_references(
    state: &InnerState,
    resource: &str,
    force: bool,
    project: Option<&str>,
    dry_run: bool,
    force_references: bool,
) -> Result<()> {
    let parts: Vec<&str> = resource.split('/').collect();
    if parts.len() != 2 {
        return Err(classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("invalid resource format: {resource} (use kind/name)"),
        ));
    }
    let (kind, name) = (parts[0], parts[1]);

    if !force {
        return Err(classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("use --force to confirm deletion of {kind}/{name}"),
        ));
    }

    let config = {
        let active = read_active_config(state)?;
        active.config.clone()
    };

    let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let is_source_template = matches!(
        kind,
        "sourcetasktemplate" | "source-task-template" | "source_task_template" | "stt"
    );
    let is_trigger = matches!(kind, "trigger" | "tg");
    if force_references && !is_source_template && !is_trigger {
        return Err(classify_resource_error(
            "resource.delete",
            anyhow::anyhow!(
                "--force-references is only valid for SourceTaskTemplate or Trigger deletion"
            ),
        ));
    }
    let binding_references = if is_source_template || is_trigger {
        source_task_binding_references(&config, project_id, name, is_trigger)
    } else {
        Vec::new()
    };
    if !binding_references.is_empty() && !force_references {
        return Err(crate::error::OrchestratorError::invalid_state(
            "resource.delete",
            anyhow::anyhow!(
                "{} '{}/{}' is referenced by SourceTaskBinding(s): {}; use --force-references with Admin authorization to remove them atomically",
                if is_trigger {
                    "Trigger"
                } else {
                    "SourceTaskTemplate"
                },
                project_id,
                name,
                binding_references.join(", ")
            ),
        ));
    }

    if dry_run {
        if kind == "project" {
            if config.projects.contains_key(name) {
                return Ok(());
            } else {
                return Err(classify_resource_error(
                    "resource.delete",
                    anyhow::anyhow!("project '{name}' not found"),
                ));
            }
        }
        // CRD dry-run check
        if kind == "crd" || kind == "customresourcedefinition" {
            if config.custom_resource_definitions.contains_key(name) {
                return Ok(());
            } else {
                return Err(classify_resource_error(
                    "resource.delete",
                    anyhow::anyhow!("CRD '{name}' not found"),
                ));
            }
        }
        // Custom resource dry-run check (skip kinds with dedicated ProjectConfig projections)
        if let Some(crd) = crate::crd::resolve::find_crd_by_kind_or_alias(&config, kind)
            && !crate::crd::resolve::is_builtin_kind(&crd.kind)
        {
            let storage_key = format!("{}/{}", crd.kind, name);
            if config.custom_resources.contains_key(&storage_key) {
                return Ok(());
            } else {
                return Err(classify_resource_error(
                    "resource.delete",
                    anyhow::anyhow!("{}/{} not found", crd.kind, name),
                ));
            }
        }
        let proj_cfg = match config.projects.get(project_id) {
            Some(p) => p,
            None => {
                return Err(classify_resource_error(
                    "resource.delete",
                    anyhow::anyhow!("{kind}/{name} not found in project '{project_id}'"),
                ));
            }
        };
        // Dispatches on the resolved kind for the same reason
        // `delete_resource_from_project` does, and carried the same defect: the
        // SecretStore aliases looked in `env_stores`, so a dry run reported a
        // real SecretStore as absent. A dry run that disagrees with the delete it
        // previews is worse than no dry run, so the two must read the same map —
        // the wildcard-free match is what keeps them reading it.
        let exists = match crate::resource::resource_kind_from_alias(kind) {
            Some(ResourceKind::Workspace) => proj_cfg.workspaces.contains_key(name),
            Some(ResourceKind::Agent) => proj_cfg.agents.contains_key(name),
            Some(ResourceKind::Workflow) => proj_cfg.workflows.contains_key(name),
            Some(ResourceKind::StepTemplate) => proj_cfg.step_templates.contains_key(name),
            Some(ResourceKind::SourceTaskTemplate) => {
                proj_cfg.source_task_templates.contains_key(name)
            }
            Some(ResourceKind::SourceTaskBinding) => {
                proj_cfg.source_task_bindings.contains_key(name)
            }
            Some(ResourceKind::ExecutionProfile) => proj_cfg.execution_profiles.contains_key(name),
            Some(ResourceKind::EnvStore) => proj_cfg.env_stores.contains_key(name),
            Some(ResourceKind::SecretStore) => proj_cfg.secret_stores.contains_key(name),
            Some(ResourceKind::Trigger) => proj_cfg.triggers.contains_key(name),
            Some(ResourceKind::Project | ResourceKind::RuntimePolicy) | None => false,
        };
        if !exists {
            return Err(classify_resource_error(
                "resource.delete",
                anyhow::anyhow!("{kind}/{name} not found in project '{project_id}'"),
            ));
        }
        return Ok(());
    }

    let mut config = config;
    if force_references {
        remove_source_task_binding_references(&mut config, project_id, name, is_trigger);
    }

    // Handle CRD and custom resource deletion (not project-scoped)
    if kind == "crd" || kind == "customresourcedefinition" {
        let deleted = crate::crd::delete_crd(&mut config, name)?;
        if !deleted {
            return Err(classify_resource_error(
                "resource.delete",
                anyhow::anyhow!("CRD '{name}' not found"),
            ));
        }
        persist_config_for_delete(state, config, "daemon-delete", &[])?;
        crate::trigger_engine::notify_trigger_reload(state);
        return Ok(());
    }

    if let Some(crd) = crate::crd::resolve::find_crd_by_kind_or_alias(&config, kind)
        && !crate::crd::resolve::is_builtin_kind(&crd.kind)
    {
        let crd_kind = crd.kind.clone();
        let deleted = crate::crd::delete_custom_resource(&mut config, &crd_kind, name)?;
        if !deleted {
            return Err(classify_resource_error(
                "resource.delete",
                anyhow::anyhow!("{crd_kind}/{name} not found"),
            ));
        }
        persist_config_for_delete(state, config, "daemon-delete", &[])?;
        crate::trigger_engine::notify_trigger_reload(state);
        return Ok(());
    }

    if kind == "project" {
        // 1. Clear task data (tasks, items, runs, events)
        let _stats = crate::db::reset_project_data(state, name)?;

        // 2. Clean auto-ticket files from project workspaces
        let mut _tickets_cleaned: u64 = 0;
        if let Some(project_cfg) = config.projects.get(name) {
            for ws_config in project_cfg.workspaces.values() {
                let ticket_path = state
                    .data_dir
                    .join(&ws_config.root_path)
                    .join(&ws_config.ticket_dir);
                if ticket_path.is_dir()
                    && let Ok(entries) = std::fs::read_dir(&ticket_path)
                {
                    for entry in entries.flatten() {
                        let fname = entry.file_name();
                        let fname_str = fname.to_string_lossy();
                        if fname_str.starts_with("auto_")
                            && fname_str.ends_with(".md")
                            && std::fs::remove_file(entry.path()).is_ok()
                        {
                            _tickets_cleaned += 1;
                        }
                    }
                }
            }
        }

        // 3. Remove project config entry and resource_store entries
        config.projects.remove(name);
        config.resource_store.remove_all_for_project(name);

        // 4. Also remove resource DB rows for this project
        crate::db::delete_project_resources(&state.db_path, name)
            .map_err(classify_resource_persistence_error)?;

        // 5. Persist (using delete-safe path)
        persist_config_for_delete(state, config, "project-delete", &[])?;
        crate::trigger_engine::notify_trigger_reload(state);
        return Ok(());
    }

    let project_id = project.unwrap_or(crate::config::DEFAULT_PROJECT_ID);
    let proj_cfg = config.projects.get_mut(project_id).ok_or_else(|| {
        classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("project not found: {project_id}"),
        )
    })?;
    let canonical_kind = canonical_project_kind(kind)?;
    let deleted = delete_resource_from_project(proj_cfg, kind, name)?;
    if !deleted {
        return Err(classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("{kind}/{name} not found in project '{project_id}'"),
        ));
    }
    let mut deleted_resources = vec![ResourceRemoval {
        kind: canonical_kind.to_string(),
        project_id: project_id.to_string(),
        name: name.to_string(),
    }];
    if force_references {
        deleted_resources.extend(binding_references.into_iter().map(|binding_name| {
            ResourceRemoval {
                kind: "SourceTaskBinding".to_string(),
                project_id: project_id.to_string(),
                name: binding_name,
            }
        }));
    }
    persist_config_for_delete(state, config, "daemon-delete", &deleted_resources)?;
    crate::trigger_engine::notify_trigger_reload(state);
    Ok(())
}

fn source_task_binding_references(
    config: &crate::config::OrchestratorConfig,
    project_id: &str,
    referenced_name: &str,
    trigger_reference: bool,
) -> Vec<String> {
    let field = if trigger_reference {
        "triggerRef"
    } else {
        "templateRef"
    };
    let snake_field = if trigger_reference {
        "trigger_ref"
    } else {
        "template_ref"
    };
    let mut names: Vec<String> = config
        .projects
        .get(project_id)
        .into_iter()
        .flat_map(|project| project.source_task_bindings.iter())
        .filter(|(_, binding)| {
            if trigger_reference {
                binding.trigger_ref == referenced_name
            } else {
                binding.template_ref == referenced_name
            }
        })
        .map(|(name, _)| name.clone())
        .collect();
    names.extend(
        config
            .resource_store
            .list_by_kind_for_project("SourceTaskBinding", project_id)
            .into_iter()
            .filter(|resource| {
                resource
                    .spec
                    .get(field)
                    .or_else(|| resource.spec.get(snake_field))
                    .and_then(serde_json::Value::as_str)
                    == Some(referenced_name)
            })
            .map(|resource| resource.metadata.name.clone()),
    );
    for resource in config.custom_resources.values() {
        if resource.kind == "SourceTaskBinding"
            && resource.metadata.project.as_deref() == Some(project_id)
            && resource
                .spec
                .get(field)
                .or_else(|| resource.spec.get(snake_field))
                .and_then(serde_json::Value::as_str)
                == Some(referenced_name)
            && !names.contains(&resource.metadata.name)
        {
            names.push(resource.metadata.name.clone());
        }
    }
    names.sort();
    names.dedup();
    names
}

fn remove_source_task_binding_references(
    config: &mut crate::config::OrchestratorConfig,
    project_id: &str,
    referenced_name: &str,
    trigger_reference: bool,
) {
    let names =
        source_task_binding_references(config, project_id, referenced_name, trigger_reference);
    if let Some(project) = config.projects.get_mut(project_id) {
        for name in &names {
            project.source_task_bindings.remove(name);
        }
    }
    for name in &names {
        config
            .resource_store
            .remove_namespaced("SourceTaskBinding", project_id, name);
    }
    config.custom_resources.retain(|_, resource| {
        !(resource.kind == "SourceTaskBinding"
            && resource.metadata.project.as_deref() == Some(project_id)
            && names.contains(&resource.metadata.name))
    });
}

/// Removes one project-scoped resource, dispatching on the resolved kind.
///
/// The match is exhaustive and wildcard-free: a thirteenth `ResourceKind` must
/// fail to compile here rather than fall into a catch-all that reports "not
/// found" for a kind nobody wired up.
///
/// **The `SecretStore` arm is a fix, not a transcription.** The string-keyed
/// version this replaced routed `secretstore | secret-store | secret_store` into
/// `proj.env_stores`, alongside the EnvStore aliases — the two kinds have
/// separate maps (`ProjectConfig::env_stores` and `ProjectConfig::secret_stores`),
/// so a SecretStore delete searched the wrong one. It reported `SecretStore/x not
/// found in project` for a store that existed, which made SecretStores
/// undeletable; and when an EnvStore happened to share the name it removed *that*
/// instead, while `canonical_project_kind` returned `SecretStore` so the tombstone
/// row, the deletion guard and the resource-row delete all named a kind that had
/// not been touched. Nothing observed it because no test deleted a SecretStore —
/// FR-167 is the first to assert on one, and DD-182 had meanwhile written down
/// "moving a value between the two is a delete and re-apply", advice the code
/// could not carry out.
pub(super) fn delete_resource_from_project(
    proj: &mut crate::config::ProjectConfig,
    kind: &str,
    name: &str,
) -> Result<bool> {
    use crate::cli_types::ResourceKind;

    let unknown = || {
        classify_resource_error(
            "resource.delete",
            anyhow::anyhow!("unknown resource type for project delete: {kind}"),
        )
    };
    match crate::resource::resource_kind_from_alias(kind).ok_or_else(unknown)? {
        ResourceKind::Workspace => Ok(proj.workspaces.remove(name).is_some()),
        ResourceKind::Agent => Ok(proj.agents.remove(name).is_some()),
        ResourceKind::Workflow => Ok(proj.workflows.remove(name).is_some()),
        ResourceKind::StepTemplate => Ok(proj.step_templates.remove(name).is_some()),
        ResourceKind::SourceTaskTemplate => Ok(proj.source_task_templates.remove(name).is_some()),
        ResourceKind::SourceTaskBinding => Ok(proj.source_task_bindings.remove(name).is_some()),
        ResourceKind::ExecutionProfile => Ok(proj.execution_profiles.remove(name).is_some()),
        ResourceKind::EnvStore => Ok(proj.env_stores.remove(name).is_some()),
        ResourceKind::SecretStore => Ok(proj.secret_stores.remove(name).is_some()),
        ResourceKind::Trigger => Ok(proj.triggers.remove(name).is_some()),
        // Handled before this point (`Project`) or not deletable at all
        // (`RuntimePolicy` has no `ProjectConfig` map). Named rather than left to
        // a wildcard so a new variant cannot join them by accident.
        ResourceKind::Project | ResourceKind::RuntimePolicy => Err(unknown()),
    }
}

/// Canonical manifest name for a project-scoped delete target.
///
/// Delegates to the single alias table in `resource::parse` rather than carrying
/// its own copy. FR-167 made the delete path's audit action name derive from that
/// same table, and two independent tables would fail in one direction silently:
/// an alias accepted here but absent there deletes the resource and records the
/// generic `resource.delete`, with nothing in any log to say a kind went unnamed.
///
/// `Project` and `RuntimePolicy` are excluded deliberately, and the exclusion is
/// not symmetric. A `Project` delete is handled earlier in
/// [`delete_resource_with_references`] and never reaches here. A `RuntimePolicy`
/// delete reaches here and is refused: there is no `ProjectConfig` map to remove
/// it from, so the kind is not deletable at all. The audit row is still reserved
/// before execution, so the attempt is recorded as
/// `resource.runtime_policy.delete` with `status = failed`.
pub(super) fn canonical_project_kind(kind: &str) -> Result<&'static str> {
    crate::resource::resource_kind_from_alias(kind)
        .filter(|resolved| {
            !matches!(
                resolved,
                ResourceKind::Project | ResourceKind::RuntimePolicy
            )
        })
        .map(crate::resource::kind_canonical_name)
        .ok_or_else(|| {
            classify_resource_error(
                "resource.delete",
                anyhow::anyhow!("unknown resource type for project delete: {kind}"),
            )
        })
}

#[cfg(test)]
mod source_template_reference_tests {
    use super::*;
    use crate::cli_types::ResourceMetadata;
    use crate::crd::types::CustomResource;

    #[test]
    fn finds_and_removes_future_shaped_binding_references() {
        let mut config = crate::config::OrchestratorConfig::default();
        config.resource_store.put(CustomResource {
            kind: "SourceTaskBinding".to_string(),
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: ResourceMetadata {
                name: "slack-docs".to_string(),
                project: Some("alpha".to_string()),
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({
                "templateRef": "docs",
                "suspend": false,
            }),
            generation: 1,
            created_at: "2026-07-17T00:00:00Z".to_string(),
            updated_at: "2026-07-17T00:00:00Z".to_string(),
        });

        assert_eq!(
            source_task_binding_references(&config, "alpha", "docs", false),
            vec!["slack-docs"]
        );
        remove_source_task_binding_references(&mut config, "alpha", "docs", false);
        assert!(source_task_binding_references(&config, "alpha", "docs", false).is_empty());
    }
    /// FR-130 Phase C parity: a driver failure surfacing from resource deletion
    /// must still be `ExternalDependency`, as the deleted blanket `From` impl for
    /// the driver's error type guaranteed.
    ///
    /// The error is a real one from a real unmigrated database rather than a
    /// hand-written `anyhow!`, because the message is the whole point: SQLite
    /// reports `no such table: resources`, and the message-based classifier this
    /// call site must not use would read that as `NotFound`. Asserting through
    /// the production mapping function rather than restating it is what stops the
    /// test staying green while the call site drifts.
    #[test]
    fn phase_c_preserves_the_external_dependency_category() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = temp.path().join("unmigrated.db");

        let failure = crate::db::delete_project_resources(&db_path, "any-project")
            .expect_err("deleting from an unmigrated database must fail");
        assert!(
            failure.to_string().contains("no such table"),
            "the fixture no longer produces the message this test is about: {failure}"
        );

        let mapped = classify_resource_persistence_error(failure);
        assert_eq!(
            mapped.category(),
            crate::error::ErrorCategory::ExternalDependency
        );
        assert_eq!(mapped.operation(), "resource.delete");
    }
}

/// The one-table invariant FR-167 rests on.
///
/// The audit action name a delete records is derived from
/// `resource::resource_kind_from_alias`. If the two sides of the delete path
/// disagreed about what a kind string means, the disagreement would not raise
/// anything: the resource would be removed and the row would carry the generic
/// name. These assertions are derived from `kind_aliases` rather than listing
/// strings, so a future alias joins them without anyone remembering to.
#[cfg(test)]
mod alias_table_is_single_sourced {
    use super::*;
    use crate::cli_types::ResourceKind;
    use crate::resource::{ALL_RESOURCE_KINDS, kind_aliases};

    /// Kinds that a project-scoped delete can name. `Project` short-circuits
    /// earlier in `delete_resource_with_references`; `RuntimePolicy` has no
    /// `ProjectConfig` map and is refused.
    fn project_scoped() -> Vec<ResourceKind> {
        ALL_RESOURCE_KINDS
            .into_iter()
            .filter(|kind| !matches!(kind, ResourceKind::Project | ResourceKind::RuntimePolicy))
            .collect()
    }

    #[test]
    fn every_alias_of_a_project_scoped_kind_is_accepted_by_both_halves() {
        let mut config = crate::config::ProjectConfig::default();
        for kind in project_scoped() {
            for alias in kind_aliases(kind) {
                assert_eq!(
                    canonical_project_kind(alias).expect("alias must resolve"),
                    crate::resource::kind_canonical_name(kind),
                    "canonical_project_kind disagrees about {alias}"
                );
                // The removal half must recognise the same alias. `false` here
                // means "recognised, nothing to remove" — an `Err` would mean
                // the two halves disagree, which is the failure under test.
                let removed = delete_resource_from_project(&mut config, alias, "absent-fixture")
                    .unwrap_or_else(|error| {
                        panic!(
                            "delete_resource_from_project rejects {alias}, which canonical_project_kind accepts: {error}"
                        )
                    });
                assert!(!removed, "the empty fixture reported a removal for {alias}");
            }
        }
    }

    /// The two kinds outside the project-scoped set are refused, and refused
    /// with the diagnostic callers already see. The message is asserted rather
    /// than only the error, because `canonical_project_kind` now shares its
    /// resolution with kinds that *are* accepted.
    #[test]
    fn project_and_runtime_policy_are_refused_by_name() {
        for kind in [ResourceKind::Project, ResourceKind::RuntimePolicy] {
            for alias in kind_aliases(kind) {
                let error = canonical_project_kind(alias).expect_err(&format!(
                    "{alias} must not be deletable as a project-scoped kind"
                ));
                assert!(
                    error.to_string().contains(&format!(
                        "unknown resource type for project delete: {alias}"
                    )),
                    "unexpected diagnostic for {alias}: {error}"
                );
            }
        }
    }

    #[test]
    fn unresolvable_kinds_keep_their_diagnostic() {
        let error = canonical_project_kind("promptlibrary")
            .expect_err("a CRD-defined kind is not a project-scoped builtin");
        assert!(
            error
                .to_string()
                .contains("unknown resource type for project delete: promptlibrary"),
            "unexpected diagnostic: {error}"
        );
    }
}
