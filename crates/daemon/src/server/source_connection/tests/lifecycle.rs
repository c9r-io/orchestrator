//! Connection lifecycle: manifest upgrade, App deletion, disconnect, ownership transfer.

use orchestrator_proto::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tonic::{Code, Request};
use zeroize::Zeroizing;

use super::super::{
    DEDICATED_MANIFEST_VERSION, DedicatedLifecycleSession, dedicated_delete,
    dedicated_upgrade_apply, dedicated_upgrade_preview, disconnect, ensure_default_trigger,
    transfer,
};
use super::{Fixture, installation, rfc3339_in};
use agent_orchestrator::source_connection::{SourceConnectionMode, SourceConnectionState};

const APP_ID: &str = "A0TESTAPP01";
const CONNECTION_ID: &str = "conn-T0DEDIC";
const PROVISIONING_ID: &str = "dedicated-live";

fn app_id_digest() -> String {
    hex::encode(Sha256::digest(APP_ID.as_bytes()))
}

fn upgraded_manifest() -> (Value, String) {
    let manifest = json!({
        "oauth_config": {
            "scopes": {"bot": ["chat:write", "reactions:read"]},
            "redirect_urls": ["https://gateway.example/slack/connections/dedicated-live/oauth/callback"]
        },
        "settings": {
            "event_subscriptions": {
                "request_url": "https://gateway.example/slack/connections/dedicated-live/events",
                "bot_events": ["app_uninstalled", "reaction_added", "tokens_revoked"]
            },
            "token_rotation_enabled": false
        }
    });
    let digest = hex::encode(Sha256::digest(
        serde_json::to_vec(&manifest).expect("serialize manifest"),
    ));
    (manifest, digest)
}

/// Puts the in-memory review session `dedicated_upgrade_preview` produces into place,
/// alongside the durable dedicated identity it refers to.
async fn seed_lifecycle_session(
    fixture: &Fixture,
    lifecycle_id: &str,
    permission_expansion: bool,
    expires_at: String,
) -> String {
    fixture
        .seed_dedicated_identity(CONNECTION_ID, PROVISIONING_ID, APP_ID)
        .await;
    let (manifest, digest) = upgraded_manifest();
    fixture
        .server
        .dedicated_lifecycle_sessions
        .lock()
        .await
        .insert(
            lifecycle_id.to_string(),
            DedicatedLifecycleSession {
                project_id: "default".into(),
                connection_id: CONNECTION_ID.into(),
                expected_version: 1,
                provisioning_id: PROVISIONING_ID.into(),
                app_id: Zeroizing::new(APP_ID.into()),
                app_id_digest: app_id_digest(),
                manifest,
                manifest_digest: digest.clone(),
                diff: vec![],
                permission_expansion,
                config_token: Zeroizing::new("xoxe.config-token".into()),
                expires_at,
            },
        );
    digest
}

fn apply_request(lifecycle_id: &str) -> SourceConnectionDedicatedUpgradeApplyRequest {
    SourceConnectionDedicatedUpgradeApplyRequest {
        project_id: "default".into(),
        id: CONNECTION_ID.into(),
        expected_version: 1,
        lifecycle_id: lifecycle_id.into(),
        idempotency_key: "upgrade-1".into(),
        reason: "adopt the reviewed manifest".into(),
    }
}

#[tokio::test]
async fn a_narrowing_upgrade_completes_without_asking_for_reauthorization() {
    let fixture = Fixture::with_gateway().await;
    seed_lifecycle_session(
        &fixture,
        "lifecycle-1",
        false,
        rfc3339_in(chrono::Duration::minutes(10)),
    )
    .await;
    let daemon_id = fixture.daemon_id().await;
    fixture
        .stub
        .reply(
            "/api/apps.manifest.update",
            json!({"ok": true, "permissions_updated": false}),
        )
        .reply("/v1/dedicated/apps/manifest", {
            let mut value = installation(&daemon_id, "T0DEDIC", 2);
            value["app_id_digest"] = json!(app_id_digest());
            value["provisioning_mode"] = json!("managed_dedicated");
            value
        });

    let response =
        dedicated_upgrade_apply(&fixture.server, Request::new(apply_request("lifecycle-1")))
            .await
            .expect("upgrade succeeds")
            .into_inner();

    assert_eq!(response.status, "completed");
    assert!(!response.permission_expansion);
    assert_eq!(response.oauth_intent_id, None);
    assert!(
        !fixture.stub.was_called("/v1/installations/suspend"),
        "a narrowing upgrade must never suspend delivery"
    );

    let connection = response.connection.expect("the upgraded connection");
    assert_eq!(connection.state, "active");
    assert_eq!(connection.provision_state.as_deref(), Some("completed"));
    assert_eq!(
        connection.manifest_version.as_deref(),
        Some(DEDICATED_MANIFEST_VERSION)
    );
}

