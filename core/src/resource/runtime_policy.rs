use crate::cli_types::{
    OrchestratorResource, ResourceKind, ResourceSpec, ResumeSpec, RunnerSpec, RuntimePolicySpec,
};
use crate::config::{
    OrchestratorConfig, ResumeConfig, RunnerConfig, RunnerExecutorKind, RunnerPolicy,
};
use anyhow::{anyhow, Result};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
/// Builtin manifest adapter for the global `RuntimePolicy` singleton.
pub struct RuntimePolicyResource {
    /// Resource metadata from the manifest.
    pub metadata: ResourceMetadata,
    /// Manifest spec payload for runtime policy.
    pub spec: RuntimePolicySpec,
}

impl Resource for RuntimePolicyResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::RuntimePolicy
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;
        if self.spec.runner.policy == "allowlist" {
            let mut errors = Vec::new();
            if self.spec.runner.allowed_shells.is_empty() {
                errors.push("runner.allowed_shells cannot be empty when policy=allowlist");
            }
            if self.spec.runner.allowed_shell_args.is_empty() {
                errors.push("runner.allowed_shell_args cannot be empty when policy=allowlist");
            }
            if !errors.is_empty() {
                return Err(anyhow!(errors.join("; ")));
            }
        }
        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        use crate::crd::projection::{CrdProjectable, RuntimePolicyProjection};
        let incoming_runner = runner_spec_to_config(&self.spec.runner);
        let incoming_resume = ResumeConfig {
            auto: self.spec.resume.auto,
        };
        let rp = RuntimePolicyProjection {
            runner: incoming_runner,
            resume: incoming_resume,
            observability: crate::config::ObservabilityConfig::default(),
        };
        let spec_value = rp.to_cr_spec();
        Ok(super::apply_to_store(
            config,
            "RuntimePolicy",
            "runtime",
            &self.metadata,
            spec_value,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::RuntimePolicy,
            &self.metadata,
            ResourceSpec::RuntimePolicy(self.spec.clone()),
        )
    }

    fn get_from_project(
        config: &OrchestratorConfig,
        _name: &str,
        _project_id: Option<&str>,
    ) -> Option<Self> {
        // RuntimePolicy is a global singleton, not scoped to a project.
        use crate::config_ext::OrchestratorConfigExt as _;
        let rp = config.runtime_policy();
        Some(Self {
            metadata: super::metadata_with_name("runtime"),
            spec: RuntimePolicySpec {
                runner: runner_config_to_spec(&rp.runner),
                resume: ResumeSpec {
                    auto: rp.resume.auto,
                },
                observability: serde_json::to_value(&rp.observability).ok(),
            },
        })
    }

    fn delete_from_project(
        _config: &mut OrchestratorConfig,
        _name: &str,
        _project_id: Option<&str>,
    ) -> bool {
        // RuntimePolicy cannot be deleted.
        false
    }
}

/// Builds a typed `RuntimePolicyResource` from a generic manifest wrapper.
pub(super) fn build_runtime_policy(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::RuntimePolicy {
        return Err(anyhow!("resource kind/spec mismatch for RuntimePolicy"));
    }
    match spec {
        ResourceSpec::RuntimePolicy(spec) => {
            Ok(RegisteredResource::RuntimePolicy(RuntimePolicyResource {
                metadata,
                spec,
            }))
        }
        _ => Err(anyhow!("resource kind/spec mismatch for RuntimePolicy")),
    }
}

/// Converts a runtime-policy manifest runner spec into runtime config.
pub(crate) fn runner_spec_to_config(spec: &RunnerSpec) -> RunnerConfig {
    RunnerConfig {
        shell: spec.shell.clone(),
        shell_arg: spec.shell_arg.clone(),
        policy: match spec.policy.as_str() {
            "unsafe" => RunnerPolicy::Unsafe,
            _ => RunnerPolicy::Allowlist,
        },
        executor: match spec.executor.as_str() {
            "shell" => RunnerExecutorKind::Shell,
            _ => RunnerExecutorKind::Shell,
        },
        allowed_shells: spec.allowed_shells.clone(),
        allowed_shell_args: spec.allowed_shell_args.clone(),
        env_allowlist: spec.env_allowlist.clone(),
        redaction_patterns: spec.redaction_patterns.clone(),
    }
}

