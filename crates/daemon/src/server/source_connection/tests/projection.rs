//! Read surface, validation, and the pure projections the handlers compose.

use orchestrator_proto::*;
use tonic::{Code, Request};

use super::super::{
    catalog, dedicated_urls, get, list, local_terminal_intent_status, manifest_diff,
    safe_url_origin, semantic_manifest_diff, validate_config_token, validate_id, validate_label,
    validate_mutation, watch,
};
use super::Fixture;
use agent_orchestrator::source_connection::SourceConnectionMode;
use serde_json::json;

#[test]
fn gateway_expiry_is_projected_as_a_local_expired_intent() {
    assert_eq!(
        local_terminal_intent_status("failed", Some("oauth_intent_expired")),
        "expired"
    );
    assert_eq!(local_terminal_intent_status("expired", None), "expired");
    assert_eq!(local_terminal_intent_status("cancelled", None), "cancelled");
    assert_eq!(
        local_terminal_intent_status("failed", Some("provider_denied")),
        "failed"
    );
}

#[test]
fn semantic_upgrade_diff_is_stable_and_flags_only_expansion() {
    let current = json!({
        "oauth_config": {
            "scopes": {"bot": ["reactions:read"]},
            "redirect_urls": ["https://gateway.example/old/callback"]
        },
        "settings": {
            "event_subscriptions": {
                "request_url": "https://gateway.example/old/events",
                "bot_events": ["reaction_added"]
            },
            "token_rotation_enabled": false
        }
    });
    let target = json!({
        "oauth_config": {
            "scopes": {"bot": ["chat:write", "reactions:read", "reactions:read"]},
            "redirect_urls": ["https://gateway.example/new/callback"]
        },
        "settings": {
            "event_subscriptions": {
                "request_url": "https://gateway.example/new/events",
                "bot_events": ["reaction_added"]
            },
            "token_rotation_enabled": true
        }
    });

    let diff = semantic_manifest_diff(&current, &target).expect("semantic diff");
    assert_eq!(diff.len(), 5);
    assert_eq!(diff[0].field, "oauth.scopes.bot");
    assert_eq!(diff[0].change, "add");
    assert!(diff[0].permission_expansion);
    assert_eq!(diff[0].after, vec!["chat:write", "reactions:read"]);
    assert_eq!(diff[1].change, "unchanged");
    assert!(!diff[1].permission_expansion);
    assert_eq!(diff[2].before, vec!["https://gateway.example"]);
    assert_eq!(diff[2].after, vec!["https://gateway.example"]);
    assert_eq!(diff[4].change, "change");
    assert!(!diff[4].permission_expansion);
}

#[test]
fn a_removed_scope_is_not_reported_as_a_permission_expansion() {
    let current = json!({
        "oauth_config": {
            "scopes": {"bot": ["chat:write", "reactions:read"]},
            "redirect_urls": ["https://gateway.example/callback"]
        },
        "settings": {
            "event_subscriptions": {
                "request_url": "https://gateway.example/events",
                "bot_events": ["reaction_added"]
            },
            "token_rotation_enabled": false
        }
    });
    let target = json!({
        "oauth_config": {
            "scopes": {"bot": ["reactions:read"]},
            "redirect_urls": ["https://gateway.example/callback"]
        },
        "settings": {
            "event_subscriptions": {
                "request_url": "https://gateway.example/events",
                "bot_events": ["reaction_added"]
            },
            "token_rotation_enabled": false
        }
    });

    let diff = semantic_manifest_diff(&current, &target).expect("semantic diff");
    assert_eq!(diff[0].change, "remove");
    assert!(
        !diff[0].permission_expansion,
        "narrowing a scope must never require reauthorization"
    );
    assert!(diff.iter().all(|entry| !entry.permission_expansion));
}

