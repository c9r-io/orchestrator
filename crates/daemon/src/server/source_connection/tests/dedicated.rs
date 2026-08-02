//! Dedicated Slack App provisioning: preview, checkpoint reads, abandon, approve.

use orchestrator_proto::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tonic::{Code, Request};
use zeroize::Zeroizing;

use super::super::projection::dedicated_urls;
use super::super::{
    DEDICATED_MANIFEST_VERSION, DedicatedSession, dedicated_abandon, dedicated_approve,
    dedicated_get, dedicated_preview,
};
use super::{ENROLLMENT_KEY, Fixture, rfc3339_in};
use agent_orchestrator::source_connection::StoreDedicatedProvisioning;

const APP_ID: &str = "A0TESTAPP01";
const PROVISIONING_ID: &str = "dedicated-1";

/// Renders the bundled manifest against a public HTTPS origin, the way `dedicated_preview`
/// does in production. A loopback origin cannot be used: `render_manifest_endpoints`
/// refuses a plaintext endpoint outright, which is asserted in `projection.rs` and again
/// end to end in `preview_checks_gateway_capabilities_then_refuses_a_plaintext_origin`.
fn reviewed_manifest() -> (Value, String, String, String) {
    let (callback, events) =
        dedicated_urls("https://gateway.example", PROVISIONING_ID).expect("derive endpoints");
    let mut manifest: Value = serde_json::from_str(include_str!(
        "../../../../assets/dedicated-app-manifest.json"
    ))
    .expect("bundled manifest parses");
    orchestrator_slack_gateway::slack::render_manifest_endpoints(&mut manifest, &callback, &events)
        .expect("render endpoints");
    let digest = hex::encode(Sha256::digest(
        serde_json::to_vec(&manifest).expect("serialize manifest"),
    ));
    (manifest, digest, callback, events)
}

/// Reproduces the HMAC receipt `import_dedicated_app` verifies. A stub that skipped it
/// would be rejected by the client, so this also proves the check is live.
fn receipt(
    connection_id: &str,
    app_id_digest: &str,
    generation: i64,
    manifest_digest: &str,
) -> String {
    use hmac::{Hmac, Mac};
    let mut mac =
        <Hmac<Sha256> as hmac::digest::KeyInit>::new_from_slice(ENROLLMENT_KEY.as_bytes())
            .expect("receipt key");
    mac.update(b"orchestrator-dedicated-app-receipt-v1:");
    mac.update(
        format!("{connection_id}:{app_id_digest}:{generation}:{manifest_digest}").as_bytes(),
    );
    hex::encode(mac.finalize().into_bytes())
}

/// Puts the durable checkpoint and the in-memory session into the exact state
/// `dedicated_preview` leaves behind, which is the only way to reach `dedicated_approve`
/// without a public HTTPS Gateway origin.
async fn seed_awaiting_approval(fixture: &Fixture, target_connection_id: Option<String>) -> Value {
    let (manifest, digest, _, _) = reviewed_manifest();
    let daemon_id = fixture.daemon_id().await;
    fixture
        .repository()
        .store_dedicated_provisioning(StoreDedicatedProvisioning {
            id: PROVISIONING_ID.into(),
            project_id: "default".into(),
            display_label: "Acme workspace".into(),
            owner_daemon_id: daemon_id.clone(),
            target_connection_id,
            manifest_version: DEDICATED_MANIFEST_VERSION.into(),
            manifest_digest: digest.clone(),
            expires_at: rfc3339_in(chrono::Duration::minutes(10)),
        })
        .await
        .expect("seed checkpoint");
    fixture.server.dedicated_sessions.lock().await.insert(
        PROVISIONING_ID.into(),
        DedicatedSession {
            project_id: "default".into(),
            display_label: "Acme workspace".into(),
            owner_daemon_id: daemon_id,
            manifest: manifest.clone(),
            manifest_digest: digest,
            config_token: Zeroizing::new("xoxe.config-token".into()),
            import_secret: None,
            created_credentials: None,
        },
    );
    manifest
}

