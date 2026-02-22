use crate::config_validation::{
    ErrorCode, ValidationError, ValidationResult, ValidationWarning, WarningCode,
};
use serde_yaml::Value;

/// Pre-check YAML syntax before full deserialization
pub fn precheck_yaml(content: &str) -> ValidationResult {
    let mut result = ValidationResult::new();
    result.is_valid = true;

    if content.trim().is_empty() {
        result.add_warning(ValidationWarning {
            code: WarningCode::MissingRecommendedField,
            message: "Empty YAML configuration".to_string(),
            field: None,
            suggestion: Some(
                "Add required fields: runner, workspaces, agents, workflows".to_string(),
            ),
        });
        return result;
    }

    match serde_yaml::from_str::<Value>(content) {
        Ok(value) => check_structure(&value, &mut result),
        Err(e) => {
            let location = e
                .location()
                .map(|loc| format!("line {}, column {}", loc.line(), loc.column()))
                .unwrap_or_else(|| "unknown".to_string());

            result.add_error(ValidationError {
                code: ErrorCode::YamlSyntaxError,
                message: format!("YAML syntax error: {}", e),
                field: None,
                context: Some(location),
            });
        }
    }

    result
}

fn check_structure(value: &Value, result: &mut ValidationResult) {
    match value {
        Value::Mapping(map) => {
            let recommended = ["runner", "workspaces", "agents", "workflows"];
            for field in recommended {
                if !map.contains_key(&Value::String(field.to_string())) {
                    result.add_warning(ValidationWarning {
                        code: WarningCode::MissingRecommendedField,
                        message: format!("Recommended field '{}' is missing", field),
                        field: Some(field.to_string()),
                        suggestion: Some(format!("Add '{}' to your configuration", field)),
                    });
                }
            }
        }
        Value::Sequence(_) | Value::String(_) => {
            result.add_error(ValidationError {
                code: ErrorCode::YamlStructureError,
                message: "Root must be a mapping (dictionary), not a list or scalar".to_string(),
                field: None,
                context: None,
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_yaml() {
        let yaml = "runner:\n  shell: /bin/bash\nworkspaces: {}";
        let result = precheck_yaml(yaml);
        assert!(result.is_valid);
    }

    #[test]
    fn test_broken_yaml() {
        let yaml = "invalid: yaml: content: [";
        let result = precheck_yaml(yaml);
        assert!(!result.is_valid);
        assert!(matches!(result.errors[0].code, ErrorCode::YamlSyntaxError));
    }

    #[test]
    fn test_empty_yaml() {
        let yaml = "";
        let result = precheck_yaml(yaml);
        assert!(result.is_valid);
        assert!(
            !result.warnings.is_empty(),
            "Expected warnings for missing fields"
        );
    }

    #[test]
    fn test_root_is_sequence() {
        let yaml = "- item1\n- item2";
        let result = precheck_yaml(yaml);
        assert!(!result.is_valid);
        assert!(matches!(
            result.errors[0].code,
            ErrorCode::YamlStructureError
        ));
    }
}
