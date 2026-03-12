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

/// Extract the `result` field from the last `{"type":"result",...}` line in stream-json JSONL.
pub fn extract_stream_json_result(content: &str) -> Option<String> {
    for line in content.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains("\"type\":\"result\"") || trimmed.contains("\"type\": \"result\"") {
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                if let Some(result) = parsed.get("result").and_then(|v| v.as_str()) {
                    return Some(result.to_string());
                }
            }
            if let Some(extracted) = extract_result_field_raw(trimmed) {
                return Some(extracted);
            }
        }
    }
    None
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

fn extract_result_field_raw(line: &str) -> Option<String> {
    let marker = "\"result\":\"";
    let pos = line.find(marker)?;
    let value_start = pos + marker.len();
    let bytes = line.as_bytes();

    let mut i = value_start;
    let mut result = String::new();
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => {
                match bytes[i + 1] {
                    b'"' => result.push('"'),
                    b'\\' => result.push('\\'),
                    b'n' => result.push('\n'),
                    b'r' => result.push('\r'),
                    b't' => result.push('\t'),
                    b'/' => result.push('/'),
                    b'u' if i + 5 < bytes.len() => {
                        let hex = &line[i + 2..i + 6];
                        if let Ok(cp) = u32::from_str_radix(hex, 16) {
                            if let Some(ch) = char::from_u32(cp) {
                                result.push(ch);
                            }
                        }
                        i += 6;
                        continue;
                    }
                    other => {
                        result.push('\\');
                        result.push(other as char);
                    }
                }
                i += 2;
            }
            b'"' => return Some(result),
            _ => {
                result.push(bytes[i] as char);
                i += 1;
            }
        }
    }
    None
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
        assert_eq!(extract_field(&value, "$.meta.id"), Some("abc".to_string()));
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
        assert_eq!(extract_field(&value, "$.active"), Some("true".to_string()));
    }

    #[test]
    fn extract_stream_json_result_prefers_last_result_line() {
        let content = concat!(
            "{\"type\":\"result\",\"result\":\"{\\\"score\\\":1}\"}\n",
            "{\"type\":\"result\",\"result\":\"{\\\"score\\\":2}\"}\n"
        );

        assert_eq!(
            extract_stream_json_result(content),
            Some("{\"score\":2}".to_string())
        );
    }

    #[test]
    fn extract_stream_json_result_handles_redacted_lines() {
        let content =
            "{\"type\":\"result\",\"cost_usd\":[REDACTED],\"result\":\"{\\\"score\\\":42}\"}";

        assert_eq!(
            extract_stream_json_result(content),
            Some("{\"score\":42}".to_string())
        );
    }
}
