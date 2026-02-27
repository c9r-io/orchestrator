use crate::cli::{EditCommands, ManifestCommands, OutputFormat};
use crate::cli_types::OrchestratorResource;
use crate::config_load::{persist_config_and_reload, read_active_config};
use crate::resource::{dispatch_resource, kind_as_str, ApplyResult, RegisteredResource, Resource};
use anyhow::{Context, Result};
use std::process::ExitStatus;

use super::parse::parse_resource_selector;
use super::CliHandler;

impl CliHandler {
    pub(super) fn handle_manifest(&self, cmd: &ManifestCommands) -> Result<i32> {
        match cmd {
            ManifestCommands::Validate { .. } => {
                anyhow::bail!("manifest validate is handled as a preflight command")
            }
            ManifestCommands::Export { output, file } => {
                if *output == OutputFormat::Table {
                    anyhow::bail!("unsupported export output format: table");
                }
                let active = read_active_config(&self.state)?;
                let resources = crate::resource::export_manifest_resources(&active.config);
                let content = match output {
                    OutputFormat::Yaml => resources
                        .iter()
                        .map(Resource::to_yaml)
                        .collect::<Result<Vec<_>>>()?
                        .join("---\n"),
                    OutputFormat::Json => serde_json::to_string_pretty(
                        &crate::resource::export_manifest_documents(&active.config),
                    )?,
                    OutputFormat::Table => unreachable!(),
                };
                if let Some(path) = file {
                    std::fs::write(path, &content)
                        .with_context(|| format!("failed to write export file: {}", path))?;
                    println!("Manifest exported to {}", path);
                } else {
                    println!("{}", content);
                }
                Ok(0)
            }
        }
    }

    pub(super) fn handle_edit(&self, cmd: &EditCommands) -> Result<i32> {
        match cmd {
            EditCommands::Export { selector } => {
                let (kind_str, name) = parse_resource_selector(selector)?;
                let active = read_active_config(&self.state)?;
                let resource = RegisteredResource::get_from(&active.config, name)
                    .with_context(|| format!("resource not found: {}/{}", kind_str, name))?;
                let yaml = resource.to_yaml()?;
                let temp_file = write_to_temp_file(&yaml)?;
                println!("{}", temp_file.display());
                Ok(0)
            }
            EditCommands::Open { selector } => self.edit_open(selector),
        }
    }

    pub(super) fn edit_open(&self, selector: &str) -> Result<i32> {
        let (kind_str, name) = parse_resource_selector(selector)?;
        let (resource, mut merged_config) = {
            let active = read_active_config(&self.state)?;
            let resource = RegisteredResource::get_from(&active.config, name)
                .with_context(|| format!("resource not found: {}/{}", kind_str, name))?;
            (resource, active.config.clone())
        };

        let yaml = resource.to_yaml()?;
        let temp_file = write_to_temp_file(&yaml)?;
        let _temp_guard = TempFileGuard::new(temp_file.clone());

        let editor = std::env::var("EDITOR").context("$EDITOR is not set")?;
        loop {
            let status = self.run_editor(&editor, &temp_file)?;
            if is_ctrl_c_exit(&status) {
                eprintln!("Edit aborted by Ctrl+C");
                return Ok(130);
            }

            if !status.success() {
                anyhow::bail!("editor exited with non-zero status: {}", status);
            }

            let edited = std::fs::read_to_string(&temp_file)
                .with_context(|| format!("failed to read temp file: {}", temp_file.display()))?;
            if edited.trim().is_empty() {
                eprintln!("Edit aborted: empty file");
                return Ok(1);
            }

            let manifest: OrchestratorResource = match serde_yaml::from_str(&edited) {
                Ok(resource) => resource,
                Err(error) => {
                    eprintln!("Edited manifest is invalid YAML: {}", error);
                    continue;
                }
            };

            if let Err(error) = manifest.validate_version() {
                eprintln!("Edited manifest has invalid apiVersion: {}", error);
                continue;
            }

            let registered = match dispatch_resource(manifest) {
                Ok(resource) => resource,
                Err(error) => {
                    eprintln!("Edited manifest has invalid kind/spec: {}", error);
                    continue;
                }
            };

            if let Err(error) = registered.validate() {
                eprintln!(
                    "{} / {} invalid: {}",
                    kind_as_str(registered.kind()),
                    registered.name(),
                    error
                );
                continue;
            }

            let result = registered.apply(&mut merged_config);
            let merged_yaml = serde_yaml::to_string(&merged_config)
                .context("failed to serialize edited configuration")?;
            persist_config_and_reload(&self.state, merged_config, merged_yaml, "cli")?;

            let action = match result {
                ApplyResult::Created => "created",
                ApplyResult::Configured | ApplyResult::Unchanged => "configured",
            };
            println!(
                "{}/{} {}",
                kind_as_str(registered.kind()),
                registered.name(),
                action
            );
            return Ok(0);
        }
    }