#[test]
fn an_incomplete_exported_manifest_is_refused_rather_than_diffed_against_nothing() {
    let complete = json!({
        "oauth_config": {"scopes": {"bot": []}, "redirect_urls": []},
        "settings": {"event_subscriptions": {"request_url": "", "bot_events": []}}
    });
    // Each pointer the differ requires, removed one at a time.
    for missing in [
        "/oauth_config/scopes/bot",
        "/oauth_config/redirect_urls",
        "/settings/event_subscriptions/bot_events",
    ] {
        let mut broken = complete.clone();
        let (parent, key) = missing.rsplit_once('/').expect("pointer has a parent");
        broken
            .pointer_mut(parent)
            .and_then(serde_json::Value::as_object_mut)
            .expect("parent object")
            .remove(key);
        let error = semantic_manifest_diff(&broken, &complete)
            .expect_err("a manifest missing {missing} cannot be diffed");
        assert_eq!(error.code(), Code::FailedPrecondition, "missing {missing}");
    }
}

#[test]
fn configuration_tokens_are_bounded_without_echoing_the_value() {
    assert!(validate_config_token("xoxe.fixture").is_ok());
    assert_eq!(
        validate_config_token("")
            .expect_err("empty token rejected")
            .message(),
        "Configuration Token must contain 1-8192 characters"
    );
    let marker = "secret-marker".repeat(700);
    let error = validate_config_token(&marker).expect_err("oversized token rejected");
    assert!(!error.message().contains("secret-marker"));
}

#[test]
fn identifier_validation_rejects_traversal_and_control_characters() {
    assert!(validate_id("conn-T0AAAA", "connection id").is_ok());
    assert!(validate_id("a.b:c_d-e", "connection id").is_ok());
    for hostile in ["", "  ", "../etc/passwd", "a/b", "a b", "a\0b", "a\nb"] {
        assert!(
            validate_id(hostile, "connection id").is_err(),
            "{hostile:?} must be rejected"
        );
    }
    assert!(validate_id(&"a".repeat(128), "connection id").is_ok());
    assert!(validate_id(&"a".repeat(129), "connection id").is_err());

    assert!(validate_label("Acme workspace").is_ok());
    assert!(validate_label("line\nbreak").is_err());
    assert!(validate_label("").is_err());

    assert!(validate_mutation("a good reason", "key-1").is_ok());
    assert!(validate_mutation("", "key-1").is_err());
    assert!(validate_mutation(&"r".repeat(501), "key-1").is_err());
    assert!(validate_mutation("a good reason", "bad key").is_err());
}

#[test]
fn a_url_is_reduced_to_its_origin_and_an_unparseable_one_is_named_as_such() {
    assert_eq!(
        safe_url_origin("https://gateway.example/very/secret/path?token=abc"),
        "https://gateway.example"
    );
    assert_eq!(safe_url_origin("not a url"), "invalid-origin");
    assert!(
        !safe_url_origin("https://gateway.example/callback?token=abc").contains("abc"),
        "query material must never survive into a diff"
    );
}

#[test]
fn dedicated_endpoints_are_derived_from_the_gateway_origin() {
    let (callback, events) =
        dedicated_urls("https://gateway.example", "dedicated-1").expect("derive endpoints");
    assert_eq!(
        callback,
        "https://gateway.example/slack/connections/dedicated-1/oauth/callback"
    );
    assert_eq!(
        events,
        "https://gateway.example/slack/connections/dedicated-1/events"
    );
    assert!(dedicated_urls("not a url", "dedicated-1").is_err());
}

