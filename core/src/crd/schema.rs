use anyhow::{anyhow, Result};
use serde_json::Value;

/// Validate a JSON value against a JSON Schema subset.
///
/// Supported keywords: type, required, properties, items, enum,
/// minLength/maxLength, minimum/maximum, minItems/maxItems,
/// additionalProperties (bool), pattern (basic regex via glob-style).
pub fn validate_json_schema(instance: &Value, schema: &Value) -> Result<()> {
    validate_at_path(instance, schema, "$")
}

fn validate_at_path(instance: &Value, schema: &Value, path: &str) -> Result<()> {
    let schema_obj = match schema.as_object() {
        Some(obj) => obj,
        None => return Ok(()), // non-object schema = accept anything
    };

    // type check
    if let Some(type_val) = schema_obj.get("type") {
        if let Some(expected_type) = type_val.as_str() {
            validate_type(instance, expected_type, path)?;
        }
    }

    // enum check
    if let Some(enum_val) = schema_obj.get("enum") {
        if let Some(variants) = enum_val.as_array() {
            if !variants.contains(instance) {
                return Err(anyhow!("{}: value must be one of {:?}", path, variants));
            }
        }
    }

    // string constraints
    if instance.is_string() {
        let s = instance.as_str().unwrap_or_default();
        if let Some(min) = schema_obj.get("minLength").and_then(|v| v.as_u64()) {
            if (s.len() as u64) < min {
                return Err(anyhow!(
                    "{}: string length {} is less than minLength {}",
                    path,
                    s.len(),
                    min
                ));
            }
        }
        if let Some(max) = schema_obj.get("maxLength").and_then(|v| v.as_u64()) {
            if (s.len() as u64) > max {
                return Err(anyhow!(
                    "{}: string length {} exceeds maxLength {}",
                    path,
                    s.len(),
                    max
                ));
            }
        }
        if let Some(pattern) = schema_obj.get("pattern").and_then(|v| v.as_str()) {
            validate_pattern(s, pattern, path)?;
        }
    }

    // number constraints
    if let Some(n) = instance.as_f64() {
        if let Some(min) = schema_obj.get("minimum").and_then(|v| v.as_f64()) {
            if n < min {
                return Err(anyhow!(
                    "{}: value {} is less than minimum {}",
                    path,
                    n,
                    min
                ));
            }
        }
        if let Some(max) = schema_obj.get("maximum").and_then(|v| v.as_f64()) {
            if n > max {
                return Err(anyhow!("{}: value {} exceeds maximum {}", path, n, max));
            }
        }
    }

    // object constraints
    if let Some(obj) = instance.as_object() {
        // required
        if let Some(required) = schema_obj.get("required").and_then(|v| v.as_array()) {
            for req in required {
                if let Some(field_name) = req.as_str() {
                    if !obj.contains_key(field_name) {
                        return Err(anyhow!("{}: missing required field '{}'", path, field_name));
                    }
                }
            }
        }

        // properties
        if let Some(properties) = schema_obj.get("properties").and_then(|v| v.as_object()) {
            for (prop_name, prop_schema) in properties {
                if let Some(prop_value) = obj.get(prop_name) {
                    let prop_path = format!("{}.{}", path, prop_name);
                    validate_at_path(prop_value, prop_schema, &prop_path)?;
                }
            }
        }

        // additionalProperties
        if let Some(additional) = schema_obj.get("additionalProperties") {
            if additional.as_bool() == Some(false) {
                if let Some(properties) = schema_obj.get("properties").and_then(|v| v.as_object()) {
                    for key in obj.keys() {
                        if !properties.contains_key(key) {
                            return Err(anyhow!("{}: unexpected property '{}'", path, key));
                        }
                    }
                }
            }
        }
    }

    // array constraints
    if let Some(arr) = instance.as_array() {
        if let Some(min) = schema_obj.get("minItems").and_then(|v| v.as_u64()) {
            if (arr.len() as u64) < min {
                return Err(anyhow!(
                    "{}: array length {} is less than minItems {}",
                    path,
                    arr.len(),
                    min
                ));
            }
        }
        if let Some(max) = schema_obj.get("maxItems").and_then(|v| v.as_u64()) {
            if (arr.len() as u64) > max {
                return Err(anyhow!(
                    "{}: array length {} exceeds maxItems {}",
                    path,
                    arr.len(),
                    max
                ));
            }
        }
        if let Some(items_schema) = schema_obj.get("items") {
            for (i, item) in arr.iter().enumerate() {
                let item_path = format!("{}[{}]", path, i);
                validate_at_path(item, items_schema, &item_path)?;
            }
        }
    }

    Ok(())
}