fn approve_request() -> SourceConnectionDedicatedMutationRequest {
    SourceConnectionDedicatedMutationRequest {
        project_id: "default".into(),
        provisioning_id: PROVISIONING_ID.into(),
        idempotency_key: "approve-1".into(),
        reason: "reviewed the manifest".into(),
    }
}

/// Scripts the whole Gateway and Slack side of one successful approval.
fn script_successful_approval(fixture: &Fixture, callback: &str, events: &str, digest: &str) {
    let app_id_digest = hex::encode(Sha256::digest(APP_ID.as_bytes()));
    fixture
        .stub
        .reply(
            "/v1/dedicated/import-slots",
            json!({
                "connection_id": PROVISIONING_ID,
                "import_secret": "import-secret-1",
                "expires_at": rfc3339_in(chrono::Duration::minutes(10)),
                "oauth_callback_url": callback,
                "events_url": events,
            }),
        )
        .reply(
            "/api/apps.manifest.create",
            json!({
                "ok": true,
                "app_id": APP_ID,
                "credentials": {
                    "client_id": "1234.5678",
                    "client_secret": "client-secret-value",
                    "signing_secret": "signing-secret-value"
                }
            }),
        )
        .reply(
            "/v1/dedicated/import",
            json!({
                "connection_id": PROVISIONING_ID,
                "app_id_digest": app_id_digest,
                "credential_generation": 1,
                "receipt_signature": receipt(PROVISIONING_ID, &app_id_digest, 1, digest),
                "intent_id": "gw-dedicated-intent-1",
                "authorize_url": "https://slack.example/oauth/authorize?state=dedicated",
                "poll_secret": "poll-secret-dedicated",
                "expires_at": rfc3339_in(chrono::Duration::minutes(15)),
            }),
        );
}

/// A loopback Gateway origin is refused before any Slack traffic: the dedicated
/// endpoints derived from it are plaintext, and `render_manifest_endpoints` rejects
/// those outright. This is exactly why the approval flow below has to be reached by
/// seeding the session rather than by calling `dedicated_preview` — recorded here as
/// a behaviour rather than left as an unexplained gap.
#[tokio::test]
async fn preview_checks_gateway_capabilities_then_refuses_a_plaintext_origin() {
    let fixture = Fixture::with_gateway().await;
    fixture.stub.reply(
        "/v1/capabilities",
        json!({
            "protocol_version": 1,
            "supported_modes": ["managed_shared", "managed_dedicated"],
            "max_delivery_batch": 50,
            "permalink_proxy": true
        }),
    );
    fixture
        .stub
        .reply_manifest_error("/api/apps.manifest.validate", "invalid_manifest");

    let error = dedicated_preview(
        &fixture.server,
        Request::new(SourceConnectionDedicatedPreviewRequest {
            project_id: "default".into(),
            display_label: "Acme workspace".into(),
            config_token: "xoxe.config-token".into(),
            idempotency_key: "preview-1".into(),
            reason: "provision a dedicated app".into(),
            target_connection_id: None,
        }),
    )
    .await
    .expect_err("a plaintext dedicated endpoint cannot be previewed");

    assert_eq!(error.code(), Code::Internal);
    assert!(
        error.message().contains("HTTPS"),
        "the failure must name the transport requirement, got {}",
        error.message()
    );
    // Capabilities were negotiated first — the mode check is not skipped.
    assert!(fixture.stub.was_called("/v1/capabilities"));
    // And the Configuration Token never left the process for an unusable manifest.
    assert!(
        !fixture.stub.was_called("/api/apps.manifest.validate"),
        "no manifest may be sent to Slack once its endpoints are rejected"
    );

    // Nothing was persisted: a refused preview leaves no checkpoint to resume.
    assert!(
        fixture.server.dedicated_sessions.lock().await.is_empty(),
        "a refused preview must not hold a Configuration Token"
    );
}

