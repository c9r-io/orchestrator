use crate::cli_types::ResourceMetadata;
use crate::config::ExecutionProfileConfig;
use crate::crd_scope::CrdScope;
use serde::{Deserialize, Serialize};

/// A Custom Resource Definition — defines a new resource type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomResourceDefinition {
    /// PascalCase kind name, e.g. "PromptLibrary"
    pub kind: String,
    /// CLI plural form, e.g. "promptlibraries"
    pub plural: String,
    /// CLI short aliases, e.g. ["pl"]
    #[serde(default)]
    pub short_names: Vec<String>,
    /// API group, e.g. "extensions.orchestrator.dev"
    pub group: String,
    /// Versioned schemas
    pub versions: Vec<CrdVersion>,
    /// Lifecycle hooks
    #[serde(default)]
    pub hooks: CrdHooks,
    /// Scope: Namespaced (project-scoped), Cluster (global), or Singleton
    #[serde(default)]
    pub scope: CrdScope,
    /// If true, this CRD is a builtin type and cannot be deleted or overwritten by users
    #[serde(default)]
    pub builtin: bool,
    /// Plugins that extend daemon behavior (interceptors, transformers, cron tasks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<CrdPlugin>,
}

/// A single version definition within a CRD.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrdVersion {
    /// Version name, e.g. "v1"
    pub name: String,
    /// JSON Schema subset for spec validation
    #[serde(default = "default_schema")]
    pub schema: serde_json::Value,
    /// Whether this version is served (active)
    #[serde(default = "default_true")]
    pub served: bool,
    /// CEL validation rules applied after schema validation
    #[serde(default)]
    pub cel_rules: Vec<CelValidationRule>,
}

fn default_schema() -> serde_json::Value {
    serde_json::json!({"type": "object"})
}

fn default_true() -> bool {
    true
}

/// A CEL expression validation rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CelValidationRule {
    /// CEL expression; `self` is bound to the spec value
    pub rule: String,
    /// Error message when the rule evaluates to false
    pub message: String,
}

/// Lifecycle hooks for custom resources.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CrdHooks {
    /// Optional command executed after a resource is created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_create: Option<String>,
    /// Optional command executed after a resource is updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_update: Option<String>,
    /// Optional command executed before or after a resource is deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_delete: Option<String>,
}

/// A CRD plugin — a named, typed extension that injects custom logic at a
/// well-defined phase in the daemon's request or scheduling pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrdPlugin {
    /// Unique name within this CRD (e.g. "verify-signature").
    pub name: String,
    /// Plugin type: "interceptor", "transformer", or "cron".
    #[serde(rename = "type")]
    pub plugin_type: String,
    /// Extension phase (required for interceptor/transformer, e.g. "webhook.authenticate").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// Shell command to execute.
    pub command: String,
    /// Timeout in seconds (default: 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// Cron expression (required for type "cron").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// IANA timezone for cron scheduling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Optional per-plugin execution profile override.  When set, takes
    /// precedence over the policy-level default execution profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_profile: Option<ExecutionProfileConfig>,
}

impl CrdPlugin {
    /// Returns the effective timeout in seconds (default 5).
    pub fn effective_timeout(&self) -> u64 {
        self.timeout.unwrap_or(5)
    }
}

/// A custom resource instance stored in config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomResource {
    /// Kind name of the resource instance.
    pub kind: String,
    /// Fully qualified API version such as `group/v1`.
    pub api_version: String,
    /// Resource metadata including name and optional project.
    pub metadata: ResourceMetadata,
    /// Untyped resource specification payload.
    pub spec: serde_json::Value,
    /// Monotonic generation incremented on each persisted update.
    pub generation: u64,
    /// Timestamp when the resource was first created.
    pub created_at: String,
    /// Timestamp when the resource was most recently updated.
    pub updated_at: String,
}

