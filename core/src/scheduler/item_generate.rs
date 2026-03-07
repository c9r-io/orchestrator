use crate::config::{GenerateItemsAction, NewDynamicItem};
use crate::json_extract::{extract_field, extract_json_array};
use crate::state::InnerState;
use anyhow::{Context, Result};
use rusqlite::params;
use std::collections::HashMap;
use tracing::{info, warn};
use uuid::Uuid;

/// Resolve the effective JSON content for a pipeline variable.
///
/// When the inline variable is truncated (contains the spill marker), falls back
/// to reading the full content from the companion `{key}_path` spill file.
/// If the content looks like stream-json JSONL (claude `--output-format stream-json`),
/// extracts the `result` field from the last `type: result` line.
fn resolve_pipeline_var_content(
    pipeline_vars: &HashMap<String, String>,
    key: &str,
) -> Result<String> {
    let inline = pipeline_vars.get(key).with_context(|| {
        format!("pipeline variable '{}' not found for generate_items", key)
    })?;

    // If not truncated, check if it's raw stream-json JSONL and extract the result
    if !inline.contains("[truncated \u{2014}") && !inline.contains("[truncated —") {
        if let Some(result_text) = extract_stream_json_result(inline) {
            info!(key, "resolved pipeline var from inline stream-json result field");
            return Ok(result_text);
        }
        return Ok(inline.clone());
    }

    // Fall back to spill file
    let path_key = format!("{}_path", key);
    let path = pipeline_vars.get(&path_key).with_context(|| {
        format!("pipeline variable '{}' is truncated but no spill path '{}' found", key, path_key)
    })?;

    let content = std::fs::read_to_string(path).with_context(|| {
        format!("failed to read spill file at '{}'", path)
    })?;

    // Check if content looks like stream-json JSONL (multiple JSON lines from claude)
    // and extract the `result` field from the last `type: result` line
    if let Some(result_text) = extract_stream_json_result(&content) {
        info!(key, "resolved pipeline var from stream-json result field");
        return Ok(result_text);
    }

    // Otherwise return the full file content as-is
    Ok(content)
}

/// Extract the `result` field from the last `{"type":"result",...}` line in stream-json JSONL.
///
/// The line may have been partially redacted (e.g. `[REDACTED]` in numeric fields),
/// which breaks `serde_json::from_str`.  We first try full JSON parsing; if that
/// fails we fall back to a substring extraction of the `"result":"..."` field.
fn extract_stream_json_result(content: &str) -> Option<String> {
    for line in content.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Look for stream-json result line
        if trimmed.contains("\"type\":\"result\"") || trimmed.contains("\"type\": \"result\"") {
            // Try full JSON parse first
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(result) = parsed.get("result").and_then(|v| v.as_str()) {
                    return Some(result.to_string());
                }
            }
            // Fallback: extract "result":"..." manually (handles redacted JSON)
            if let Some(extracted) = extract_result_field_raw(trimmed) {
                return Some(extracted);
            }
        }
    }
    None
}

/// Manually extract the value of the `"result"` key from a JSON-like string.
///
/// Handles the case where the JSON line is not fully parseable (e.g. due to
/// `[REDACTED]` markers in numeric fields).  Finds `"result":"` and then reads
/// the escaped string value by tracking quote/backslash state.
fn extract_result_field_raw(line: &str) -> Option<String> {
    // Find "result":" pattern
    let marker = "\"result\":\"";
    let pos = line.find(marker)?;
    let value_start = pos + marker.len();
    let bytes = line.as_bytes();

    // Walk the escaped string to find the closing quote
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
                    // \uXXXX
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
            b'"' => {
                // End of the string value
                return Some(result);
            }
            _ => {
                result.push(bytes[i] as char);
                i += 1;
            }
        }
    }
    None // unterminated string
}

