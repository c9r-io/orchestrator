//! Behavioural coverage for the delete path's action-audit completeness (FR-167).
//!
//! Before FR-167 the guard was `force_references || is_source_task_binding`, so
//! eleven of the twelve kinds wrote no `control_action_audit` row when deleted.
//! Unlike apply's pre-FR-164 condition it had no `context.is_some()` disjunct at
//! all: an envelope was accepted, ignored and discarded, and the CLI sends one on
//! every delete — so the default path was the dropped one. With
//! `action_audit::begin` unreachable, the `enforced`-mode rejection inside
//! `resolve_context` could not fire either, which meant the mode neither audited
//! an ordinary delete nor refused it.
//!
//! These assertions read `control_action_audit` and nothing else, deliberately.
//! A successful delete also writes a `resource_versions` tombstone
//! (`version = -1`, `spec_json = '"deleted"'`) whose `author` is the constant
//! `"daemon-delete"`, so an assertion phrased as "a row exists" would be
//! satisfied by a row written whether or not this FR works.
//!
//! They run against a real [`OrchestratorServer`] and call the production
//! `resource::delete` handler. The gRPC harness in `crates/integration-tests`
//! cannot be used here: its `delete` reimplements the RPC by calling
//! `delete_resource` directly, so it never enters the audit path under test —
//! the same structural blindness FR-164 found in its `apply`.

use std::sync::Arc;
use std::time::Duration;

use agent_orchestrator::action_audit::{ActionAuditFilter, AsyncActionAuditRepository};
use agent_orchestrator::test_utils::TestState;
use orchestrator_proto::{ActionAuditContext, ApplyRequest, DeleteRequest};
use tokio::sync::{Mutex, Notify};
use tonic::{Code, Request};

use super::{OrchestratorServer, resource};

struct DeleteFixture {
    server: OrchestratorServer,
    state: TestState,
}

impl DeleteFixture {
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

    async fn delete(&self, target: &str, audit: Option<ActionAuditContext>) -> Result<(), String> {
        self.delete_request(DeleteRequest {
            resource: target.to_string(),
            force: true,
            project: Some("default".into()),
            dry_run: false,
            force_references: false,
            audit,
        })
        .await
    }

    async fn delete_request(&self, request: DeleteRequest) -> Result<(), String> {
        resource::delete(&self.server, Request::new(request))
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
                limit: 100,
                ..Default::default()
            })
            .await
            .expect("list action audit")
    }
}

const SECRET_STORE: &str = r#"apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: fr167-store
spec:
  data:
    token: fr167-secret-value
"#;

const ENFORCED_POLICY: &str = r#"apiVersion: orchestrator.dev/v2
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
"#;

/// Requirement 1: an envelope-less delete leaves a named row.
///
/// A SecretStore is the subject on purpose — it is the kind whose silent removal
/// matters most, and the one FR-164 used for the same assertion on apply, so the
/// two halves of the mutation surface are pinned by the same object.
#[tokio::test]
async fn envelope_less_secret_store_delete_is_audited() {
    let fixture = DeleteFixture::new();
    fixture
        .apply(SECRET_STORE, None)
        .await
        .expect("seeding the SecretStore should succeed");

    fixture
        .delete("secretstore/fr167-store", None)
        .await
        .expect("envelope-less SecretStore delete should succeed");

    let rows = fixture.rows(Some("resource.secret_store.delete")).await;
    assert_eq!(
        rows.len(),
        1,
        "envelope-less SecretStore delete must leave exactly one named audit row"
    );
    assert_eq!(rows[0].target_type, "secret_store");
    assert_eq!(rows[0].target_id, "secretstore/fr167-store");
    assert_eq!(
        rows[0].reason_code, "legacy_client",
        "an envelope-less client must be recorded as legacy_client, not dropped"
    );
    assert_eq!(rows[0].status, "succeeded");
}

