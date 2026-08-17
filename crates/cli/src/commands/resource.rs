use anyhow::Result;
use orchestrator_proto::OrchestratorServiceClient;
use tonic::transport::Channel;

use super::common::{format_grpc_error, format_to_string, read_input_or_file, resolve_resource};
use crate::{Commands, OutputFormat};

pub(crate) async fn dispatch(
    client: &mut OrchestratorServiceClient<Channel>,
    command: Commands,
) -> Result<Option<Commands>> {
    match command {
        Commands::Apply {
            file,
            dry_run,
            prune,
            project,
        } => {
            let content = read_input_or_file(&file)?;
            let resp = client
                .apply(orchestrator_proto::ApplyRequest {
                    content,
                    dry_run,
                    project,
                    prune,
                    audit: Some(orchestrator_proto::ActionAuditContext {
                        reason_code: "operator_resource_apply".to_string(),
                        operator_reason: None,
                        idempotency_key: Some(format!(
                            "cli-resource-apply-{}",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|duration| duration.as_nanos())
                                .unwrap_or_default()
                        )),
                    }),
                    expected_revision: None,
                    require_absent: false,
                })
                .await
                .map_err(format_grpc_error)?
                .into_inner();

            for entry in &resp.results {
                let scope = entry
                    .project_scope
                    .as_ref()
                    .map(|p| format!(" (project: {p})"))
                    .unwrap_or_default();
                if dry_run {
                    println!(
                        "{}/{} would be {} (dry run){}",
                        entry.kind, entry.name, entry.action, scope
                    );
                } else {
                    println!("{}/{} {}{}", entry.kind, entry.name, entry.action, scope);
                }
            }
            if let Some(version) = resp.config_version {
                println!("configuration version: {version}");
            }
            for warning in &resp.warnings {
                eprintln!("Warning: {warning}");
            }
            for err in &resp.errors {
                eprintln!("Error: {err}");
            }
            for diagnostic in &resp.diagnostics {
                eprintln!(
                    "Diagnostic [{}]{}: {}",
                    diagnostic.code,
                    diagnostic
                        .field_path
                        .as_ref()
                        .map(|path| format!(" at {path}"))
                        .unwrap_or_default(),
                    diagnostic.message
                );
            }
            if !resp.diagnostics.is_empty()
                || resp
                    .warnings
                    .iter()
                    .chain(resp.errors.iter())
                    .any(|line| contains_bracketed_code(line))
            {
                eprintln!(
                    "Hint: run 'orchestrator guide error-codes' or see docs/guide/error-codes.md for what each [code] means"
                );
            }
            if !resp.errors.is_empty() {
                std::process::exit(1);
            }
            Ok(None)
        }
        Commands::Get {
            resource,
            name,
            output,
            selector,
            project,
        } => {
            let resource = resolve_resource(&resource, name.as_deref());
            let resp = client
                .get(orchestrator_proto::GetRequest {
                    resource,
                    selector,
                    output_format: format_to_string(output),
                    project,
                })
                .await?
                .into_inner();
            print!("{}", resp.content);
            Ok(None)
        }
        Commands::Describe {
            resource,
            name,
            output,
            project,
        } => {
            let resource = resolve_resource(&resource, name.as_deref());
            let resp = client
                .describe(orchestrator_proto::DescribeRequest {
                    resource,
                    output_format: format_to_string(output),
                    project,
                })
                .await?
                .into_inner();
            print!("{}", resp.content);
            Ok(None)
        }
        Commands::Delete {
            resource,
            name,
            force,
            force_references,
            dry_run,
            project,
        } => {
            let resource = resolve_resource(&resource, name.as_deref());
            let resp = client
                .delete(resource_delete_request(
                    resource,
                    force,
                    dry_run,
                    project,
                    force_references,
                ))
                .await
                .map_err(format_grpc_error)?
                .into_inner();
            println!("{}", resp.message);
            Ok(None)
        }
        other => Ok(Some(other)),
    }
}

/// Builds the delete request, envelope included.
///
/// Extracted so the envelope is assertable on the request actually constructed.
/// Since FR-167 every non-dry-run delete reserves an audit row, so an absent
/// envelope is what `action_audit_mode: enforced` refuses and a wrong one is what
/// gets stored forever.
///
/// The three fields were all written for `--force-references`, because until
/// FR-167 that was the only case the daemon read them in: an ordinary delete had
/// its envelope discarded, so the operator reason it carried was never stored and
/// so never wrong. Now that it is stored, a plain `delete secretstore/x` would
/// have persisted "atomically delete SourceTaskTemplate binding references" as
/// the operator's stated reason. The reason and the retry identity are therefore
/// scoped to the operation that justifies them.
fn resource_delete_request(
    resource: String,
    force: bool,
    dry_run: bool,
    project: Option<String>,
    force_references: bool,
) -> orchestrator_proto::DeleteRequest {
    orchestrator_proto::DeleteRequest {
        resource,
        force,
        dry_run,
        project,
        force_references,
        audit: Some(orchestrator_proto::ActionAuditContext {
            reason_code: if force_references {
                "operator_force_reference_cleanup".to_string()
            } else {
                "operator_resource_delete".to_string()
            },
            operator_reason: force_references
                .then(|| "atomically delete SourceTaskTemplate binding references".to_string()),
            idempotency_key: Some(format!(
                "{}-{}",
                if force_references {
                    "cli-resource-delete-references"
                } else {
                    "cli-resource-delete"
                },
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default()
            )),
        }),
    }
}