#[tokio::test]
async fn preview_refuses_a_gateway_that_does_not_offer_dedicated_mode() {
    let fixture = Fixture::with_gateway().await;
    fixture.stub.reply(
        "/v1/capabilities",
        json!({
            "protocol_version": 1,
            "supported_modes": ["managed_shared"],
            "max_delivery_batch": 50,
            "permalink_proxy": true
        }),
    );

    let error = dedicated_preview(
        &fixture.server,
        Request::new(SourceConnectionDedicatedPreviewRequest {
            project_id: "default".into(),
            display_label: "Acme workspace".into(),
            config_token: "xoxe.config-token".into(),
            idempotency_key: "preview-2".into(),
            reason: "provision a dedicated app".into(),
            target_connection_id: None,
        }),
    )
    .await
    .expect_err("dedicated must be advertised before it is offered");

    assert_eq!(error.code(), Code::FailedPrecondition);
    assert!(
        !fixture.stub.was_called("/api/apps.manifest.validate"),
        "an unsupported mode must stop before any Slack traffic"
    );
}

#[tokio::test]
async fn preview_rejects_an_empty_configuration_token_without_contacting_anyone() {
    let fixture = Fixture::with_gateway().await;
    let error = dedicated_preview(
        &fixture.server,
        Request::new(SourceConnectionDedicatedPreviewRequest {
            project_id: "default".into(),
            display_label: "Acme workspace".into(),
            config_token: "   ".into(),
            idempotency_key: "preview-3".into(),
            reason: "provision a dedicated app".into(),
            target_connection_id: None,
        }),
    )
    .await
    .expect_err("a blank token is invalid");
    assert_eq!(error.code(), Code::InvalidArgument);
    assert!(fixture.stub.calls().is_empty());
}

#[tokio::test]
async fn approving_a_reviewed_checkpoint_creates_the_app_imports_it_and_opens_oauth() {
    let fixture = Fixture::with_gateway().await;
    let (_, digest, callback, events) = reviewed_manifest();
    seed_awaiting_approval(&fixture, None).await;
    script_successful_approval(&fixture, &callback, &events, &digest);

    let response = dedicated_approve(&fixture.server, Request::new(approve_request()))
        .await
        .expect("approval succeeds")
        .into_inner();

    assert_eq!(response.status, "oauth_pending");
    assert_eq!(response.manifest_version, DEDICATED_MANIFEST_VERSION);
    assert_eq!(
        response.authorize_url.as_deref(),
        Some("https://slack.example/oauth/authorize?state=dedicated")
    );
    let intent_id = response
        .oauth_intent_id
        .expect("an OAuth intent was opened");

    // The three outbound steps happened, in order, each authorised by the right secret.
    assert_eq!(
        fixture.stub.paths(),
        vec![
            "/v1/dedicated/import-slots".to_string(),
            "/api/apps.manifest.create".to_string(),
            "/v1/dedicated/import".to_string(),
        ]
    );
    assert_eq!(
        fixture
            .stub
            .call("/v1/dedicated/import-slots")
            .authorization
            .as_deref(),
        Some(ENROLLMENT_KEY)
    );
    assert_eq!(
        fixture
            .stub
            .call("/v1/dedicated/import")
            .authorization
            .as_deref(),
        Some("import-secret-1"),
        "the import is authorised by the slot secret, never by the enrollment key"
    );

    // The App's own secrets went to the Gateway and were never persisted locally.
    let import = fixture.stub.call("/v1/dedicated/import");
    assert_eq!(import.body["credentials"]["app_id"], json!(APP_ID));
    assert_eq!(
        import.body["credentials"]["signing_secret"],
        json!("signing-secret-value")
    );

    let stored = fixture
        .repository()
        .intent_credential("default", &intent_id, &fixture.daemon_id().await)
        .await
        .expect("read intent")
        .expect("intent stored");
    assert_eq!(stored.gateway_intent_id, "gw-dedicated-intent-1");
    assert!(
        !stored
            .poll_secret_ciphertext
            .contains("poll-secret-dedicated")
    );

    // The checkpoint advanced, and its App identity is a digest rather than the ID.
    let checkpoint = fixture
        .repository()
        .dedicated_provisioning("default", PROVISIONING_ID)
        .await
        .expect("read checkpoint")
        .expect("checkpoint exists");
    assert_eq!(checkpoint.status, "oauth_pending");
    assert_eq!(
        checkpoint.app_id_digest.as_deref(),
        Some(hex::encode(Sha256::digest(APP_ID.as_bytes())).as_str())
    );
    assert_eq!(
        checkpoint.oauth_intent_id.as_deref(),
        Some(intent_id.as_str())
    );
}

