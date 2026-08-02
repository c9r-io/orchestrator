//! OAuth intent lifecycle: create, poll, terminal projection, cancel, reauthorize.

use orchestrator_proto::*;
use tonic::{Code, Request};

use super::super::{cancel, connect, intent_get, migrate_to_shared, reauthorize};
use super::{Fixture, installation, rfc3339_in};
use agent_orchestrator::source_connection::SourceConnectionMode;
use serde_json::json;

fn connect_request() -> SourceConnectionConnectRequest {
    SourceConnectionConnectRequest {
        project_id: "default".into(),
        provider: "slack".into(),
        provisioning_mode: "managed_shared".into(),
        display_label: "Acme workspace".into(),
        idempotency_key: "connect-1".into(),
        reason: "connect the workspace".into(),
    }
}

/// Scripts a Gateway that hands back a fresh intent, and returns its expiry so the
/// caller can echo it in the poll response — the handler rejects a poll whose expiry
/// disagrees with what it stored.
fn script_intent_created(fixture: &Fixture, path: &str) -> String {
    let expires_at = rfc3339_in(chrono::Duration::minutes(15));
    fixture.stub.reply(
        path,
        json!({
            "intent_id": "gw-intent-1",
            "authorize_url": "https://slack.example/oauth/authorize?state=abc",
            "poll_secret": "poll-secret-1",
            "expires_at": expires_at,
        }),
    );
    expires_at
}

#[tokio::test]
async fn connect_creates_an_intent_and_returns_the_gateway_authorize_url() {
    let fixture = Fixture::with_gateway().await;
    script_intent_created(&fixture, "/v1/oauth/intents");

    let response = connect(&fixture.server, Request::new(connect_request()))
        .await
        .expect("connect succeeds")
        .into_inner();

    assert_eq!(response.provisioning_mode, "managed_shared");
    assert_eq!(response.status, "pending");
    assert_eq!(
        response.authorize_url.as_deref(),
        Some("https://slack.example/oauth/authorize?state=abc")
    );
    assert!(response.id.starts_with("intent-"));

    // The Gateway was genuinely reached, with the enrollment key, and the daemon
    // identity it was told matches the one the repository reports.
    let call = fixture.stub.call("/v1/oauth/intents");
    assert_eq!(call.authorization.as_deref(), Some(super::ENROLLMENT_KEY));
    assert_eq!(
        call.body["daemon_id"].as_str(),
        Some(fixture.daemon_id().await.as_str())
    );
    assert_eq!(call.body["project_id"].as_str(), Some("default"));
    assert_eq!(call.body["migration_installation_id"], json!(null));

    // The authorize URL and poll secret are encrypted at rest, never stored in clear.
    let stored = fixture
        .repository()
        .intent_credential("default", &response.id, &fixture.daemon_id().await)
        .await
        .expect("read intent")
        .expect("intent exists");
    assert!(!stored.authorize_url_ciphertext.contains("slack.example"));
    assert!(!stored.poll_secret_ciphertext.contains("poll-secret-1"));
}

#[tokio::test]
async fn connect_refuses_modes_this_capability_version_does_not_implement() {
    let fixture = Fixture::with_gateway().await;
    let mut request = connect_request();
    request.provisioning_mode = "managed_dedicated".into();

    let error = connect(&fixture.server, Request::new(request))
        .await
        .expect_err("dedicated is not connectable through this RPC");

    assert_eq!(error.code(), Code::FailedPrecondition);
    // Rejected before any Gateway traffic — the precondition is local.
    assert!(fixture.stub.calls().is_empty());
}

#[tokio::test]
async fn connect_reports_a_gateway_outage_as_unavailable_and_records_the_failed_attempt() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .stub
        .reply_error("/v1/oauth/intents", 502, "gateway_upstream_failed");

    let error = connect(&fixture.server, Request::new(connect_request()))
        .await
        .expect_err("gateway outage surfaces");

    assert_eq!(error.code(), Code::Unavailable);
    assert!(error.message().contains("gateway_upstream_failed"));
    assert!(fixture.stub.was_called("/v1/oauth/intents"));
}

