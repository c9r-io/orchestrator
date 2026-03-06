use anyhow::{Context, Result};
use serde_json::Value;

/// Extract a JSON array from a JSON string using a simple path expression.
///
/// Supports paths like `$.field_name` or `$.field.nested`.
/// Returns the array found at the given path.
pub fn extract_json_array(json_str: &str, path: &str) -> Result<Vec<Value>> {
    let root: Value = serde_json::from_str(json_str).context("invalid JSON")?;
    let target = resolve_path(&root, path)?;
    match target {
        Value::Array(arr) => Ok(arr.clone()),
        _ => anyhow::bail!("path '{}' does not point to an array", path),
    }
}

/// Extract a single field value from a JSON Value using a simple dot-path.
///
/// Supports paths like `$.field_name` or `$.field.nested`.
/// Returns the string representation of the value, or None if not found.
pub fn extract_field(value: &Value, path: &str) -> Option<String> {
    let resolved = resolve_path(value, path).ok()?;
    match resolved {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn resolve_path<'a>(root: &'a Value, path: &str) -> Result<&'a Value> {
    let path = path.strip_prefix("$.").unwrap_or(path);
    let mut current = root;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        current = current
            .get(segment)
            .with_context(|| format!("field '{}' not found", segment))?;
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_array_simple() {
        let json = r#"{"goals": ["a", "b", "c"]}"#;
        let arr = extract_json_array(json, "$.goals").expect("extract goals");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], json!("a"));
    }

    #[test]
    fn extract_array_nested() {
        let json = r#"{"result": {"items": [1, 2]}}"#;
        let arr = extract_json_array(json, "$.result.items").expect("extract nested");
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn extract_array_not_array_fails() {
        let json = r#"{"goals": "not_array"}"#;
        let result = extract_json_array(json, "$.goals");
        assert!(result.is_err());
    }

    #[test]
    fn extract_array_missing_field_fails() {
        let json = r#"{"goals": []}"#;
        let result = extract_json_array(json, "$.missing");
        assert!(result.is_err());
    }

    #[test]
    fn extract_field_string() {
        let value = json!({"name": "test", "score": 42});
        assert_eq!(extract_field(&value, "$.name"), Some("test".to_string()));
    }

    #[test]
    fn extract_field_number() {
        let value = json!({"score": 42});
        assert_eq!(extract_field(&value, "$.score"), Some("42".to_string()));
    }

    #[test]
    fn extract_field_nested() {
        let value = json!({"meta": {"id": "abc"}});
        assert_eq!(
            extract_field(&value, "$.meta.id"),
            Some("abc".to_string())
        );
    }

    #[test]
    fn extract_field_missing_returns_none() {
        let value = json!({"name": "test"});
        assert_eq!(extract_field(&value, "$.missing"), None);
    }

    #[test]
    fn extract_field_null_returns_none() {
        let value = json!({"name": null});
        assert_eq!(extract_field(&value, "$.name"), None);
    }

    #[test]
    fn extract_field_boolean() {
        let value = json!({"active": true});
        assert_eq!(
            extract_field(&value, "$.active"),
            Some("true".to_string())
        );
    }
}