/// The branch that separates FR-167 from FR-164, and the one that is red today.
///
/// Apply's old condition began with `context.is_some()`, so a client that sent an
/// envelope was still audited; delete's did not, so the envelope was received and
/// thrown away. Asserting the *client's* reason code rather than merely "a row
/// exists" is what distinguishes "the envelope was honoured" from "something got
/// recorded": under the old guard this returns zero rows, and under a fix that
/// audited unconditionally while ignoring the context it would return one row
/// reading `legacy_client`.
#[tokio::test]
async fn enveloped_delete_preserves_the_clients_reason_code() {
    let fixture = DeleteFixture::new();
    fixture
        .apply(SECRET_STORE, None)
        .await
        .expect("seeding the SecretStore should succeed");

    fixture
        .delete(
            "secretstore/fr167-store",
            Some(ActionAuditContext {
                reason_code: "operator_resource_delete".into(),
                operator_reason: Some("rotating the signing store".into()),
                idempotency_key: Some("fr167-delete-key".into()),
            }),
        )
        .await
        .expect("enveloped SecretStore delete should succeed");

    let rows = fixture.rows(Some("resource.secret_store.delete")).await;
    assert_eq!(rows.len(), 1, "an enveloped delete must record one row");
    assert_eq!(
        rows[0].reason_code, "operator_resource_delete",
        "the client's reason code was replaced or discarded"
    );
    assert_eq!(
        rows[0].operator_reason.as_deref(),
        Some("rotating the signing store"),
        "the client's operator reason was discarded"
    );
    assert_eq!(
        rows[0].idempotency_key.as_deref(),
        Some("fr167-delete-key"),
        "the client's retry identity was discarded"
    );
    assert_eq!(rows[0].status, "succeeded");
}

/// A dry run is not a mutation and must stay unaudited — the one case the
/// unconditional `!dry_run` must still exclude, asserted separately so that
/// "audit everything" cannot be over-applied.
#[tokio::test]
async fn dry_run_delete_is_not_audited() {
    let fixture = DeleteFixture::new();
    fixture
        .apply(SECRET_STORE, None)
        .await
        .expect("seeding the SecretStore should succeed");
    let seeded = fixture.rows(None).await.len();

    fixture
        .delete_request(DeleteRequest {
            resource: "secretstore/fr167-store".into(),
            force: true,
            project: Some("default".into()),
            dry_run: true,
            force_references: false,
            audit: None,
        })
        .await
        .expect("dry-run delete should succeed");

    assert_eq!(
        fixture.rows(None).await.len(),
        seeded,
        "a dry-run delete must not reserve an audit envelope"
    );
    assert!(
        fixture
            .rows(Some("resource.secret_store.delete"))
            .await
            .is_empty(),
        "a dry-run delete must record no delete action"
    );
}

/// Requirement 1, enforced half: the rejection that could not previously fire.
///
/// The **diagnostic string** is asserted, not only the gRPC code. `InvalidArgument`
/// is also what a delete without `--force` and a malformed `kind/name` return, so
/// a code-only check would pass on a delete that never reached the audit layer —
/// which is precisely the pre-FR behaviour this test must fail against.
#[tokio::test]
async fn enforced_mode_rejects_envelope_less_delete() {
    let fixture = DeleteFixture::new();
    fixture
        .apply(SECRET_STORE, None)
        .await
        .expect("seeding the SecretStore should succeed");
    fixture
        .apply(
            ENFORCED_POLICY,
            Some(ActionAuditContext {
                reason_code: "test_seed_policy".into(),
                operator_reason: None,
                idempotency_key: Some("fr167-seed-policy".into()),
            }),
        )
        .await
        .expect("seeding the enforced policy should succeed");

    let error = fixture
        .delete("secretstore/fr167-store", None)
        .await
        .expect_err("enforced mode must reject an envelope-less delete");
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
            .rows(Some("resource.secret_store.delete"))
            .await
            .is_empty(),
        "a rejected delete must not record a mutation row"
    );

    // And the refusal is a refusal, not a silent no-op: the store is still there.
    fixture
        .delete(
            "secretstore/fr167-store",
            Some(ActionAuditContext {
                reason_code: "operator_resource_delete".into(),
                operator_reason: None,
                idempotency_key: Some("fr167-enforced-delete".into()),
            }),
        )
        .await
        .expect("an enveloped delete must still succeed under enforced mode");
}

