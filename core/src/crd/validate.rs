use crate::config::OrchestratorConfig;
use crate::crd::resolve::{find_crd_for_kind, is_builtin_alias, is_builtin_kind, resolve_version};
use crate::crd::schema::{validate_json_schema, validate_schema_definition};
use crate::crd::types::{CelValidationRule, CrdManifest, CustomResourceManifest};
use crate::resource::validate_resource_name;
use anyhow::{Result, anyhow};
use cel_interpreter::{Context as CelContext, Program, Value as CelValue};
use std::collections::HashMap;

/// Validate a CRD definition itself (the meta-schema).
pub fn validate_crd_definition(config: &OrchestratorConfig, manifest: &CrdManifest) -> Result<()> {
    validate_resource_name(&manifest.metadata.name)?;

    let spec = &manifest.spec;

    // kind must be PascalCase
    if spec.kind.is_empty() || !spec.kind.chars().next().unwrap_or('a').is_ascii_uppercase() {
        return Err(anyhow!(
            "CRD kind '{}' must be PascalCase (start with uppercase letter)",
            spec.kind
        ));
    }

    // kind must not conflict with builtin kinds
    if is_builtin_kind(&spec.kind) {
        return Err(anyhow!(
            "CRD kind '{}' conflicts with builtin resource kind",
            spec.kind
        ));
    }

    // plural must not conflict with builtin aliases
    if is_builtin_alias(&spec.plural) {
        return Err(anyhow!(
            "CRD plural '{}' conflicts with builtin resource alias",
            spec.plural
        ));
    }

    // short_names must not conflict with builtin aliases
    for short in &spec.short_names {
        if is_builtin_alias(short) {
            return Err(anyhow!(
                "CRD short_name '{}' conflicts with builtin resource alias",
                short
            ));
        }
    }

    // group must be non-empty
    if spec.group.trim().is_empty() {
        return Err(anyhow!("CRD group cannot be empty"));
    }

    // at least one version
    if spec.versions.is_empty() {
        return Err(anyhow!("CRD must have at least one version"));
    }

    // at least one served version
    if !spec.versions.iter().any(|v| v.served) {
        return Err(anyhow!("CRD must have at least one served version"));
    }

    // validate each version
    for version in &spec.versions {
        validate_schema_definition(&version.schema)?;
        // pre-compile CEL rules to validate syntax
        for cel_rule in &version.cel_rules {
            validate_cel_syntax(&cel_rule.rule)?;
        }
    }

    // kind + group uniqueness: check for conflict with existing CRDs
    for existing in config.custom_resource_definitions.values() {
        if existing.kind == spec.kind && existing.group == spec.group {
            // Allow update of same CRD (matched by metadata.name)
            let expected_name = format!("{}.{}", spec.plural, spec.group);
            if manifest.metadata.name != expected_name {
                return Err(anyhow!(
                    "CRD kind '{}' with group '{}' already registered under different name",
                    spec.kind,
                    spec.group
                ));
            }
        }
    }

    // Validate plugins
    validate_crd_plugins(&spec.plugins)?;

    Ok(())
}

