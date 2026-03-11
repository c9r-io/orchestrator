use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use serde_json::json;
use tonic::transport::Channel;

use crate::{OutputFormat, SecretCommands, SecretKeyCommands};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: SecretCommands,
) -> Result<()> {
    match cmd {
        SecretCommands::Key(key_cmd) => dispatch_key(client, key_cmd).await,
    }
}

async fn dispatch_key(
    client: &mut OrchestratorServiceClient<Channel>,
    cmd: SecretKeyCommands,
) -> Result<()> {
    match cmd {
        SecretKeyCommands::Status { output } => {
            let resp = client
                .secret_key_status(orchestrator_proto::SecretKeyStatusRequest {})
                .await?
                .into_inner();
            print_status(&resp, output)
        }

        SecretKeyCommands::List { output } => {
            let resp = client
                .secret_key_list(orchestrator_proto::SecretKeyListRequest {})
                .await?
                .into_inner();
            print_list(&resp, output)
        }

        SecretKeyCommands::Rotate { resume } => {
            let resp = client
                .secret_key_rotate(orchestrator_proto::SecretKeyRotateRequest { resume })
                .await?
                .into_inner();
            println!("{}", resp.message);
            if resp.resources_updated > 0 || resp.versions_updated > 0 {
                println!(
                    "Re-encrypted: {} resources, {} versions",
                    resp.resources_updated, resp.versions_updated
                );
            }
            if !resp.errors.is_empty() {
                eprintln!("Errors:");
                for err in &resp.errors {
                    eprintln!("  - {err}");
                }
            }
            Ok(())
        }

        SecretKeyCommands::Revoke { key_id, force } => {
            let resp = client
                .secret_key_revoke(orchestrator_proto::SecretKeyRevokeRequest { key_id, force })
                .await?
                .into_inner();
            println!("{}", resp.message);
            Ok(())
        }

        SecretKeyCommands::History {
            limit,
            key_id,
            output,
        } => {
            let resp = client
                .secret_key_history(orchestrator_proto::SecretKeyHistoryRequest {
                    limit: limit as u64,
                    key_id,
                })
                .await?
                .into_inner();
            print_history(&resp, output)
        }
    }
}

fn print_status(
    resp: &orchestrator_proto::SecretKeyStatusResponse,
    output: OutputFormat,
) -> Result<()> {
    match output {
        OutputFormat::Json => {
            let active = resp.active_key.as_ref().map(key_record_to_json);
            let all: Vec<_> = resp.all_keys.iter().map(key_record_to_json).collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "active_key": active,
                    "all_keys": all,
                }))?
            );
            Ok(())
        }
        OutputFormat::Table => {
            if let Some(active) = &resp.active_key {
                println!("Active Key:");
                println!("  ID:          {}", active.key_id);
                println!("  State:       {}", active.state);
                println!("  Fingerprint: {}", active.fingerprint);
                println!("  Created:     {}", active.created_at);
            } else {
                println!("Active Key:    NONE (writes blocked)");
            }
            println!();
            println!("Total keys: {}", resp.all_keys.len());
            if resp.all_keys.len() > 1 {
                println!();
                println!("{:<24} {:<14} {:<18}", "KEY_ID", "STATE", "FINGERPRINT");
                for key in &resp.all_keys {
                    println!("{:<24} {:<14} {:<18}", key.key_id, key.state, key.fingerprint);
                }
            }
            Ok(())
        }
        OutputFormat::Yaml => anyhow::bail!("secret key commands support only table or json output"),
    }
}

fn print_list(
    resp: &orchestrator_proto::SecretKeyListResponse,
    output: OutputFormat,
) -> Result<()> {
    match output {
        OutputFormat::Json => {
            let keys: Vec<_> = resp.keys.iter().map(key_record_to_json).collect();
            println!("{}", serde_json::to_string_pretty(&json!({ "keys": keys }))?);
            Ok(())
        }
        OutputFormat::Table => {
            println!(
                "{:<24} {:<14} {:<18} {:<24}",
                "KEY_ID", "STATE", "FINGERPRINT", "CREATED_AT"
            );
            for key in &resp.keys {
                println!(
                    "{:<24} {:<14} {:<18} {:<24}",
                    key.key_id, key.state, key.fingerprint, key.created_at
                );
            }
            Ok(())
        }
        OutputFormat::Yaml => anyhow::bail!("secret key commands support only table or json output"),
    }
}

fn print_history(
    resp: &orchestrator_proto::SecretKeyHistoryResponse,
    output: OutputFormat,
) -> Result<()> {
    match output {
        OutputFormat::Json => {
            let events: Vec<_> = resp
                .events
                .iter()
                .map(|e| {
                    json!({
                        "event_kind": e.event_kind,
                        "key_id": e.key_id,
                        "key_fingerprint": e.key_fingerprint,
                        "actor": e.actor,
                        "detail_json": e.detail_json,
                        "created_at": e.created_at,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "events": events }))?
            );
            Ok(())
        }
        OutputFormat::Table => {
            println!(
                "{:<22} {:<24} {:<18} {:<20}",
                "EVENT", "KEY_ID", "ACTOR", "CREATED_AT"
            );
            for event in &resp.events {
                println!(
                    "{:<22} {:<24} {:<18} {:<20}",
                    event.event_kind, event.key_id, event.actor, event.created_at
                );
            }
            Ok(())
        }
        OutputFormat::Yaml => anyhow::bail!("secret key commands support only table or json output"),
    }
}

fn key_record_to_json(key: &orchestrator_proto::SecretKeyRecord) -> serde_json::Value {
    json!({
        "key_id": key.key_id,
        "state": key.state,
        "fingerprint": key.fingerprint,
        "file_path": key.file_path,
        "created_at": key.created_at,
        "activated_at": key.activated_at,
        "rotated_out_at": key.rotated_out_at,
        "retired_at": key.retired_at,
        "revoked_at": key.revoked_at,
    })
}
