use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

/// Repair unquoted JSON by adding quotes around bare keys and string values.
///
/// Handles LLM output like `{id: docs/qa/foo.md, count: 42, ok: true}` and
/// converts it to valid JSON. Idempotent on already-valid JSON since all
/// keys/strings are already quoted.
pub fn repair_unquoted_json(input: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum Context {
        Object,
        Array,
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Expecting {
        Key,
        Value,
        ArrayElement,
    }

    let mut out = String::with_capacity(input.len() + 64);
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_string = false;
    let mut context_stack: Vec<Context> = Vec::new();
    let mut expecting = Expecting::Value; // top-level
    let mut ever_opened = false; // track if we ever entered a structure

    while i < len {
        let b = bytes[i];

        if in_string {
            out.push(b as char);
            if b == b'\\' && i + 1 < len {
                i += 1;
                out.push(bytes[i] as char);
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        match b {
            b'"' => {
                in_string = true;
                out.push('"');
                i += 1;
            }
            b'{' => {
                out.push('{');
                context_stack.push(Context::Object);
                expecting = Expecting::Key;
                ever_opened = true;
                i += 1;
            }
            b'[' => {
                out.push('[');
                context_stack.push(Context::Array);
                expecting = Expecting::ArrayElement;
                ever_opened = true;
                i += 1;
            }
            b'}' => {
                out.push('}');
                context_stack.pop();
                if ever_opened && context_stack.is_empty() {
                    // Top-level structure closed; stop — don't corrupt trailing text
                    out.push_str(&input[i + 1..]);
                    return out;
                }
                i += 1;
            }
            b']' => {
                out.push(']');
                context_stack.pop();
                if ever_opened && context_stack.is_empty() {
                    out.push_str(&input[i + 1..]);
                    return out;
                }
                i += 1;
            }
            b':' => {
                out.push(':');
                expecting = Expecting::Value;
                i += 1;
            }
            b',' => {
                out.push(',');
                expecting = match context_stack.last() {
                    Some(Context::Object) => Expecting::Key,
                    Some(Context::Array) => Expecting::ArrayElement,
                    None => Expecting::Value,
                };
                i += 1;
            }
            b if b.is_ascii_whitespace() => {
                out.push(b as char);
                i += 1;
            }
            _ => {
                // Bare token — accumulate it
                let start = i;
                if expecting == Expecting::Key {
                    // Key: accumulate [a-zA-Z0-9_-]
                    while i < len
                        && (bytes[i].is_ascii_alphanumeric()
                            || bytes[i] == b'_'
                            || bytes[i] == b'-')
                    {
                        i += 1;
                    }
                    let token = &input[start..i];
                    out.push('"');
                    out.push_str(token);
                    out.push('"');
                } else {
                    // Value or ArrayElement: accumulate until , } ] or end
                    while i < len && bytes[i] != b',' && bytes[i] != b'}' && bytes[i] != b']' {
                        i += 1;
                    }
                    let token = input[start..i].trim();
                    // Check if it's a number, bool, or null — leave as-is
                    if token == "true"
                        || token == "false"
                        || token == "null"
                        || token.parse::<f64>().is_ok()
                    {
                        out.push_str(token);
                    } else {
                        out.push('"');
                        out.push_str(token);
                        out.push('"');
                    }
                }
            }
        }
    }

    out
}

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

    // 2.5 Try repairing unquoted JSON
    let repaired = repair_unquoted_json(json_str);
    if repaired != json_str {
        if let Ok(root) = serde_json::from_str::<Value>(&repaired) {
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
            if let Ok(Value::Array(arr)) = resolve_path(&root, path) {
                return Some(arr.clone());
            }
        }
        // Fallback: try repairing unquoted JSON in this slice
        let repaired = repair_unquoted_json(slice);
        if repaired != slice {
            let mut de = serde_json::Deserializer::from_str(&repaired);
            if let Ok(root) = <Value as Deserialize>::deserialize(&mut de) {
                if let Ok(Value::Array(arr)) = resolve_path(&root, path) {
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
    if path == "$" {
        return Ok(root);
    }
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
        let arr = extract_json_array(fenced, "$.items").expect("should extract from fenced block");
        assert_eq!(arr.len(), 3);
    }

    #[test]
    fn extract_array_from_unfenced_code_block() {
        let fenced = r#"Results:

```
{"goals": ["x", "y"]}
```
"#;
        let arr =
            extract_json_array(fenced, "$.goals").expect("should extract from unfenced block");
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
    fn extract_array_unquoted_json_succeeds() {
        let input = "I found: {targets: [a, b]}";
        let arr = extract_json_array(input, "$.targets").expect("should repair and extract");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], json!("a"));
        assert_eq!(arr[1], json!("b"));
    }

    #[test]
    fn extract_array_truly_unparsable_fails() {
        let bad = "I found: <<<not json at all>>>";
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

    // --- repair_unquoted_json tests ---

    #[test]
    fn repair_unquoted_json_keys_and_values() {
        let input = r#"{id: docs/qa/foo.md, name: test}"#;
        let repaired = repair_unquoted_json(input);
        let parsed: Value = serde_json::from_str(&repaired).expect("should be valid JSON");
        assert_eq!(parsed["id"], json!("docs/qa/foo.md"));
        assert_eq!(parsed["name"], json!("test"));
    }

    #[test]
    fn repair_unquoted_json_nested_array() {
        let input = r#"{items: [{id: a}, {id: b}]}"#;
        let repaired = repair_unquoted_json(input);
        let parsed: Value = serde_json::from_str(&repaired).expect("should be valid JSON");
        assert_eq!(parsed["items"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["items"][0]["id"], json!("a"));
        assert_eq!(parsed["items"][1]["id"], json!("b"));
    }

    #[test]
    fn repair_unquoted_json_preserves_valid() {
        let input = r#"{"id":"a"}"#;
        let repaired = repair_unquoted_json(input);
        assert_eq!(repaired, input);
    }

    #[test]
    fn repair_unquoted_json_mixed_quoted() {
        let input = r#"{"id": "a", name: b}"#;
        let repaired = repair_unquoted_json(input);
        let parsed: Value = serde_json::from_str(&repaired).expect("should be valid JSON");
        assert_eq!(parsed["id"], json!("a"));
        assert_eq!(parsed["name"], json!("b"));
    }

    #[test]
    fn repair_unquoted_json_numbers_bools_null() {
        let input = r#"{count: 42, ok: true, x: null}"#;
        let repaired = repair_unquoted_json(input);
        let parsed: Value = serde_json::from_str(&repaired).expect("should be valid JSON");
        assert_eq!(parsed["count"], json!(42));
        assert_eq!(parsed["ok"], json!(true));
        assert_eq!(parsed["x"], json!(null));
    }

    #[test]
    fn repair_unquoted_json_file_paths() {
        let input = r#"{id: docs/qa/orchestrator/02-cli-task-lifecycle.md}"#;
        let repaired = repair_unquoted_json(input);
        let parsed: Value = serde_json::from_str(&repaired).expect("should be valid JSON");
        assert_eq!(
            parsed["id"],
            json!("docs/qa/orchestrator/02-cli-task-lifecycle.md")
        );
    }

    #[test]
    fn extract_array_unquoted_regression_targets() {
        let input = r#"{regression_targets: [{id: docs/qa/foo.md, scope: unit}, {id: docs/qa/bar.md, scope: e2e}, {id: docs/qa/baz.md, scope: unit}, {id: docs/qa/qux.md, scope: integration}, {id: docs/qa/quux.md, scope: unit}]}"#;
        let arr = extract_json_array(input, "$.regression_targets")
            .expect("should extract unquoted regression targets");
        assert_eq!(arr.len(), 5);
        assert_eq!(arr[0]["id"], json!("docs/qa/foo.md"));
        assert_eq!(arr[2]["scope"], json!("unit"));
    }

    #[test]
    fn extract_array_mixed_text_unquoted() {
        let input = r#"Based on my analysis, here are the targets:

{regression_targets: [{id: target-a, name: A}, {id: target-b, name: B}]}

That's all."#;
        let arr = extract_json_array(input, "$.regression_targets")
            .expect("should extract from mixed text with unquoted JSON");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], json!("target-a"));
        assert_eq!(arr[1]["name"], json!("B"));
    }
}
