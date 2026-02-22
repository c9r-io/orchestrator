use crate::config::OrchestratorConfig;
use crate::config_validation::{
    ErrorCode, PathValidationOptions, ValidationError, ValidationResult, ValidationWarning,
    WarningCode,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn validate_paths(
    config: &OrchestratorConfig,
    app_root: &Path,
    options: &PathValidationOptions,
) -> ValidationResult {
    let mut result = ValidationResult::new();
    result.is_valid = true;

    for (id, ws) in &config.workspaces {
        validate_workspace_paths(id, ws, app_root, options, &mut result);
    }

    result
}

fn validate_workspace_paths(
    id: &str,
    ws: &crate::config::WorkspaceConfig,
    app_root: &Path,
    options: &PathValidationOptions,
    result: &mut ValidationResult,
) {
    let field_prefix = format!("workspaces.{}", id);
    let root_path = app_root.join(&ws.root_path);

    if root_path.exists() {
        if let Ok(canonical) = root_path.canonicalize() {
            if let Ok(app_canonical) = app_root.canonicalize() {
                if !canonical.starts_with(&app_canonical) {
                    result.add_error(ValidationError {
                        code: ErrorCode::PathOutsideWorkspace,
                        message: "Workspace root_path is outside app root".to_string(),
                        field: Some(format!("{}.root_path", field_prefix)),
                        context: Some(canonical.display().to_string()),
                    });
                }
            }
        }
    } else {
        if options.missing_path_is_error {
            result.add_error(ValidationError {
                code: ErrorCode::PathNotFound,
                message: format!("Workspace root_path does not exist: {}", ws.root_path),
                field: Some(format!("{}.root_path", field_prefix)),
                context: None,
            });
        } else {
            result.add_warning(ValidationWarning {
                code: WarningCode::PathNotExists,
                message: format!("Workspace root_path does not exist: {}", ws.root_path),
                field: Some(format!("{}.root_path", field_prefix)),
                suggestion: Some("Create the directory or update root_path".to_string()),
            });
        }
    }

    for (idx, target) in ws.qa_targets.iter().enumerate() {
        let target_path = root_path.join(target);
        let field_name = format!("{}.qa_targets[{}]", field_prefix, idx);

        if target_path.exists() {
            if !target_path.is_dir() {
                result.add_error(ValidationError {
                    code: ErrorCode::PathNotDirectory,
                    message: "qa_targets must be directories".to_string(),
                    field: Some(field_name.clone()),
                    context: Some(target_path.display().to_string()),
                });
            }

            if options.check_path_escape {
                if let Err(e) = check_path_escape(&root_path, &target_path, &field_name, result) {
                    result.add_error(e);
                }
            }
        } else {
            result.add_warning(ValidationWarning {
                code: WarningCode::PathNotExists,
                message: format!("qa_targets path does not exist: {}", target),
                field: Some(field_name),
                suggestion: Some("Create the directory or update qa_targets".to_string()),
            });
        }
    }

    let ticket_field_name = format!("{}.ticket_dir", field_prefix);
    let ticket_path = root_path.join(&ws.ticket_dir);

    if ticket_path.exists() && !ticket_path.is_dir() {
        result.add_error(ValidationError {
            code: ErrorCode::PathNotDirectory,
            message: "ticket_dir must be a directory".to_string(),
            field: Some(ticket_field_name.clone()),
            context: Some(ticket_path.display().to_string()),
        });
    }

    if options.check_path_escape && ticket_path.exists() {
        if let Err(e) = check_path_escape(&root_path, &ticket_path, &ticket_field_name, result) {
            result.add_error(e);
        }
    }
}

fn check_path_escape(
    root: &Path,
    target: &Path,
    field: &str,
    result: &mut ValidationResult,
) -> Result<(), ValidationError> {
    let root_canonical = root.canonicalize().map_err(|e| ValidationError {
        code: ErrorCode::PathNotFound,
        message: format!("Failed to canonicalize root: {}", e),
        field: Some(field.to_string()),
        context: None,
    })?;

    let target_canonical = target.canonicalize().map_err(|e| ValidationError {
        code: ErrorCode::PathNotFound,
        message: format!("Failed to canonicalize target: {}", e),
        field: Some(field.to_string()),
        context: None,
    })?;

    if !target_canonical.starts_with(&root_canonical) {
        return Err(ValidationError {
            code: ErrorCode::PathOutsideWorkspace,
            message: "Path escapes workspace root".to_string(),
            field: Some(field.to_string()),
            context: Some(target.display().to_string()),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn test_config() -> OrchestratorConfig {
        let mut config = OrchestratorConfig::default();
        config.workspaces.insert(
            "test".to_string(),
            crate::config::WorkspaceConfig {
                root_path: ".".to_string(),
                qa_targets: vec!["src".to_string()],
                ticket_dir: "tickets".to_string(),
            },
        );
        config
    }

    #[test]
    fn test_existing_paths() {
        let config = test_config();
        let app_root = env::current_dir().unwrap();
        let options = PathValidationOptions::default();

        let result = validate_paths(&config, &app_root, &options);
        assert!(result.is_valid);
    }

    #[test]
    fn test_missing_paths_warn() {
        let mut config = OrchestratorConfig::default();
        config.workspaces.insert(
            "missing".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "/nonexistent/path/xyz123".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
            },
        );

        let app_root = env::current_dir().unwrap();
        let options = PathValidationOptions::default();

        let result = validate_paths(&config, &app_root, &options);
        assert!(result.is_valid);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn test_missing_paths_error() {
        let mut config = OrchestratorConfig::default();
        config.workspaces.insert(
            "missing".to_string(),
            crate::config::WorkspaceConfig {
                root_path: "/nonexistent/path/xyz123".to_string(),
                qa_targets: vec!["docs".to_string()],
                ticket_dir: "tickets".to_string(),
            },
        );

        let app_root = env::current_dir().unwrap();
        let options = PathValidationOptions {
            missing_path_is_error: true,
            ..Default::default()
        };

        let result = validate_paths(&config, &app_root, &options);
        assert!(!result.is_valid);
    }
}