/// Requirement 2: each of the twelve kinds records its own delete action.
///
/// Asserted per kind rather than by count — a count-only check passes when two
/// kinds swap names, which is the confusion the FR exists to end. Deletes run in
/// reverse dependency order so that reference guards do not refuse them.
///
/// `RuntimePolicy` is the one asymmetry and it is asserted rather than hidden:
/// the kind is not deletable at all (`canonical_project_kind` has no arm for it),
/// so the row records `status = failed`. That the row exists is the point — the
/// audit envelope is reserved before execution, so an *attempt* to delete a
/// runtime policy is recorded under its own name even though it cannot succeed.
#[tokio::test]
async fn every_resource_kind_records_its_named_delete_action() {
    let fixture = DeleteFixture::new();
    let ws_root = fixture.workspace_root();

    // (action, target_type, delete target, manifest)
    let kinds: Vec<(&str, &str, &str, String)> = vec![
        (
            "resource.workspace.delete",
            "workspace",
            "workspace/fr167",
            format!(
                "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: fr167\nspec:\n  root_path: \"{ws_root}\"\n  qa_targets: [docs/qa/orchestrator]\n  ticket_dir: docs/ticket\n"
            ),
        ),
        (
            "resource.agent.delete",
            "agent",
            "agent/fr167",
            "apiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: fr167\nspec:\n  driver:\n    provider: shell\n    transport: cli\n  capabilities: [fr167_cap]\n  command: \"echo fr167\"\n".into(),
        ),
        (
            "resource.workflow.delete",
            "workflow",
            "workflow/fr167",
            "apiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: fr167\nspec:\n  steps:\n    - id: inspect\n      type: fr167_cap\n      required_capability: fr167_cap\n      enabled: true\n      behavior:\n        side_effect_class: none\n  loop:\n    mode: once\n".into(),
        ),
        (
            "resource.project.delete",
            "project",
            "project/fr167-project",
            "apiVersion: orchestrator.dev/v2\nkind: Project\nmetadata:\n  name: fr167-project\nspec:\n  description: FR-167 audit naming coverage\n".into(),
        ),
        (
            "resource.runtime_policy.delete",
            "runtime_policy",
            "runtimepolicy/default",
            ENFORCED_POLICY.replace("action_audit_mode: enforced\n  ", ""),
        ),
        (
            "resource.step_template.delete",
            "step_template",
            "steptemplate/fr167",
            "apiVersion: orchestrator.dev/v2\nkind: StepTemplate\nmetadata:\n  name: fr167\nspec:\n  description: FR-167 template\n  prompt: \"{goal}\"\n".into(),
        ),
        (
            "resource.execution_profile.delete",
            "execution_profile",
            "executionprofile/fr167",
            "apiVersion: orchestrator.dev/v2\nkind: ExecutionProfile\nmetadata:\n  name: fr167\nspec:\n  mode: sandbox\n  fs_mode: workspace_rw_scoped\n  network_mode: deny\n".into(),
        ),
        (
            "resource.env_store.delete",
            "env_store",
            "envstore/fr167-env",
            "apiVersion: orchestrator.dev/v2\nkind: EnvStore\nmetadata:\n  name: fr167-env\nspec:\n  data:\n    KEY: value\n".into(),
        ),
        (
            "resource.secret_store.delete",
            "secret_store",
            "secretstore/fr167-secret",
            "apiVersion: orchestrator.dev/v2\nkind: SecretStore\nmetadata:\n  name: fr167-secret\nspec:\n  data:\n    signing: fr167-signing\n    bot-token: fr167-token\n".into(),
        ),
        (
            "source.template.delete",
            "source_task_template",
            "sourcetasktemplate/fr167-template",
            "apiVersion: orchestrator.dev/v2\nkind: SourceTaskTemplate\nmetadata:\n  name: fr167-template\nspec:\n  skill:\n    name: code-analysis\n    invocation: \"$code-analysis\"\n    args: [\"--concise\"]\n  action:\n    workflow: fr167\n    workspace: fr167\n    start: false\n  goalTemplate: \"{skill_invocation}: {source_message_url}\"\n  allowedVariables: [skill_invocation, source_message_url]\n".into(),
        ),
        (
            "resource.trigger.delete",
            "trigger",
            "trigger/fr167-trigger",
            "apiVersion: orchestrator.dev/v2\nkind: Trigger\nmetadata:\n  name: fr167-trigger\nspec:\n  event:\n    source: webhook\n    webhook:\n      provider: slack\n      installationId: T_FR167\n      actorRoles:\n        U_OPERATOR: operator\n      reactionRouting: bindings\n      secret:\n        fromRef: fr167-secret\n      outboundCredential:\n        fromRef: fr167-secret\n        key: bot-token\n  action:\n    workflow: fr167\n    workspace: fr167\n    start: false\n  concurrencyPolicy: Allow\n".into(),
        ),
        (
            "source.binding.delete",
            "source_task_binding",
            "sourcetaskbinding/fr167-binding",
            "apiVersion: orchestrator.dev/v2\nkind: SourceTaskBinding\nmetadata:\n  name: fr167-binding\nspec:\n  triggerRef: fr167-trigger\n  match:\n    eventKind: reaction_added\n    reaction: fr167-analyze\n    targetKind: message\n    channels: [C_FR167]\n  templateRef: fr167-template\n  allowedActorRoles: [operator]\n  suspend: false\n".into(),
        ),
    ];

    assert_eq!(
        kinds.len(),
        agent_orchestrator::resource::ALL_RESOURCE_KINDS.len(),
        "every ResourceKind variant must be exercised"
    );

    for (action, _, _, manifest) in &kinds {
        fixture
            .apply(manifest, None)
            .await
            .unwrap_or_else(|error| panic!("seeding apply for {action} failed: {error}"));
    }

    // Reverse dependency order: a binding references a trigger and a template,
    // and a template's delete is refused while a binding still points at it.
    for (action, target_type, target, _) in kinds.iter().rev() {
        let outcome = fixture.delete(target, None).await;

        let rows = fixture.rows(Some(action)).await;
        assert_eq!(rows.len(), 1, "{action} must record exactly one row");
        assert_eq!(&rows[0].target_type, target_type);
        assert_eq!(&rows[0].target_id, target);
        assert_eq!(rows[0].reason_code, "legacy_client");

        if *action == "resource.runtime_policy.delete" {
            // Named, recorded, and refused. The kind has no ProjectConfig map to
            // remove it from, so the delete cannot succeed; the attempt is still
            // attributable, which is what an audit trail is for.
            let error = outcome.expect_err("a RuntimePolicy delete cannot succeed");
            assert!(
                error.contains("unknown resource type for project delete: runtimepolicy"),
                "expected the RuntimePolicy refusal diagnostic, got: {error}"
            );
            assert_eq!(
                rows[0].status, "failed",
                "a refused delete must be recorded as failed, not succeeded"
            );
        } else {
            outcome.unwrap_or_else(|error| panic!("delete for {action} failed: {error}"));
            assert_eq!(rows[0].status, "succeeded", "{action} did not succeed");
        }
    }
}