#[tokio::test]
async fn an_expanding_upgrade_suspends_delivery_and_opens_a_dedicated_reauthorization() {
    let fixture = Fixture::with_gateway().await;
    seed_lifecycle_session(
        &fixture,
        "lifecycle-2",
        true,
        rfc3339_in(chrono::Duration::minutes(10)),
    )
    .await;
    let daemon_id = fixture.daemon_id().await;
    let dedicated_installation = |version: i64, state: &str| {
        let mut value = installation(&daemon_id, "T0DEDIC", version);
        value["app_id_digest"] = json!(app_id_digest());
        value["provisioning_mode"] = json!("managed_dedicated");
        value["state"] = json!(state);
        value
    };
    fixture
        .stub
        .reply(
            "/api/apps.manifest.update",
            json!({"ok": true, "permissions_updated": true}),
        )
        .reply(
            "/v1/dedicated/apps/manifest",
            dedicated_installation(2, "active"),
        )
        .reply(
            "/v1/installations/suspend",
            dedicated_installation(3, "suspended"),
        )
        .reply(
            "/v1/dedicated/oauth/intents",
            json!({
                "intent_id": "gw-reauth-1",
                "authorize_url": "https://slack.example/oauth/authorize?state=reauth",
                "poll_secret": "poll-secret-reauth",
                "expires_at": rfc3339_in(chrono::Duration::minutes(15)),
            }),
        );

    let response =
        dedicated_upgrade_apply(&fixture.server, Request::new(apply_request("lifecycle-2")))
            .await
            .expect("upgrade succeeds")
            .into_inner();

    assert_eq!(response.status, "reauthorization_required");
    assert!(response.permission_expansion);
    assert_eq!(
        response.authorize_url.as_deref(),
        Some("https://slack.example/oauth/authorize?state=reauth")
    );
    assert!(response.oauth_intent_id.is_some());

    // Delivery really stopped: the connection is suspended with the stable code.
    let connection = response.connection.expect("the upgraded connection");
    assert_eq!(connection.state, "suspended");
    assert_eq!(
        connection.provision_state.as_deref(),
        Some("reauthorization_required")
    );
    assert_eq!(
        connection.last_error_code.as_deref(),
        Some("slack_manifest_reauthorization_required")
    );

    // The reauthorization went through the dedicated endpoint, carrying the App identity.
    let intent_call = fixture.stub.call("/v1/dedicated/oauth/intents");
    assert_eq!(intent_call.body["connection_id"], json!(PROVISIONING_ID));

    // And the operator was told, rather than delivery silently stopping.
    let candidates = fixture
        .server
        .state
        .attention_repo
        .list(
            agent_orchestrator::attention::AttentionFilter {
                project_id: Some("default".into()),
                active_only: true,
                limit: 10,
                ..Default::default()
            },
            None,
        )
        .await
        .expect("attention list");
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.kind == "source_connection_reauthorization_required"),
        "a suspended connection must surface an attention item"
    );
}