/// Validate CRD plugin definitions.
fn validate_crd_plugins(plugins: &[crate::crd::types::CrdPlugin]) -> Result<()> {
    use std::collections::HashSet;

    let known_types = ["interceptor", "transformer", "cron"];
    let mut names = HashSet::new();

    for plugin in plugins {
        // Plugin names must be unique within a CRD
        if !names.insert(&plugin.name) {
            return Err(anyhow!(
                "duplicate plugin name '{}' in CRD",
                plugin.name
            ));
        }

        // Plugin name must not be empty
        if plugin.name.trim().is_empty() {
            return Err(anyhow!("plugin name cannot be empty"));
        }

        // Plugin type must be known
        if !known_types.contains(&plugin.plugin_type.as_str()) {
            return Err(anyhow!(
                "unknown plugin type '{}' for plugin '{}' (expected one of: {})",
                plugin.plugin_type,
                plugin.name,
                known_types.join(", ")
            ));
        }

        // Command must not be empty
        if plugin.command.trim().is_empty() {
            return Err(anyhow!(
                "plugin '{}' command cannot be empty",
                plugin.name
            ));
        }

        // interceptor/transformer require a phase
        if (plugin.plugin_type == "interceptor" || plugin.plugin_type == "transformer")
            && plugin.phase.is_none()
        {
            return Err(anyhow!(
                "plugin '{}' of type '{}' requires a phase",
                plugin.name,
                plugin.plugin_type
            ));
        }

        // cron plugins require a schedule
        if plugin.plugin_type == "cron" {
            let schedule = plugin.schedule.as_deref().unwrap_or("");
            if schedule.trim().is_empty() {
                return Err(anyhow!(
                    "cron plugin '{}' requires a schedule",
                    plugin.name
                ));
            }
            // Validate cron expression
            use cron::Schedule;
            use std::str::FromStr;
            Schedule::from_str(schedule).map_err(|e| {
                anyhow!(
                    "cron plugin '{}' has invalid schedule '{}': {}",
                    plugin.name,
                    schedule,
                    e
                )
            })?;
        }
    }

    Ok(())
}

/// Validate a custom resource instance against its CRD.
pub fn validate_custom_resource(
    config: &OrchestratorConfig,
    manifest: &CustomResourceManifest,
) -> Result<()> {
    validate_resource_name(&manifest.metadata.name)?;

    let crd = find_crd_for_kind(config, &manifest.kind)?;
    let version = resolve_version(crd, &manifest.api_version)?;

    // JSON Schema validation
    validate_json_schema(&manifest.spec, &version.schema)?;

    // CEL rule validation
    validate_cel_rules(&manifest.spec, &version.cel_rules)?;

    Ok(())
}

/// Validate CEL expression syntax by attempting to compile it.
fn validate_cel_syntax(expression: &str) -> Result<()> {
    let expr = expression.trim();
    if expr.is_empty() {
        return Err(anyhow!("CEL rule expression cannot be empty"));
    }
    let compiled = std::panic::catch_unwind(|| Program::compile(expr))
        .map_err(|_| anyhow!("CEL rule '{}' caused parser panic", expr))?;
    compiled.map_err(|err| anyhow!("CEL rule '{}' is invalid: {}", expr, err))?;
    Ok(())
}

/// Evaluate CEL rules against a spec value.
fn validate_cel_rules(spec: &serde_json::Value, rules: &[CelValidationRule]) -> Result<()> {
    if rules.is_empty() {
        return Ok(());
    }

    let cel_value = json_to_cel_value(spec);

    for rule in rules {
        let program = std::panic::catch_unwind(|| Program::compile(&rule.rule))
            .map_err(|_| anyhow!("CEL rule '{}' caused parser panic", rule.rule))?
            .map_err(|err| anyhow!("CEL rule '{}' compile error: {}", rule.rule, err))?;

        let mut context = CelContext::default();
        context
            .add_variable("self", cel_value.clone())
            .map_err(|err| anyhow!("failed to bind 'self' in CEL context: {}", err))?;

        let result = program.execute(&context);
        match result {
            Ok(CelValue::Bool(true)) => {} // rule passed
            Ok(CelValue::Bool(false)) => {
                return Err(anyhow!("CEL validation failed: {}", rule.message));
            }
            Ok(other) => {
                return Err(anyhow!(
                    "CEL rule '{}' returned non-boolean value: {:?}",
                    rule.rule,
                    other
                ));
            }
            Err(err) => {
                return Err(anyhow!("CEL rule '{}' execution error: {}", rule.rule, err));
            }
        }
    }

    Ok(())
}