/// Extract dynamic items from a pipeline variable using the action's mapping.
pub fn extract_dynamic_items(
    pipeline_vars: &HashMap<String, String>,
    action: &GenerateItemsAction,
) -> Result<Vec<NewDynamicItem>> {
    let json_str = resolve_pipeline_var_content(pipeline_vars, &action.from_var)?;

    let items = match extract_json_array(&json_str, &action.json_path) {
        Ok(items) => items,
        Err(e) => {
            warn!(
                from_var = %action.from_var,
                json_path = %action.json_path,
                content_len = json_str.len(),
                content_preview = %&json_str[..json_str.len().min(200)],
                "extract_json_array failed: {}",
                e
            );
            return Err(e);
        }
    };
    let mut result = Vec::new();

    for item_value in &items {
        let item_id = match extract_field(item_value, &action.mapping.item_id) {
            Some(id) => id,
            None => continue,
        };

        let label = action
            .mapping
            .label
            .as_ref()
            .and_then(|path| extract_field(item_value, path));

        let mut vars = HashMap::new();
        for (var_name, json_path) in &action.mapping.vars {
            if let Some(val) = extract_field(item_value, json_path) {
                vars.insert(var_name.clone(), val);
            }
        }

        result.push(NewDynamicItem {
            item_id,
            label,
            vars,
        });
    }

    Ok(result)
}

/// Insert dynamic items into the database for a given task.
pub fn create_dynamic_task_items(
    conn: &rusqlite::Connection,
    task_id: &str,
    items: &[NewDynamicItem],
    replace: bool,
) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();

    if replace {
        // Remove existing non-static items
        conn.execute(
            "DELETE FROM task_items WHERE task_id = ?1 AND source = 'dynamic'",
            params![task_id],
        )?;
    }

    // Get the current max order_no
    let max_order: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(order_no), 0) FROM task_items WHERE task_id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut created = 0;
    for (idx, item) in items.iter().enumerate() {
        let id = Uuid::new_v4().to_string();
        let order_no = max_order + (idx as i64) + 1;
        let dynamic_vars_json = if item.vars.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&item.vars)?)
        };

        conn.execute(
            "INSERT INTO task_items (id, task_id, order_no, qa_file_path, status, ticket_files_json, ticket_content_json, fix_required, fixed, last_error, started_at, completed_at, created_at, updated_at, dynamic_vars_json, label, source) VALUES (?1, ?2, ?3, ?4, 'pending', '[]', '[]', 0, 0, '', NULL, NULL, ?5, ?5, ?6, ?7, 'dynamic')",
            params![
                id,
                task_id,
                order_no,
                item.item_id,
                now,
                dynamic_vars_json,
                item.label,
            ],
        )?;
        created += 1;
    }

    info!(
        task_id = task_id,
        count = created,
        "created dynamic task items"
    );
    Ok(created)
}