/// The composition `dedicated_preview` performs. A loopback Gateway origin cannot be
/// used for it end to end — a reviewed manifest must carry public HTTPS endpoints —
/// so the composition is asserted directly, including that rejection.
#[test]
fn a_rendered_dedicated_manifest_satisfies_the_reviewed_contract_only_over_https() {
    let asset: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../assets/dedicated-app-manifest.json"
    ))
    .expect("bundled manifest parses");

    let (callback, events) =
        dedicated_urls("https://gateway.example", "dedicated-1").expect("https endpoints");
    let mut manifest = asset.clone();
    orchestrator_slack_gateway::slack::render_manifest_endpoints(&mut manifest, &callback, &events)
        .expect("render https endpoints");
    let contract = orchestrator_slack_gateway::slack::reviewed_manifest_contract(&manifest)
        .expect("https manifest is reviewable");
    assert_eq!(contract.bot_scopes, vec!["reactions:read"]);
    assert_eq!(contract.redirect_url, callback);

    let diff = manifest_diff(&contract);
    assert_eq!(diff.len(), 5);
    assert!(
        diff.iter()
            .filter(|entry| entry.permission_expansion)
            .count()
            == 4,
        "a first grant expands everything except token rotation"
    );
    assert_eq!(diff[2].after, vec!["https://gateway.example"]);

    // The rejection lands one step earlier than the contract check: rendering itself
    // refuses a plaintext endpoint, so a loopback Gateway origin can never produce a
    // reviewable dedicated manifest.
    let (loopback_callback, loopback_events) =
        dedicated_urls("http://127.0.0.1:9", "dedicated-1").expect("loopback endpoints");
    let mut loopback = asset;
    let rendered = orchestrator_slack_gateway::slack::render_manifest_endpoints(
        &mut loopback,
        &loopback_callback,
        &loopback_events,
    );
    let rejection = rendered.expect_err("a plaintext endpoint must never be rendered");
    assert!(
        rejection.to_string().contains("HTTPS"),
        "the rejection must name the transport requirement, got {rejection}"
    );
}

#[tokio::test]
async fn the_catalog_reports_gateway_absence_rather_than_failing() {
    let fixture = Fixture::without_gateway().await;
    let response = catalog(
        &fixture.server,
        Request::new(SourceConnectionCatalogRequest {}),
    )
    .await
    .expect("catalog answers without a gateway")
    .into_inner();

    assert!(!response.gateway_configured);
    assert!(!response.permalink_proxy);
    let managed = response
        .modes
        .iter()
        .find(|mode| mode.mode == "managed_shared")
        .expect("managed_shared is listed");
    assert!(!managed.available);
    assert_eq!(
        managed.unavailable_reason.as_deref(),
        Some("gateway_not_configured")
    );
    let manual = response
        .modes
        .iter()
        .find(|mode| mode.mode == "manual")
        .expect("manual is listed");
    assert!(manual.available, "manual never depends on the gateway");
}

#[tokio::test]
async fn the_catalog_projects_gateway_capabilities_and_refuses_a_protocol_mismatch() {
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

    let response = catalog(
        &fixture.server,
        Request::new(SourceConnectionCatalogRequest {}),
    )
    .await
    .expect("catalog succeeds")
    .into_inner();
    assert!(response.gateway_configured);
    assert!(response.permalink_proxy);
    assert!(
        response
            .modes
            .iter()
            .find(|mode| mode.mode == "managed_shared")
            .expect("shared listed")
            .available
    );
    assert!(
        !response
            .modes
            .iter()
            .find(|mode| mode.mode == "managed_dedicated")
            .expect("dedicated listed")
            .available,
        "a mode the Gateway does not support must not be offered"
    );

    // A Gateway that reports a batch size of zero cannot deliver anything.
    fixture.stub.reply(
        "/v1/capabilities",
        json!({
            "protocol_version": 1,
            "supported_modes": ["managed_shared"],
            "max_delivery_batch": 0,
            "permalink_proxy": true
        }),
    );
    let error = catalog(
        &fixture.server,
        Request::new(SourceConnectionCatalogRequest {}),
    )
    .await
    .expect_err("a zero delivery batch is a capability mismatch");
    assert_eq!(error.code(), Code::FailedPrecondition);
}

