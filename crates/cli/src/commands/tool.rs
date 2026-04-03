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
            "unsupported algorithm '{}' (only sha256 is supported)",
            algo
        ));
    }

    let hex_sig = signature.strip_prefix("sha256=").unwrap_or(signature);
    let expected = hex::decode(hex_sig).map_err(|e| anyhow!("invalid signature hex: {}", e))?;

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| anyhow!("invalid secret: {}", e))?;
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
        .map_err(|e| anyhow!("failed to read stdin: {}", e))?;

    let value: serde_json::Value =
        serde_json::from_str(input.trim()).map_err(|e| anyhow!("invalid JSON input: {}", e))?;

    let result = extract_path(&value, path);
    match result {
        Some(v) => {
            if let Some(s) = v.as_str() {
                println!("{}", s);
            } else {
                println!("{}", serde_json::to_string(&v).unwrap_or_default());
            }
            Ok(())
        }
        None => {
            eprintln!("path '{}' not found", path);
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
    let resource_path = format!("secretstore/{}", store);
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
        serde_yaml::from_str(&resp.content).map_err(|e| anyhow!("failed to parse store: {}", e))?;

    // Navigate to spec.data and set the key
    let data = manifest
        .get_mut("spec")
        .and_then(|s| s.get_mut("data"))
        .ok_or_else(|| anyhow!("SecretStore '{}' has no spec.data", store))?;

    data[serde_yaml::Value::String(key.to_string())] = serde_yaml::Value::String(value.to_string());

    // Re-apply via gRPC
    let yaml_content = serde_yaml::to_string(&manifest)?;
    let apply_resp = client
        .apply(orchestrator_proto::ApplyRequest {
            content: yaml_content,
            dry_run: false,
            prune: false,
            project: Some(project_id),
        })
        .await?
        .into_inner();

    for entry in &apply_resp.results {
        println!("{}/{} {}", entry.kind, entry.name, entry.action);
    }
    Ok(())
}