#[tokio::test]
async fn an_endpoint_the_gateway_disagrees_with_stops_the_import_and_raises_attention() {
    let fixture = Fixture::with_gateway().await;
    let (_, _, _, events) = reviewed_manifest();
    seed_awaiting_approval(&fixture, None).await;
    fixture.stub.reply(
        "/v1/dedicated/import-slots",
        json!({
            "connection_id": PROVISIONING_ID,
            "import_secret": "import-secret-1",
            "expires_at": rfc3339_in(chrono::Duration::minutes(10)),
            // A callback URL that is not the reviewed one: the operator approved
            // a manifest pointing somewhere else.
            "oauth_callback_url": "https://attacker.example/callback",
            "events_url": events,
        }),
    );

    let error = dedicated_approve(&fixture.server, Request::new(approve_request()))
        .await
        .expect_err("an endpoint mismatch must not proceed to App creation");

    assert_eq!(error.code(), Code::DataLoss);
    assert!(
        !fixture.stub.was_called("/api/apps.manifest.create"),
        "no Slack App may be created once the endpoints disagree"
    );
    let checkpoint = fixture
        .repository()
        .dedicated_provisioning("default", PROVISIONING_ID)
        .await
        .expect("read checkpoint")
        .expect("checkpoint exists");
    assert_eq!(checkpoint.status, "attention");
    assert_eq!(
        checkpoint.error_code.as_deref(),
        Some("gateway_endpoint_mismatch")
    );

    // An operator-visible decision was raised, not just a log line.
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
            .any(|candidate| candidate.kind == "source_connection_provisioning_attention"),
        "a stalled provisioning must surface as an attention item"
    );
}

#[tokio::test]
async fn a_forged_import_receipt_is_refused() {
    let fixture = Fixture::with_gateway().await;
    let (_, digest, callback, events) = reviewed_manifest();
    seed_awaiting_approval(&fixture, None).await;
    script_successful_approval(&fixture, &callback, &events, &digest);
    let app_id_digest = hex::encode(Sha256::digest(APP_ID.as_bytes()));
    // Same shape, signature computed over a different generation.
    fixture.stub.reply(
        "/v1/dedicated/import",
        json!({
            "connection_id": PROVISIONING_ID,
            "app_id_digest": app_id_digest,
            "credential_generation": 1,
            "receipt_signature": receipt(PROVISIONING_ID, &app_id_digest, 7, &digest),
            "intent_id": "gw-dedicated-intent-1",
            "authorize_url": "https://slack.example/oauth/authorize?state=dedicated",
            "poll_secret": "poll-secret-dedicated",
            "expires_at": rfc3339_in(chrono::Duration::minutes(15)),
        }),
    );

    let error = dedicated_approve(&fixture.server, Request::new(approve_request()))
        .await
        .expect_err("an unverifiable receipt must not be adopted");

    assert_eq!(error.code(), Code::Unavailable);
    assert!(error.message().contains("receipt"));
    // The session is restored so the operator can retry rather than losing the App.
    assert!(
        fixture
            .server
            .dedicated_sessions
            .lock()
            .await
            .contains_key(PROVISIONING_ID),
        "a failed import must leave the session resumable"
    );
}