/// Untyped manifest for a custom resource instance (parsed from YAML).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomResourceManifest {
    #[serde(rename = "apiVersion")]
    /// Fully qualified API version such as `group/v1`.
    pub api_version: String,
    /// Kind name of the resource instance.
    pub kind: String,
    /// Resource metadata including name and optional project.
    pub metadata: ResourceMetadata,
    /// Untyped resource specification payload.
    pub spec: serde_json::Value,
}

/// Manifest for a CRD definition (parsed from YAML).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrdManifest {
    #[serde(rename = "apiVersion")]
    /// API version of the CRD manifest schema.
    pub api_version: String,
    /// Manifest metadata such as the CRD name.
    pub metadata: ResourceMetadata,
    /// Declared CRD specification.
    pub spec: CrdSpec,
}

/// The spec portion of a CRD manifest (maps to CustomResourceDefinition fields).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrdSpec {
    /// Kind name produced by this CRD.
    pub kind: String,
    /// Plural CLI/resource name for the custom kind.
    pub plural: String,
    #[serde(default)]
    /// Optional short aliases accepted by the CLI.
    pub short_names: Vec<String>,
    /// API group for served resource versions.
    pub group: String,
    /// Version definitions served by the CRD.
    pub versions: Vec<CrdVersion>,
    #[serde(default)]
    /// Lifecycle hooks applied to custom resource operations.
    pub hooks: CrdHooks,
    #[serde(default)]
    /// Scope used for instances of this CRD.
    pub scope: CrdScope,
    #[serde(default)]
    /// Whether the CRD is builtin and protected from user deletion.
    pub builtin: bool,
    /// Plugins that extend daemon behavior (interceptors, transformers, cron tasks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<CrdPlugin>,
}

