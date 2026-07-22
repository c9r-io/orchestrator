use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec, TriggerSpec};
use crate::config::{
    OrchestratorConfig, TriggerActionConfig, TriggerConfig, TriggerCronConfig, TriggerEventConfig,
    TriggerEventFilterConfig, TriggerFilesystemConfig, TriggerHistoryLimitConfig,
    TriggerOutboundCredentialRef, TriggerSecretRef, TriggerThrottleConfig, TriggerWebhookConfig,
};
use anyhow::{Result, anyhow};

use super::{ApplyResult, RegisteredResource, Resource, ResourceMetadata};

#[derive(Debug, Clone)]
/// Builtin manifest adapter for `Trigger` resources.
pub struct TriggerResource {
    /// Resource metadata from the manifest.
    pub metadata: ResourceMetadata,
    /// Manifest spec payload for the trigger.
    pub spec: TriggerSpec,
}

impl Resource for TriggerResource {
    fn kind(&self) -> ResourceKind {
        ResourceKind::Trigger
    }

    fn name(&self) -> &str {
        &self.metadata.name
    }

    fn validate(&self) -> Result<()> {
        super::validate_resource_name(self.name())?;

        // Exactly one of cron or event must be set.
        match (&self.spec.cron, &self.spec.event) {
            (Some(_), Some(_)) => {
                return Err(anyhow!(
                    "trigger '{}': exactly one of 'cron' or 'event' must be set, not both",
                    self.name()
                ));
            }
            (None, None) => {
                return Err(anyhow!(
                    "trigger '{}': exactly one of 'cron' or 'event' must be set",
                    self.name()
                ));
            }
            _ => {}
        }

        // Validate cron expression if present.
        if let Some(ref cron) = self.spec.cron
            && cron.schedule.trim().is_empty()
        {
            return Err(anyhow!(
                "trigger '{}': cron.schedule cannot be empty",
                self.name()
            ));
        }

        // Validate event source if present.
        if let Some(ref event) = self.spec.event {
            let valid_sources = ["task_completed", "task_failed", "webhook", "filesystem"];
            if !valid_sources.contains(&event.source.as_str()) {
                return Err(anyhow!(
                    "trigger '{}': event.source must be one of {:?}, got '{}'",
                    self.name(),
                    valid_sources,
                    event.source,
                ));
            }

            // Filesystem-specific validation.
            if event.source == "filesystem" {
                let fs = event.filesystem.as_ref().ok_or_else(|| {
                    anyhow!(
                        "trigger '{}': source 'filesystem' requires a 'filesystem' configuration block",
                        self.name()
                    )
                })?;
                if fs.paths.is_empty() {
                    return Err(anyhow!(
                        "trigger '{}': filesystem.paths must not be empty",
                        self.name()
                    ));
                }
                let valid_events = ["create", "modify", "delete"];
                for ev in &fs.events {
                    if !valid_events.contains(&ev.as_str()) {
                        return Err(anyhow!(
                            "trigger '{}': filesystem.events must be one of {:?}, got '{}'",
                            self.name(),
                            valid_events,
                            ev,
                        ));
                    }
                }
                if fs.debounce_ms > 60000 {
                    return Err(anyhow!(
                        "trigger '{}': filesystem.debounce_ms must be <= 60000, got {}",
                        self.name(),
                        fs.debounce_ms,
                    ));
                }
            }

            if let Some(webhook) = event.webhook.as_ref()
                && let Some(provider) = webhook.provider.as_deref()
            {
                if event.source != "webhook" {
                    return Err(anyhow!(
                        "trigger '{}': webhook.provider is only valid for event.source=webhook",
                        self.name()
                    ));
                }
                if provider.trim().is_empty() || provider.len() > 64 {
                    return Err(anyhow!(
                        "trigger '{}': webhook.provider must contain 1-64 characters",
                        self.name()
                    ));
                }
                if webhook
                    .installation_id
                    .as_deref()
                    .is_none_or(|value| value.trim().is_empty() || value.len() > 128)
                {
                    return Err(anyhow!(
                        "trigger '{}': source webhook requires installationId",
                        self.name()
                    ));
                }
                if let Some(connection_ref) = webhook.connection_ref.as_deref() {
                    if provider != "slack" {
                        return Err(anyhow!(
                            "trigger '{}': connectionRef currently requires provider=slack",
                            self.name()
                        ));
                    }
                    if connection_ref.trim().is_empty() || connection_ref.len() > 128 {
                        return Err(anyhow!(
                            "trigger '{}': connectionRef must contain 1-128 characters",
                            self.name()
                        ));
                    }
                    if webhook.secret.is_some() || webhook.outbound_credential.is_some() {
                        return Err(anyhow!(
                            "trigger '{}': connectionRef is mutually exclusive with secret and outboundCredential",
                            self.name()
                        ));
                    }
                }
                if provider == "slack" {
                    if webhook.connection_ref.is_none() && webhook.secret.is_none() {
                        return Err(anyhow!(
                            "trigger '{}': Slack source webhook requires SecretStore signing secret",
                            self.name()
                        ));
                    }
                    if !(1..=900).contains(&webhook.timestamp_tolerance_secs) {
                        return Err(anyhow!(
                            "trigger '{}': timestampToleranceSecs must be between 1 and 900",
                            self.name()
                        ));
                    }
                }
                if webhook
                    .actor_roles
                    .values()
                    .any(|role| !matches!(role.as_str(), "read_only" | "operator" | "admin"))
                {
                    return Err(anyhow!(
                        "trigger '{}': actorRoles values must be read_only, operator, or admin",
                        self.name()
                    ));
                }
                if !matches!(webhook.reaction_routing.as_str(), "disabled" | "bindings") {
                    return Err(anyhow!(
                        "trigger '{}': reactionRouting must be disabled or bindings",
                        self.name()
                    ));
                }
                if webhook.reaction_routing == "bindings" && provider != "slack" {
                    return Err(anyhow!(
                        "trigger '{}': reactionRouting=bindings requires provider=slack",
                        self.name()
                    ));
                }
                if webhook.reaction_routing == "bindings" && webhook.connection_ref.is_none() {
                    let outbound = webhook.outbound_credential.as_ref().ok_or_else(|| {
                            anyhow!(
                                "trigger '{}': reactionRouting=bindings requires outboundCredential or connectionRef",
                                self.name()
                            )
                    })?;
                    if outbound.from_ref.trim().is_empty() || outbound.key.trim().is_empty() {
                        return Err(anyhow!(
                            "trigger '{}': outboundCredential.fromRef and key cannot be empty",
                            self.name()
                        ));
                    }
                }
            }
        }

        // Action fields must be non-empty.
        if self.spec.action.workflow.trim().is_empty() {
            return Err(anyhow!(
                "trigger '{}': action.workflow cannot be empty",
                self.name()
            ));
        }
        if self.spec.action.workspace.trim().is_empty() {
            return Err(anyhow!(
                "trigger '{}': action.workspace cannot be empty",
                self.name()
            ));
        }

        Ok(())
    }