#[tokio::test]
async fn approval_refuses_a_session_whose_manifest_is_not_the_reviewed_one() {
    let fixture = Fixture::with_gateway().await;
    seed_awaiting_approval(&fixture, None).await;
    // The operator reviewed one manifest; the session now claims another.
    {
        let mut sessions = fixture.server.dedicated_sessions.lock().await;
        let session = sessions.get_mut(PROVISIONING_ID).expect("seeded session");
        session.manifest_digest = "a-different-digest".into();
    }

    let error = dedicated_approve(&fixture.server, Request::new(approve_request()))
        .await
        .expect_err("a substituted manifest must be refused");

    assert_eq!(error.code(), Code::PermissionDenied);
    assert!(fixture.stub.calls().is_empty());
}

#[tokio::test]
async fn approval_without_a_live_session_reports_the_lost_session_rather_than_recreating_it() {
    let fixture = Fixture::with_gateway().await;
    seed_awaiting_approval(&fixture, None).await;
    fixture.server.dedicated_sessions.lock().await.clear();

    let error = dedicated_approve(&fixture.server, Request::new(approve_request()))
        .await
        .expect_err("a lost session cannot be approved");

    assert_eq!(error.code(), Code::FailedPrecondition);
    assert!(error.message().contains("provisioning_session_lost"));
    assert!(
        !fixture.stub.was_called("/api/apps.manifest.create"),
        "automatic App recreation is disabled by design"
    );
}

#[tokio::test]
async fn an_expired_checkpoint_cannot_be_approved_and_is_marked_for_attention() {
    let fixture = Fixture::with_gateway().await;
    let (manifest, digest, _, _) = reviewed_manifest();
    let daemon_id = fixture.daemon_id().await;
    fixture
        .repository()
        .store_dedicated_provisioning(StoreDedicatedProvisioning {
            id: PROVISIONING_ID.into(),
            project_id: "default".into(),
            display_label: "Acme workspace".into(),
            owner_daemon_id: daemon_id.clone(),
            target_connection_id: None,
            manifest_version: DEDICATED_MANIFEST_VERSION.into(),
            manifest_digest: digest.clone(),
            expires_at: rfc3339_in(chrono::Duration::minutes(-1)),
        })
        .await
        .expect("seed expired checkpoint");
    fixture.server.dedicated_sessions.lock().await.insert(
        PROVISIONING_ID.into(),
        DedicatedSession {
            project_id: "default".into(),
            display_label: "Acme workspace".into(),
            owner_daemon_id: daemon_id,
            manifest,
            manifest_digest: digest,
            config_token: Zeroizing::new("xoxe.config-token".into()),
            import_secret: None,
            created_credentials: None,
        },
    );

    let error = dedicated_approve(&fixture.server, Request::new(approve_request()))
        .await
        .expect_err("an expired session cannot be approved");

    assert_eq!(error.code(), Code::FailedPrecondition);
    let checkpoint = fixture
        .repository()
        .dedicated_provisioning("default", PROVISIONING_ID)
        .await
        .expect("read checkpoint")
        .expect("checkpoint exists");
    assert_eq!(checkpoint.status, "attention");
    assert_eq!(
        checkpoint.error_code.as_deref(),
        Some("provisioning_session_expired")
    );
    assert!(
        !fixture
            .server
            .dedicated_sessions
            .lock()
            .await
            .contains_key(PROVISIONING_ID),
        "the expired session's Configuration Token must be dropped"
    );
}

