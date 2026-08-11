//! Behavioural coverage for the apply path's action-audit completeness (FR-164).
//!
//! Before FR-164 `audited_mutation` required an audit envelope, raw driver args,
//! or one of two Source kinds. Every other apply — Workflow, SecretStore, Trigger,
//! RuntimePolicy — wrote no `control_action_audit` row at all. Because the first
//! disjunct was `context.is_some()`, `action_audit::begin` was never reached for
//! an envelope-less caller, so the `enforced`-mode rejection inside
//! `resolve_context` was unreachable in exactly the case it exists to refuse.
//!
//! These assertions read `control_action_audit` and nothing else, deliberately.
//! Every successful non-dry-run apply also writes `resource_versions` and
//! `orchestrator_config_versions` rows whose `author` is the constant
//! `"daemon-apply"`, so an assertion phrased as "an audit row exists" would be
//! satisfied by a row written whether or not this FR works.
//!
//! They run against a real [`OrchestratorServer`] and call the production
//! `resource::apply` handler. The gRPC harness in `crates/integration-tests`
//! cannot be used here: its `apply` reimplements the RPC by calling
//! `apply_manifests` directly, so it never enters the audit path under test.

use std::sync::Arc;
use std::time::Duration;

use agent_orchestrator::action_audit::{ActionAuditFilter, AsyncActionAuditRepository};
use agent_orchestrator::test_utils::TestState;
use orchestrator_proto::{ActionAuditContext, ApplyRequest};
use tokio::sync::{Mutex, Notify};
use tonic::{Code, Request};

use super::{OrchestratorServer, resource};

struct ApplyFixture {
    server: OrchestratorServer,
    state: TestState,
}

impl ApplyFixture {
    fn new() -> Self {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let slack = orchestrator_slack_gateway::slack::SlackClient::new(
            "http://127.0.0.1:9",
            Duration::from_millis(50),
        )
        .expect("test Slack client");
        let server = OrchestratorServer::new(
            state,
            Arc::new(Notify::new()),
            None,
            None,
            None,
            Arc::new(slack),
            Arc::new(Mutex::new(())),
        );
        Self {
            server,
            state: fixture,
        }
    }

    fn workspace_root(&self) -> String {
        self.state
            .temp_root()
            .join("workspace/default")
            .display()
            .to_string()
    }

    async fn apply(&self, content: &str, audit: Option<ActionAuditContext>) -> Result<(), String> {
        resource::apply(
            &self.server,
            Request::new(ApplyRequest {
                content: content.to_string(),
                dry_run: false,
                prune: false,
                project: Some("default".into()),
                audit,
                expected_revision: None,
                require_absent: false,
            }),
        )
        .await
        .map(|_| ())
        .map_err(|status| format!("{}: {}", status.code() as i32, status.message()))
    }

    async fn rows(
        &self,
        action: Option<&str>,
    ) -> Vec<agent_orchestrator::action_audit::ActionAuditRecord> {
        AsyncActionAuditRepository::new(self.server.state.async_database.clone())
            .list(ActionAuditFilter {
                project_id: "default".into(),
                action: action.map(str::to_owned),
                limit: 50,
                ..Default::default()
            })
            .await
            .expect("list action audit")
    }
}

const SECRET_STORE: &str = r#"apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: fr164-store
spec:
  data:
    token: fr164-secret-value
"#;

const ENV_STORE: &str = r#"apiVersion: orchestrator.dev/v2
kind: EnvStore
metadata:
  name: fr164-env
spec:
  data:
    KEY: value
"#;

/// Requirement 1: an envelope-less mutating apply leaves a named row.
///
/// Asserts the action name and the `legacy_client` reason code together. The
/// reason code distinguishes "the daemon audited an envelope-less client" from
/// "some row happens to be here"; the action name is what the FR adds. A
/// SecretStore is the subject on purpose — it is the kind `tool secret-rotate`
/// writes, and the one whose silent mutation matters most.
#[tokio::test]
async fn envelope_less_secret_store_apply_is_audited() {
    let fixture = ApplyFixture::new();
    fixture
        .apply(SECRET_STORE, None)
        .await
        .expect("envelope-less SecretStore apply should succeed");

    let rows = fixture.rows(Some("resource.secret_store.apply")).await;
    assert_eq!(
        rows.len(),
        1,
        "envelope-less SecretStore apply must leave exactly one named audit row"
    );
    assert_eq!(rows[0].target_type, "secret_store");
    assert_eq!(rows[0].target_id, "SecretStore/fr164-store");
    assert_eq!(
        rows[0].reason_code, "legacy_client",
        "an envelope-less client must be recorded as legacy_client, not dropped"
    );
    assert_eq!(rows[0].status, "succeeded");
}