    fn apply(&self, config: &mut OrchestratorConfig) -> Result<ApplyResult> {
        let incoming = trigger_spec_to_config(&self.spec);
        let project = config.ensure_project(self.metadata.project.as_deref());
        Ok(super::helpers::apply_to_map(
            &mut project.triggers,
            self.name(),
            incoming,
        ))
    }

    fn to_yaml(&self) -> Result<String> {
        super::manifest_yaml(
            ResourceKind::Trigger,
            &self.metadata,
            ResourceSpec::Trigger(self.spec.clone()),
        )
    }

    fn get_from_project(
        config: &OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> Option<Self> {
        config
            .project(project_id)?
            .triggers
            .get(name)
            .map(|cfg| Self {
                metadata: super::metadata_with_name(name),
                spec: trigger_config_to_spec(cfg),
            })
    }

    fn delete_from_project(
        config: &mut OrchestratorConfig,
        name: &str,
        project_id: Option<&str>,
    ) -> bool {
        config
            .project_mut(project_id)
            .map(|project| project.triggers.remove(name).is_some())
            .unwrap_or(false)
    }
}

/// Builds a typed `TriggerResource` from a generic manifest wrapper.
pub(super) fn build_trigger(resource: OrchestratorResource) -> Result<RegisteredResource> {
    let OrchestratorResource {
        kind,
        metadata,
        spec,
        ..
    } = resource;
    if kind != ResourceKind::Trigger {
        return Err(anyhow!("resource kind/spec mismatch for Trigger"));
    }
    match spec {
        ResourceSpec::Trigger(spec) => Ok(RegisteredResource::Trigger(TriggerResource {
            metadata,
            spec,
        })),
        _ => Err(anyhow!("resource kind/spec mismatch for Trigger")),
    }
}

// ── Spec ↔ Config conversions ────────────────────────────────────────────────

pub(crate) fn trigger_spec_to_config(spec: &TriggerSpec) -> TriggerConfig {
    TriggerConfig {
        cron: spec.cron.as_ref().map(|c| TriggerCronConfig {
            schedule: c.schedule.clone(),
            timezone: c.timezone.clone(),
        }),
        event: spec.event.as_ref().map(|e| TriggerEventConfig {
            source: e.source.clone(),
            filter: e.filter.as_ref().map(|f| TriggerEventFilterConfig {
                workflow: f.workflow.clone(),
                condition: f.condition.clone(),
            }),
            webhook: e.webhook.as_ref().map(|w| TriggerWebhookConfig {
                connection_ref: w.connection_ref.clone(),
                secret: w.secret.as_ref().map(|s| TriggerSecretRef {
                    from_ref: s.from_ref.clone(),
                }),
                outbound_credential: w.outbound_credential.as_ref().map(|credential| {
                    TriggerOutboundCredentialRef {
                        from_ref: credential.from_ref.clone(),
                        key: credential.key.clone(),
                    }
                }),
                signature_header: w.signature_header.clone(),
                crd_ref: w.crd_ref.clone(),
                provider: w.provider.clone(),
                installation_id: w.installation_id.clone(),
                actor_roles: w.actor_roles.clone(),
                reaction_routing: w.reaction_routing.clone(),
                timestamp_tolerance_secs: w.timestamp_tolerance_secs,
            }),
            filesystem: e.filesystem.as_ref().map(|fs| TriggerFilesystemConfig {
                paths: fs.paths.clone(),
                events: fs.events.clone(),
                debounce_ms: fs.debounce_ms,
            }),
        }),
        action: TriggerActionConfig {
            workflow: spec.action.workflow.clone(),
            workspace: spec.action.workspace.clone(),
            args: spec.action.args.clone(),
            start: spec.action.start,
        },
        concurrency_policy: spec.concurrency_policy,
        suspend: spec.suspend,
        history_limit: spec
            .history_limit
            .as_ref()
            .map(|h| TriggerHistoryLimitConfig {
                successful: h.successful,
                failed: h.failed,
            }),
        throttle: spec.throttle.as_ref().map(|t| TriggerThrottleConfig {
            min_interval: t.min_interval,
        }),
    }
}

pub(crate) fn trigger_config_to_spec(cfg: &TriggerConfig) -> TriggerSpec {
    use crate::cli_types::{
        TriggerActionSpec, TriggerCronSpec, TriggerEventFilter, TriggerEventSpec,
        TriggerFilesystemSpec, TriggerHistoryLimit, TriggerThrottleSpec, TriggerWebhookSpec,
        WebhookOutboundCredentialRef, WebhookSecretRef,
    };

    TriggerSpec {
        cron: cfg.cron.as_ref().map(|c| TriggerCronSpec {
            schedule: c.schedule.clone(),
            timezone: c.timezone.clone(),
        }),
        event: cfg.event.as_ref().map(|e| TriggerEventSpec {
            source: e.source.clone(),
            filter: e.filter.as_ref().map(|f| TriggerEventFilter {
                workflow: f.workflow.clone(),
                condition: f.condition.clone(),
            }),
            webhook: e.webhook.as_ref().map(|w| TriggerWebhookSpec {
                connection_ref: w.connection_ref.clone(),
                secret: w.secret.as_ref().map(|s| WebhookSecretRef {
                    from_ref: s.from_ref.clone(),
                }),
                outbound_credential: w.outbound_credential.as_ref().map(|credential| {
                    WebhookOutboundCredentialRef {
                        from_ref: credential.from_ref.clone(),
                        key: credential.key.clone(),
                    }
                }),
                signature_header: w.signature_header.clone(),
                crd_ref: w.crd_ref.clone(),
                provider: w.provider.clone(),
                installation_id: w.installation_id.clone(),
                actor_roles: w.actor_roles.clone(),
                reaction_routing: w.reaction_routing.clone(),
                timestamp_tolerance_secs: w.timestamp_tolerance_secs,
            }),
            filesystem: e.filesystem.as_ref().map(|fs| TriggerFilesystemSpec {
                paths: fs.paths.clone(),
                events: fs.events.clone(),
                debounce_ms: fs.debounce_ms,
            }),
        }),
        action: TriggerActionSpec {
            workflow: cfg.action.workflow.clone(),
            workspace: cfg.action.workspace.clone(),
            args: cfg.action.args.clone(),
            start: cfg.action.start,
        },
        concurrency_policy: cfg.concurrency_policy,
        suspend: cfg.suspend,
        history_limit: cfg.history_limit.as_ref().map(|h| TriggerHistoryLimit {
            successful: h.successful,
            failed: h.failed,
        }),
        throttle: cfg.throttle.as_ref().map(|t| TriggerThrottleSpec {
            min_interval: t.min_interval,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_types::OrchestratorResource;
    use crate::resource::dispatch_resource;

    use super::super::test_fixtures::make_config;

    fn trigger_cron_manifest(name: &str, schedule: &str) -> OrchestratorResource {
        let yaml = format!(
            r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: {name}
spec:
  cron:
    schedule: "{schedule}"
  action:
    workflow: test-wf
    workspace: test-ws
"#,
        );
        serde_yaml::from_str(&yaml).expect("should parse trigger YAML")
    }

    fn trigger_event_manifest(name: &str, source: &str) -> OrchestratorResource {
        let yaml = format!(
            r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: {name}
spec:
  event:
    source: {source}
    filter:
      workflow: my-wf
      condition: "status == 'completed'"
  action:
    workflow: deploy
    workspace: main
  concurrencyPolicy: Replace
"#,
        );
        serde_yaml::from_str(&yaml).expect("should parse trigger event YAML")
    }

    #[test]
    fn trigger_dispatch_and_kind() {
        let resource = dispatch_resource(trigger_cron_manifest("nightly", "0 2 * * *"))
            .expect("dispatch should succeed");
        assert_eq!(resource.kind(), ResourceKind::Trigger);
        assert_eq!(resource.name(), "nightly");
    }

    #[test]
    fn trigger_validate_accepts_valid_cron() {
        let resource = dispatch_resource(trigger_cron_manifest("nightly", "0 2 * * *"))
            .expect("dispatch should succeed");
        assert!(resource.validate().is_ok());
    }

    #[test]
    fn trigger_validate_accepts_valid_event() {
        let resource = dispatch_resource(trigger_event_manifest("on-complete", "task_completed"))
            .expect("dispatch should succeed");
        assert!(resource.validate().is_ok());
    }

    #[test]
    fn reaction_routing_requires_dedicated_outbound_credential() {
        let missing = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata: {name: slack-main}
spec:
  event:
    source: webhook
    webhook:
      provider: slack
      installationId: T1
      reactionRouting: bindings
      secret: {fromRef: slack-signing}
  action: {workflow: docs, workspace: default}
"#;
        let manifest: OrchestratorResource = serde_yaml::from_str(missing).expect("manifest");
        let error = dispatch_resource(manifest)
            .expect("dispatch")
            .validate()
            .expect_err("outbound credential required");
        assert!(error.to_string().contains("requires outboundCredential"));

        let valid = missing.replace(
            "      secret: {fromRef: slack-signing}",
            "      secret: {fromRef: slack-signing}\n      outboundCredential: {fromRef: slack-api, key: BOT_TOKEN}",
        );
        let manifest: OrchestratorResource = serde_yaml::from_str(&valid).expect("manifest");
        assert!(
            dispatch_resource(manifest)
                .expect("dispatch")
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn managed_connection_ref_replaces_both_secret_references() {
        let managed = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata: {name: slack-managed}
spec:
  event:
    source: webhook
    webhook:
      provider: slack
      installationId: install-1
      connectionRef: conn-install-1
      reactionRouting: bindings
  action: {workflow: docs, workspace: default}
"#;
        let manifest: OrchestratorResource = serde_yaml::from_str(managed).expect("manifest");
        assert!(
            dispatch_resource(manifest)
                .expect("dispatch")
                .validate()
                .is_ok()
        );

        let mixed = managed.replace(
            "      connectionRef: conn-install-1",
            "      connectionRef: conn-install-1\n      secret: {fromRef: forbidden}",
        );
        let manifest: OrchestratorResource = serde_yaml::from_str(&mixed).expect("manifest");
        let error = dispatch_resource(manifest)
            .expect("dispatch")
            .validate()
            .expect_err("mixed credential authorities");
        assert!(error.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn trigger_validate_rejects_empty_name() {
        let resource = dispatch_resource(trigger_cron_manifest("", "0 2 * * *"))
            .expect("dispatch should succeed");
        assert!(resource.validate().is_err());
    }

    #[test]
    fn trigger_validate_rejects_both_cron_and_event() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad
spec:
  cron:
    schedule: "0 2 * * *"
  event:
    source: task_completed
  action:
    workflow: wf
    workspace: ws
"#;
        let resource: OrchestratorResource = serde_yaml::from_str(yaml).expect("should parse YAML");
        let registered = dispatch_resource(resource).expect("dispatch");
        let err = registered.validate().expect_err("should reject both");
        assert!(err.to_string().contains("not both"));
    }

    #[test]
    fn trigger_validate_rejects_neither_cron_nor_event() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad
spec:
  action:
    workflow: wf
    workspace: ws
"#;
        let resource: OrchestratorResource = serde_yaml::from_str(yaml).expect("should parse YAML");
        let registered = dispatch_resource(resource).expect("dispatch");
        let err = registered.validate().expect_err("should reject neither");
        assert!(err.to_string().contains("must be set"));
    }

    #[test]
    fn trigger_validate_rejects_invalid_event_source() {
        let resource = dispatch_resource(trigger_event_manifest("bad", "invalid_source"))
            .expect("dispatch should succeed");
        let err = resource.validate().expect_err("should reject");
        assert!(err.to_string().contains("event.source must be one of"));
    }

    #[test]
    fn trigger_apply_created_then_unchanged() {
        let mut config = make_config();
        let resource = dispatch_resource(trigger_cron_manifest("nightly", "0 2 * * *"))
            .expect("dispatch should succeed");
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Created
        );
        assert_eq!(
            resource.apply(&mut config).expect("apply"),
            ApplyResult::Unchanged
        );
    }

    #[test]
    fn trigger_apply_to_project_honors_explicit_scope() {
        let mut config = make_config();
        let resource = dispatch_resource(trigger_event_manifest("scoped", "webhook"))
            .expect("dispatch should succeed");
        crate::resource::apply_to_project(&resource, &mut config, "tenant-a")
            .expect("scoped apply");
        assert!(config.projects["tenant-a"].triggers.contains_key("scoped"));
        assert!(!config.projects["default"].triggers.contains_key("scoped"));
    }

    #[test]
    fn trigger_get_from_and_delete_from() {
        let mut config = make_config();
        let resource = dispatch_resource(trigger_cron_manifest("nightly", "0 2 * * *"))
            .expect("dispatch should succeed");
        resource.apply(&mut config).expect("apply");

        let loaded = TriggerResource::get_from(&config, "nightly");
        assert!(loaded.is_some());

        assert!(TriggerResource::delete_from(&mut config, "nightly"));
        assert!(TriggerResource::get_from(&config, "nightly").is_none());
    }

    #[test]
    fn trigger_to_yaml() {
        let resource = dispatch_resource(trigger_cron_manifest("nightly", "0 2 * * *"))
            .expect("dispatch should succeed");
        let yaml = resource.to_yaml().expect("should serialize");
        assert!(yaml.contains("kind: Trigger"));
        assert!(yaml.contains("nightly"));
    }

    #[test]
    fn trigger_yaml_roundtrip_cron() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: nightly-qa
spec:
  cron:
    schedule: "0 2 * * *"
    timezone: Asia/Shanghai
  action:
    workflow: full-qa
    workspace: main-workspace
  concurrencyPolicy: Forbid
  suspend: false
  historyLimit:
    successful: 3
    failed: 3
"#;
        let resource: OrchestratorResource = serde_yaml::from_str(yaml).expect("should parse YAML");
        resource
            .validate_version()
            .expect("version should be valid");
        assert_eq!(resource.kind, ResourceKind::Trigger);
        if let ResourceSpec::Trigger(ref spec) = resource.spec {
            assert!(spec.cron.is_some());
            assert!(spec.event.is_none());
            assert_eq!(spec.cron.as_ref().unwrap().schedule, "0 2 * * *");
            assert_eq!(
                spec.cron.as_ref().unwrap().timezone.as_deref(),
                Some("Asia/Shanghai")
            );
            assert_eq!(spec.action.workflow, "full-qa");
            assert_eq!(spec.action.workspace, "main-workspace");
            assert!(spec.action.start); // default true
        } else {
            panic!("expected Trigger spec");
        }
    }

    #[test]
    fn trigger_yaml_roundtrip_event() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: auto-deploy
spec:
  event:
    source: task_completed
    filter:
      workflow: full-qa
      condition: "status == 'completed' && unresolved_items == 0"
  action:
    workflow: deploy-staging
    workspace: main-workspace
  concurrencyPolicy: Replace
  throttle:
    minInterval: 300
"#;
        let resource: OrchestratorResource = serde_yaml::from_str(yaml).expect("should parse YAML");
        assert_eq!(resource.kind, ResourceKind::Trigger);
        if let ResourceSpec::Trigger(ref spec) = resource.spec {
            assert!(spec.event.is_some());
            assert!(spec.cron.is_none());
            let event = spec.event.as_ref().unwrap();
            assert_eq!(event.source, "task_completed");
            assert_eq!(
                event.filter.as_ref().unwrap().workflow.as_deref(),
                Some("full-qa")
            );
            assert_eq!(
                spec.concurrency_policy,
                crate::cli_types::ConcurrencyPolicy::Replace
            );
            assert_eq!(spec.throttle.as_ref().unwrap().min_interval, 300);
        } else {
            panic!("expected Trigger spec");
        }
    }

    #[test]
    fn trigger_validate_accepts_filesystem_source() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: fr-watch
spec:
  event:
    source: filesystem
    filesystem:
      paths:
        - docs/feature_request/
      events:
        - create
      debounce_ms: 500
    filter:
      condition: "payload_filename.matches('^FR-.*\\.md$')"
  action:
    workflow: fr-governance
    workspace: default
"#;
        let resource: OrchestratorResource = serde_yaml::from_str(yaml).expect("should parse YAML");
        let registered = dispatch_resource(resource).expect("dispatch");
        assert!(registered.validate().is_ok());
    }

    #[test]
    fn trigger_validate_filesystem_requires_paths() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad-fs
spec:
  event:
    source: filesystem
    filesystem:
      paths: []
  action:
    workflow: wf
    workspace: ws
"#;
        let resource: OrchestratorResource = serde_yaml::from_str(yaml).expect("should parse YAML");
        let registered = dispatch_resource(resource).expect("dispatch");
        let err = registered
            .validate()
            .expect_err("should reject empty paths");
        assert!(err.to_string().contains("paths must not be empty"));
    }

    #[test]
    fn trigger_validate_filesystem_requires_block() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad-fs
spec:
  event:
    source: filesystem
  action:
    workflow: wf
    workspace: ws
"#;
        let resource: OrchestratorResource = serde_yaml::from_str(yaml).expect("should parse YAML");
        let registered = dispatch_resource(resource).expect("dispatch");
        let err = registered
            .validate()
            .expect_err("should reject missing filesystem");
        assert!(
            err.to_string()
                .contains("requires a 'filesystem' configuration block")
        );
    }