    pub(super) fn run_editor(&self, editor: &str, temp_file: &std::path::Path) -> Result<ExitStatus> {
        std::process::Command::new(editor)
            .arg(temp_file)
            .status()
            .with_context(|| format!("failed to start editor command: {}", editor))
    }

    pub(super) fn apply_or_preview_manifest(
        &self,
        manifest: OrchestratorResource,
        dry_run: bool,
        output: OutputFormat,
    ) -> Result<i32> {
        manifest
            .validate_version()
            .map_err(anyhow::Error::msg)
            .context("invalid apiVersion in generated manifest")?;
        let registered = dispatch_resource(manifest.clone())?;
        registered.validate()?;

        if dry_run {
            match output {
                OutputFormat::Yaml => println!("{}", serde_yaml::to_string(&manifest)?),
                OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&manifest)?),
                OutputFormat::Table => {
                    anyhow::bail!("dry-run output format does not support table; use yaml or json")
                }
            }
            return Ok(0);
        }

        let mut merged_config = {
            let active = read_active_config(&self.state)?;
            active.config.clone()
        };
        let result = registered.apply(&mut merged_config);
        let merged_yaml = serde_yaml::to_string(&merged_config)
            .context("failed to serialize updated configuration")?;
        persist_config_and_reload(&self.state, merged_config, merged_yaml, "cli")?;

        let action = match result {
            ApplyResult::Created => "created",
            ApplyResult::Configured | ApplyResult::Unchanged => "configured",
        };
        println!(
            "{}/{} {}",
            kind_as_str(registered.kind()),
            registered.name(),
            action
        );
        Ok(0)
    }
}

pub(super) fn write_to_temp_file(content: &str) -> Result<std::path::PathBuf> {
    let temp_dir = std::env::temp_dir();
    let uuid = uuid::Uuid::new_v4();
    let filename = format!("orchestrator-edit-{}.yaml", uuid);
    let temp_file = temp_dir.join(&filename);
    std::fs::write(&temp_file, content)
        .with_context(|| format!("failed to write temp file: {}", temp_file.display()))?;
    Ok(temp_file)
}

pub(super) struct TempFileGuard {
    path: std::path::PathBuf,
}

impl TempFileGuard {
    pub(super) fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(super) fn is_ctrl_c_exit(status: &ExitStatus) -> bool {
    if status.code() == Some(130) {
        return true;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() == Some(2) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::super::CliHandler;
    use crate::cli::{Cli, Commands, EditCommands};
    use crate::config_load::read_active_config;
    use crate::test_utils::TestState;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Mutex, OnceLock};

    fn editor_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_editor_env<T>(editor: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = editor_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var("EDITOR").ok();

        match editor {
            Some(value) => unsafe { std::env::set_var("EDITOR", value) },
            None => unsafe { std::env::remove_var("EDITOR") },
        }

        let result = f();

        match previous {
            Some(value) => unsafe { std::env::set_var("EDITOR", value) },
            None => unsafe { std::env::remove_var("EDITOR") },
        }

        result
    }

    fn write_mock_editor_script(path: &std::path::Path, body: &str) {
        let script = format!("#!/bin/sh\nset -eu\n{}\n", body);
        std::fs::write(path, script).expect("mock editor script should be writable");
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(path, perms).expect("mock editor script should be executable");
    }

    fn ensure_workspace_structure(temp_root: &std::path::Path, root_path: &str) {
        let root = temp_root.join(root_path);
        std::fs::create_dir_all(root.join("docs/qa")).expect("qa dir should be creatable");
        std::fs::create_dir_all(root.join("docs/ticket")).expect("ticket dir should be creatable");
    }

    #[test]
    fn edit_export_returns_temp_file_path() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Edit(EditCommands::Export {
                selector: "workspace/default".to_string(),
            }),
            verbose: false,
        };