/// A dry run is not a mutation and must stay unaudited — the one case the
/// unconditional `!dry_run` must still exclude.
#[tokio::test]
async fn dry_run_apply_is_not_audited() {
    let fixture = ApplyFixture::new();
    resource::apply(
        &fixture.server,
        Request::new(ApplyRequest {
            content: SECRET_STORE.to_string(),
            dry_run: true,
            prune: false,
            project: Some("default".into()),
            audit: None,
            expected_revision: None,
            require_absent: false,
        }),
    )
    .await
    .expect("dry-run apply should succeed");

    assert!(
        fixture.rows(None).await.is_empty(),
        "a dry run must not reserve an audit envelope"
    );
}

/// The bundle decision: one aggregate row per request, never one per document.
///
/// Pins the negative half too. An implementation that expanded documents into
/// per-kind rows would still satisfy a bare "a row exists" check, so the absence
/// of per-kind rows is asserted explicitly.
#[tokio::test]
async fn multi_document_bundle_records_one_aggregate_row() {
    let fixture = ApplyFixture::new();
    let bundle = format!("{SECRET_STORE}---\n{ENV_STORE}");
    fixture
        .apply(&bundle, None)
        .await
        .expect("bundle apply should succeed");

    let rows = fixture.rows(None).await;
    assert_eq!(rows.len(), 1, "a bundle must record exactly one envelope");
    assert_eq!(rows[0].action, "resource.apply");
    assert_eq!(rows[0].target_type, "resource_manifest");
    assert!(
        rows[0].target_id.starts_with("manifest:"),
        "aggregate row identifies the bundle by content hash, got {}",
        rows[0].target_id
    );
}

/// Requirement 1, enforced half: the rejection that could not previously fire.
///
/// Asserts the diagnostic string, not only the gRPC code. `InvalidArgument` is
/// also returned by manifest parse failures and project mismatches, so a
/// code-only check would pass on an apply that never reached the audit layer —
/// which is precisely the pre-FR behaviour this test must fail against.
#[tokio::test]
async fn enforced_mode_rejects_envelope_less_apply() {
    let fixture = ApplyFixture::new();
    fixture
        .apply(
            r#"apiVersion: orchestrator.dev/v2
kind: RuntimePolicy
metadata:
  name: default
spec:
  action_audit_mode: enforced
  resume:
    auto: false
  runner:
    shell: /bin/bash
    shell_arg: -lc
    policy: allowlist
    executor: shell
    allowed_shells: [/bin/bash, /bin/sh, sh]
    allowed_shell_args: [-lc, -c]
"#,
            Some(ActionAuditContext {
                reason_code: "test_seed_policy".into(),
                operator_reason: None,
                idempotency_key: Some("fr164-seed-policy".into()),
            }),
        )
        .await
        .expect("seeding the enforced policy should succeed");

    let error = fixture
        .apply(SECRET_STORE, None)
        .await
        .expect_err("enforced mode must reject an envelope-less apply");
    assert!(
        error.contains("action audit context is required"),
        "expected the enforced-mode envelope diagnostic, got: {error}"
    );
    assert!(
        error.starts_with(&format!("{}: ", Code::InvalidArgument as i32)),
        "expected InvalidArgument, got: {error}"
    );

    assert!(
        fixture
            .rows(Some("resource.secret_store.apply"))
            .await
            .is_empty(),
        "a rejected apply must not record a succeeded mutation row"
    );
}