/// The surface outside `ResourceKind`. A CRD and a CRD-defined custom resource
/// are both deletable and, before FR-167, both unaudited. They resolve to no
/// kind, so they record the generic name — mirroring how an apply that resolves
/// to no single builtin manifest records `resource.apply` / `resource_manifest`.
#[tokio::test]
async fn crd_and_custom_resource_deletes_record_the_generic_action() {
    let fixture = DeleteFixture::new();
    fixture
        .apply(
            r#"apiVersion: orchestrator.dev/v2
kind: CustomResourceDefinition
metadata:
  name: fr167libraries.orchestrator.dev
spec:
  kind: Fr167Library
  plural: fr167libraries
  group: orchestrator.dev
  versions:
    - name: v1
      served: true
      schema:
        type: object
        properties:
          note:
            type: string
"#,
            None,
        )
        .await
        .expect("seeding the CRD should succeed");
    fixture
        .apply(
            "apiVersion: orchestrator.dev/v1\nkind: Fr167Library\nmetadata:\n  name: fr167-lib\nspec:\n  note: hello\n",
            None,
        )
        .await
        .expect("seeding the custom resource should succeed");

    fixture
        .delete("fr167library/fr167-lib", None)
        .await
        .expect("custom resource delete should succeed");
    fixture
        .delete("crd/Fr167Library", None)
        .await
        .expect("CRD delete should succeed");

    let rows = fixture.rows(Some("resource.delete")).await;
    assert_eq!(
        rows.len(),
        2,
        "both deletes outside ResourceKind must record the generic action"
    );
    for row in &rows {
        assert_eq!(row.target_type, "resource_manifest");
        assert_eq!(row.status, "succeeded");
        assert_eq!(row.reason_code, "legacy_client");
    }
    let mut targets: Vec<&str> = rows.iter().map(|row| row.target_id.as_str()).collect();
    targets.sort_unstable();
    assert_eq!(
        targets,
        vec!["crd/Fr167Library", "fr167library/fr167-lib"],
        "the generic row must still identify what was deleted"
    );
}

