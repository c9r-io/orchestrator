use anyhow::{Result, anyhow};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::ToolCommands;

type HmacSha256 = Hmac<Sha256>;

/// Dispatch a tool subcommand.
pub async fn dispatch(cmd: ToolCommands, control_plane_config: Option<&str>) -> Result<()> {
    match cmd {
        ToolCommands::WebhookVerifyHmac {
            algo,
            secret,
            body,
            signature,
        } => verify_hmac_cmd(&algo, &secret, &body, &signature),

        ToolCommands::PayloadExtract { path } => payload_extract_cmd(&path),

        ToolCommands::SecretRotate {
            store,
            key,
            value,
            project,
        } => {
            secret_rotate_cmd(
                control_plane_config,
                &store,
                &key,
                &value,
                project.as_deref(),
            )
            .await
        }
    }
}

fn verify_hmac_cmd(algo: &str, secret: &str, body: &str, signature: &str) -> Result<()> {
    if algo != "sha256" {
        return Err(anyhow!(
            "unsupported algorithm '{algo}' (only sha256 is supported)"
        ));
    }

    let hex_sig = signature.strip_prefix("sha256=").unwrap_or(signature);
    let expected = hex::decode(hex_sig).map_err(|e| anyhow!("invalid signature hex: {e}"))?;

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| anyhow!("invalid secret: {e}"))?;
    mac.update(body.as_bytes());

    if mac.verify_slice(&expected).is_ok() {
        println!("valid");
        Ok(())
    } else {
        eprintln!("invalid");
        std::process::exit(1);
    }
}

fn payload_extract_cmd(path: &str) -> Result<()> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
        .map_err(|e| anyhow!("failed to read stdin: {e}"))?;

    let value: serde_json::Value =
        serde_json::from_str(input.trim()).map_err(|e| anyhow!("invalid JSON input: {e}"))?;

    let result = extract_path(&value, path);
    match result {
        Some(v) => {
            if let Some(s) = v.as_str() {
                println!("{s}");
            } else {
                crate::output::render::emit(v, crate::output::render::Encoding::JsonCompact)?;
            }
            Ok(())
        }
        None => {
            eprintln!("path '{path}' not found");
            std::process::exit(1);
        }
    }
}

fn extract_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

async fn secret_rotate_cmd(
    control_plane_config: Option<&str>,
    store: &str,
    key: &str,
    value: &str,
    project: Option<&str>,
) -> Result<()> {
    let mut client = crate::client::connect(control_plane_config).await?;

    // Read current SecretStore via describe, update the key, and re-apply.
    let project_id = project.unwrap_or("default").to_string();
    let resource_path = format!("secretstore/{store}");
    let resp = client
        .describe(orchestrator_proto::DescribeRequest {
            resource: resource_path,
            output_format: "yaml".to_string(),
            project: Some(project_id.clone()),
        })
        .await?
        .into_inner();

    // Parse the existing manifest, update the target key
    let mut manifest: serde_yaml::Value =
        serde_yaml::from_str(&resp.content).map_err(|e| anyhow!("failed to parse store: {e}"))?;

    // Navigate to spec.data and set the key
    let data = manifest
        .get_mut("spec")
        .and_then(|s| s.get_mut("data"))
        .ok_or_else(|| anyhow!("SecretStore '{store}' has no spec.data"))?;

    data[serde_yaml::Value::String(key.to_string())] = serde_yaml::Value::String(value.to_string());

    // Re-apply via gRPC
    let yaml_content = serde_yaml::to_string(&manifest)?;
    let apply_resp = client
        .apply(secret_rotate_apply_request(yaml_content, project_id))
        .await?
        .into_inner();

    for entry in &apply_resp.results {
        println!("{}/{} {}", entry.kind, entry.name, entry.action);
    }
    Ok(())
}

/// Builds the apply request that rotates a SecretStore key.
///
/// Extracted so the envelope is assertable. Rotating a key rewrites a secret
/// value, and without the envelope the mutation left no attributable record:
/// the only trace was a `resource_versions` row whose author is the constant
/// `"daemon-apply"`. Under `action_audit_mode: enforced` it also slipped past
/// the rejection instead of being refused, because the daemon only consulted
/// the audit layer when a context was already present.
fn secret_rotate_apply_request(
    yaml_content: String,
    project_id: String,
) -> orchestrator_proto::ApplyRequest {
    orchestrator_proto::ApplyRequest {
        content: yaml_content,
        dry_run: false,
        prune: false,
        project: Some(project_id),
        audit: Some(orchestrator_proto::ActionAuditContext {
            reason_code: "operator_secret_rotate".to_string(),
            operator_reason: None,
            idempotency_key: Some(format!(
                "cli-secret-rotate-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default()
            )),
        }),
        expected_revision: None,
        require_absent: false,
    }
}

#[cfg(test)]
mod secret_rotate_tests {
    use super::secret_rotate_apply_request;

    /// A secret rotation must be attributable. This asserts the envelope on the
    /// request that is actually sent, rather than the presence of the text in
    /// the source: a commented-out envelope satisfies a grep and would leave the
    /// rotation unaudited exactly as before.
    #[test]
    fn rotation_carries_an_audit_envelope() {
        let request = secret_rotate_apply_request("spec: {}\n".into(), "default".into());
        let audit = request
            .audit
            .expect("secret rotation must carry an audit envelope");
        assert_eq!(audit.reason_code, "operator_secret_rotate");
        assert!(
            audit
                .idempotency_key
                .as_deref()
                .is_some_and(|key| key.starts_with("cli-secret-rotate-")),
            "rotation needs a retry identity so a replayed rotation is recognised"
        );
        assert!(
            !request.dry_run,
            "a rotation is a mutation; a dry run would not be audited"
        );
    }

    /// Two rotations must not collide on one retry identity, or the second would
    /// be rejected as a replay of the first.
    #[test]
    fn successive_rotations_get_distinct_retry_identities() {
        let first = secret_rotate_apply_request("spec: {}\n".into(), "default".into())
            .audit
            .and_then(|audit| audit.idempotency_key)
            .expect("first key");
        std::thread::sleep(std::time::Duration::from_nanos(1));
        let second = secret_rotate_apply_request("spec: {}\n".into(), "default".into())
            .audit
            .and_then(|audit| audit.idempotency_key)
            .expect("second key");
        assert_ne!(first, second);
    }
}