/// Async wrapper for `create_dynamic_task_items` that uses the async database writer.
pub async fn create_dynamic_task_items_async(
    state: &InnerState,
    task_id: &str,
    items: &[NewDynamicItem],
    replace: bool,
) -> Result<usize> {
    let task_id = task_id.to_string();
    let items = items.to_vec();
    state
        .async_database
        .writer()
        .call(move |conn| {
            create_dynamic_task_items(conn, &task_id, &items, replace)
                .map_err(|e| tokio_rusqlite::Error::Other(e.into()))
        })
        .await
        .map_err(|e| anyhow::anyhow!("async db error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DynamicItemMapping, GenerateItemsAction};

    #[test]
    fn test_extract_dynamic_items() {
        let mut vars = HashMap::new();
        vars.insert(
            "candidates".to_string(),
            r#"{"items": [
                {"id": "approach_a", "name": "Approach A", "config": "/a.yaml"},
                {"id": "approach_b", "name": "Approach B", "config": "/b.yaml"}
            ]}"#
            .to_string(),
        );

        let action = GenerateItemsAction {
            from_var: "candidates".to_string(),
            json_path: "$.items".to_string(),
            mapping: DynamicItemMapping {
                item_id: "$.id".to_string(),
                label: Some("$.name".to_string()),
                vars: {
                    let mut m = HashMap::new();
                    m.insert("config".to_string(), "$.config".to_string());
                    m
                },
            },
            replace: false,
        };

        let items = extract_dynamic_items(&vars, &action).expect("extract items");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].item_id, "approach_a");
        assert_eq!(items[0].label, Some("Approach A".to_string()));
        assert_eq!(items[0].vars.get("config"), Some(&"/a.yaml".to_string()));
        assert_eq!(items[1].item_id, "approach_b");
    }

    #[test]
    fn test_extract_dynamic_items_missing_var() {
        let vars = HashMap::new();
        let action = GenerateItemsAction {
            from_var: "missing".to_string(),
            json_path: "$.items".to_string(),
            mapping: DynamicItemMapping {
                item_id: "$.id".to_string(),
                label: None,
                vars: HashMap::new(),
            },
            replace: false,
        };
        assert!(extract_dynamic_items(&vars, &action).is_err());
    }

    #[test]
    fn test_extract_dynamic_items_skips_missing_id() {
        let mut vars = HashMap::new();
        vars.insert(
            "data".to_string(),
            r#"{"items": [{"name": "no_id"}, {"id": "has_id"}]}"#.to_string(),
        );

        let action = GenerateItemsAction {
            from_var: "data".to_string(),
            json_path: "$.items".to_string(),
            mapping: DynamicItemMapping {
                item_id: "$.id".to_string(),
                label: None,
                vars: HashMap::new(),
            },
            replace: false,
        };

        let items = extract_dynamic_items(&vars, &action).expect("extract items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item_id, "has_id");
    }

    #[test]
    fn test_extract_stream_json_result() {
        // Valid stream-json with result field
        let content = r#"{"type": "text", "text": "some output"}
{"type": "result", "result": "{\"items\": []}"}
{"type": "done"}"#;
        let result = extract_stream_json_result(content);
        assert_eq!(result, Some(r#"{"items": []}"#.to_string()));
    }

    #[test]
    fn test_extract_stream_json_result_no_result() {
        // Stream-json without result field
        let content = r#"{"type": "text", "text": "hello"}
{"type": "done"}"#;
        let result = extract_stream_json_result(content);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_stream_json_result_redacted() {
        // Stream-json with [REDACTED] markers that break JSON parsing
        let content = r#"{"type":"system","session_id":"abc[REDACTED]def"}
{"type":"result","subtype":"success","is_error":false,"duration_ms":9[REDACTED]294,"result":"{\"items\": [{\"id\": \"a\"}]}"}"#;
        let result = extract_stream_json_result(content);
        assert_eq!(result, Some(r#"{"items": [{"id": "a"}]}"#.to_string()));
    }

    #[test]
    fn test_extract_stream_json_result_whitespace_lines() {
        // Content with empty/whitespace lines
        let content = r#"

{"type": "result", "result": "data"}
"#;
        let result = extract_stream_json_result(content);
        assert_eq!(result, Some("data".to_string()));
    }

    #[test]
    fn test_resolve_pipeline_var_content_not_truncated() {
        let mut vars = HashMap::new();
        vars.insert("data".to_string(), r#"{"items": []}"#.to_string());

        let result = resolve_pipeline_var_content(&vars, "data").expect("resolve content");
        assert_eq!(result, r#"{"items": []}"#);
    }

    #[test]
    fn test_resolve_pipeline_var_content_truncated() {
        // Create a temp file for the spill
        let temp_dir = std::env::temp_dir();
        let spill_path = temp_dir.join("test_spill.json");
        std::fs::write(&spill_path, r#"{"items": [{"id": "a"}]}"#).unwrap();

        let mut vars = HashMap::new();
        vars.insert("data".to_string(), "[truncated — output too long]".to_string());
        vars.insert("data_path".to_string(), spill_path.to_string_lossy().to_string());

        let result = resolve_pipeline_var_content(&vars, "data").expect("resolve content");
        assert_eq!(result, r#"{"items": [{"id": "a"}]}"#);

        std::fs::remove_file(&spill_path).ok();
    }

    #[test]
    fn test_resolve_pipeline_var_content_truncated_stream_json() {
        // Create a temp file for the spill with stream-json format
        let temp_dir = std::env::temp_dir();
        let spill_path = temp_dir.join("test_stream_spill.json");
        let spill_content = r#"{"type": "text", "text": "thinking..."}
{"type": "result", "result": "{\"id\": \"approach-a\"}"}
{"type": "done"}"#;
        std::fs::write(&spill_path, spill_content).unwrap();

        let mut vars = HashMap::new();
        vars.insert("data".to_string(), "[truncated — output too long]".to_string());
        vars.insert("data_path".to_string(), spill_path.to_string_lossy().to_string());

        let result = resolve_pipeline_var_content(&vars, "data").expect("resolve content");
        assert_eq!(result, r#"{"id": "approach-a"}"#);

        std::fs::remove_file(&spill_path).ok();
    }

    #[test]
    fn test_resolve_pipeline_var_content_missing_var() {
        let vars = HashMap::new();
        let result = resolve_pipeline_var_content(&vars, "missing");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_pipeline_var_content_truncated_missing_path() {
        let mut vars = HashMap::new();
        vars.insert("data".to_string(), "[truncated — output too long]".to_string());

        let result = resolve_pipeline_var_content(&vars, "data");
        assert!(result.is_err());
    }
}