impl CrdSpec {
    /// Convert to a full CRD definition.
    pub fn into_crd(self) -> CustomResourceDefinition {
        CustomResourceDefinition {
            kind: self.kind,
            plural: self.plural,
            short_names: self.short_names,
            group: self.group,
            versions: self.versions,
            hooks: self.hooks,
            scope: self.scope,
            builtin: self.builtin,
            plugins: self.plugins,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crd_serde_round_trip() {
        let crd = CustomResourceDefinition {
            kind: "PromptLibrary".to_string(),
            plural: "promptlibraries".to_string(),
            short_names: vec!["pl".to_string()],
            group: "extensions.orchestrator.dev".to_string(),
            versions: vec![CrdVersion {
                name: "v1".to_string(),
                schema: serde_json::json!({
                    "type": "object",
                    "required": ["prompts"],
                    "properties": {
                        "prompts": { "type": "array" }
                    }
                }),
                served: true,
                cel_rules: vec![CelValidationRule {
                    rule: "size(self.prompts) > 0".to_string(),
                    message: "at least one prompt is required".to_string(),
                }],
            }],
            hooks: CrdHooks {
                on_create: Some("echo created".to_string()),
                on_update: None,
                on_delete: None,
            },
            scope: CrdScope::default(),
            builtin: false,
            plugins: vec![],
        };

        let json = serde_json::to_string(&crd).expect("serialize");
        let crd2: CustomResourceDefinition = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(crd, crd2);
    }

    #[test]
    fn custom_resource_serde_round_trip() {
        let cr = CustomResource {
            kind: "PromptLibrary".to_string(),
            api_version: "extensions.orchestrator.dev/v1".to_string(),
            metadata: ResourceMetadata {
                name: "qa-prompts".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({"prompts": [{"name": "test", "template": "t"}]}),
            generation: 1,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&cr).expect("serialize");
        let cr2: CustomResource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cr, cr2);
    }

    #[test]
    fn crd_manifest_yaml_parse() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
metadata:
  name: promptlibraries.extensions.orchestrator.dev
spec:
  kind: PromptLibrary
  plural: promptlibraries
  short_names: [pl]
  group: extensions.orchestrator.dev
  versions:
    - name: v1
      served: true
      schema:
        type: object
        required: [prompts]
        properties:
          prompts:
            type: array
"#;
        let manifest: CrdManifest = serde_yaml::from_str(yaml).expect("parse CRD manifest");
        assert_eq!(manifest.spec.kind, "PromptLibrary");
        assert_eq!(manifest.spec.plural, "promptlibraries");
        assert_eq!(manifest.spec.versions.len(), 1);
        assert!(manifest.spec.versions[0].served);
    }

    #[test]
    fn custom_resource_manifest_yaml_parse() {
        let yaml = r#"
apiVersion: extensions.orchestrator.dev/v1
kind: PromptLibrary
metadata:
  name: qa-prompts
  labels:
    team: platform
spec:
  prompts:
    - name: code-review
      template: "Review the code"
"#;
        let manifest: CustomResourceManifest =
            serde_yaml::from_str(yaml).expect("parse CR manifest");
        assert_eq!(manifest.kind, "PromptLibrary");
        assert_eq!(manifest.metadata.name, "qa-prompts");
        assert!(manifest.spec.is_object());
    }

    #[test]
    fn crd_spec_into_crd() {
        let spec = CrdSpec {
            kind: "Foo".to_string(),
            plural: "foos".to_string(),
            short_names: vec![],
            group: "test.dev".to_string(),
            versions: vec![],
            hooks: CrdHooks::default(),
            scope: CrdScope::default(),
            builtin: false,
            plugins: vec![],
        };
        let crd = spec.into_crd();
        assert_eq!(crd.kind, "Foo");
        assert_eq!(crd.group, "test.dev");
    }

    #[test]
    fn crd_hooks_default_is_empty() {
        let hooks = CrdHooks::default();
        assert!(hooks.on_create.is_none());
        assert!(hooks.on_update.is_none());
        assert!(hooks.on_delete.is_none());
    }

    #[test]
    fn crd_manifest_yaml_parse_with_plugins() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
metadata:
  name: slackintegrations.integrations.orchestrator.dev
spec:
  kind: SlackIntegration
  plural: slackintegrations
  group: integrations.orchestrator.dev
  versions:
    - name: v1
      schema: { type: object }
  plugins:
    - name: verify-sig
      type: interceptor
      phase: webhook.authenticate
      command: "scripts/verify.sh"
      timeout: 3
    - name: rotate
      type: cron
      schedule: "0 0 * * *"
      command: "scripts/rotate.sh"
      timezone: "Asia/Taipei"
"#;
        let manifest: CrdManifest = serde_yaml::from_str(yaml).expect("parse CRD with plugins");
        assert_eq!(manifest.spec.kind, "SlackIntegration");
        assert_eq!(manifest.spec.plugins.len(), 2);

        let interceptor = &manifest.spec.plugins[0];
        assert_eq!(interceptor.name, "verify-sig");
        assert_eq!(interceptor.plugin_type, "interceptor");
        assert_eq!(interceptor.phase.as_deref(), Some("webhook.authenticate"));
        assert_eq!(interceptor.command, "scripts/verify.sh");
        assert_eq!(interceptor.effective_timeout(), 3);

        let cron = &manifest.spec.plugins[1];
        assert_eq!(cron.name, "rotate");
        assert_eq!(cron.plugin_type, "cron");
        assert_eq!(cron.schedule.as_deref(), Some("0 0 * * *"));
        assert_eq!(cron.timezone.as_deref(), Some("Asia/Taipei"));
    }

    #[test]
    fn crd_manifest_yaml_without_plugins_defaults_empty() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
metadata:
  name: foos.test.dev
spec:
  kind: Foo
  plural: foos
  group: test.dev
  versions:
    - name: v1
      schema: { type: object }
"#;
        let manifest: CrdManifest = serde_yaml::from_str(yaml).expect("parse CRD without plugins");
        assert!(manifest.spec.plugins.is_empty());
    }
}