#[cfg(test)]
mod resource_delete_envelope_tests {
    use super::resource_delete_request;

    /// An ordinary delete must be attributable, and must not borrow the
    /// force-references justification. Asserted on the request that is actually
    /// built rather than on the presence of the text in the source: a
    /// commented-out envelope satisfies a grep and would leave the delete
    /// unaudited exactly as before FR-167.
    #[test]
    fn ordinary_delete_carries_its_own_envelope() {
        let request = resource_delete_request(
            "secretstore/prod".into(),
            true,
            false,
            Some("default".into()),
            false,
        );
        let audit = request.audit.expect("an ordinary delete must be audited");
        assert_eq!(audit.reason_code, "operator_resource_delete");
        assert_eq!(
            audit.operator_reason, None,
            "a plain delete must not claim to be a binding cleanup"
        );
        let key = audit.idempotency_key.expect("retry identity is required");
        assert!(
            key.starts_with("cli-resource-delete-")
                && !key.starts_with("cli-resource-delete-references-"),
            "an ordinary delete must not share the reference-cleanup key space: {key}"
        );
    }

    #[test]
    fn force_references_delete_keeps_the_cleanup_envelope() {
        let request = resource_delete_request(
            "sourcetasktemplate/analyze".into(),
            true,
            false,
            Some("default".into()),
            true,
        );
        let audit = request.audit.expect("a cleanup must be audited");
        assert_eq!(audit.reason_code, "operator_force_reference_cleanup");
        assert_eq!(
            audit.operator_reason.as_deref(),
            Some("atomically delete SourceTaskTemplate binding references")
        );
        assert!(
            audit
                .idempotency_key
                .expect("retry identity is required")
                .starts_with("cli-resource-delete-references-")
        );
    }

    /// Successive deletes must get distinct retry identities, or the second is
    /// refused as a replay of the first.
    #[test]
    fn successive_deletes_get_distinct_retry_identities() {
        let first = resource_delete_request("agent/a".into(), true, false, None, false)
            .audit
            .and_then(|audit| audit.idempotency_key)
            .expect("retry identity is required");
        let second = resource_delete_request("agent/a".into(), true, false, None, false)
            .audit
            .and_then(|audit| audit.idempotency_key)
            .expect("retry identity is required");
        assert_ne!(first, second, "two deletes shared one retry identity");
    }
}

/// True when a warning or error line carries a bracketed machine code like
/// `[driver_config_invalid]` — the shapes documented in
/// docs/guide/error-codes.md. Hand-rolled scan; a regex dependency for one
/// hint line would be over-engineering.
fn contains_bracketed_code(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut index = 0;
    while let Some(offset) = line[index..].find('[') {
        let start = index + offset + 1;
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_lowercase()
                || bytes[end].is_ascii_uppercase()
                || bytes[end].is_ascii_digit()
                || bytes[end] == b'_')
        {
            end += 1;
        }
        if end > start && end < bytes.len() && bytes[end] == b']' {
            return true;
        }
        index = start;
    }
    false
}

#[allow(dead_code)]
fn _assert_output_format_send(_format: OutputFormat) {}

#[cfg(test)]
mod tests {
    use super::contains_bracketed_code;

    #[test]
    fn bracketed_codes_are_detected() {
        assert!(contains_bracketed_code(
            "[secret_value_placeholder_rejected] SecretStore 's' key 'K' carries the placeholder"
        ));
        assert!(contains_bracketed_code(
            "[FILE_SHARING_GLOBAL_SKILL_UNTRUSTED] global Skill directory is untrusted"
        ));
        assert!(contains_bracketed_code(
            "workflow 'w' step 's': [driver_config_invalid] mid-line placement"
        ));
    }

    #[test]
    fn plain_prose_and_empty_brackets_are_not() {
        assert!(!contains_bracketed_code(
            "no agent supports capability 'qa'"
        ));
        assert!(!contains_bracketed_code("empty [] brackets"));
        assert!(!contains_bracketed_code("unclosed [bracket at end"));
        assert!(!contains_bracketed_code(
            "[spaced words] are prose, not a code"
        ));
    }
}
