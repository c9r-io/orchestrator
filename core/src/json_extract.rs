use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

/// Extract a JSON array from a JSON string using a simple path expression.
///
/// Supports paths like `$.field_name` or `$.field.nested`.
/// Returns the array found at the given path.
///
/// Resilient to mixed-text input (e.g. LLM agent output with natural language
/// before/after JSON). Tries in order:
/// 1. Parse the entire string as JSON
/// 2. Extract from a fenced code block (```json ... ```)
/// 3. Scan for the first `{` or `[` and try parsing from there
pub fn extract_json_array(json_str: &str, path: &str) -> Result<Vec<Value>> {
    // 1. Try parsing the whole string as JSON
    if let Ok(root) = serde_json::from_str::<Value>(json_str) {
        let target = resolve_path(&root, path)?;
        return match target {
            Value::Array(arr) => Ok(arr.clone()),
            _ => anyhow::bail!("path '{}' does not point to an array", path),
        };
    }

    // 2. Try extracting from a fenced code block (```json ... ``` or ``` ... ```)
    if let Some(json_block) = extract_fenced_json(json_str) {
        if let Ok(root) = serde_json::from_str::<Value>(&json_block) {
            if let Ok(target) = resolve_path(&root, path) {
                return match target {
                    Value::Array(arr) => Ok(arr.clone()),
                    _ => anyhow::bail!("path '{}' does not point to an array", path),
                };
            }
        }
    }

    // 3. Scan for JSON objects/arrays starting at each `{` or `[`
    if let Some(arr) = scan_for_json_with_path(json_str, path) {
        return Ok(arr);
    }

    anyhow::bail!("no valid JSON containing path '{}' found in text", path)
}

/// Extract JSON content from a markdown fenced code block.
fn extract_fenced_json(text: &str) -> Option<String> {
    // Match ```json ... ``` or ``` ... ```
    let fence_start_markers = ["```json\n", "```json\r\n", "```\n", "```\r\n"];
    for marker in &fence_start_markers {
        if let Some(start) = text.find(marker) {
            let content_start = start + marker.len();
            if let Some(end) = text[content_start..].find("```") {
                return Some(text[content_start..content_start + end].trim().to_string());
            }
        }
    }
    None
}

/// Scan text for JSON objects starting at each `{` or `[`, try to parse and resolve path.
/// Uses `serde_json::Deserializer::from_str` to parse a single value from a prefix,
/// allowing trailing text after the JSON.
fn scan_for_json_with_path(text: &str, path: &str) -> Option<Vec<Value>> {
    for (i, ch) in text.char_indices() {
        if ch != '{' && ch != '[' {
            continue;
        }
        let slice = &text[i..];
        let mut de = serde_json::Deserializer::from_str(slice);
        if let Ok(root) = <Value as Deserialize>::deserialize(&mut de) {
            if let Ok(target) = resolve_path(&root, path) {
                if let Value::Array(arr) = target {
                    return Some(arr.clone());
                }
            }
        }
    }
    None
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
    fn extract_array_from_mixed_text_with_preamble() {
        let mixed = r#"Based on my analysis, I identified these targets:

{"regression_targets": [{"id": "target-a", "name": "A"}, {"id": "target-b", "name": "B"}]}"#;
        let arr = extract_json_array(mixed, "$.regression_targets")
            .expect("should extract from mixed text");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], json!("target-a"));
        assert_eq!(arr[1]["id"], json!("target-b"));
    }

    #[test]
    fn extract_array_from_fenced_code_block() {
        let fenced = r#"Here are the results:

```json
{"items": [{"id": "a"}, {"id": "b"}, {"id": "c"}]}
```

Done."#;
        let arr =
            extract_json_array(fenced, "$.items").expect("should extract from fenced block");
        assert_eq!(arr.len(), 3);
    }

    #[test]
    fn extract_array_from_unfenced_code_block() {
        let fenced = r#"Results:

```
{"goals": ["x", "y"]}
```
"#;
        let arr = extract_json_array(fenced, "$.goals")
            .expect("should extract from unfenced block");
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn extract_array_multiple_json_objects_finds_correct_one() {
        let multi = r#"Summary: {"status": "ok", "count": 3}

Details:
{"regression_targets": [{"id": "rt-1"}, {"id": "rt-2"}]}

Footer: {"ts": "2026-01-01"}"#;
        let arr = extract_json_array(multi, "$.regression_targets")
            .expect("should find the correct JSON object");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], json!("rt-1"));
    }

    #[test]
    fn extract_array_malformed_json_fails() {
        let bad = "I found: {targets: [a, b]}";
        let result = extract_json_array(bad, "$.targets");
        assert!(result.is_err());
    }

    #[test]
    fn extract_array_no_matching_path_in_mixed_text_fails() {
        let mixed = r#"Some text {"other_field": [1, 2]}"#;
        let result = extract_json_array(mixed, "$.regression_targets");
        assert!(result.is_err());
    }

    #[test]
    fn extract_array_pure_json_still_works() {
        // Regression guard: pure JSON must keep working
        let pure = r#"{"items": [{"id": "clean"}]}"#;
        let arr = extract_json_array(pure, "$.items").expect("pure JSON must work");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], json!("clean"));
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