/// Regression guard on the two actions that already existed.
///
/// `delete_references` is a cross-resource cleanup — it removes bindings the
/// caller never named — so it keeps its own name rather than joining the per-kind
/// surface, and the SourceTaskTemplate it targets must *not* also produce a
/// `source.template.delete` row for the same request. The negative half is
/// asserted because an implementation that recorded both would satisfy a bare
/// "delete_references exists" check.
#[tokio::test]
async fn force_references_still_records_delete_references_alone() {
    let fixture = DeleteFixture::new();
    let ws_root = fixture.workspace_root();
    for manifest in [
        format!(
            "apiVersion: orchestrator.dev/v2\nkind: Workspace\nmetadata:\n  name: fr167\nspec:\n  root_path: \"{ws_root}\"\n  qa_targets: [docs/qa/orchestrator]\n  ticket_dir: docs/ticket\n"
        ),
        "apiVersion: orchestrator.dev/v2\nkind: Agent\nmetadata:\n  name: fr167\nspec:\n  driver:\n    provider: shell\n    transport: cli\n  capabilities: [fr167_cap]\n  command: \"echo fr167\"\n".into(),
        "apiVersion: orchestrator.dev/v2\nkind: Workflow\nmetadata:\n  name: fr167\nspec:\n  steps:\n    - id: inspect\n      type: fr167_cap\n      required_capability: fr167_cap\n      enabled: true\n      behavior:\n        side_effect_class: none\n  loop:\n    mode: once\n".into(),
        "apiVersion: orchestrator.dev/v2\nkind: SecretStore\nmetadata:\n  name: fr167-secret\nspec:\n  data:\n    signing: fr167-signing\n    bot-token: fr167-token\n".into(),
        "apiVersion: orchestrator.dev/v2\nkind: SourceTaskTemplate\nmetadata:\n  name: fr167-template\nspec:\n  skill:\n    name: code-analysis\n    invocation: \"$code-analysis\"\n    args: [\"--concise\"]\n  action:\n    workflow: fr167\n    workspace: fr167\n    start: false\n  goalTemplate: \"{skill_invocation}: {source_message_url}\"\n  allowedVariables: [skill_invocation, source_message_url]\n".into(),
        "apiVersion: orchestrator.dev/v2\nkind: Trigger\nmetadata:\n  name: fr167-trigger\nspec:\n  event:\n    source: webhook\n    webhook:\n      provider: slack\n      installationId: T_FR167\n      actorRoles:\n        U_OPERATOR: operator\n      reactionRouting: bindings\n      secret:\n        fromRef: fr167-secret\n      outboundCredential:\n        fromRef: fr167-secret\n        key: bot-token\n  action:\n    workflow: fr167\n    workspace: fr167\n    start: false\n  concurrencyPolicy: Allow\n".into(),
        "apiVersion: orchestrator.dev/v2\nkind: SourceTaskBinding\nmetadata:\n  name: fr167-binding\nspec:\n  triggerRef: fr167-trigger\n  match:\n    eventKind: reaction_added\n    reaction: fr167-analyze\n    targetKind: message\n    channels: [C_FR167]\n  templateRef: fr167-template\n  allowedActorRoles: [operator]\n  suspend: false\n".into(),
    ] {
        fixture
            .apply(&manifest, None)
            .await
            .expect("seeding should succeed");
    }

    fixture
        .delete_request(DeleteRequest {
            resource: "sourcetasktemplate/fr167-template".into(),
            force: true,
            project: Some("default".into()),
            dry_run: false,
            force_references: true,
            audit: Some(ActionAuditContext {
                reason_code: "operator_force_reference_cleanup".into(),
                operator_reason: Some("atomic binding cleanup".into()),
                idempotency_key: Some("fr167-force-refs".into()),
            }),
        })
        .await
        .expect("force-references delete should succeed");

    let cleanup = fixture.rows(Some("delete_references")).await;
    assert_eq!(cleanup.len(), 1, "the cleanup action must record one row");
    assert_eq!(cleanup[0].target_type, "source_task_template");
    assert_eq!(cleanup[0].target_id, "sourcetasktemplate/fr167-template");
    assert_eq!(cleanup[0].reason_code, "operator_force_reference_cleanup");
    assert_eq!(cleanup[0].status, "succeeded");

    assert!(
        fixture
            .rows(Some("source.template.delete"))
            .await
            .is_empty(),
        "a reference cleanup must not also record the per-kind delete action"
    );
}
