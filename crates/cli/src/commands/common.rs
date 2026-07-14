use anyhow::Result;

use crate::OutputFormat;

pub(crate) fn resolve_resource(resource: &str, name: Option<&str>) -> String {
    match name {
        Some(n) => format!("{}/{}", resource, n),
        None => resource.to_string(),
    }
}

/// Strip gRPC protocol noise from error messages for human-friendly output.
pub(crate) fn format_grpc_error(e: tonic::Status) -> anyhow::Error {
    let msg = e.message().to_string();
    let request_id = e
        .metadata()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| format!(" (request_id: {value})"))
        .unwrap_or_default();
    match e.code() {
        tonic::Code::FailedPrecondition => {
            if msg.starts_with("use --force") {
                anyhow::anyhow!(
                    "{}{}\nhint: check --force to confirm the requested deletion",
                    msg,
                    request_id
                )
            } else {
                anyhow::anyhow!("{}{}", msg, request_id)
            }
        }
        _ => anyhow::anyhow!("{}{}", msg, request_id),
    }
}

pub(crate) fn read_input_or_file(file: &str) -> Result<String> {
    if file == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        std::fs::read_to_string(file)
            .map_err(|e| anyhow::anyhow!("failed to read manifest file '{}': {}", file, e))
    }
}

pub(crate) fn format_to_string(f: OutputFormat) -> String {
    match f {
        OutputFormat::Table => "table".to_string(),
        OutputFormat::Json => "json".to_string(),
        OutputFormat::Yaml => "yaml".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_grpc_error_preserves_failed_precondition_hint() {
        let err = format_grpc_error(tonic::Status::failed_precondition(
            "use --force to confirm task deletion",
        ));
        let rendered = err.to_string();
        assert!(rendered.contains("use --force to confirm task deletion"));
        assert!(rendered.contains("hint: check --force"));
    }

    #[test]
    fn format_grpc_error_preserves_not_found_message() {
        let err = format_grpc_error(tonic::Status::not_found("task.info: task not found: abc"));
        assert_eq!(err.to_string(), "task.info: task not found: abc");
    }

    #[test]
    fn format_grpc_error_includes_action_request_id() {
        let mut status = tonic::Status::failed_precondition("stale version");
        status
            .metadata_mut()
            .insert("x-request-id", "req-123".parse().expect("metadata"));
        let rendered = format_grpc_error(status).to_string();
        assert!(rendered.contains("request_id: req-123"));
    }
}
