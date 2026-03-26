use crate::cli_types::{OrchestratorResource, ResourceKind, ResourceSpec, TriggerSpec};
use crate::config::{
    OrchestratorConfig, TriggerActionConfig, TriggerConfig, TriggerCronConfig, TriggerEventConfig,
    TriggerEventFilterConfig, TriggerFilesystemConfig, TriggerHistoryLimitConfig, TriggerSecretRef,
    TriggerThrottleConfig, TriggerWebhookConfig,
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
        if let Some(ref cron) = self.spec.cron {
            if cron.schedule.trim().is_empty() {
                return Err(anyhow!(
                    "trigger '{}': cron.schedule cannot be empty",
                    self.name()
                ));
            }
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
        let incoming = to_config(&self.spec);
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
                spec: from_config(cfg),
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

fn to_config(spec: &TriggerSpec) -> TriggerConfig {
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
                secret: w.secret.as_ref().map(|s| TriggerSecretRef {
                    from_ref: s.from_ref.clone(),
                }),
                signature_header: w.signature_header.clone(),
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

fn from_config(cfg: &TriggerConfig) -> TriggerSpec {
    use crate::cli_types::{
        TriggerActionSpec, TriggerCronSpec, TriggerEventFilter, TriggerEventSpec,
        TriggerFilesystemSpec, TriggerHistoryLimit, TriggerThrottleSpec, TriggerWebhookSpec,
        WebhookSecretRef,
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
                secret: w.secret.as_ref().map(|s| WebhookSecretRef {
                    from_ref: s.from_ref.clone(),
                }),
                signature_header: w.signature_header.clone(),
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
