use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
struct VersionInfo<'a> {
    version: &'a str,
    git_hash: &'a str,
    build_time: &'a str,
}

fn local_version_info() -> VersionInfo<'static> {
    VersionInfo {
        version: env!("CARGO_PKG_VERSION"),
        git_hash: env!("BUILD_GIT_HASH"),
        build_time: env!("BUILD_TIMESTAMP"),
    }
}

/// Print build metadata without opening a daemon session. `json` is the
/// deprecated `--json` alias for `-o json`, kept one release cycle.
pub async fn run(
    _control_plane_config: Option<&str>,
    output: crate::OutputFormat,
    json: bool,
) -> Result<()> {
    let version = local_version_info();
    let format = if json {
        crate::OutputFormat::Json
    } else {
        output
    };
    match format.encoding() {
        Some(encoding) => crate::output::render::emit(&serde_json::to_value(&version)?, encoding),
        None => {
            println!("Version:    {}", version.version);
            println!("Git Hash:   {}", version.git_hash);
            println!("Build Time: {}", version.build_time);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::local_version_info;

    #[test]
    fn local_version_info_exposes_build_metadata() {
        let info = local_version_info();
        assert!(!info.version.is_empty());
        assert!(!info.git_hash.is_empty());
        assert!(!info.build_time.is_empty());
    }
}