    #[test]
    fn trigger_validate_filesystem_rejects_invalid_events() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bad-fs
spec:
  event:
    source: filesystem
    filesystem:
      paths:
        - src/
      events:
        - invalid_event
  action:
    workflow: wf
    workspace: ws
"#;
        let resource: OrchestratorResource = serde_yaml::from_str(yaml).expect("should parse YAML");
        let registered = dispatch_resource(resource).expect("dispatch");
        let err = registered
            .validate()
            .expect_err("should reject invalid events");
        assert!(err.to_string().contains("filesystem.events must be one of"));
    }

    #[test]
    fn trigger_yaml_roundtrip_filesystem() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: fr-watch
spec:
  event:
    source: filesystem
    filesystem:
      paths:
        - docs/feature_request/
      events:
        - create
      debounce_ms: 1000
  action:
    workflow: fr-governance
    workspace: default
  concurrencyPolicy: Forbid
"#;
        let resource: OrchestratorResource = serde_yaml::from_str(yaml).expect("should parse YAML");
        assert_eq!(resource.kind, ResourceKind::Trigger);
        if let ResourceSpec::Trigger(ref spec) = resource.spec {
            let event = spec.event.as_ref().unwrap();
            assert_eq!(event.source, "filesystem");
            let fs = event.filesystem.as_ref().unwrap();
            assert_eq!(fs.paths, vec!["docs/feature_request/"]);
            assert_eq!(fs.events, vec!["create"]);
            assert_eq!(fs.debounce_ms, 1000);
        } else {
            panic!("expected Trigger spec");
        }
    }
}