#[tokio::test]
async fn a_gateway_projection_that_disagrees_with_the_local_fence_is_data_loss() {
    let fixture = Fixture::with_gateway().await;
    seed_lifecycle_session(
        &fixture,
        "lifecycle-3",
        false,
        rfc3339_in(chrono::Duration::minutes(10)),
    )
    .await;
    let daemon_id = fixture.daemon_id().await;
    fixture
        .stub
        .reply(
            "/api/apps.manifest.update",
            json!({"ok": true, "permissions_updated": false}),
        )
        // Version 5 where the local fence expects 2: the Gateway moved without us.
        .reply("/v1/dedicated/apps/manifest", {
            let mut value = installation(&daemon_id, "T0DEDIC", 5);
            value["app_id_digest"] = json!(app_id_digest());
            value
        });

    let error =
        dedicated_upgrade_apply(&fixture.server, Request::new(apply_request("lifecycle-3")))
            .await
            .expect_err("a projection mismatch must not be adopted");

    assert_eq!(error.code(), Code::DataLoss);
    let connection = fixture
        .repository()
        .get("default", CONNECTION_ID)
        .await
        .expect("read connection")
        .expect("connection exists");
    assert_eq!(
        connection.version, 1,
        "a refused upgrade must leave the local version untouched"
    );
}

#[tokio::test]
async fn an_expired_review_session_cannot_be_applied() {
    let fixture = Fixture::with_gateway().await;
    seed_lifecycle_session(
        &fixture,
        "lifecycle-4",
        false,
        rfc3339_in(chrono::Duration::minutes(-1)),
    )
    .await;

    let error =
        dedicated_upgrade_apply(&fixture.server, Request::new(apply_request("lifecycle-4")))
            .await
            .expect_err("an expired review cannot be applied");

    assert_eq!(error.code(), Code::FailedPrecondition);
    assert!(
        error
            .message()
            .contains("dedicated_lifecycle_session_expired")
    );
    assert!(fixture.stub.calls().is_empty());
}

#[tokio::test]
async fn applying_a_review_against_another_connection_is_refused() {
    let fixture = Fixture::with_gateway().await;
    seed_lifecycle_session(
        &fixture,
        "lifecycle-5",
        false,
        rfc3339_in(chrono::Duration::minutes(10)),
    )
    .await;
    let mut request = apply_request("lifecycle-5");
    request.id = "conn-somewhere-else".into();

    let error = dedicated_upgrade_apply(&fixture.server, Request::new(request))
        .await
        .expect_err("a review is bound to the connection it was produced for");

    assert_eq!(error.code(), Code::PermissionDenied);
    assert!(fixture.stub.calls().is_empty());
}

#[tokio::test]
async fn an_unknown_review_session_is_reported_as_lost_rather_than_re_reviewed() {
    let fixture = Fixture::with_gateway().await;
    let error = dedicated_upgrade_apply(
        &fixture.server,
        Request::new(apply_request("lifecycle-absent")),
    )
    .await
    .expect_err("an unknown review cannot be applied");

    assert_eq!(error.code(), Code::FailedPrecondition);
    assert!(error.message().contains("dedicated_lifecycle_session_lost"));
}

#[tokio::test]
async fn upgrade_preview_requires_an_active_dedicated_connection_at_the_expected_version() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_connection("conn-S", "T0SHARED", SourceConnectionMode::ManagedShared)
        .await;

    let wrong_mode = dedicated_upgrade_preview(
        &fixture.server,
        Request::new(SourceConnectionDedicatedUpgradePreviewRequest {
            project_id: "default".into(),
            id: "conn-S".into(),
            expected_version: 1,
            config_token: "xoxe.config-token".into(),
            idempotency_key: "preview-1".into(),
            reason: "upgrade the manifest".into(),
        }),
    )
    .await
    .expect_err("a shared connection has no dedicated App to upgrade");
    assert_eq!(wrong_mode.code(), Code::FailedPrecondition);

    fixture
        .seed_dedicated_identity(CONNECTION_ID, PROVISIONING_ID, APP_ID)
        .await;
    let stale = dedicated_upgrade_preview(
        &fixture.server,
        Request::new(SourceConnectionDedicatedUpgradePreviewRequest {
            project_id: "default".into(),
            id: CONNECTION_ID.into(),
            expected_version: 99,
            config_token: "xoxe.config-token".into(),
            idempotency_key: "preview-2".into(),
            reason: "upgrade the manifest".into(),
        }),
    )
    .await
    .expect_err("a stale version is refused");
    assert_eq!(stale.code(), Code::Aborted);
    assert!(
        !fixture.stub.was_called("/api/apps.manifest.export"),
        "the fences must be checked before the App identity is decrypted"
    );
}