#[tokio::test]
async fn a_still_pending_gateway_intent_stays_pending_locally() {
    let fixture = Fixture::with_gateway().await;
    let expires_at = script_intent_created(&fixture, "/v1/oauth/intents");
    let created = connect(&fixture.server, Request::new(connect_request()))
        .await
        .expect("connect succeeds")
        .into_inner();

    fixture.stub.reply(
        "/v1/oauth/intents/gw-intent-1",
        json!({
            "intent_id": "gw-intent-1",
            "status": "pending",
            "expires_at": expires_at,
            "error_code": null,
            "installation": null,
            "pairing_secret": null,
        }),
    );

    let polled = intent_get(
        &fixture.server,
        Request::new(SourceConnectionIntentGetRequest {
            project_id: "default".into(),
            intent_id: created.id.clone(),
        }),
    )
    .await
    .expect("poll succeeds")
    .into_inner();

    assert_eq!(polled.status, "pending");
    assert_eq!(polled.connection, None);
    // The decrypted authorize URL is handed back so the operator can resume.
    assert_eq!(
        polled.authorize_url.as_deref(),
        Some("https://slack.example/oauth/authorize?state=abc")
    );
}

#[tokio::test]
async fn an_identity_mismatch_from_the_gateway_is_data_loss_not_a_status_update() {
    let fixture = Fixture::with_gateway().await;
    script_intent_created(&fixture, "/v1/oauth/intents");
    let created = connect(&fixture.server, Request::new(connect_request()))
        .await
        .expect("connect succeeds")
        .into_inner();

    // Same intent id, different expiry: the Gateway is describing another intent.
    fixture.stub.reply(
        "/v1/oauth/intents/gw-intent-1",
        json!({
            "intent_id": "gw-intent-1",
            "status": "completed",
            "expires_at": rfc3339_in(chrono::Duration::minutes(99)),
            "error_code": null,
            "installation": null,
            "pairing_secret": null,
        }),
    );

    let error = intent_get(
        &fixture.server,
        Request::new(SourceConnectionIntentGetRequest {
            project_id: "default".into(),
            intent_id: created.id,
        }),
    )
    .await
    .expect_err("identity mismatch fails closed");

    assert_eq!(error.code(), Code::DataLoss);
}

/// The projection table `local_terminal_intent_status` implements, driven end to end
/// rather than through the pure helper alone.
#[tokio::test]
async fn every_non_completed_gateway_terminal_projects_to_its_local_status() {
    for (gateway_status, error_code, expected) in [
        ("cancelled", None, "cancelled"),
        ("expired", None, "expired"),
        ("failed", Some("oauth_intent_expired"), "expired"),
        ("failed", Some("provider_denied"), "failed"),
        ("failed", None, "failed"),
    ] {
        let fixture = Fixture::with_gateway().await;
        let expires_at = script_intent_created(&fixture, "/v1/oauth/intents");
        let created = connect(&fixture.server, Request::new(connect_request()))
            .await
            .expect("connect succeeds")
            .into_inner();

        fixture.stub.reply(
            "/v1/oauth/intents/gw-intent-1",
            json!({
                "intent_id": "gw-intent-1",
                "status": gateway_status,
                "expires_at": expires_at,
                "error_code": error_code,
                "installation": null,
                "pairing_secret": null,
            }),
        );

        let polled = intent_get(
            &fixture.server,
            Request::new(SourceConnectionIntentGetRequest {
                project_id: "default".into(),
                intent_id: created.id.clone(),
            }),
        )
        .await
        .expect("poll succeeds")
        .into_inner();

        assert_eq!(
            polled.status, expected,
            "gateway {gateway_status:?}/{error_code:?} must project to {expected}"
        );
        assert_eq!(polled.error_code.as_deref(), error_code);
    }
}

#[tokio::test]
async fn a_completed_intent_activates_a_connection_and_provisions_its_default_trigger() {
    let fixture = Fixture::with_gateway().await;
    let expires_at = script_intent_created(&fixture, "/v1/oauth/intents");
    let created = connect(&fixture.server, Request::new(connect_request()))
        .await
        .expect("connect succeeds")
        .into_inner();
    let daemon_id = fixture.daemon_id().await;

    fixture.stub.reply(
        "/v1/oauth/intents/gw-intent-1",
        json!({
            "intent_id": "gw-intent-1",
            "status": "completed",
            "expires_at": expires_at,
            "error_code": null,
            "installation": installation(&daemon_id, "T0TESTTEAM", 1),
            "pairing_secret": "pairing-secret-value",
        }),
    );

    let polled = intent_get(
        &fixture.server,
        Request::new(SourceConnectionIntentGetRequest {
            project_id: "default".into(),
            intent_id: created.id.clone(),
        }),
    )
    .await
    .expect("poll succeeds")
    .into_inner();

    assert_eq!(polled.status, "completed");
    let connection = polled
        .connection
        .expect("completed intent carries connection");
    assert_eq!(connection.id, "conn-T0TESTTEAM");
    assert_eq!(connection.state, "active");
    assert_eq!(connection.provisioning_mode, "managed_shared");
    assert_eq!(connection.app_ownership, "orchestrator");
    assert_eq!(
        connection.trigger_name.as_deref(),
        Some("slack-conn-T0TESTTEAM")
    );

    // The end-to-end behaviour the flow exists for: a Trigger now routes this
    // installation's events. Without it the connection is inert.
    let active = agent_orchestrator::config_load::read_active_config(&fixture.server.state)
        .expect("active config");
    let trigger = active
        .config
        .projects
        .get("default")
        .and_then(|project| project.triggers.get("slack-conn-T0TESTTEAM"))
        .expect("default managed Slack trigger was applied");
    let webhook = trigger
        .event
        .as_ref()
        .and_then(|event| event.webhook.as_ref())
        .expect("trigger carries a webhook");
    assert_eq!(webhook.provider.as_deref(), Some("slack"));
    assert_eq!(webhook.installation_id.as_deref(), Some("T0TESTTEAM"));
    assert_eq!(webhook.connection_ref.as_deref(), Some("conn-T0TESTTEAM"));

    // The pairing secret reached durable storage encrypted.
    let credential = fixture
        .repository()
        .credential("default", "conn-T0TESTTEAM", &daemon_id)
        .await
        .expect("read credential")
        .expect("credential stored");
    assert!(
        !credential
            .pairing_secret_ciphertext
            .contains("pairing-secret-value")
    );
}