/// Requirement 2: each of the twelve kinds records its own action name.
///
/// The expectation is asserted per kind rather than by count — a count-only
/// check passes when two kinds swap names, which is the confusion this FR
/// exists to end. Kinds are applied in dependency order so that cross-reference
/// validation succeeds. The list is checked against `ResourceKind` by the
/// wildcard-free matches in `resource.rs` and by `apply_action_naming`; here it
/// is the observable audit row, not the mapping, that is under test.
#[tokio::test]
async fn every_resource_kind_records_its_named_action() {
    let fixture = ApplyFixture::new();
    let ws_root = fixture.workspace_root();

    let documents: Vec<(&str, &str, String)> = vec![
        (
            "resource.workspace.apply",
            "workspace",
            format!(
                "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: fr164\nspec:\n  root_path: \"{ws_root}\"\n  qa_targets: [docs/qa/orchestrator]\n  ticket_dir: docs/ticket\n"
            ),
        ),
        (
            "resource.agent.apply",
            "agent",
            "apiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: fr164\nspec:\n  driver:\n    provider: shell\n    transport: cli\n  capabilities: [fr164_cap]\n  command: \"echo fr164\"\n".into(),
        ),
        (
            "resource.workflow.apply",
            "workflow",
            "apiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: fr164\nspec:\n  steps:\n    - id: inspect\n      type: fr164_cap\n      required_capability: fr164_cap\n      enabled: true\n      behavior:\n        side_effect_class: none\n  loop:\n    mode: once\n".into(),
        ),
        (
            "resource.project.apply",
            "project",
            "apiVersion: orchestrator.dev/v2\nkind: Project\nmetadata:\n  name: fr164-project\nspec:\n  description: FR-164 audit naming coverage\n".into(),
        ),
        (
            "resource.runtime_policy.apply",
            "runtime_policy",
            "apiVersion: orchestrator.dev/v2\nkind: RuntimePolicy\nmetadata:\n  name: default\nspec:\n  resume:\n    auto: false\n  runner:\n    shell: /bin/bash\n    shell_arg: -lc\n    policy: allowlist\n    executor: shell\n    allowed_shells: [/bin/bash, /bin/sh, sh]\n    allowed_shell_args: [-lc, -c]\n".into(),
        ),
        (
            "resource.step_template.apply",
            "step_template",
            "apiVersion: orchestrator.dev/v2\nkind: StepTemplate\nmetadata:\n  name: fr164\nspec:\n  description: FR-164 template\n  prompt: \"{goal}\"\n".into(),
        ),
        (
            "resource.execution_profile.apply",
            "execution_profile",
            "apiVersion: orchestrator.dev/v2\nkind: ExecutionProfile\nmetadata:\n  name: fr164\nspec:\n  mode: sandbox\n  fs_mode: workspace_rw_scoped\n  network_mode: deny\n".into(),
        ),
        ("resource.env_store.apply", "env_store", ENV_STORE.into()),
        (
            "resource.secret_store.apply",
            "secret_store",
            "apiVersion: orchestrator.dev/v2\nkind: SecretStore\nmetadata:\n  name: fr164-secret\nspec:\n  data:\n    signing: fr164-signing\n    bot-token: fr164-token\n".into(),
        ),
        (
            "source.template.apply",
            "source_task_template",
            "apiVersion: orchestrator.dev/v2\nkind: SourceTaskTemplate\nmetadata:\n  name: fr164-template\nspec:\n  skill:\n    name: code-analysis\n    invocation: \"$code-analysis\"\n    args: [\"--concise\"]\n  action:\n    workflow: fr164\n    workspace: fr164\n    start: false\n  goalTemplate: \"{skill_invocation}: {source_message_url}\"\n  allowedVariables: [skill_invocation, source_message_url]\n".into(),
        ),
        (
            "resource.trigger.apply",
            "trigger",
            "apiVersion: orchestrator.dev/v2\nkind: Trigger\nmetadata:\n  name: fr164-trigger\nspec:\n  event:\n    source: webhook\n    webhook:\n      provider: slack\n      installationId: T_FR164\n      actorRoles:\n        U_OPERATOR: operator\n      reactionRouting: bindings\n      secret:\n        fromRef: fr164-secret\n      outboundCredential:\n        fromRef: fr164-secret\n        key: bot-token\n  action:\n    workflow: fr164\n    workspace: fr164\n    start: false\n  concurrencyPolicy: Allow\n".into(),
        ),
        (
            "source.binding.apply",
            "source_task_binding",
            "apiVersion: orchestrator.dev/v2\nkind: SourceTaskBinding\nmetadata:\n  name: fr164-binding\nspec:\n  triggerRef: fr164-trigger\n  match:\n    eventKind: reaction_added\n    reaction: fr164-analyze\n    targetKind: message\n    channels: [C_FR164]\n  templateRef: fr164-template\n  allowedActorRoles: [operator]\n  suspend: false\n".into(),
        ),
    ];

    assert_eq!(
        documents.len(),
        12,
        "every ResourceKind variant must be exercised"
    );

    for (expected_action, expected_target_type, manifest) in &documents {
        fixture
            .apply(manifest, None)
            .await
            .unwrap_or_else(|error| panic!("apply for {expected_action} failed: {error}"));

        let rows = fixture.rows(Some(expected_action)).await;
        assert_eq!(
            rows.len(),
            1,
            "{expected_action} must record exactly one row"
        );
        assert_eq!(&rows[0].target_type, expected_target_type);
        assert_eq!(rows[0].status, "succeeded");
        assert_eq!(rows[0].reason_code, "legacy_client");
    }
}
