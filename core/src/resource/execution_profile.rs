use crate::cli_types::{ExecutionProfileSpec, OrchestratorResource, ResourceKind, ResourceSpec};
use crate::config::{ExecutionProfileConfig, OrchestratorConfig};
use anyhow::{anyhow, Result};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
pub struct ExecutionProfileResource {
    pub metadata: ResourceMetadata,
    pub spec: ExecutionProfileSpec,
}

impl Resource for ExecutionProfileResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::ExecutionProfile
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;
        if self.spec.mode == "host" && self.spec.fs_mode != "inherit" {
            return Err(anyhow!(
                "executionprofile.spec.fs_mode is only valid when mode=sandbox"
            ));
        }
        if self.spec.network_mode == "allowlist" && self.spec.network_allowlist.is_empty() {
            return Err(anyhow!(
                "executionprofile.spec.network_allowlist cannot be empty when network_mode=allowlist"
            ));
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        let mut metadata = self.metadata.clone();
        metadata.project = Some(
            config
                .effective_project_id(metadata.project.as_deref())
                .to_string(),
        );
        Ok(super::apply_to_store(
            config,
            "ExecutionProfile",
            self.name(),
            &metadata,
            serde_json::to_value(&self.spec)?,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::ExecutionProfile,
            &self.metadata,
            ResourceSpec::ExecutionProfile(self.spec.clone()),
        )
    }

    fn get_from_project(
        config: &OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> Option<Self> {
        config
            .project(project_id)?
            .execution_profiles
            .get(name)
            .map(|profile| Self {
                metadata: super::metadata_from_store(config, "ExecutionProfile", name, project_id),
                spec: execution_profile_config_to_spec(profile),
            })
    }

    fn delete_from_project(
        config: &mut OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> bool {
        super::helpers::delete_from_store_project(config, "ExecutionProfile", name, project_id)
    }
}

pub(super) fn build_execution_profile(
    resource: OrchestratorResource,
) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::ExecutionProfile {
        return Err(anyhow!("resource kind/spec mismatch for ExecutionProfile"));
    }
    match spec {
        ResourceSpec::ExecutionProfile(spec) => Ok(RegisteredResource::ExecutionProfile(
            ExecutionProfileResource { metadata, spec },
        )),
        _ => Err(anyhow!("resource kind/spec mismatch for ExecutionProfile")),
    }
}

pub(crate) fn execution_profile_spec_to_config(
    spec: &ExecutionProfileSpec,
) -> ExecutionProfileConfig {
    ExecutionProfileConfig {
        mode: match spec.mode.as_str() {
            "sandbox" => crate::config::ExecutionProfileMode::Sandbox,
            _ => crate::config::ExecutionProfileMode::Host,
        },
        fs_mode: match spec.fs_mode.as_str() {
            "workspace_readonly" => crate::config::ExecutionFsMode::WorkspaceReadonly,
            "workspace_rw_scoped" => crate::config::ExecutionFsMode::WorkspaceRwScoped,
            _ => crate::config::ExecutionFsMode::Inherit,
        },
        writable_paths: spec.writable_paths.clone(),
        network_mode: match spec.network_mode.as_str() {
            "deny" => crate::config::ExecutionNetworkMode::Deny,
            "allowlist" => crate::config::ExecutionNetworkMode::Allowlist,
            _ => crate::config::ExecutionNetworkMode::Inherit,
        },
        network_allowlist: spec.network_allowlist.clone(),
        max_memory_mb: spec.max_memory_mb,
        max_cpu_seconds: spec.max_cpu_seconds,
        max_processes: spec.max_processes,
        max_open_files: spec.max_open_files,
    }
}

pub(crate) fn execution_profile_config_to_spec(
    config: &ExecutionProfileConfig,
) -> ExecutionProfileSpec {
    ExecutionProfileSpec {
        mode: match config.mode {
            crate::config::ExecutionProfileMode::Host => "host".to_string(),
            crate::config::ExecutionProfileMode::Sandbox => "sandbox".to_string(),
        },
        fs_mode: match config.fs_mode {
            crate::config::ExecutionFsMode::Inherit => "inherit".to_string(),
            crate::config::ExecutionFsMode::WorkspaceReadonly => "workspace_readonly".to_string(),
            crate::config::ExecutionFsMode::WorkspaceRwScoped => "workspace_rw_scoped".to_string(),
        },
        writable_paths: config.writable_paths.clone(),
        network_mode: match config.network_mode {
            crate::config::ExecutionNetworkMode::Inherit => "inherit".to_string(),
            crate::config::ExecutionNetworkMode::Deny => "deny".to_string(),
            crate::config::ExecutionNetworkMode::Allowlist => "allowlist".to_string(),
        },
        network_allowlist: config.network_allowlist.clone(),
        max_memory_mb: config.max_memory_mb,
        max_cpu_seconds: config.max_cpu_seconds,
        max_processes: config.max_processes,
        max_open_files: config.max_open_files,
    }
}