fn validate_type(instance: &Value, expected: &str, path: &str) -> Result<()> {
    let matches = match expected {
        "string" => instance.is_string(),
        "number" => instance.is_number(),
        "integer" => instance.is_i64() || instance.is_u64(),
        "boolean" => instance.is_boolean(),
        "array" => instance.is_array(),
        "object" => instance.is_object(),
        "null" => instance.is_null(),
        _ => true, // unknown type = accept
    };
    if !matches {
        return Err(anyhow!(
            "{}: expected type '{}', got {}",
            path,
            expected,
            json_type_name(instance)
        ));
    }
    Ok(())
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Simple pattern matching. We use basic char-by-char matching for
/// common regex patterns (anchored `^...$` with literal chars).
/// For full regex support, users should use CEL rules.
fn validate_pattern(value: &str, pattern: &str, path: &str) -> Result<()> {
    // Simple substring check: if pattern has no regex metacharacters, just check contains
    let has_meta = pattern.chars().any(|c| {
        matches!(
            c,
            '^' | '$' | '.' | '*' | '+' | '?' | '[' | ']' | '(' | ')' | '{' | '}' | '|' | '\\'
        )
    });

    if !has_meta {
        // Plain substring match
        if !value.contains(pattern) {
            return Err(anyhow!(
                "{}: string '{}' does not match pattern '{}'",
                path,
                value,
                pattern
            ));
        }
        return Ok(());
    }

    // For patterns with metacharacters, do a best-effort anchored check:
    // ^literal$ → exact match, ^literal → starts with, literal$ → ends with
    let trimmed = pattern.trim();
    let (anchored_start, rest) = if let Some(rest) = trimmed.strip_prefix('^') {
        (true, rest)
    } else {
        (false, trimmed)
    };
    let (anchored_end, rest) = if let Some(rest) = rest.strip_suffix('$') {
        (true, rest)
    } else {
        (false, rest)
    };

    // If the remaining pattern is purely literal (no other metacharacters)
    let remaining_has_meta = rest.chars().any(|c| {
        matches!(
            c,
            '.' | '*' | '+' | '?' | '[' | ']' | '(' | ')' | '{' | '}' | '|' | '\\'
        )
    });

    if !remaining_has_meta {
        let matched = match (anchored_start, anchored_end) {
            (true, true) => value == rest,
            (true, false) => value.starts_with(rest),
            (false, true) => value.ends_with(rest),
            (false, false) => value.contains(rest),
        };
        if !matched {
            return Err(anyhow!(
                "{}: string '{}' does not match pattern '{}'",
                path,
                value,
                pattern
            ));
        }
        return Ok(());
    }

    // Complex regex patterns: accept (no-op) — users should use CEL for complex validation
    Ok(())
}

/// Validate that a schema definition itself is well-formed (root must be type=object).
pub fn validate_schema_definition(schema: &Value) -> Result<()> {
    let obj = schema
        .as_object()
        .ok_or_else(|| anyhow!("schema must be a JSON object"))?;

    match obj.get("type").and_then(|v| v.as_str()) {
        Some("object") => Ok(()),
        Some(other) => Err(anyhow!(
            "schema root type must be 'object', got '{}'",
            other
        )),
        None => Err(anyhow!("schema must have a 'type' field set to 'object'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validate_type_string() {
        let schema = json!({"type": "string"});
        assert!(validate_json_schema(&json!("hello"), &schema).is_ok());
        assert!(validate_json_schema(&json!(42), &schema).is_err());
    }

    #[test]
    fn validate_type_number() {
        let schema = json!({"type": "number"});
        assert!(validate_json_schema(&json!(3.15), &schema).is_ok());
        assert!(validate_json_schema(&json!(42), &schema).is_ok());
        assert!(validate_json_schema(&json!("not a number"), &schema).is_err());
    }

    #[test]
    fn validate_type_integer() {
        let schema = json!({"type": "integer"});
        assert!(validate_json_schema(&json!(42), &schema).is_ok());
        assert!(validate_json_schema(&json!("not an int"), &schema).is_err());
    }

    #[test]
    fn validate_type_boolean() {
        let schema = json!({"type": "boolean"});
        assert!(validate_json_schema(&json!(true), &schema).is_ok());
        assert!(validate_json_schema(&json!("true"), &schema).is_err());
    }

    #[test]
    fn validate_type_array() {
        let schema = json!({"type": "array"});
        assert!(validate_json_schema(&json!([1, 2]), &schema).is_ok());
        assert!(validate_json_schema(&json!({}), &schema).is_err());
    }

    #[test]
    fn validate_type_object() {
        let schema = json!({"type": "object"});
        assert!(validate_json_schema(&json!({}), &schema).is_ok());
        assert!(validate_json_schema(&json!([]), &schema).is_err());
    }

    #[test]
    fn validate_type_null() {
        let schema = json!({"type": "null"});
        assert!(validate_json_schema(&Value::Null, &schema).is_ok());
        assert!(validate_json_schema(&json!(0), &schema).is_err());
    }

    #[test]
    fn validate_required_fields() {
        let schema = json!({
            "type": "object",
            "required": ["name", "value"]
        });
        assert!(validate_json_schema(&json!({"name": "a", "value": 1}), &schema).is_ok());
        assert!(validate_json_schema(&json!({"name": "a"}), &schema).is_err());
        assert!(validate_json_schema(&json!({}), &schema).is_err());
    }

    #[test]
    fn validate_properties_nested() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": {"type": "integer"},
                "label": {"type": "string"}
            }
        });
        assert!(validate_json_schema(&json!({"count": 5, "label": "hi"}), &schema).is_ok());
        assert!(validate_json_schema(&json!({"count": "not int"}), &schema).is_err());
    }

    #[test]
    fn validate_additional_properties_false() {
        let schema = json!({
            "type": "object",
            "properties": {"a": {"type": "string"}},
            "additionalProperties": false
        });
        assert!(validate_json_schema(&json!({"a": "ok"}), &schema).is_ok());
        assert!(validate_json_schema(&json!({"a": "ok", "b": "extra"}), &schema).is_err());
    }

    #[test]
    fn validate_enum() {
        let schema = json!({"enum": ["red", "green", "blue"]});
        assert!(validate_json_schema(&json!("red"), &schema).is_ok());
        assert!(validate_json_schema(&json!("yellow"), &schema).is_err());
    }

    #[test]
    fn validate_string_length() {
        let schema = json!({"type": "string", "minLength": 2, "maxLength": 5});
        assert!(validate_json_schema(&json!("ab"), &schema).is_ok());
        assert!(validate_json_schema(&json!("abcde"), &schema).is_ok());
        assert!(validate_json_schema(&json!("a"), &schema).is_err());
        assert!(validate_json_schema(&json!("abcdef"), &schema).is_err());
    }

    #[test]
    fn validate_number_range() {
        let schema = json!({"type": "number", "minimum": 0, "maximum": 100});
        assert!(validate_json_schema(&json!(50), &schema).is_ok());
        assert!(validate_json_schema(&json!(-1), &schema).is_err());
        assert!(validate_json_schema(&json!(101), &schema).is_err());
    }

    #[test]
    fn validate_array_items() {
        let schema = json!({
            "type": "array",
            "items": {"type": "string"},
            "minItems": 1,
            "maxItems": 3
        });
        assert!(validate_json_schema(&json!(["a"]), &schema).is_ok());
        assert!(validate_json_schema(&json!(["a", "b", "c"]), &schema).is_ok());
        assert!(validate_json_schema(&json!([]), &schema).is_err()); // minItems
        assert!(validate_json_schema(&json!(["a", "b", "c", "d"]), &schema).is_err()); // maxItems
        assert!(validate_json_schema(&json!([1, 2]), &schema).is_err()); // items type
    }

    #[test]
    fn validate_pattern_literal() {
        let schema = json!({"type": "string", "pattern": "^hello$"});
        assert!(validate_json_schema(&json!("hello"), &schema).is_ok());
        assert!(validate_json_schema(&json!("hello world"), &schema).is_err());
    }

    #[test]
    fn validate_pattern_starts_with() {
        let schema = json!({"type": "string", "pattern": "^foo"});
        assert!(validate_json_schema(&json!("foobar"), &schema).is_ok());
        assert!(validate_json_schema(&json!("barfoo"), &schema).is_err());
    }

    #[test]
    fn validate_schema_definition_valid() {
        assert!(validate_schema_definition(&json!({"type": "object"})).is_ok());
    }

    #[test]
    fn validate_schema_definition_invalid_type() {
        assert!(validate_schema_definition(&json!({"type": "array"})).is_err());
    }

    #[test]
    fn validate_schema_definition_missing_type() {
        assert!(validate_schema_definition(&json!({"properties": {}})).is_err());
    }

    #[test]
    fn validate_non_object_schema_accepts_anything() {
        assert!(validate_json_schema(&json!(42), &json!("not an object schema")).is_ok());
    }

    #[test]
    fn validate_deeply_nested_objects() {
        let schema = json!({
            "type": "object",
            "properties": {
                "inner": {
                    "type": "object",
                    "required": ["x"],
                    "properties": {
                        "x": {"type": "integer"}
                    }
                }
            }
        });
        assert!(validate_json_schema(&json!({"inner": {"x": 5}}), &schema).is_ok());
        assert!(validate_json_schema(&json!({"inner": {}}), &schema).is_err());
        assert!(validate_json_schema(&json!({"inner": {"x": "bad"}}), &schema).is_err());
    }
}
