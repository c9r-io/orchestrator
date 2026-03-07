//! Schema validation for store entry values.

use anyhow::{anyhow, Result};

/// Validate a JSON value against a JSON Schema (simplified subset).
///
/// Supports: type checking (object, string, integer, number, boolean, array),
/// required properties, and minimum/maximum for numbers.
pub fn validate_schema(value: &serde_json::Value, schema: &serde_json::Value) -> Result<()> {
    validate_node(value, schema, "$")
}

fn validate_node(value: &serde_json::Value, schema: &serde_json::Value, path: &str) -> Result<()> {
    // Type check
    if let Some(expected_type) = schema.get("type").and_then(|v| v.as_str()) {
        let actual_type = json_type_name(value);
        if !type_matches(value, expected_type) {
            return Err(anyhow!(
                "schema validation failed at {}: expected type '{}', got '{}'",
                path,
                expected_type,
                actual_type
            ));
        }
    }

    // Object-specific validation
    if let Some(obj) = value.as_object() {
        // Check required fields
        if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
            for req in required {
                if let Some(field_name) = req.as_str() {
                    if !obj.contains_key(field_name) {
                        return Err(anyhow!(
                            "schema validation failed at {}: missing required field '{}'",
                            path,
                            field_name
                        ));
                    }
                }
            }
        }

        // Validate properties
        if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
            for (prop_name, prop_schema) in properties {
                if let Some(prop_value) = obj.get(prop_name) {
                    let prop_path = format!("{}.{}", path, prop_name);
                    validate_node(prop_value, prop_schema, &prop_path)?;
                }
            }
        }
    }

    // Number range validation
    if let Some(num) = value.as_f64() {
        if let Some(min) = schema.get("minimum").and_then(|v| v.as_f64()) {
            if num < min {
                return Err(anyhow!(
                    "schema validation failed at {}: value {} is below minimum {}",
                    path,
                    num,
                    min
                ));
            }
        }
        if let Some(max) = schema.get("maximum").and_then(|v| v.as_f64()) {
            if num > max {
                return Err(anyhow!(
                    "schema validation failed at {}: value {} exceeds maximum {}",
                    path,
                    num,
                    max
                ));
            }
        }
    }

    Ok(())
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer"
            } else {
                "number"
            }
        }
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn type_matches(value: &serde_json::Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true, // unknown type — permissive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_object() {
        let schema = json!({
            "type": "object",
            "properties": {
                "test_count": { "type": "integer", "minimum": 0 }
            }
        });
        let value = json!({"test_count": 42});
        assert!(validate_schema(&value, &schema).is_ok());
    }

    #[test]
    fn wrong_type() {
        let schema = json!({"type": "object"});
        let value = json!("not an object");
        let err = validate_schema(&value, &schema);
        assert!(err.is_err());
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("expected type 'object'"));
    }

    #[test]
    fn missing_required_field() {
        let schema = json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string" }
            }
        });
        let value = json!({});
        let err = validate_schema(&value, &schema);
        assert!(err.is_err());
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("missing required field 'name'"));
    }

    #[test]
    fn number_below_minimum() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": { "type": "integer", "minimum": 0 }
            }
        });
        let value = json!({"count": -1});
        let err = validate_schema(&value, &schema);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("below minimum"));
    }

    #[test]
    fn number_above_maximum() {
        let schema = json!({
            "type": "integer",
            "maximum": 100
        });
        let value = json!(200);
        let err = validate_schema(&value, &schema);
        assert!(err.is_err());
    }

    #[test]
    fn nested_property_validation() {
        let schema = json!({
            "type": "object",
            "properties": {
                "inner": {
                    "type": "object",
                    "properties": {
                        "val": { "type": "string" }
                    }
                }
            }
        });
        let value = json!({"inner": {"val": 42}});
        let err = validate_schema(&value, &schema);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("$.inner.val"));
    }

    #[test]
    fn no_schema_always_passes() {
        let schema = json!({});
        let value = json!({"anything": "goes"});
        assert!(validate_schema(&value, &schema).is_ok());
    }
}