#[tokio::test]
async fn listing_honours_the_project_boundary_the_provider_filter_and_the_default_limit() {
    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_connection("conn-A", "T0AAAA", SourceConnectionMode::ManagedShared)
        .await;
    fixture
        .seed_connection("conn-B", "T0BBBB", SourceConnectionMode::ManagedDedicated)
        .await;

    let all = list(
        &fixture.server,
        Request::new(SourceConnectionListRequest {
            project_id: "default".into(),
            limit: 0,
            ..Default::default()
        }),
    )
    .await
    .expect("list succeeds")
    .into_inner();
    assert_eq!(all.connections.len(), 2);

    let filtered = list(
        &fixture.server,
        Request::new(SourceConnectionListRequest {
            project_id: "default".into(),
            provider: Some("github".into()),
            ..Default::default()
        }),
    )
    .await
    .expect("list succeeds")
    .into_inner();
    assert!(
        filtered.connections.is_empty(),
        "the provider filter must actually filter"
    );

    let other_project = list(
        &fixture.server,
        Request::new(SourceConnectionListRequest {
            project_id: "other".into(),
            ..Default::default()
        }),
    )
    .await
    .expect("list succeeds")
    .into_inner();
    assert!(other_project.connections.is_empty());
}

#[tokio::test]
async fn get_projects_every_safe_field_and_hides_the_missing_one() {
    let fixture = Fixture::with_gateway().await;
    let seeded = fixture
        .seed_connection("conn-A", "T0AAAA", SourceConnectionMode::ManagedDedicated)
        .await;

    let projected = get(
        &fixture.server,
        Request::new(SourceConnectionGetRequest {
            project_id: "default".into(),
            id: "conn-A".into(),
        }),
    )
    .await
    .expect("get succeeds")
    .into_inner();

    assert_eq!(projected.id, seeded.id);
    assert_eq!(projected.provisioning_mode, "managed_dedicated");
    assert_eq!(projected.app_ownership, "workspace");
    assert_eq!(projected.installation_id, "T0AAAA");
    assert_eq!(projected.state, "active");
    assert_eq!(projected.generation, 1);
    assert_eq!(projected.version, 1);
    assert_eq!(projected.scopes, vec!["reactions:read".to_string()]);

    let missing = get(
        &fixture.server,
        Request::new(SourceConnectionGetRequest {
            project_id: "default".into(),
            id: "conn-absent".into(),
        }),
    )
    .await
    .expect_err("an absent connection is not found");
    assert_eq!(missing.code(), Code::NotFound);
}

#[tokio::test]
async fn watch_streams_the_transitions_a_connection_has_already_recorded() {
    use futures::StreamExt;

    let fixture = Fixture::with_gateway().await;
    fixture
        .seed_connection("conn-A", "T0AAAA", SourceConnectionMode::ManagedShared)
        .await;

    let mut stream = watch(
        &fixture.server,
        Request::new(SourceConnectionWatchRequest {
            project_id: "default".into(),
            after_cursor: 0,
            interval_millis: 250,
        }),
    )
    .await
    .expect("watch opens")
    .into_inner();

    let delta = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("a delta arrives before the timeout")
        .expect("the stream yields")
        .expect("the delta is not an error");
    assert_eq!(delta.state, "active");
    assert!(delta.cursor > 0);
    assert_eq!(
        delta.connection.expect("delta carries the connection").id,
        "conn-A"
    );
}

#[tokio::test]
async fn a_hostile_project_id_is_rejected_by_every_read_rpc() {
    let fixture = Fixture::with_gateway().await;
    for hostile in ["", "  ", "../other", "a/b"] {
        assert_eq!(
            list(
                &fixture.server,
                Request::new(SourceConnectionListRequest {
                    project_id: hostile.into(),
                    ..Default::default()
                }),
            )
            .await
            .expect_err("list rejects {hostile}")
            .code(),
            Code::InvalidArgument
        );
        assert_eq!(
            get(
                &fixture.server,
                Request::new(SourceConnectionGetRequest {
                    project_id: hostile.into(),
                    id: "conn-A".into(),
                }),
            )
            .await
            .expect_err("get rejects {hostile}")
            .code(),
            Code::InvalidArgument
        );
        // `expect_err` is unavailable here: the success arm is a boxed stream with
        // no `Debug`, so the error is taken explicitly.
        let watch_error = watch(
            &fixture.server,
            Request::new(SourceConnectionWatchRequest {
                project_id: hostile.into(),
                ..Default::default()
            }),
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("watch must reject {hostile:?}"));
        assert_eq!(watch_error.code(), Code::InvalidArgument);
    }
}