#[tokio::test]
async fn a_completed_intent_owned_by_another_daemon_is_refused() {
    let fixture = Fixture::with_gateway().await;
    let expires_at = script_intent_created(&fixture, "/v1/oauth/intents");
    let created = connect(&fixture.server, Request::new(connect_request()))
        .await
        .expect("connect succeeds")
        .into_inner();

    fixture.stub.reply(
        "/v1/oauth/intents/gw-intent-1",
        json!({
            "intent_id": "gw-intent-1",
            "status": "completed",
            "expires_at": expires_at,
            "error_code": null,
            "installation": installation("some-other-daemon", "T0TESTTEAM", 1),
            "pairing_secret": "pairing-secret-value",
        }),
    );

    let error = intent_get(
        &fixture.server,
        Request::new(SourceConnectionIntentGetRequest {
            project_id: "default".into(),
            intent_id: created.id,
        }),
    )
    .await
    .expect_err("owner boundary is enforced");

    assert_eq!(error.code(), Code::PermissionDenied);
    assert!(
        fixture
            .repository()
            .get("default", "conn-T0TESTTEAM")
            .await
            .expect("read connection")
            .is_none(),
        "a refused installation must not leave a local connection behind"
    );
}

#[tokio::test]
async fn a_completed_intent_without_an_installation_is_data_loss() {
    let fixture = Fixture::with_gateway().await;
    let expires_at = script_intent_created(&fixture, "/v1/oauth/intents");
    let created = connect(&fixture.server, Request::new(connect_request()))
        .await
        .expect("connect succeeds")
        .into_inner();

    fixture.stub.reply(
        "/v1/oauth/intents/gw-intent-1",
        json!({
            "intent_id": "gw-intent-1",
            "status": "completed",
            "expires_at": expires_at,
            "error_code": null,
            "installation": null,
            "pairing_secret": "pairing-secret-value",
        }),
    );

    let error = intent_get(
        &fixture.server,
        Request::new(SourceConnectionIntentGetRequest {
            project_id: "default".into(),
            intent_id: created.id,
        }),
    )
    .await
    .expect_err("a completed intent must carry an installation");

    assert_eq!(error.code(), Code::DataLoss);
}

#[tokio::test]
async fn cancelling_a_pending_intent_reaches_the_gateway_and_settles_locally() {
    let fixture = Fixture::with_gateway().await;
    script_intent_created(&fixture, "/v1/oauth/intents");
    let created = connect(&fixture.server, Request::new(connect_request()))
        .await
        .expect("connect succeeds")
        .into_inner();
    fixture
        .stub
        .reply_no_content("/v1/oauth/intents/gw-intent-1");

    let cancelled = cancel(
        &fixture.server,
        Request::new(SourceConnectionIntentMutationRequest {
            project_id: "default".into(),
            intent_id: created.id.clone(),
            idempotency_key: "cancel-1".into(),
            reason: "operator abandoned the flow".into(),
        }),
    )
    .await
    .expect("cancel succeeds")
    .into_inner();

    assert_eq!(cancelled.status, "cancelled");
    // The poll secret authorises the cancel, not the enrollment key.
    let call = fixture.stub.call("/v1/oauth/intents/gw-intent-1");
    assert_eq!(call.authorization.as_deref(), Some("poll-secret-1"));

    // A second cancel finds the intent no longer pending.
    let again = cancel(
        &fixture.server,
        Request::new(SourceConnectionIntentMutationRequest {
            project_id: "default".into(),
            intent_id: created.id,
            idempotency_key: "cancel-2".into(),
            reason: "operator retried".into(),
        }),
    )
    .await
    .expect_err("a settled intent cannot be cancelled again");
    assert_eq!(again.code(), Code::FailedPrecondition);
}