/// Converts runtime runner config into its manifest spec representation.
pub(crate) fn runner_config_to_spec(config: &RunnerConfig) -> RunnerSpec {
    RunnerSpec {
        shell: config.shell.clone(),
        shell_arg: config.shell_arg.clone(),
        policy: match config.policy {
            RunnerPolicy::Unsafe => "unsafe".to_string(),
            RunnerPolicy::Allowlist => "allowlist".to_string(),
        },
        executor: match config.executor {
            RunnerExecutorKind::Shell => "shell".to_string(),
        },
        allowed_shells: config.allowed_shells.clone(),
        allowed_shell_args: config.allowed_shell_args.clone(),
        env_allowlist: config.env_allowlist.clone(),
        redaction_patterns: config.redaction_patterns.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::{ProjectSpec, ResourceMetadata, ResourceSpec};
    use crate::resource::{dispatch_resource, API_VERSION};

    use super::super::test_fixtures::{make_config, runtime_policy_manifest};

    #[test]
    fn runtime_policy_dispatch_and_kind() {
        let resource =
            dispatch_resource(runtime_policy_manifest()).expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::RuntimePolicy);
        assert_eq!(resource.name(), "runtime");
    }

    #[test]
    fn runtime_policy_apply_unchanged_when_same() {
        let mut config = make_config();
        let r1 = dispatch_resource(runtime_policy_manifest()).expect("dispatch should succeed");
        r1.apply(&mut config).expect("apply");
        assert_eq!(
            r1.apply(&mut config).expect("apply"),
            ApplyResult::Unchanged
        );
    }

    #[test]
    fn runtime_policy_apply_configured_on_change() {
        let mut config = make_config();
        let r1 = dispatch_resource(runtime_policy_manifest()).expect("dispatch should succeed");
        r1.apply(&mut config).expect("apply");

        // Change the runner policy
        let mut manifest = runtime_policy_manifest();
        if let ResourceSpec::RuntimePolicy(ref mut spec) = manifest.spec {
            spec.runner.policy = "allowlist".to_string();
        }
        let r2 = dispatch_resource(manifest).expect("dispatch should succeed");
        assert_eq!(
            r2.apply(&mut config).expect("apply"),
            ApplyResult::Configured
        );
    }

    #[test]
    fn runtime_policy_get_from_always_returns_some() {
        let config = make_config();
        let loaded = RuntimePolicyResource::get_from(&config, "runtime");
        let loaded = loaded.expect("runtime policy should always be present");
        assert_eq!(loaded.metadata.name, "runtime");
    }

    #[test]
    fn runtime_policy_delete_returns_false() {
        let mut config = make_config();
        assert!(!RuntimePolicyResource::delete_from(&mut config, "runtime"));
    }

    #[test]
    fn runtime_policy_to_yaml() {
        let resource =
            dispatch_resource(runtime_policy_manifest()).expect("dispatch should succeed");
        let yaml = resource.to_yaml().expect("should serialize");
        assert!(yaml.contains("kind: RuntimePolicy"));
        assert!(yaml.contains("/bin/bash"));
    }

    #[test]
    fn build_runtime_policy_rejects_wrong_kind() {
        let resource = OrchestratorResource {
            api_version: API_VERSION.to_string(),
            kind: ResourceKind::RuntimePolicy,
            metadata: ResourceMetadata {
                name: "bad".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: ResourceSpec::Project(ProjectSpec { description: None }),
        };
        let err = dispatch_resource(resource).expect_err("operation should fail");
        assert!(err.to_string().contains("mismatch"));
    }

    // ── runner_spec_to_config / runner_config_to_spec roundtrip ──────

    #[test]
    fn runner_spec_config_roundtrip() {
        let spec = RunnerSpec {
            shell: "/bin/zsh".to_string(),
            shell_arg: "-c".to_string(),
            policy: "allowlist".to_string(),
            executor: "shell".to_string(),
            allowed_shells: vec!["/bin/bash".to_string()],
            allowed_shell_args: vec!["-c".to_string()],
            env_allowlist: vec!["PATH".to_string()],
            redaction_patterns: vec!["SECRET_.*".to_string()],
        };

        let config = runner_spec_to_config(&spec);
        assert_eq!(config.shell, "/bin/zsh");
        assert!(matches!(config.policy, RunnerPolicy::Allowlist));
        assert!(matches!(config.executor, RunnerExecutorKind::Shell));
        assert_eq!(config.allowed_shells, vec!["/bin/bash".to_string()]);
        assert_eq!(config.env_allowlist, vec!["PATH".to_string()]);

        let roundtripped = runner_config_to_spec(&config);
        assert_eq!(roundtripped.shell, "/bin/zsh");
        assert_eq!(roundtripped.policy, "allowlist");
        assert_eq!(roundtripped.executor, "shell");
        assert_eq!(roundtripped.allowed_shells, vec!["/bin/bash".to_string()]);
    }

    #[test]
    fn validate_rejects_allowlist_with_empty_shells() {
        let mut manifest = runtime_policy_manifest();
        if let ResourceSpec::RuntimePolicy(ref mut spec) = manifest.spec {
            spec.runner.policy = "allowlist".to_string();
            spec.runner.allowed_shells = vec![];
            spec.runner.allowed_shell_args = vec!["-lc".to_string()];
        }
        let resource = dispatch_resource(manifest).expect("dispatch should succeed");
        let err = resource.validate().expect_err("operation should fail");
        assert!(err
            .to_string()
            .contains("runner.allowed_shells cannot be empty"));
    }

    #[test]
    fn validate_rejects_allowlist_with_empty_shell_args() {
        let mut manifest = runtime_policy_manifest();
        if let ResourceSpec::RuntimePolicy(ref mut spec) = manifest.spec {
            spec.runner.policy = "allowlist".to_string();
            spec.runner.allowed_shells = vec!["/bin/bash".to_string()];
            spec.runner.allowed_shell_args = vec![];
        }
        let resource = dispatch_resource(manifest).expect("dispatch should succeed");
        let err = resource.validate().expect_err("operation should fail");
        assert!(err
            .to_string()
            .contains("runner.allowed_shell_args cannot be empty"));
    }

    #[test]
    fn validate_accepts_allowlist_with_populated_lists() {
        let mut manifest = runtime_policy_manifest();
        if let ResourceSpec::RuntimePolicy(ref mut spec) = manifest.spec {
            spec.runner.policy = "allowlist".to_string();
            spec.runner.allowed_shells = vec!["/bin/bash".to_string()];
            spec.runner.allowed_shell_args = vec!["-lc".to_string()];
        }
        let resource = dispatch_resource(manifest).expect("dispatch should succeed");
        assert!(resource.validate().is_ok());
    }

    #[test]
    fn validate_accepts_unsafe_with_empty_lists() {
        let manifest = runtime_policy_manifest();
        let resource = dispatch_resource(manifest).expect("dispatch should succeed");
        assert!(resource.validate().is_ok());
    }

    #[test]
    fn runner_spec_unsafe_policy() {
        let spec = RunnerSpec {
            shell: "/bin/sh".to_string(),
            shell_arg: "-c".to_string(),
            policy: "unsafe".to_string(),
            executor: "shell".to_string(),
            allowed_shells: vec![],
            allowed_shell_args: vec![],
            env_allowlist: vec![],
            redaction_patterns: vec![],
        };
        let config = runner_spec_to_config(&spec);
        assert!(matches!(config.policy, RunnerPolicy::Unsafe));
        let back = runner_config_to_spec(&config);
        assert_eq!(back.policy, "unsafe");
    }

    #[test]
    fn runner_spec_unknown_policy_falls_back_to_allowlist() {
        let spec = RunnerSpec {
            shell: "/bin/sh".to_string(),
            shell_arg: "-c".to_string(),
            policy: "unknown".to_string(),
            executor: "shell".to_string(),
            allowed_shells: vec![],
            allowed_shell_args: vec![],
            env_allowlist: vec![],
            redaction_patterns: vec![],
        };
        let config = runner_spec_to_config(&spec);
        assert!(matches!(config.policy, RunnerPolicy::Allowlist));
    }

    #[test]
    fn default_runner_spec_produces_allowlist() {
        let json = r#"{"shell":"/bin/bash"}"#;
        let spec: RunnerSpec =
            serde_json::from_str(json).expect("runner spec json should deserialize");
        assert_eq!(spec.policy, "allowlist");
        assert!(!spec.allowed_shells.is_empty());
        assert!(!spec.allowed_shell_args.is_empty());
        let config = runner_spec_to_config(&spec);
        assert!(matches!(config.policy, RunnerPolicy::Allowlist));
    }
}