#[tokio::test]
async fn upgrade_preview_exports_the_live_manifest_with_the_decrypted_app_identity() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_dedicated_identity(CONNECTION_ID, PROVISIONING_ID, APP_ID)
        .await;
    fixture
        .stub
        .reply_manifest_error("/api/apps.manifest.export", "app_not_found");

    let error = dedicated_upgrade_preview(
        &fixture.server,
        Request::new(SourceConnectionDedicatedUpgradePreviewRequest {
            project_id: "default".into(),
            id: CONNECTION_ID.into(),
            expected_version: 1,
            config_token: "xoxe.config-token".into(),
            idempotency_key: "preview-3".into(),
            reason: "upgrade the manifest".into(),
        }),
    )
    .await
    .expect_err("an export failure surfaces");
    assert_eq!(error.code(), Code::Unavailable);

    // The exact App ID was recovered from its ciphertext and sent to Slack — a
    // preview that skipped the decryption would send nothing or the digest.
    let call = fixture.stub.call("/api/apps.manifest.export");
    assert_eq!(call.body["app_id"], json!(APP_ID));
    assert_eq!(call.authorization.as_deref(), Some("xoxe.config-token"));
}

#[tokio::test]
async fn deleting_a_dedicated_app_requires_the_operator_to_type_its_exact_id() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_dedicated_identity(CONNECTION_ID, PROVISIONING_ID, APP_ID)
        .await;
    fixture
        .repository()
        .transition(
            "default",
            CONNECTION_ID,
            1,
            SourceConnectionState::Disconnected,
            None,
            "req-disconnect-seed",
        )
        .await
        .expect("disconnect the seeded connection");

    let mistyped = dedicated_delete(
        &fixture.server,
        Request::new(SourceConnectionDedicatedDeleteRequest {
            project_id: "default".into(),
            id: CONNECTION_ID.into(),
            expected_version: 2,
            config_token: "xoxe.config-token".into(),
            typed_app_id: "A0WRONGAPP".into(),
            idempotency_key: "delete-1".into(),
            reason: "retire the app".into(),
        }),
    )
    .await
    .expect_err("a mistyped App ID must not delete anything");

    assert_eq!(mistyped.code(), Code::InvalidArgument);
    assert!(
        !fixture.stub.was_called("/api/apps.manifest.delete"),
        "no deletion may be attempted on a mistyped confirmation"
    );
}

#[tokio::test]
async fn deleting_a_dedicated_app_exports_it_deletes_it_and_retires_it_at_the_gateway() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_dedicated_identity(CONNECTION_ID, PROVISIONING_ID, APP_ID)
        .await;
    fixture
        .repository()
        .transition(
            "default",
            CONNECTION_ID,
            1,
            SourceConnectionState::Disconnected,
            None,
            "req-disconnect-seed",
        )
        .await
        .expect("disconnect the seeded connection");
    fixture
        .stub
        .reply(
            "/api/apps.manifest.export",
            json!({"ok": true, "manifest": {"display_information": {"name": "Orchestrator"}}}),
        )
        .reply("/api/apps.manifest.delete", json!({"ok": true}))
        .reply_no_content("/v1/dedicated/apps/delete");

    let response = dedicated_delete(
        &fixture.server,
        Request::new(SourceConnectionDedicatedDeleteRequest {
            project_id: "default".into(),
            id: CONNECTION_ID.into(),
            expected_version: 2,
            config_token: "xoxe.config-token".into(),
            typed_app_id: APP_ID.into(),
            idempotency_key: "delete-2".into(),
            reason: "retire the app".into(),
        }),
    )
    .await
    .expect("deletion succeeds")
    .into_inner();

    assert_eq!(response.state, "disconnected");
    assert_eq!(response.provision_state.as_deref(), Some("app_deleted"));

    // The export precedes the delete: the App is captured before it is destroyed.
    assert_eq!(
        fixture.stub.paths(),
        vec![
            "/api/apps.manifest.export".to_string(),
            "/api/apps.manifest.delete".to_string(),
            "/v1/dedicated/apps/delete".to_string(),
        ]
    );
    let retire = fixture.stub.call("/v1/dedicated/apps/delete");
    assert_eq!(retire.body["app_id_digest"], json!(app_id_digest()));
    assert_eq!(retire.body["connection_id"], json!(PROVISIONING_ID));
}

