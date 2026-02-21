use crate::config::OrchestratorConfig;
use crate::config_validation::{
    ErrorCode, PathValidationOptions, ValidationLevel, ValidationReport, ValidationResult,
};
use std::path::{Path, PathBuf};

pub struct ConfigValidator {
    app_root: PathBuf,
    level: ValidationLevel,
    path_options: PathValidationOptions,
}

impl ConfigValidator {
    pub fn new(app_root: impl Into<PathBuf>) -> Self {
        Self {
            app_root: app_root.into(),
            level: ValidationLevel::Full,
            path_options: PathValidationOptions::default(),
        }
    }

    pub fn with_level(mut self, level: ValidationLevel) -> Self {
        self.level = level;
        self
    }

    pub fn with_path_options(mut self, options: PathValidationOptions) -> Self {
        self.path_options = options;
        self
    }

    pub fn validate_yaml(&self, yaml_content: &str) -> ValidationResult {
        let mut result = crate::config_validation::precheck::precheck_yaml(yaml_content);

        if !result.is_valid {
            return result;
        }

        let config: OrchestratorConfig = match serde_yaml::from_str(yaml_content) {
            Ok(c) => c,
            Err(e) => {
                result.add_error(crate::config_validation::ValidationError {
                    code: ErrorCode::YamlStructureError,
                    message: format!("Failed to deserialize config: {}", e),
                    field: None,
                    context: None,
                });
                return result;
            }
        };

        self.validate_config(&config, result)
    }

    pub fn validate_config(
        &self,
        config: &OrchestratorConfig,
        mut result: ValidationResult,
    ) -> ValidationResult {
        if self.level >= ValidationLevel::Schema {
            let schema_result = crate::config_validation::schema::validate_schema(config);
            result.merge(schema_result);

            if !result.is_valid && self.level == ValidationLevel::Schema {
                return result;
            }
        }

        if self.level >= ValidationLevel::Full {
            let path_result = crate::config_validation::path_resolver::validate_paths(
                config,
                &self.app_root,
                &self.path_options,
            );
            result.merge(path_result);
        }

        result
    }

    pub fn validate_yaml_with_report(
        &self,
        yaml_content: &str,
    ) -> Result<ValidationReport, String> {
        let result = self.validate_yaml(yaml_content);

        let is_valid = result.is_valid;
        let errors = result.errors.clone();
        let warnings = result.warnings.clone();
        let summary = result.report();

        if errors.is_empty() {
            let config: OrchestratorConfig = serde_yaml::from_str(yaml_content)
                .map_err(|e| format!("Failed to parse: {}", e))?;

            let normalized = serde_yaml::to_string(&config)
                .map_err(|e| format!("Failed to serialize: {}", e))?;

            Ok(ValidationReport {
                valid: is_valid,
                normalized_yaml: normalized,
                errors,
                warnings,
                summary,
            })
        } else {
            Ok(ValidationReport {
                valid: is_valid,
                normalized_yaml: String::new(),
                errors,
                warnings,
                summary,
            })
        }
    }
}

pub fn validate(yaml_content: &str, app_root: &Path) -> ValidationResult {
    ConfigValidator::new(app_root)
        .with_level(ValidationLevel::Full)
        .validate_yaml(yaml_content)
}

pub fn validate_with_report(
    yaml_content: &str,
    app_root: &Path,
) -> Result<ValidationReport, String> {
    ConfigValidator::new(app_root)
        .with_level(ValidationLevel::Full)
        .validate_yaml_with_report(yaml_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_yaml() {
        let yaml = r#"
runner:
  shell: /bin/bash
  shell_arg: -lc
resume:
  auto: false
defaults:
  project: default
  workspace: default
  workflow: basic
workspaces:
  default:
    root_path: .
    qa_targets:
      - src
    ticket_dir: tickets
agents:
  echo:
    capabilities:
      - qa
    templates:
      qa: echo test
workflows:
  basic:
    steps:
      - id: qa
        type: qa
        enabled: true
"#;
        let result = validate(yaml, std::path::Path::new("."));
        assert!(
            result.is_valid,
            "Errors: {:?}, Warnings: {:?}",
            result.errors, result.warnings
        );
    }

    #[test]
    fn test_broken_yaml() {
        let yaml = "invalid: yaml: content: [";
        let result = validate(yaml, std::path::Path::new("."));
        assert!(!result.is_valid);
        assert!(matches!(result.errors[0].code, ErrorCode::YamlSyntaxError));
    }

    #[test]
    fn test_missing_required_fields() {
        let yaml = r#"
runner:
  shell: /bin/bash
workspaces: {}
agents: {}
workflows: {}
"#;
        let result = validate(yaml, std::path::Path::new("."));
        assert!(!result.is_valid);
    }

    #[test]
    fn test_report_generation() {
        let yaml = r#"
runner:
  shell: /bin/bash
workspaces: {}
agents: {}
workflows: {}
"#;
        let result = validate(yaml, std::path::Path::new("."));
        let report = result.report();
        assert!(!report.is_empty());
    }
}
