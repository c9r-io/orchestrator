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
    match e.code() {
        tonic::Code::FailedPrecondition => {
            if msg.starts_with("use --force") {
                anyhow::anyhow!(
                    "{}\nhint: check --force to confirm the requested deletion",
                    msg
                )
            } else {
                anyhow::anyhow!("{}", msg)
            }
        }
        _ => anyhow::anyhow!("{}", msg),
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