#[tokio::test]
async fn reading_a_checkpoint_whose_session_vanished_reports_the_loss() {
    let fixture = Fixture::with_gateway().await;
    seed_awaiting_approval(&fixture, None).await;
    // Advance to a status that requires a live session, then lose it.
    fixture
        .repository()
        .update_dedicated_provisioning(
            agent_orchestrator::source_connection::UpdateDedicatedProvisioning {
                project_id: "default".into(),
                id: PROVISIONING_ID.into(),
                expected_status: "awaiting_approval".into(),
                status: "creating".into(),
                app_id_ciphertext: None,
                app_id_digest: None,
                oauth_intent_id: None,
                error_code: None,
            },
        )
        .await
        .expect("advance checkpoint");
    fixture.server.dedicated_sessions.lock().await.clear();

    let response = dedicated_get(
        &fixture.server,
        Request::new(SourceConnectionDedicatedGetRequest {
            project_id: "default".into(),
            provisioning_id: PROVISIONING_ID.into(),
        }),
    )
    .await
    .expect("get succeeds")
    .into_inner();

    assert_eq!(response.status, "attention");
    assert_eq!(
        response.error_code.as_deref(),
        Some("provisioning_session_lost")
    );
}

#[tokio::test]
async fn reading_an_unexpired_checkpoint_leaves_it_alone() {
    let fixture = Fixture::with_gateway().await;
    seed_awaiting_approval(&fixture, None).await;

    let response = dedicated_get(
        &fixture.server,
        Request::new(SourceConnectionDedicatedGetRequest {
            project_id: "default".into(),
            provisioning_id: PROVISIONING_ID.into(),
        }),
    )
    .await
    .expect("get succeeds")
    .into_inner();

    assert_eq!(response.status, "awaiting_approval");
    assert_eq!(response.error_code, None);
    assert!(
        response.diff.is_empty(),
        "a read must not re-render the diff"
    );

    let missing = dedicated_get(
        &fixture.server,
        Request::new(SourceConnectionDedicatedGetRequest {
            project_id: "default".into(),
            provisioning_id: "dedicated-absent".into(),
        }),
    )
    .await
    .expect_err("an unknown checkpoint is not found");
    assert_eq!(missing.code(), Code::NotFound);
}

#[tokio::test]
async fn abandoning_a_checkpoint_drops_its_session_and_resolves_the_attention() {
    let fixture = Fixture::with_gateway().await;
    seed_awaiting_approval(&fixture, None).await;

    let response = dedicated_abandon(
        &fixture.server,
        Request::new(SourceConnectionDedicatedMutationRequest {
            project_id: "default".into(),
            provisioning_id: PROVISIONING_ID.into(),
            idempotency_key: "abandon-1".into(),
            reason: "operator changed their mind".into(),
        }),
    )
    .await
    .expect("abandon succeeds")
    .into_inner();

    assert_eq!(response.status, "abandoned");
    assert_eq!(
        response.error_code.as_deref(),
        Some("provisioning_abandoned")
    );
    assert!(
        !fixture
            .server
            .dedicated_sessions
            .lock()
            .await
            .contains_key(PROVISIONING_ID),
        "abandoning must drop the held Configuration Token"
    );

    let again = dedicated_abandon(
        &fixture.server,
        Request::new(SourceConnectionDedicatedMutationRequest {
            project_id: "default".into(),
            provisioning_id: PROVISIONING_ID.into(),
            idempotency_key: "abandon-2".into(),
            reason: "operator retried".into(),
        }),
    )
    .await
    .expect_err("a terminal checkpoint cannot be abandoned twice");
    assert_eq!(again.code(), Code::FailedPrecondition);
}

#[tokio::test]
async fn a_migration_target_that_changed_after_review_stops_the_approval() {
    let fixture = Fixture::with_gateway().await;
    let (_, digest, callback, events) = reviewed_manifest();
    // The checkpoint names a migration target that does not exist any more.
    seed_awaiting_approval(&fixture, Some("conn-vanished".into())).await;
    script_successful_approval(&fixture, &callback, &events, &digest);

    let error = dedicated_approve(&fixture.server, Request::new(approve_request()))
        .await
        .expect_err("a vanished migration target must stop the approval");

    assert_eq!(error.code(), Code::NotFound);
    assert!(
        !fixture.stub.was_called("/v1/dedicated/import"),
        "the import must not run once the migration target is gone"
    );
}
