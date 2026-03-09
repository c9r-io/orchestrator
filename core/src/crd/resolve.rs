use crate::config::OrchestratorConfig;
use crate::crd::types::{CrdVersion, CustomResourceDefinition};
use anyhow::{anyhow, Result};

/// Find a CRD by kind name, plural name, or short_name alias.
pub fn find_crd_by_kind_or_alias<'a>(
    config: &'a OrchestratorConfig,
    query: &str,
) -> Option<&'a CustomResourceDefinition> {
    let lower = query.to_lowercase();
    config.custom_resource_definitions.values().find(|crd| {
        crd.kind.eq_ignore_ascii_case(&lower)
            || crd.plural.eq_ignore_ascii_case(&lower)
            || crd
                .short_names
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&lower))
    })
}

/// Find the CRD that defines the given kind (exact PascalCase match).
pub fn find_crd_for_kind<'a>(
    config: &'a OrchestratorConfig,
    kind: &str,
) -> Result<&'a CustomResourceDefinition> {
    config
        .custom_resource_definitions
        .values()
        .find(|crd| crd.kind == kind)
        .ok_or_else(|| anyhow!("no CustomResourceDefinition found for kind '{}'", kind))
}

/// Resolve an apiVersion string (e.g. "extensions.orchestrator.dev/v1") to
/// the matching CrdVersion within a CRD.
pub fn resolve_version<'a>(
    crd: &'a CustomResourceDefinition,
    api_version: &str,
) -> Result<&'a CrdVersion> {
    // apiVersion format: "{group}/{version}"
    let expected_prefix = format!("{}/", crd.group);
    let version_name = api_version.strip_prefix(&expected_prefix).ok_or_else(|| {
        anyhow!(
            "apiVersion '{}' does not match CRD group '{}' (expected '{}<version>')",
            api_version,
            crd.group,
            expected_prefix
        )
    })?;

    crd.versions
        .iter()
        .find(|v| v.name == version_name && v.served)
        .ok_or_else(|| {
            anyhow!(
                "version '{}' not found or not served in CRD '{}'",
                version_name,
                crd.kind
            )
        })
}

/// Check if a kind string matches one of the builtin resource kinds.
pub fn is_builtin_kind(kind: &str) -> bool {
    matches!(
        kind,
        "Workspace"
            | "Agent"
            | "Workflow"
            | "Project"
            | "RuntimePolicy"
            | "StepTemplate"
            | "EnvStore"
            | "SecretStore"
    )
}

/// The set of builtin plural/alias names that CRD plurals and short_names
/// must not conflict with.
pub fn is_builtin_alias(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "ws" | "workspace"
            | "workspaces"
            | "agent"
            | "agents"
            | "wf"
            | "workflow"
            | "workflows"
            | "project"
            | "projects"
            | "runtimepolicy"
            | "runtime-policy"
            | "steptemplate"
            | "step_template"
            | "step-template"
            | "steptemplates"
            | "envstore"
            | "env-store"
            | "env_store"
            | "envstores"
            | "secretstore"
            | "secret-store"
            | "secret_store"
            | "secretstores"
            | "task"
            | "tasks"
            | "t"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crd::types::{CrdHooks, CrdVersion, CustomResourceDefinition};

    fn make_config_with_crd() -> OrchestratorConfig {
        let mut config = OrchestratorConfig::default();
        config.custom_resource_definitions.insert(
            "PromptLibrary".to_string(),
            CustomResourceDefinition {
                kind: "PromptLibrary".to_string(),
                plural: "promptlibraries".to_string(),
                short_names: vec!["pl".to_string()],
                group: "extensions.orchestrator.dev".to_string(),
                versions: vec![CrdVersion {
                    name: "v1".to_string(),
                    schema: serde_json::json!({"type": "object"}),
                    served: true,
                    cel_rules: vec![],
                }],
                hooks: CrdHooks::default(),
                scope: crate::crd::scope::CrdScope::default(),
                builtin: false,
            },
        );
        config
    }

    #[test]
    fn find_by_kind() {
        let config = make_config_with_crd();
        assert!(find_crd_by_kind_or_alias(&config, "PromptLibrary").is_some());
        assert!(find_crd_by_kind_or_alias(&config, "promptlibrary").is_some());
    }

    #[test]
    fn find_by_plural() {
        let config = make_config_with_crd();
        assert!(find_crd_by_kind_or_alias(&config, "promptlibraries").is_some());
    }

    #[test]
    fn find_by_short_name() {
        let config = make_config_with_crd();
        assert!(find_crd_by_kind_or_alias(&config, "pl").is_some());
    }

    #[test]
    fn find_returns_none_for_unknown() {
        let config = make_config_with_crd();
        assert!(find_crd_by_kind_or_alias(&config, "nonexistent").is_none());
    }

    #[test]
    fn find_crd_for_kind_exact() {
        let config = make_config_with_crd();
        assert!(find_crd_for_kind(&config, "PromptLibrary").is_ok());
        assert!(find_crd_for_kind(&config, "promptlibrary").is_err()); // case-sensitive
    }

    #[test]
    fn resolve_version_ok() {
        let config = make_config_with_crd();
        let crd = find_crd_for_kind(&config, "PromptLibrary").expect("crd should exist");
        let ver = resolve_version(crd, "extensions.orchestrator.dev/v1");
        assert!(ver.is_ok());
        assert_eq!(ver.expect("version should resolve").name, "v1");
    }

    #[test]
    fn resolve_version_wrong_group() {
        let config = make_config_with_crd();
        let crd = find_crd_for_kind(&config, "PromptLibrary").expect("crd should exist");
        assert!(resolve_version(crd, "wrong.group/v1").is_err());
    }

    #[test]
    fn resolve_version_unknown_version() {
        let config = make_config_with_crd();
        let crd = find_crd_for_kind(&config, "PromptLibrary").expect("crd should exist");
        assert!(resolve_version(crd, "extensions.orchestrator.dev/v99").is_err());
    }

    #[test]
    fn is_builtin_kind_true() {
        assert!(is_builtin_kind("Workspace"));
        assert!(is_builtin_kind("Agent"));
        assert!(is_builtin_kind("SecretStore"));
    }

    #[test]
    fn is_builtin_kind_false() {
        assert!(!is_builtin_kind("PromptLibrary"));
        assert!(!is_builtin_kind("workspace")); // case-sensitive
    }

    #[test]
    fn is_builtin_alias_true() {
        assert!(is_builtin_alias("ws"));
        assert!(is_builtin_alias("wf"));
        assert!(is_builtin_alias("agent"));
        assert!(is_builtin_alias("task"));
        assert!(is_builtin_alias("envstore"));
    }

    #[test]
    fn is_builtin_alias_false() {
        assert!(!is_builtin_alias("promptlibraries"));
        assert!(!is_builtin_alias("pl"));
    }
}