        let code = handler.execute(&cli).expect("edit export should succeed");
        assert_eq!(code, 0);
    }

    #[test]
    fn edit_export_returns_error_for_missing_resource() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Edit(EditCommands::Export {
                selector: "workspace/nonexistent".to_string(),
            }),
            verbose: false,
        };

        let result = handler.execute(&cli);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result);
        assert!(err_msg.contains("not found"));
    }

    #[test]
    fn edit_open_requires_editor_env() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());

        let cli = Cli {
            command: Commands::Edit(EditCommands::Open {
                selector: "workspace/default".to_string(),
            }),
            verbose: false,
        };

        let result = with_editor_env(None, || handler.execute(&cli));
        assert!(result.is_err());
        let err_text = format!(
            "{:#}",
            result.expect_err("should fail when EDITOR is unset")
        );
        assert!(err_text.contains("$EDITOR is not set"));
    }

    #[test]
    fn edit_open_applies_valid_edit() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let editor_path = fixture.temp_root().join("mock-editor-valid.sh");

        ensure_workspace_structure(fixture.temp_root(), "workspace/default-updated");

        write_mock_editor_script(
            &editor_path,
            r#"cat <<'YAML' > "$1"
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: workspace/default-updated
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
YAML"#,
        );

        let cli = Cli {
            command: Commands::Edit(EditCommands::Open {
                selector: "workspace/default".to_string(),
            }),
            verbose: false,
        };

        let code = with_editor_env(Some(&editor_path.display().to_string()), || {
            handler.execute(&cli).expect("edit open should succeed")
        });
        assert_eq!(code, 0);

        let active = read_active_config(&state).expect("config should be readable");
        let workspace = active
            .config
            .workspaces
            .get("default")
            .expect("workspace should exist");
        assert_eq!(workspace.root_path, "workspace/default-updated");
    }

    #[test]
    fn edit_validation_reopens_until_manifest_is_valid() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let editor_path = fixture.temp_root().join("mock-editor-reopen.sh");
        let count_file = fixture.temp_root().join("mock-editor-count.txt");

        ensure_workspace_structure(fixture.temp_root(), "workspace/default-reopened");

        write_mock_editor_script(
            &editor_path,
            &format!(
                r#"count_file="{}"
count=0
if [ -f "$count_file" ]; then
  count=$(cat "$count_file")
fi
count=$((count + 1))
printf "%s" "$count" > "$count_file"

if [ "$count" -eq 1 ]; then
  cat <<'YAML' > "$1"
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: ""
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
YAML
else
  cat <<'YAML' > "$1"
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: workspace/default-reopened
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
YAML
fi"#,
                count_file.display()
            ),
        );

        let cli = Cli {
            command: Commands::Edit(EditCommands::Open {
                selector: "workspace/default".to_string(),
            }),
            verbose: false,
        };

        let code = with_editor_env(Some(&editor_path.display().to_string()), || {
            handler
                .execute(&cli)
                .expect("edit open should eventually succeed")
        });
        assert_eq!(code, 0);

        let count = std::fs::read_to_string(&count_file).expect("count file should be present");
        assert_eq!(count.trim(), "2");

        let active = read_active_config(&state).expect("config should be readable");
        let workspace = active
            .config
            .workspaces
            .get("default")
            .expect("workspace should exist");
        assert_eq!(workspace.root_path, "workspace/default-reopened");
    }

    #[test]
    fn edit_open_handles_ctrl_c_gracefully() {
        let mut fixture = TestState::new();
        let state = fixture.build();
        let handler = CliHandler::new(state.clone());
        let editor_path = fixture.temp_root().join("mock-editor-ctrlc.sh");

        write_mock_editor_script(&editor_path, "exit 130");

        let cli = Cli {
            command: Commands::Edit(EditCommands::Open {
                selector: "workspace/default".to_string(),
            }),
            verbose: false,
        };

        let code = with_editor_env(Some(&editor_path.display().to_string()), || {
            handler
                .execute(&cli)
                .expect("ctrl+c should return exit code, not error")
        });
        assert_eq!(code, 130);
    }

    #[test]
    fn multi_document_yaml_parses_all_documents() {
        let yaml = r#"
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: ws-a
spec:
  root_path: workspace/ws-a
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: ws-b
spec:
  root_path: workspace/ws-b
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
"#;

        let parsed = crate::resource::parse_resources_from_yaml(yaml)
            .expect("multi-document parsing should succeed");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].metadata.name, "ws-a");
        assert_eq!(parsed[1].metadata.name, "ws-b");
    }
}