#[tokio::test]
async fn deleting_an_app_behind_a_still_connected_connection_is_refused() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_dedicated_identity(CONNECTION_ID, PROVISIONING_ID, APP_ID)
        .await;

    let error = dedicated_delete(
        &fixture.server,
        Request::new(SourceConnectionDedicatedDeleteRequest {
            project_id: "default".into(),
            id: CONNECTION_ID.into(),
            expected_version: 1,
            config_token: "xoxe.config-token".into(),
            typed_app_id: APP_ID.into(),
            idempotency_key: "delete-3".into(),
            reason: "retire the app".into(),
        }),
    )
    .await
    .expect_err("an active connection must be disconnected first");

    assert_eq!(error.code(), Code::FailedPrecondition);
    assert!(fixture.stub.calls().is_empty());
}

#[tokio::test]
async fn disconnecting_revokes_at_the_gateway_before_the_local_transition() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_connection("conn-A", "T0AAAA", SourceConnectionMode::ManagedShared)
        .await;
    let daemon_id = fixture.daemon_id().await;
    fixture.stub.reply("/v1/installations/disconnect", {
        let mut value = installation(&daemon_id, "T0AAAA", 2);
        value["state"] = json!("disconnected");
        value
    });

    let response = disconnect(
        &fixture.server,
        Request::new(SourceConnectionMutationRequest {
            project_id: "default".into(),
            id: "conn-A".into(),
            expected_version: 1,
            idempotency_key: "disconnect-1".into(),
            reason: "retire the workspace".into(),
        }),
    )
    .await
    .expect("disconnect succeeds")
    .into_inner();

    assert_eq!(response.state, "disconnected");
    // The pairing secret authorises it, so a caller without the credential cannot.
    let call = fixture.stub.call("/v1/installations/disconnect");
    assert_eq!(call.authorization.as_deref(), Some("pairing-secret-value"));
    assert_eq!(call.body["expected_version"], json!(1));
}

#[tokio::test]
async fn a_gateway_that_refuses_the_disconnect_leaves_the_connection_active() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_connection("conn-A", "T0AAAA", SourceConnectionMode::ManagedShared)
        .await;
    fixture
        .stub
        .reply_error("/v1/installations/disconnect", 409, "version_conflict");

    let error = disconnect(
        &fixture.server,
        Request::new(SourceConnectionMutationRequest {
            project_id: "default".into(),
            id: "conn-A".into(),
            expected_version: 1,
            idempotency_key: "disconnect-2".into(),
            reason: "retire the workspace".into(),
        }),
    )
    .await
    .expect_err("a refused revocation must not be projected locally");

    assert_eq!(error.code(), Code::Unavailable);
    let connection = fixture
        .repository()
        .get("default", "conn-A")
        .await
        .expect("read connection")
        .expect("connection exists");
    assert_eq!(
        connection.state,
        SourceConnectionState::Active,
        "the local projection must not claim a disconnect the Gateway refused"
    );
}