#[tokio::test]
async fn reauthorize_fences_on_the_connection_version() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_connection("conn-A", "T0AAAA", SourceConnectionMode::ManagedShared)
        .await;
    script_intent_created(&fixture, "/v1/oauth/intents");

    let stale = reauthorize(
        &fixture.server,
        Request::new(SourceConnectionMutationRequest {
            project_id: "default".into(),
            id: "conn-A".into(),
            expected_version: 99,
            idempotency_key: "reauth-stale".into(),
            reason: "stale version".into(),
        }),
    )
    .await
    .expect_err("a stale version is refused");
    assert_eq!(stale.code(), Code::Aborted);
    assert!(
        !fixture.stub.was_called("/v1/oauth/intents"),
        "the version fence must be checked before any Gateway call"
    );

    let fresh = reauthorize(
        &fixture.server,
        Request::new(SourceConnectionMutationRequest {
            project_id: "default".into(),
            id: "conn-A".into(),
            expected_version: 1,
            idempotency_key: "reauth-fresh".into(),
            reason: "rotate the grant".into(),
        }),
    )
    .await
    .expect("reauthorize succeeds")
    .into_inner();
    assert_eq!(fresh.status, "pending");
    assert_eq!(fresh.provisioning_mode, "managed_shared");
    // A shared connection reauthorizes through the shared endpoint.
    assert!(fixture.stub.was_called("/v1/oauth/intents"));
    assert!(!fixture.stub.was_called("/v1/dedicated/oauth/intents"));
}

#[tokio::test]
async fn migrating_to_shared_requires_an_active_dedicated_connection_and_carries_the_fence() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_connection("conn-S", "T0SHARED", SourceConnectionMode::ManagedShared)
        .await;

    let wrong_mode = migrate_to_shared(
        &fixture.server,
        Request::new(SourceConnectionMutationRequest {
            project_id: "default".into(),
            id: "conn-S".into(),
            expected_version: 1,
            idempotency_key: "migrate-1".into(),
            reason: "already shared".into(),
        }),
    )
    .await
    .expect_err("a shared connection cannot migrate to shared");
    assert_eq!(wrong_mode.code(), Code::FailedPrecondition);

    fixture
        .seed_connection("conn-D", "T0DEDIC", SourceConnectionMode::ManagedDedicated)
        .await;
    script_intent_created(&fixture, "/v1/oauth/intents");

    let migrated = migrate_to_shared(
        &fixture.server,
        Request::new(SourceConnectionMutationRequest {
            project_id: "default".into(),
            id: "conn-D".into(),
            expected_version: 1,
            idempotency_key: "migrate-2".into(),
            reason: "return to the shared app".into(),
        }),
    )
    .await
    .expect("migration intent is created")
    .into_inner();
    assert_eq!(migrated.status, "pending");

    // The migration fence must travel to the Gateway, otherwise the old installation
    // is never superseded and both apps stay live.
    let call = fixture.stub.call("/v1/oauth/intents");
    assert_eq!(call.body["migration_installation_id"], json!("T0DEDIC"));
    assert_eq!(call.body["migration_expected_version"], json!(1));
    assert_eq!(
        call.body["migration_source_mode"],
        json!("managed_dedicated")
    );
}

#[tokio::test]
async fn every_intent_rpc_fails_closed_when_no_gateway_is_configured() {
    let fixture = Fixture::without_gateway().await;

    let connect_error = connect(&fixture.server, Request::new(connect_request()))
        .await
        .expect_err("connect needs a gateway");
    assert_eq!(connect_error.code(), Code::FailedPrecondition);

    fixture
        .seed_connection("conn-A", "T0AAAA", SourceConnectionMode::ManagedShared)
        .await;
    let reauth_error = reauthorize(
        &fixture.server,
        Request::new(SourceConnectionMutationRequest {
            project_id: "default".into(),
            id: "conn-A".into(),
            expected_version: 1,
            idempotency_key: "reauth-nogw".into(),
            reason: "no gateway".into(),
        }),
    )
    .await
    .expect_err("reauthorize needs a gateway");
    assert_eq!(reauth_error.code(), Code::FailedPrecondition);
}