/// Convert a serde_json::Value to cel_interpreter::Value.
fn json_to_cel_value(v: &serde_json::Value) -> CelValue {
    match v {
        serde_json::Value::Null => CelValue::Null,
        serde_json::Value::Bool(b) => CelValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                CelValue::Int(i)
            } else if let Some(u) = n.as_u64() {
                CelValue::UInt(u)
            } else if let Some(f) = n.as_f64() {
                CelValue::Float(f)
            } else {
                CelValue::Null
            }
        }
        serde_json::Value::String(s) => CelValue::String(s.clone().into()),
        serde_json::Value::Array(arr) => {
            CelValue::List(arr.iter().map(json_to_cel_value).collect::<Vec<_>>().into())
        }
        serde_json::Value::Object(obj) => {
            let map: HashMap<String, CelValue> = obj
                .iter()
                .map(|(k, v)| (k.clone(), json_to_cel_value(v)))
                .collect();
            CelValue::Map(map.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crd::types::{CrdHooks, CrdSpec, CrdVersion};

    fn make_crd_manifest(kind: &str, plural: &str, group: &str) -> CrdManifest {
        CrdManifest {
            api_version: "orchestrator.dev/v2".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: format!("{}.{}", plural, group),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: CrdSpec {
                kind: kind.to_string(),
                plural: plural.to_string(),
                short_names: vec![],
                group: group.to_string(),
                versions: vec![CrdVersion {
                    name: "v1".to_string(),
                    schema: serde_json::json!({"type": "object"}),
                    served: true,
                    cel_rules: vec![],
                }],
                hooks: CrdHooks::default(),
                scope: crate::crd::scope::CrdScope::default(),
                builtin: false,
                plugins: vec![],
            },
        }
    }

    #[test]
    fn validate_crd_valid() {
        let config = OrchestratorConfig::default();
        let manifest = make_crd_manifest("Foo", "foos", "test.dev");
        assert!(validate_crd_definition(&config, &manifest).is_ok());
    }

    #[test]
    fn validate_crd_rejects_lowercase_kind() {
        let config = OrchestratorConfig::default();
        let manifest = make_crd_manifest("foo", "foos", "test.dev");
        assert!(validate_crd_definition(&config, &manifest).is_err());
    }

    #[test]
    fn validate_crd_rejects_builtin_kind() {
        let config = OrchestratorConfig::default();
        let manifest = make_crd_manifest("Agent", "agents-custom", "test.dev");
        assert!(validate_crd_definition(&config, &manifest).is_err());
    }

    #[test]
    fn validate_crd_rejects_builtin_plural() {
        let config = OrchestratorConfig::default();
        let manifest = make_crd_manifest("Foo", "workspaces", "test.dev");
        assert!(validate_crd_definition(&config, &manifest).is_err());
    }

    #[test]
    fn validate_crd_rejects_empty_group() {
        let config = OrchestratorConfig::default();
        let manifest = make_crd_manifest("Foo", "foos", "");
        assert!(validate_crd_definition(&config, &manifest).is_err());
    }

    #[test]
    fn validate_crd_rejects_no_versions() {
        let config = OrchestratorConfig::default();
        let mut manifest = make_crd_manifest("Foo", "foos", "test.dev");
        manifest.spec.versions.clear();
        assert!(validate_crd_definition(&config, &manifest).is_err());
    }

    #[test]
    fn validate_crd_rejects_no_served_version() {
        let config = OrchestratorConfig::default();
        let mut manifest = make_crd_manifest("Foo", "foos", "test.dev");
        manifest.spec.versions[0].served = false;
        assert!(validate_crd_definition(&config, &manifest).is_err());
    }

    #[test]
    fn validate_crd_rejects_invalid_cel_syntax() {
        let config = OrchestratorConfig::default();
        let mut manifest = make_crd_manifest("Foo", "foos", "test.dev");
        manifest.spec.versions[0].cel_rules.push(CelValidationRule {
            rule: "invalid %%% syntax".to_string(),
            message: "bad".to_string(),
        });
        assert!(validate_crd_definition(&config, &manifest).is_err());
    }

    #[test]
    fn validate_custom_resource_valid() {
        let mut config = OrchestratorConfig::default();
        config.custom_resource_definitions.insert(
            "Foo".to_string(),
            crate::crd::types::CustomResourceDefinition {
                kind: "Foo".to_string(),
                plural: "foos".to_string(),
                short_names: vec![],
                group: "test.dev".to_string(),
                versions: vec![CrdVersion {
                    name: "v1".to_string(),
                    schema: serde_json::json!({
                        "type": "object",
                        "required": ["name"],
                        "properties": {
                            "name": {"type": "string"}
                        }
                    }),
                    served: true,
                    cel_rules: vec![],
                }],
                hooks: CrdHooks::default(),
                scope: crate::crd::scope::CrdScope::default(),
                builtin: false,
                plugins: vec![],
            },
        );
        let manifest = CustomResourceManifest {
            api_version: "test.dev/v1".to_string(),
            kind: "Foo".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "my-foo".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({"name": "hello"}),
        };
        assert!(validate_custom_resource(&config, &manifest).is_ok());
    }

    #[test]
    fn validate_custom_resource_schema_fail() {
        let mut config = OrchestratorConfig::default();
        config.custom_resource_definitions.insert(
            "Foo".to_string(),
            crate::crd::types::CustomResourceDefinition {
                kind: "Foo".to_string(),
                plural: "foos".to_string(),
                short_names: vec![],
                group: "test.dev".to_string(),
                versions: vec![CrdVersion {
                    name: "v1".to_string(),
                    schema: serde_json::json!({
                        "type": "object",
                        "required": ["name"]
                    }),
                    served: true,
                    cel_rules: vec![],
                }],
                hooks: CrdHooks::default(),
                scope: crate::crd::scope::CrdScope::default(),
                builtin: false,
                plugins: vec![],
            },
        );
        let manifest = CustomResourceManifest {
            api_version: "test.dev/v1".to_string(),
            kind: "Foo".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "bad-foo".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({}), // missing required "name"
        };
        assert!(validate_custom_resource(&config, &manifest).is_err());
    }

    #[test]
    fn validate_custom_resource_cel_fail() {
        let mut config = OrchestratorConfig::default();
        config.custom_resource_definitions.insert(
            "Foo".to_string(),
            crate::crd::types::CustomResourceDefinition {
                kind: "Foo".to_string(),
                plural: "foos".to_string(),
                short_names: vec![],
                group: "test.dev".to_string(),
                versions: vec![CrdVersion {
                    name: "v1".to_string(),
                    schema: serde_json::json!({"type": "object"}),
                    served: true,
                    cel_rules: vec![CelValidationRule {
                        rule: "size(self.items) > 0".to_string(),
                        message: "items must not be empty".to_string(),
                    }],
                }],
                hooks: CrdHooks::default(),
                scope: crate::crd::scope::CrdScope::default(),
                builtin: false,
                plugins: vec![],
            },
        );
        let manifest = CustomResourceManifest {
            api_version: "test.dev/v1".to_string(),
            kind: "Foo".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "empty-foo".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({"items": []}),
        };
        let err = validate_custom_resource(&config, &manifest);
        assert!(err.is_err());
        assert!(
            err.expect_err("should fail")
                .to_string()
                .contains("items must not be empty")
        );
    }

    #[test]
    fn validate_custom_resource_no_crd() {
        let config = OrchestratorConfig::default();
        let manifest = CustomResourceManifest {
            api_version: "test.dev/v1".to_string(),
            kind: "Nonexistent".to_string(),
            metadata: crate::cli_types::ResourceMetadata {
                name: "x".to_string(),
                project: None,
                labels: None,
                annotations: None,
            },
            spec: serde_json::json!({}),
        };
        assert!(validate_custom_resource(&config, &manifest).is_err());
    }

    #[test]
    fn json_to_cel_value_primitives() {
        assert!(matches!(
            json_to_cel_value(&serde_json::json!(null)),
            CelValue::Null
        ));
        assert!(matches!(
            json_to_cel_value(&serde_json::json!(true)),
            CelValue::Bool(true)
        ));
        assert!(matches!(
            json_to_cel_value(&serde_json::json!(42)),
            CelValue::Int(42)
        ));
        assert!(matches!(
            json_to_cel_value(&serde_json::json!(3.15)),
            CelValue::Float(_)
        ));
    }

    #[test]
    fn json_to_cel_value_string() {
        let v = json_to_cel_value(&serde_json::json!("hello"));
        assert!(matches!(v, CelValue::String(_)));
    }

    #[test]
    fn json_to_cel_value_list() {
        let v = json_to_cel_value(&serde_json::json!([1, 2, 3]));
        assert!(matches!(v, CelValue::List(_)));
    }

    #[test]
    fn json_to_cel_value_map() {
        let v = json_to_cel_value(&serde_json::json!({"a": 1}));
        assert!(matches!(v, CelValue::Map(_)));
    }

    // ── Plugin validation tests ─────────────────────────────────────────

    fn make_plugin(name: &str, ptype: &str, phase: Option<&str>, cmd: &str) -> crate::crd::types::CrdPlugin {
        crate::crd::types::CrdPlugin {
            name: name.to_string(),
            plugin_type: ptype.to_string(),
            phase: phase.map(|s| s.to_string()),
            command: cmd.to_string(),
            timeout: None,
            schedule: None,
            timezone: None,
        }
    }

    #[test]
    fn validate_plugins_rejects_duplicate_names() {
        let plugins = vec![
            make_plugin("dup", "interceptor", Some("webhook.authenticate"), "true"),
            make_plugin("dup", "transformer", Some("webhook.transform"), "cat"),
        ];
        let err = validate_crd_plugins(&plugins).unwrap_err();
        assert!(err.to_string().contains("duplicate plugin name"));
    }

    #[test]
    fn validate_plugins_rejects_unknown_type() {
        let plugins = vec![make_plugin("x", "unknown", Some("foo"), "true")];
        let err = validate_crd_plugins(&plugins).unwrap_err();
        assert!(err.to_string().contains("unknown plugin type"));
    }

    #[test]
    fn validate_plugins_rejects_interceptor_without_phase() {
        let plugins = vec![make_plugin("x", "interceptor", None, "true")];
        let err = validate_crd_plugins(&plugins).unwrap_err();
        assert!(err.to_string().contains("requires a phase"));
    }

    #[test]
    fn validate_plugins_rejects_cron_without_schedule() {
        let plugins = vec![make_plugin("x", "cron", None, "true")];
        let err = validate_crd_plugins(&plugins).unwrap_err();
        assert!(err.to_string().contains("requires a schedule"));
    }

    #[test]
    fn validate_plugins_rejects_invalid_cron_expression() {
        let mut p = make_plugin("x", "cron", None, "true");
        p.schedule = Some("not a cron".to_string());
        let err = validate_crd_plugins(&[p]).unwrap_err();
        assert!(err.to_string().contains("invalid schedule"));
    }

    #[test]
    fn validate_plugins_rejects_empty_command() {
        let plugins = vec![make_plugin("x", "interceptor", Some("webhook.authenticate"), "  ")];
        let err = validate_crd_plugins(&plugins).unwrap_err();
        assert!(err.to_string().contains("command cannot be empty"));
    }

    #[test]
    fn validate_plugins_accepts_valid_plugins() {
        let mut cron = make_plugin("daily", "cron", None, "scripts/rotate.sh");
        cron.schedule = Some("0 0 * * * *".to_string());
        let plugins = vec![
            make_plugin("auth", "interceptor", Some("webhook.authenticate"), "scripts/verify.sh"),
            make_plugin("transform", "transformer", Some("webhook.transform"), "scripts/norm.sh"),
            cron,
        ];
        assert!(validate_crd_plugins(&plugins).is_ok());
    }
}