#[tokio::test]
async fn transferring_ownership_adopts_the_gateway_projection_only_when_it_matches() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_connection("conn-A", "T0AAAA", SourceConnectionMode::ManagedShared)
        .await;

    let self_transfer = transfer(
        &fixture.server,
        Request::new(SourceConnectionTransferRequest {
            project_id: "default".into(),
            id: "conn-A".into(),
            expected_version: 1,
            target_daemon_id: fixture.daemon_id().await,
            idempotency_key: "transfer-0".into(),
            reason: "transfer to ourselves".into(),
        }),
    )
    .await
    .expect_err("transferring to the current owner is a no-op, not a transfer");
    assert_eq!(self_transfer.code(), Code::FailedPrecondition);

    // A projection that names the wrong new owner must not be adopted.
    let daemon_id = fixture.daemon_id().await;
    fixture.stub.reply(
        "/v1/installations/transfer",
        json!({"installation": installation(&daemon_id, "T0AAAA", 2)}),
    );
    let mismatch = transfer(
        &fixture.server,
        Request::new(SourceConnectionTransferRequest {
            project_id: "default".into(),
            id: "conn-A".into(),
            expected_version: 1,
            target_daemon_id: "daemon-b".into(),
            idempotency_key: "transfer-1".into(),
            reason: "hand over to daemon-b".into(),
        }),
    )
    .await
    .expect_err("a projection naming the wrong owner is data loss");
    assert_eq!(mismatch.code(), Code::DataLoss);

    fixture.stub.reply(
        "/v1/installations/transfer",
        json!({"installation": installation("daemon-b", "T0AAAA", 2)}),
    );
    let transferred = transfer(
        &fixture.server,
        Request::new(SourceConnectionTransferRequest {
            project_id: "default".into(),
            id: "conn-A".into(),
            expected_version: 1,
            target_daemon_id: "daemon-b".into(),
            idempotency_key: "transfer-2".into(),
            reason: "hand over to daemon-b".into(),
        }),
    )
    .await
    .expect("transfer succeeds")
    .into_inner();

    assert_eq!(transferred.owner_daemon_id, "daemon-b");
    assert_eq!(transferred.version, 2);
}

#[tokio::test]
async fn a_transfer_that_would_rewind_the_delivery_cursor_is_refused() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_connection("conn-A", "T0AAAA", SourceConnectionMode::ManagedShared)
        .await;
    fixture
        .repository()
        .record_delivery("default", "conn-A", 42, 0)
        .await
        .expect("record a delivery");

    let mut projection = installation("daemon-b", "T0AAAA", 2);
    projection["last_acked_cursor"] = json!(7);
    fixture.stub.reply(
        "/v1/installations/transfer",
        json!({"installation": projection}),
    );

    let error = transfer(
        &fixture.server,
        Request::new(SourceConnectionTransferRequest {
            project_id: "default".into(),
            id: "conn-A".into(),
            expected_version: 1,
            target_daemon_id: "daemon-b".into(),
            idempotency_key: "transfer-3".into(),
            reason: "hand over to daemon-b".into(),
        }),
    )
    .await
    .expect_err("a rewound cursor would replay delivered events");

    assert_eq!(error.code(), Code::DataLoss);
}

#[tokio::test]
async fn the_default_trigger_is_idempotent_and_refuses_to_steal_an_occupied_name() {
    let fixture = Fixture::with_gateway().await;

    let first = ensure_default_trigger(
        &fixture.server.state,
        &fixture.server.config_mutation_lock,
        "default",
        "conn-T0AAAA",
        "T0AAAA",
    )
    .await
    .expect("the trigger is created");
    assert_eq!(first, "slack-conn-T0AAAA");

    let again = ensure_default_trigger(
        &fixture.server.state,
        &fixture.server.config_mutation_lock,
        "default",
        "conn-T0AAAA",
        "T0AAAA",
    )
    .await
    .expect("re-running is a no-op");
    assert_eq!(again, first);

    // The same trigger name, claimed for a different connection, must not be taken over.
    let stolen = ensure_default_trigger(
        &fixture.server.state,
        &fixture.server.config_mutation_lock,
        "default",
        "conn-T0AAAA",
        "T0BBBB",
    )
    .await;
    assert!(
        stolen.is_ok(),
        "the same connection may re-derive its own trigger"
    );

    let missing_project = ensure_default_trigger(
        &fixture.server.state,
        &fixture.server.config_mutation_lock,
        "no-such-project",
        "conn-T0AAAA",
        "T0AAAA",
    )
    .await
    .expect_err("an unknown project has no trigger to create");
    assert_eq!(missing_project.code(), Code::NotFound);
}
